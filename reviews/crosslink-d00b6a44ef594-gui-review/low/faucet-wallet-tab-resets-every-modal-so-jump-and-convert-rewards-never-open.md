# While the `Faucet Wallet` tab is selected, `ui_left_pane` sets `ui.modal = Modal::None` on every pass, so `Modal::Jump` and `Modal::ConvertRewards`, which do not depend on the selected wallet, are cleared before the render pass and their three entry points appear dead

**Severity**: Low
**Validation Status**: Confirmed
**Location**: `zebra-gui/src/ui.rs:1620-1623` (the unconditional reset); `zebra-gui/src/ui.rs:4043-4045` (Ctrl+J sets `Modal::Jump`, before the left pane runs); `zebra-gui/src/ui.rs:4531-4533` (`Jump To Height...` button, after the left pane); `zebra-gui/src/ui.rs:3623-3625` (`Convert Rewards` button in `ui_right_pane`, after the left pane); `zebra-gui/src/ui.rs:4146` and `:4660` (call order of the two panes inside `run_ui`); `zebra-gui/src/ui.rs:5260-5261` (`ui_update`: input pass, then render pass); `zebra-gui/src/ui.rs:1690` and `:1821-2804` (the modal container and the `match ui.modal` that draws it); `zebra-gui/src/ui.rs:465-475` (`enum Modal`); `zebra-gui/src/ui.rs:1736-1739` (the only place modal code reads the selected tab); `zebra-gui/src/ui.rs:2865-2913` (wallet modal buttons, hidden on the Faucet Wallet tab); `zebra-gui/src/ui.rs:1606-1613` (Ctrl+Tab)
**Found by agent:** /code-review high, GUI/UX (Claude Fable 5.1), 2026-09-30; validated 2026-10-01 at dev d00b6a44ef594
**In scope of audit?** Yes. It is a GUI control that silently does nothing. The reset was introduced by `90166a4e` ("UI: View miner wallet", 2026-01-14), when every modal was a wallet modal. `Modal::Jump` arrived later in `8bd28e08` (2026-05-05), its button in `5a75c3ed` (2026-08-07), and `Modal::ConvertRewards` in `3179fcc1` (2026-09-08). None of the three revisited the reset.

## Description

The left pane has two tabs, `Your Wallet` and `Faucet Wallet`. The faucet (miner) wallet is view only, so the code closes any open modal whenever that tab is selected (`ui.rs:1620-1623`):

```rust
// You can't operate on the miner, so you can't open any modals. // @Todo: maybe you can open Receive?
if *tab_id == tab_id_miner_wallet {
    ui.modal = Modal::None;
}
```

The comment states the intent: modals that operate on a wallet make no sense for the faucet wallet. When that line was written, the modals were Send, Receive, Stake, Unstake and User, so "any modals" and "wallet modals" were the same set. Two modals added since do not operate on the selected wallet at all:

- `Modal::Jump` moves the chain view camera to a PoW or PoS height.
- `Modal::ConvertRewards` converts this node's finalizer commission bank into a bond.

Both are drawn by the same `match ui.modal` inside `ui_left_pane` (`ui.rs:1821`), below the reset. Because the reset runs on every pass of `run_ui`, a request to open either modal is erased before the render pass can draw it. The user presses Ctrl+J, or clicks `Jump To Height...`, or clicks `Convert Rewards`, and nothing happens: no modal, no message, no change of any kind.

## Attack Scenario and Steps

This is a reproduction, not an attack. Each of the three entry points fails the same way.

1. Start the GUI. The left pane opens on `Your Wallet` (`tab_ex` selects the first tab declared when `tab_id` is `Id::default()`, `ui.rs:742-744`).
2. Click the `Faucet Wallet` tab, or press Ctrl+Tab (`ui.rs:1607-1613`).
3. Do any one of:
   - press Ctrl+J;
   - click `Jump To Height...` at the bottom of the chain view;
   - click `Convert Rewards` in the Finalizers pane on the right.
4. Observe: nothing appears. The button shows its pressed colour and that is all.
5. Click `Your Wallet` and repeat step 3: the modal opens normally.

A fourth, milder variant: open Jump or Convert Commission on `Your Wallet`, then press Ctrl+Tab. The modal closes without being asked to.

