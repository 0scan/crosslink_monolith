# Esc closes a modal only while `ui.nav_enable` is false, and nothing clears `nav_enable` on Esc, so Esc is dead after a textbox click or a Tab press; the Jump To Height textbox is not focused on open, unparseable input is silently ignored, and a height above the tip moves the camera into empty space (PoW) or closes the modal with no movement (PoS)

**Severity**: Low
**Validation Status**: Confirmed
**Location**: `zebra-gui/src/ui.rs:1764-1767` (close button and the Esc test), `:771-781` (`textbox` sets `nav_id` and `nav_enable` on click), `:789` (a textbox takes keys only when `nav_id` is its id), `:646` (`key_hover` needs `nav_enable`), `:4996-5023` (end-of-frame nav bookkeeping; the commented-out Esc handler at `:5021-5023`), `:5025-5043` (Tab sets `nav_enable = true`), `:1589`, `:1601`, `:2812` (`nav_skip`), `:4043-4045` (Ctrl+J), `:4531-4533` (`Jump To Height...` button), `:1878-1896` (Jump textbox, `can_jump`, the parse), `:2128-2152` (Convert Commission textbox and its error lines, for contrast), `:3549-3557` (the seconds textbox and its digits-only filter); `zebra-gui/src/viz_gui.rs:517-529` (`goto_pow_height`), `:531-553` (`goto_pos_height`, `request_pos_height`), `:1131-1148` (pending jump requests are dropped when the chain does not reach them), `:847-849` (`is_bulk_jump`)
**Found by agent:** /code-review high, GUI/UX (Claude Fable 5.1), 2026-09-30; validated 2026-10-01 at dev d00b6a44ef594
**In scope of audit?** Yes. It is keyboard routing and input feedback in `zebra-gui` modals. The `!ui.nav_enable` condition on Esc was added by `8a138e34` ("Gui Textbox WIP: Select all on click", 2026-01-12). The Esc handler that would clear `nav_enable` has been commented out since it was first written in `545bd40d` ("Gui WIP: Tab ordering", 2025-12-23). The Jump modal is from `8bd28e08` (2026-05-05).

## Description

The review makes four claims. Each was checked separately, and each holds.

| # | Claim | Verdict |
| :- | :- | :- |
| 1 | Esc cannot close a modal once a textbox is focused | Confirmed, and wider than stated: it also holds after any Tab press, in every modal |
| 2 | The Jump textbox is not focused on open, so typed digits go nowhere | Confirmed |
| 3 | `1,000` or `12a` leaves `Jump` enabled but does nothing | Confirmed |
| 4 | A PoW height above the tip moves the camera into empty space; a PoS height above the tip closes the modal with no movement; neither shows a message | Confirmed |

**Claim 1.** `ui.nav_enable` is the flag for "keyboard navigation is active": it makes Enter act on the widget whose id is `ui.nav_id` (`ui.rs:646`) and draws the focus rectangle (`:5201`). The modal title bar closes on Esc only when that flag is false (`:1765`). The flag becomes true when a textbox is clicked (`:778-781`) or when Tab is pressed (`:5036-5037`). It becomes false only when the mouse presses a different widget (`:5005-5007`) or when the focused widget disappears (`:5017-5020`). The code that would clear it on Esc exists but is commented out (`:5021-5023`). So after a textbox click or a Tab, Esc does nothing until the user clicks some other button.

**Claim 2.** Opening the Jump modal sets `ui.modal` and nothing else (`:4044`, `:4532`). The textbox reads keys only when `ui.nav_id` equals its id (`:789`), and `nav_id` is not pointed at it.

**Claim 3.** The `Jump` button is enabled for any non-blank text (`:1886`). Activation parses the text as `u64` and does nothing at all if the parse fails (`:1888`): no message, no change to the modal.

**Claim 4.** A parsed height is not compared with any tip. The PoW path moves the camera straight to `y_for_height(h)`. The PoS path looks for a loaded BFT block at that height, finds none, records a pending request, and the modal closes either way (`:1894`).

## Attack Scenario and Steps