**Attack Requirements and Assumptions:**

- The `Faucet Wallet` tab must be the selected left pane tab. It is not the default, so a user who never clicks it is unaffected.
- All platforms (Windows, macOS, Linux X11, Linux Wayland). The path is pure UI state logic with no platform code in it.
- No wallet or chain state is needed. `Convert Rewards` is only useful to a participant running a finalizer with a non-zero commission bank, but the button is always enabled (`ui.rs:3623`), so anyone can hit the dead click.

## Impact on Users

- **Jump To Height looks broken.** A participant who has left the left pane on `Faucet Wallet` (a natural place to leave it while waiting for faucet funds) cannot jump, by keyboard or by button. Nothing tells them the tab is the reason.
- **Convert Rewards looks broken.** A finalizer operator in the same state cannot open the Convert Commission modal. The button is in a different pane from the tab that disables it, so the connection is not discoverable.
- **No wrong figures, no lost funds, no crash.** Nothing is submitted and nothing is displayed incorrectly. The workaround is one click on `Your Wallet`.

## Technical Details / Code Analysis

**Two passes per frame.** `ui_update` runs `run_ui` twice: first with the real input and `is_rendering = false`, then with a dummy input that carries held state but no press or release edges, and `is_rendering = true` (`ui.rs:5250-5262`):

```rust
let dummy_input = InputCtx {
    this_mouse_pos: ui.input().this_mouse_pos,
    last_mouse_pos: ui.input().this_mouse_pos,

    mouse_down: ui.input().mouse_down,
    keys_down1: ui.input().keys_down1,
    keys_down2: ui.input().keys_down2,

    ..Default::default()
};
let real_input = ui.input; let result =           run_ui(ui, wallet_state.clone(), data, viz, false);
ui.input = &dummy_input;   let result = result || run_ui(ui, wallet_state.clone(), data, viz, true);
ui.input =   real_input;
```

So clicks and key presses are only ever seen by the first (input) pass, and only the second (render) pass draws. A modal opened in the input pass must survive until the modal container is declared in the render pass.

**Order inside one pass of `run_ui`:**

| Step | Where | What happens to `ui.modal` |
| :- | :- | :- |
| 1 | `ui.rs:4043-4045` | Ctrl+J sets `Modal::Jump` |
| 2 | `ui.rs:4146` calls `ui_left_pane`; reset at `:1621-1623` | forced to `Modal::None` if the Faucet Wallet tab is selected |
| 3 | `ui.rs:1690`, `:1821` | modal container and contents declared, only if `ui.modal != Modal::None` |
| 4 | `ui.rs:4531-4533` | `Jump To Height...` button sets `Modal::Jump` |
| 5 | `ui.rs:4660` calls `ui_right_pane`; `:3623-3625` | `Convert Rewards` button sets `Modal::ConvertRewards` |

**Ctrl+J.** Step 1 sets `Modal::Jump` in the input pass, and step 2 of the same pass clears it. The render pass sees no key press (the dummy input has `keys_pressed1` and `keys_pressed2` zero), so it never sets it again.

```rust
if ctrl_held && ui.input().key_pressed(KEY_J) {
    ui.modal = Modal::Jump;
}
```

**The two buttons.** Steps 4 and 5 run after the left pane in the input pass, so `ui.modal` is still set when the input pass ends. The render pass then starts from the top and step 2 clears it before step 3 can declare the container. Nothing between step 4 or 5 of the input pass and step 2 of the render pass reads `ui.modal` (the only uses of `ui.modal` in `zebra-gui/src` are in `ui.rs`, all listed above or inside `ui_left_pane`), so the transient value has no visible effect at all.

```rust
if button_ex(ui, "Convert Rewards", true, true) {
    ui.modal = Modal::ConvertRewards;
}
```

**Every modal, and whether it depends on the selected wallet tab.** `enum Modal` is at `ui.rs:465-475`. Inside the modal code the selected tab is read in exactly one place, the `balance` local (`ui.rs:1736-1739`); everything else reads user wallet fields directly.