There is no attacker. These are reproductions.

*Claim 1, textbox.*

1. Press Ctrl+J. Press Esc: the modal closes. Good.
2. Press Ctrl+J again. Click the height textbox. Press Esc: nothing happens.
3. Same in Convert Commission (right pane, `Convert Rewards`): click the amount textbox, press Esc, nothing happens.

*Claim 1, Tab (any modal).*

1. Open Send. Press Tab once: a focus rectangle appears on the close button. Press Esc: nothing happens.

*Claim 2.*

1. Press Ctrl+J and type `1234`. The textbox still shows its hint, "Enter PoW height...".

*Claim 3.*

1. Ctrl+J, click the textbox, type `1,000`, click `Jump` or press Enter. Nothing happens; the modal stays as it was.

*Claim 4.*

1. Ctrl+J, click the textbox, type a height well above the current PoW tip, press Enter. The modal closes, the view shows empty space, and the `Follow Tip` lock is open.
2. Ctrl+J, select `PoS`, type a height above the BFT tip, press Enter. The modal closes and the view does not move.

**Attack Requirements and Assumptions:**

- All platforms: the logic is in `ui.rs` and `viz_gui.rs`, with no backend dependence.
- "Your Wallet" tab selected, since no modal opens on the Faucet Wallet tab (see the G5 file).
- Claim 1 with a textbox applies to the two modals that have a live one: Jump (`ui.rs:1879`) and Convert Commission (`:2134`). The textboxes in Send, Receive and Stake sit in `else` branches of `if true` (`:1916`/`:1927`, `:2073`/`:2075`, `:2181`/`:2192`) and are never built.
- Claim 1 with Tab applies to all eight modals, which share one `title_bar` closure.

## Impact on Users

- **Esc appears broken.** The user clicks into the only input of a modal, changes their mind, presses Esc, and nothing happens. The ways out are the close button or a click on the backdrop. Keyboard users who Tab into a modal lose Esc in every modal.
- **Ctrl+J then typing does nothing.** A keyboard shortcut that opens a one-field dialog and then ignores the keyboard defeats the shortcut. Reaching the field by keyboard takes four Tab presses (close button, `PoW`, `PoS`, then the textbox), and that Tab kills Esc.
- **Typed keys can land in another textbox (edge case, verified by reading).** If the "Connected less than" textbox in Finalizer Filters was focused before Ctrl+J, `nav_id` still points at it, and it stays in the nav list because the right pane is not `nav_skip`ped. Digits typed for the jump go into the finalizer filter, changing which finalizers show as online.
- **Silent rejects.** `1,000`, `12a`, `1 000`, a negative number, or a number too large for `u64` leave the `Jump` button lit and do nothing. The user cannot tell whether the jump is loading or was refused.
- **Stranded view.** A PoW height above the tip leaves the camera in empty space at zoom 2.0 with follow-tip off, and the modal that would explain it is gone. Recovery is `Reset View` or the `Follow Tip` lock.
- **Silent no-op for PoS.** A PoS height above the tip closes the modal as if it had worked.
- No funds or stake figures are involved. No flow is blocked: every modal can still be closed with the mouse.

## Technical Details / Code Analysis

**Esc is conditional on `nav_enable`** (`zebra-gui/src/ui.rs:1764-1767`):

```rust
let (clicked, colour, _) = ui.button_ex(false, BUTTON_GREY, id, true, CursorIcon::Default);
if clicked || (ui.input().key_pressed(KEY_ESC) && !ui.nav_enable) {
    ui.modal = Modal::None;
}
```

**A textbox click sets it** (`ui.rs:776-781`):

```rust
let (activated, colour, text_colour) = self.button_ex(true, BUTTON_GREY, id, true, CursorIcon::Text);

if activated {
    self.nav_id = id.id;
    self.nav_enable = true;
}
```

**The end-of-frame bookkeeping, with the Esc handler commented out** (`ui.rs:4996-5023`):