| Modal | Opened from | Operates on | Depends on the wallet tab? |
| :- | :- | :- | :- |
| `Send` | button row, `ui.rs:2909` | user wallet (`send_to_address`; `balance` at `:1966`, `:2009`, `:2049`) | Yes |
| `Receive` | `ui.rs:2910` | user wallet (`user_recv_ua`, `:2071`) | Yes |
| `Stake` | `ui.rs:2911` | user wallet (`stake_to_finalizer`; `balance` at `:2231`, `:2255-2261`, `:2318`) | Yes |
| `Unstake` (titled Edit Stake) | `ui.rs:2912` | user wallet bond positions | Yes |
| `Retarget` | from inside `Unstake`, `ui.rs:2721` | user wallet bond (`balance` at `:2347`) | Yes |
| `User` | `ui.rs:2913` | user seed and viewing key (`:2789-2792`) | Yes |
| `Jump` | Ctrl+J `:4044`; button `:4532` | chain view camera (`viz.goto_pow_height`, `viz.request_pos_height`, `:1888-1894`) | **No** |
| `ConvertRewards` | right pane button `:3624` | this node's commission bank (`bank_balance(&data.finalizer_banks, &my_pk)`, `:2119-2120`; `convert_finalizer_reward`, `:2168`, `:2172`) | **No** |

The six wallet modals cannot be opened from the Faucet Wallet tab in the first place: their button row is only declared when `*tab_id == tab_id_user_wallet` (`ui.rs:2865-2866`). For them the reset only matters when the tab changes while one is open (Ctrl+Tab at `:1607` works regardless of the modal), and there it does what the comment says.

**Is the reset deliberate?** For wallet modals, yes: the comment says so and the `@Todo` only questions Receive. For Jump and ConvertRewards there is no comment, TODO or commit message that mentions the faucet tab. `git log -S` shows the reset line predates both modals by months (see the scope field). This is read as an oversight, not a decision; that is an inference from the history, not something the code states.

## Recommendations

1. **Restrict the reset to the modals that act on the user wallet.** In `zebra-gui/src/ui.rs`, add a classification on `Modal` and use it at `:1621`. The match is exhaustive on purpose, so a modal added later must be classified before the file compiles.

   ```rust
   impl Modal {
       fn acts_on_user_wallet(self) -> bool {
           match self {
               Modal::Send | Modal::Receive | Modal::Stake | Modal::Unstake | Modal::Retarget | Modal::User => true,
               Modal::None | Modal::Jump | Modal::ConvertRewards => false,
           }
       }
   }
   ```

   ```rust
   // The faucet wallet is view only, so close any modal that acts on the user wallet.
   if *tab_id == tab_id_miner_wallet && ui.modal.acts_on_user_wallet() {
       ui.modal = Modal::None;
   }
   ```

   Nothing else needs to change for the two modals to render on the Faucet Wallet tab: `balance` (`ui.rs:1736-1739`) would hold the miner balance there, but neither `Modal::Jump` nor `Modal::ConvertRewards` reads it.

2. **Considered alternatives, not chosen.**
   - Move the reset to the moment the tab changes (inside the Ctrl+Tab branch and after `ui.tab`). This is the more precise model, since the buttons already prevent opening wallet modals on the faucet tab, but it needs the previous tab remembered across the two passes and touches more code for no user-visible gain.
   - Draw `Jump` and `ConvertRewards` outside `ui_left_pane`. Cleaner ownership (neither belongs to the left pane), but a larger move that collides with the backdrop work in `modal-backdrop-passes-clicks-through-to-the-controls-underneath.md`.
   - Delete the reset. Then Ctrl+Tab with Send or Stake open would leave a user wallet modal drawn over the faucet wallet, with `balance` taken from the miner wallet and the amount buttons gated on the wrong figure.