```rust
if !ui.input().mouse_held(BTN_LEFT) {
    ui.mouse_pressed_id = Id::default();
}
if ui.mouse_pressed_id == Id::default() {
    if ui.input().mouse_pressed(BTN_LEFT) {
        // ui.nav_enable = false;
    }
} else {
    // ui.most_recent_mouse_pressed_id = ui.mouse_pressed_id;
    if ui.nav_id != ui.mouse_pressed_id.id {
        ui.nav_enable = false;
    }
    ui.nav_id = ui.mouse_pressed_id.id;
    if ui.mouse_pressed_id != Id::VIZ_GUI
        && ui.mouse_pressed_id != Id::CHAIN_MINIMAP_POW
        && ui.mouse_pressed_id != Id::CHAIN_MINIMAP_POS
    {
        ui.capture = true;
    }
}

if !ui.nav_id_to_idx.contains_key(&ui.nav_id) {
    ui.nav_id = 0;
    ui.nav_enable = false;
}
// if ui.input().key_pressed(KEY_ESC) {
//     ui.nav_enable = false;
// }
```

Walking a textbox click through it:

- Press frame: `button_ex` sets `mouse_pressed_id` to the textbox id and returns `activated`; `nav_id` = textbox id, `nav_enable = true`. At the end of the pass `nav_id == mouse_pressed_id.id`, so the `nav_enable = false` line is skipped. The textbox registered itself in `nav_id_to_idx` (`ui.rs:699-704`), so the reset at `:5017` is skipped too.
- Later frames: nothing touches `nav_enable`. A press on empty modal padding leaves `mouse_pressed_id` at default, and the line that would clear the flag there is commented out (`:5001`).
- Esc frame: `:1765` sees `nav_enable == true` and does not close. Nothing else reacts to Esc; `KEY_ESC` appears in the crate only at `:1765`, in the comment at `:5021`, and in its definition (`lib.rs:88`).
- What re-enables Esc: a mouse press on any other `button_ex` widget, for example the `PoW` pill, because then `nav_id != mouse_pressed_id.id`.

Tab sets the flag unconditionally (`ui.rs:5036-5037`):

```rust
ui.nav_id = ui.nav_idx_to_id[idx];
ui.nav_enable = true;
```

**Opening Jump does not focus the textbox** (`ui.rs:4043-4045`, and the button at `:4531-4533`):

```rust
if ctrl_held && ui.input().key_pressed(KEY_J) {
    ui.modal = Modal::Jump;
}
```

The textbox consumes input only inside `if self.nav_id == id.id {` (`ui.rs:789`). After Ctrl+J, `nav_id` is whatever it was. After the button, `nav_id` becomes the button's id (`:5008`).

**The parse and the missing feedback** (`ui.rs:1886-1896`):

```rust
let can_jump = jump_text.trim().len() > 0;
if button_ex(ui, id("Jump To Selected Height"), "Jump", can_jump) || (ui.input().key_pressed(KEY_ENTER) && ui.nav_id == jump_input_id.id) {
    if let Ok(h) = jump_text.trim().parse::<u64>() {
        if data.jump_target_pos {
            viz.request_pos_height(h);
        } else {
            viz.goto_pow_height(h);
        }
        ui.modal = Modal::None;
    }
}
```

There is no `else`. Compare the Convert Commission modal in the same function, which prints "Enter a number of cTAZ, e.g. 1.5" and disables its button on a bad parse (`ui.rs:2142-2152`). The text input path filters control characters only (`lib.rs:1378-1380`), so letters and commas reach the buffer.

**PoW: the camera moves first and asks questions later** (`zebra-gui/src/viz_gui.rs:517-529`):

```rust
pub fn goto_pow_height(&mut self, h: u64) {
    self.follow_tip = false;
    self.camera_y = y_for_height(h as f32);
    self.zoom = 2.0;
    // An explicit jump supersedes the startup camera placement.
    self.did_initial_tip_jump = true;
    // The camera can point anywhere; the page holding the target is asked for
    // until it arrives (or the chain turns out not to reach it). A page is named
    // by the height it ends at, and the page ending at 1 includes genesis.
    if !self.bc_best_heights.contains(&h) {
        self.bc_wanted_page_end = h.max(1);
    }
}
```

The page request is then dropped once the tip is known to be below it (`viz_gui.rs:1133-1138`). The camera is not moved back.

**PoS: nothing moves** (`viz_gui.rs:550-553`, `:1142-1148`):

```rust
pub fn request_pos_height(&mut self, h: u64) {
    self.bft_wanted_jump = if self.goto_pos_height(h) { u64::MAX } else { h };
}
```

```rust
if viz_state.bft_wanted_jump != u64::MAX {
    if viz_state.goto_pos_height(viz_state.bft_wanted_jump) {
        viz_state.bft_wanted_jump = u64::MAX;
    } else if viz_state.bft_tip_height > 0 && viz_state.bft_tip_height < viz_state.bft_wanted_jump {
        viz_state.bft_wanted_jump = u64::MAX;
    }
}
```

`goto_pos_height` returns false when no loaded BFT block has that height (`:531-538`). The request is abandoned on the next update when the BFT tip is known and below it. Inferred from the same lines, not exercised: while `bft_tip_height` is still 0, the request stays pending, `is_bulk_jump` (`:847-849`) stays true, and the camera jumps whenever a block at that height first appears, possibly long after the user asked.

**Is any of it deliberate?**

- *Esc:* partly. `8a138e34` added `&& !ui.nav_enable` on purpose, in a commit about textbox editing, and `545bd40d` wrote, already commented out, the handler that makes Esc leave keyboard navigation. Together they sketch a two-stage Esc (first press leaves the field, second closes the modal) of which only the first half was switched on. Both commits are titled WIP.
- *PoW above the tip:* the comment in `goto_pow_height` ("The camera can point anywhere ... or the chain turns out not to reach it") shows the author knew the camera can end up beyond the chain and accepted it. The missing message is not discussed anywhere.
- *Focus on open, parse feedback, PoS no-op:* no comment, TODO or commit message mentions them.

## Recommendations

1. **Make Esc always close the modal, and make Esc leave keyboard focus.**
   - `ui.rs:1765`: drop the `nav_enable` condition.

   ```rust
   if clicked || ui.input().key_pressed(KEY_ESC) {
       ui.modal = Modal::None;
   }
   ```

   - `ui.rs:5021-5023`: replace the commented block with a live one that also drops text focus, so that Esc outside a modal (the Finalizer Filters seconds textbox) stops keystrokes going into the field.

   ```rust
   if ui.input().key_pressed(KEY_ESC) {
       ui.nav_id = 0;
       ui.nav_enable = false;
   }
   ```

   - UX: Esc closes any modal in one press, whatever was clicked or tabbed before. With no modal open, Esc removes the caret or the focus rectangle.
   - *Alternative, not chosen:* the two-stage Esc the history points at (uncomment `:5021-5023` only). It needs two presses to close a one-field dialog, and the first press gives no visible sign in a textbox that it did anything.

2. **Focus the Jump textbox when the modal opens.** Depends on item 1: with focus set on open and the old Esc condition, Esc would be dead from the first frame.
   - Add one field to `Context`, `pub focus_request: u32` (0 = none), next to `nav_id`.
   - Set it where the modal is opened, `ui.rs:4044` and `:4532`:

   ```rust
   ui.modal = Modal::Jump;
   ui.focus_request = id("Goto Height Input").id;
   ```

   - Consume it in `textbox`, after the `activated` block at `ui.rs:778-781`:

   ```rust
   // Wait for the opening click to be released: while the button is held, the
   // end-of-frame bookkeeping keeps pointing nav_id at the pressed widget.
   if self.focus_request == id.id && !self.input().mouse_held(BTN_LEFT) {
       self.focus_request = 0;
       self.nav_id = id.id;
       self.nav_enable = true;
       let textbox_state = data.textboxes.entry(id.id).or_default();
       textbox_state.selection.1 = 0;
       textbox_state.selection.0 = textbox_state.text_buf.len();
   }
   ```

   - The same request can be set for `"Convert Amount Textbox"` at `ui.rs:3624`.
   - To check when implementing (not verified here): whether `softer_gui` delivers an `EVENT_TEXT` for the `j` of Ctrl+J. If it does, the newly focused textbox would receive a `j` in the same pass. The digits-only filter in item 3 removes it; without that filter, skip text input on the pass that consumes the request.
   - UX: Ctrl+J shows the modal with the caret in the field and any previous text selected, so typing replaces it. Enter jumps, Esc closes.
   - *Alternative, not chosen:* set `nav_id` directly at the open sites. On the button path the textbox is not laid out in that pass, so `:5017-5020` resets `nav_id` to 0, and `:5008` keeps overwriting it while the mouse is held.