3. **Tests.**
   - What exists: `zebra-gui` has only in-file unit tests (`mod tests` at `src/lib.rs:2401`, `mod roster_identity_tests` at `src/ui.rs:5327`). There is no `tests/` directory and no test constructs a `ui::Context` with its `draw`, `input` and `clay` pointers, so nothing can drive `run_ui` today.
   - Cheapest real check: a unit test next to `roster_identity_tests` that pins the classification, so the two wallet-independent modals cannot be swept up again.

   ```rust
   #[test]
   fn only_wallet_modals_close_on_the_faucet_tab() {
       assert!(!Modal::Jump.acts_on_user_wallet());
       assert!(!Modal::ConvertRewards.acts_on_user_wallet());
       for modal in [Modal::Send, Modal::Receive, Modal::Stake, Modal::Unstake, Modal::Retarget, Modal::User] {
           assert!(modal.acts_on_user_wallet());
       }
   }
   ```

   - Manual check (the only way to cover the two-pass ordering): select `Faucet Wallet`; press Ctrl+J, expect the Jump To Height modal; close it; click `Jump To Height...`, expect the same; close it; click `Convert Rewards`, expect the Convert Commission modal. Then on `Your Wallet` open Send and press Ctrl+Tab: expect the Send modal to close.

4. **UX after the fix.** On the Faucet Wallet tab, Ctrl+J and `Jump To Height...` show the full-window Jump To Height modal exactly as on Your Wallet. `Convert Rewards` shows the Convert Commission modal over the left pane, covering the faucet wallet's balance and history until closed. Send, Receive, Stake, Edit Stake, Retarget and User still close when the user switches to the Faucet Wallet tab.

## Validation Information

**Verdict: CONFIRMED. Severity: Low.**

| Claim | Verified at |
| :- | :- |
| The reset is unconditional on modal kind and runs every pass | `ui.rs:1620-1623`, read directly |
| Two passes per frame; press edges exist only in the first; drawing only in the second | `ui.rs:5250-5262`; `is_rendering` gate at `:5073` |
| Ctrl+J sets the modal before `ui_left_pane` in the same pass | `ui.rs:4043-4045`, `:4146` |
| The two buttons set the modal after `ui_left_pane`, and the render pass clears it before the container | `ui.rs:4531-4533`, `:3623-3625`, `:4660`, `:1690` |
| Jump and ConvertRewards do not read the selected tab | `ui.rs:1823-1898`, `:2116-2176`; `tab_id` is read in modal code only at `:1738` |
| Wallet modal buttons are hidden on the Faucet Wallet tab | `ui.rs:2865-2866` |
| Default tab is Your Wallet | `ui.rs:742-744`, `:1598-1599` |
| The reset predates both wallet-independent modals | `git log -S`: `90166a4e` (2026-01-14), `8bd28e08` (2026-05-05), `5a75c3ed` (2026-08-07), `3179fcc1` (2026-09-08) |

Everything above was verified by reading. Not run: the GUI was not launched, per the brief.

**Severity justification.**

- *Why not Medium:* the flows are blocked only while a non-default tab is selected, one click restores them, and nothing wrong is displayed or submitted. A blocked flow with no workaround (for example a modal that cannot be completed on a platform) is the Medium case; this one has an immediate workaround, though not an obvious one.
- *Why not lower:* Low is the floor. It is a real defect, not a false positive: three visible controls do nothing with no feedback, and the cause sits in a different pane from two of them.

**Corrections made during validation.**

1. The review said the modal "is set and then reset before the render pass". That is exact for the two buttons. For Ctrl+J it is reset earlier still, inside the same input pass, because the shortcut is handled before `ui_left_pane` runs.
2. The review's list of modals that should still be reset (Send, Receive, Stake, Unstake, Retarget, User) is correct. Added: those six cannot be opened from the Faucet Wallet tab anyway (`ui.rs:2865`), so for them the reset only acts on a tab switch while one is open.
3. Added the fourth variant: Ctrl+Tab closes an already open Jump or Convert Commission modal.

**Cross-references.**

- `modal-backdrop-passes-clicks-through-to-the-controls-underneath.md` (G4): same modal container (`ui.rs:1674-1702`). Independent fixes; if G4 moves the Jump modal out of `ui_left_pane`, this reset must not follow it.
- `esc-cannot-close-a-modal-after-textbox-focus-and-jump-gives-no-feedback.md` (G10): same Jump modal and Ctrl+J entry point. Independent; either order.
- `send-modal-never-validates-the-address-and-a-failed-send-is-silent.md` (G6) and `pasted-non-ascii-text-panics-the-gui-on-a-byte-index-slice.md` (G3): the wallet modals this reset is meant for. No code overlap.