3. **Validate the height, say why it is refused, and keep the modal open.**
   - Add a pure function near `parse_ctaz` (`ui.rs:70`):

   ```rust
   /// `tip` is 0 when the tip is not known yet; then any height is accepted.
   pub fn parse_jump_height(text: &str, tip: u64) -> Result<u64, String> {
       let text = text.trim();
       let Ok(h) = text.parse::<u64>() else {
           return Err(format!("\"{text}\" is not a height. Digits only, e.g. 1000"));
       };
       if tip > 0 && h > tip {
           return Err(format!("{h} is above the tip ({tip})"));
       }
       Ok(h)
   }
   ```

   - In the Jump arm, pick the tip for the selected chain (`viz.bc_tip_height` or `viz.bft_tip_height`), compute the result once per pass, enable `Jump` only on `Ok`, and under the textbox show the `Err` text when the field is not empty, in the warning colour the Convert Commission modal uses (`(0xff, 0xaf, 0x0e, 0xff)`, `ui.rs:2147`). On Enter with an `Err`, do nothing; the message is already on screen.
   - Optionally also apply the digits-only filter the seconds textbox uses (`ui.rs:3557`, `text_buf.retain(..)`), so that letters never appear. The message is still needed for the range check and for overflow.
   - UX:
     - empty field: `Jump` greyed, no message;
     - `12a`: `Jump` greyed, line reads `"12a" is not a height. Digits only, e.g. 1000`;
     - PoW tip 5000, input `9000`: `Jump` greyed, line reads `9000 is above the tip (5000)`;
     - valid height: no message, `Jump` lit, Enter or click jumps and closes as today.
   - *Alternative, not chosen:* clamp to the tip silently. The user asked for a height that does not exist; telling them is more useful than taking them somewhere else.
   - *Alternative, not chosen:* accept `1,000` by stripping separators. It invites locale questions (`1.000`), and the message already tells the user what to type.

4. **Tests.**
   - *What exists:* unit tests only: `lib.rs:2400` (`mod tests`: render worker shutdown, `apply_mouse_snapshot` edges) and `ui.rs:5326` (`roster_identity_tests`, a pure function of the roster UI). Nothing drives `run_ui`; it needs a `DrawCtx` that is built only inside `main_thread_run_program` (`lib.rs:1280`).
   - *Item 3, mechanical:* a unit test beside `roster_identity_tests` that calls the real `parse_jump_height`: `""`, `"12a"`, `"1,000"`, `"-1"` and a 21-digit number are `Err`; `" 42 "` with tip 100 is `Ok(42)`; `"101"` with tip 100 is `Err`; `"101"` with tip 0 is `Ok(101)`.
   - *Items 1 and 2:* no mechanical route without a headless `run_ui` fixture, which is new infrastructure. Manual check:
     1. Ctrl+J, type `25`, Enter. Expected: digits appear as typed; the view jumps to height 25; the modal closes.
     2. Ctrl+J, Esc. Expected: closes.
     3. Ctrl+J, click the textbox, Esc. Expected: closes.
     4. Open Send, Tab, Esc. Expected: closes.
     5. Click `Jump To Height...` with the mouse, then type. Expected: digits appear without a second click.
     6. Open Finalizer Filters, click the seconds textbox, Esc, type `9`. Expected: the caret disappears on Esc and the `9` is not entered.
     7. Ctrl+J, type a height above the tip, for both `PoW` and `PoS`. Expected: the message line, `Jump` greyed, the modal stays, the view does not move.

## Validation Information

**Verdict: CONFIRMED. Severity: Low.**

| Claim | Verified at |
| :- | :- |
| Esc closes only when `!ui.nav_enable` | `ui.rs:1765` |
| Clicking a textbox sets `nav_enable = true` | `ui.rs:776-781` |
| The only code that would clear it on Esc is commented out | `ui.rs:5021-5023`; `KEY_ESC` has no other use (`ui.rs:1765`, `lib.rs:88`) |
| Tab sets `nav_enable = true` | `ui.rs:5025-5037` |
| What does clear it | `ui.rs:5005-5007`, `:5017-5020` |
| Live textboxes in modals: Jump and Convert Commission only | `ui.rs:1879`, `:2134`; dead branches at `:1916`/`:1927`, `:2073`/`:2075`, `:2181`/`:2192` |
| Ctrl+J and the button open Jump without focusing the input | `ui.rs:4043-4045`, `:4531-4533`; textbox input gate `:789` |
| `1,000` or `12a` leaves `Jump` enabled and does nothing | `ui.rs:1886-1896` |
| PoW above the tip moves the camera into empty space | `viz_gui.rs:517-529`, `:1133-1138` |
| PoS above the tip closes the modal with no movement | `ui.rs:1890`, `:1894`; `viz_gui.rs:531-553`, `:1142-1148` |
| Neither shows a message | no text element in the Jump arm besides the two pills and the textbox, `ui.rs:1823-1898` |

**Severity justification.**

- *Why not Medium:* nothing is blocked. Every modal still closes with the close button or a backdrop click, and the jump works once the user clicks the field and types plain digits. No money or stake figure is involved and nothing crashes. By the rubric this is confusing behaviour.
- *Why not lower:* Low is the floor. All four behaviours reproduce deterministically on every platform, and the first two meet the user on the very first use of Ctrl+J.

**Corrections made during validation.**

1. The review cites the commented-out code at `ui.rs:5021`. That is right; a second commented-out line at `:5001` (clear on a click that hits no widget) belongs to the same unfinished design.
2. The review limits the Esc problem to "the Jump and Convert Commission modals". Those are the only two with a live textbox, but Tab sets `nav_enable` as well, so after one Tab press Esc is dead in all eight modals.
3. "Esc is dead after the first click or Tab" is accurate with one qualification: a mouse press on any other button clears `nav_enable`, and Esc works again until the next textbox click or Tab.
4. The parse is at `ui.rs:1888`, as cited.
5. Added: after Ctrl+J, typed digits can go into the Finalizer Filters seconds textbox if that had focus, since `nav_id` is left where it was.
6. Added (inferred): with no BFT tip known yet, a PoS jump request stays pending and can move the camera later.
7. Added: the Esc condition is half of a deliberate, unfinished two-stage design (`8a138e34`, `545bd40d`), not an accident.

**Cross-references.**

- `modal-backdrop-passes-clicks-through-to-the-controls-underneath.md` (G4): quotes the same `title_bar` lines. With Esc dead, users dismiss by clicking the backdrop, which is the trigger for G4. Both findings edit `ui.rs:1764-1773`; the changes do not conflict but will touch adjacent lines.
- `chain-view-pans-and-recenters-while-the-pointer-is-over-a-pane-or-modal.md` (G9): `ui.capture` is false over the Jump modal's contents (`ui.rs:1744`), so the click needed today to focus the textbox can also inspect or recenter a block behind the modal. Item 2 here removes the need for that click; G9's item 3 fixes the cause.
- `faucet-wallet-tab-resets-every-modal-so-jump-and-convert-rewards-never-open.md` (G5): Ctrl+J and `Jump To Height...` do nothing on the Faucet Wallet tab. Item 2's `focus_request` would be left set in that case; it is harmless (it is consumed the next time the textbox is built), but if G5 lands first there is nothing to consider.
- `opening-finalizer-filters-zeroes-the-seconds-filter-and-marks-everyone-online.md` (G7): the same seconds textbox that can swallow digits typed after Ctrl+J (correction 5).
