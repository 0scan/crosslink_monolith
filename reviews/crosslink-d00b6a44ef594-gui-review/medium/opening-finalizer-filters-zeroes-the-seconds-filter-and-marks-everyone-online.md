# Opening the `Finalizer Filters` popup overwrites `filter_seconds_since_connected` (default 15) with 0, because the textbox's intended `15` seed is dead code behind `ui.textbox`'s `or_default()`, so every finalizer with a status entry, including ones never connected to, is shown ONLINE until the GUI restarts

**Severity**: Medium
**Validation Status**: Confirmed
**Location**: `zebra-gui/src/ui.rs:3549-3567` (the textbox, the dead `or_insert_with`, and the branch that writes 0); `zebra-gui/src/ui.rs:784` (`ui.textbox` creates the entry with `or_default()`); `zebra-gui/src/ui.rs:896-897` (empty text draws the hint); `zebra-gui/src/ui.rs:3515-3518` (popup toggle); `zebra-gui/src/ui.rs:3392-3398` and `:3992-3994` (filters copied out of and back into `WalletState` every pass); `wallet/src/lib.rs:1010-1026` (`FinalizerFilters`, default 15); `zebra-gui/src/ui.rs:1380-1413` (`finalizer_is_online_ex`, `finalizer_is_online`); readers of the filter: `zebra-gui/src/ui.rs:1443`, `:2569`, `:2592`, `:3645`, `:3672`, `:3760`; `zebra-crosslink/zebra-state/src/new_network/bft.rs:1546-1598` (`recency_status_from`, what a status entry means); `librustzcash/zcash_primitives/src/bft.rs:1226-1233` (`FinalizerRecencyStatus`)
**Found by agent:** /code-review high, GUI/UX (Claude Fable 5.1), 2026-09-30; validated 2026-10-01 at dev d00b6a44ef594
**In scope of audit?** Yes. It is a GUI display that changes meaning as a side effect of opening a popup. Introduced by `76c73104` ("Gui: Tweak default filters", 2026-08-26), which changed the default from `filter_height: true, filter_seconds_since_connected: 0` to `false, 15` and added the `['1', '5']` seed. The seed was already unreachable in that commit: `ui.textbox` used `or_default()` there too.

## Description

The right pane marks each finalizer ONLINE or offline. The rule is `finalizer_is_online_ex`, driven by two filters held in `WalletState::filters`: "voted at the latest height" (`filter_height`, default off) and "connected less than N seconds ago" (`filter_seconds_since_connected`, default 15, where 0 means "ignore connection time").

The seconds value is edited through a textbox inside the `Finalizer Filters` popup. The textbox's text lives in `data.textboxes`, keyed by widget id, and the popup code copies the parsed text into the filter on every pass while the popup is shown. The author meant the textbox to start out reading `15` to match the default (`ui.rs:3556`), but that line can never insert anything: six lines earlier `ui.textbox` has already created the entry, empty, with `or_default()`.

The result is that the textbox starts empty, and empty is parsed as "no time filter". So the very act of opening the popup replaces 15 with 0. With `filter_height` also off by default, both filters are now off and `finalizer_is_online_ex` returns `true` for anything it is asked about. Closing the popup stops the copy but does not put 15 back. The value stays 0 for the rest of the session.

## Attack Scenario and Steps

This is a reproduction, not an attack.

1. Start the GUI on a network where at least one roster finalizer is not reachable from this node (down, or simply never dialled). In the Finalizers pane it shows the dimmed colour chip with the attention icon, and the upper ratio bar shows part of the stake as offline.
2. Click `Finalizer Filters`. Type nothing.
3. Observe, in the same frame: the unreachable finalizer's chip turns saturated with the wifi icon, its label turns white, and the upper ratio bar shows all of that stake as online. The seconds box in the popup shows a dim `∞`, not `15`.
4. Click `Finalizer Filters` again to close the popup. The finalizers stay ONLINE.
5. Open `Edit Stake` in the left pane: the same finalizers are shown online there too.
6. The 15 second filter comes back only if the user reopens the popup and types `15`, or restarts the GUI.

**Attack Requirements and Assumptions:**

- All platforms (Windows, macOS, Linux X11, Linux Wayland). Pure UI state logic.
- The finalizer must have a status entry, which means it is in the active roster of a round this node's BFT state currently holds (see Technical Details). Finalizers with no entry stay offline whatever the filters say.
- `filter_height` must be off, which is the default. If the user ticks "Voted at latest height", that filter still applies and the effect is limited to losing the time filter.
- One click on a button labelled as a filter control. No typing is needed.

## Impact on Users

- **Liveness display is wrong after a harmless-looking click.** A finalizer this node has never exchanged a packet with is drawn exactly like a healthy one: wifi icon, bright chip, counted in the online share of stake. The roster tooltip still says "Never connected to this finalizer during this session" on hover (`ui.rs:3818-3820`), so the row contradicts its own tooltip.
- **The online/offline stake bar overstates online stake.** `online_stake` (`ui.rs:3639-3648`) sums the voting power of every finalizer that passes the filter. With both filters off that is every active-roster finalizer with an entry. A participant reading the bar to judge whether the network can finalize sees a healthier picture than the node has evidence for.
- **Staking choices lean on it.** The Edit Stake modal shows the same mark next to the user's own bonds (`ui.rs:2569`, `:2592`). A user deciding whether to retarget a bond away from a dead finalizer is told it is online. This is an inference about how the display is used; no figure for balance or bond amount is affected.
- **It is sticky and unannounced.** Nothing outside the popup shows what the filter currently is, so after closing it the user has no cue that the meaning of ONLINE changed. It lasts until restart (`FinalizerFilters` is not persisted; it is rebuilt from `Default`).
- **Not affected:** balances, bond amounts, transactions, consensus. Nothing is submitted.

## Technical Details / Code Analysis

**The default** (`wallet/src/lib.rs:1017-1026`):

```rust
impl Default for FinalizerFilters {
    fn default() -> Self {
        Self {
            show_popup: false,
            popup_pos: None,
            filter_height: false,
            filter_seconds_since_connected: 15,
        }
    }
}
```

**`ui.textbox` creates the state entry itself** (`zebra-gui/src/ui.rs:783-787`):

```rust
let text = {
    let mut textbox_state = &mut data.textboxes.entry(id.id).or_default();

    textbox_state.h = text_decl.h;
    textbox_state.font = text_decl.font;
```

It then returns the buffer's contents as a `String` (`:878-881`, `:904`). For a new entry that is the empty string. Nothing else inserts into `data.textboxes` for this id: the other `entry(..)` calls are for the Send, Receive and Stake address boxes (`:1932`, `:1955`, `:2080`, `:2099`, `:2197`, `:2220`), and Tab navigation uses `get_mut` (`:5039`).

**The popup code** (`zebra-gui/src/ui.rs:3549-3567`):

```rust
let textbox_id = ui::id("Seconds since connected Textbox");
let secs_string = ui.textbox(
    data,
    textbox_id,
    "∞",
    TextDecl { h: ui.scale(14.0), colour: WHITE, align: AlignX::Left, ..TextDecl },
);
let textbox_state = data.textboxes.entry(textbox_id.id).or_insert_with(|| TextboxState { text_buf: vec!['1', '5'], ..Default::default() });
textbox_state.text_buf.retain(|c| (*c >= '0' && *c <= '9') || *c == '∞');
let len = textbox_state.text_buf.len();
textbox_state.selection.0 = textbox_state.selection.0.min(len); // @Todo: this is very likely not the best thing to do here
textbox_state.selection.1 = textbox_state.selection.1.min(len); // @Todo: this is very likely not the best thing to do here
let secs_str = secs_string.trim();
if let Ok(value) = secs_str.parse::<u32>(){
    filters.filter_seconds_since_connected = value;
    // println!("{:?}", state.finalizer_seconds_since_connected);
} else if secs_str.len() == 0 || secs_str == "∞" {
    filters.filter_seconds_since_connected = 0;
}
```

The `or_insert_with` closure runs only if the key is absent. `ui.textbox`, called just above with the same id, has always inserted it. So the closure is unreachable and `text_buf` is never `['1', '5']`.

**The first frame the popup is shown.** `ui_update` runs `run_ui` twice per frame, an input pass with the real input and then a render pass (`ui.rs:5260-5261`). Walking the click:

| Pass | Step | State after |
| :- | :- | :- |
| input | `filters` copied from `WalletState` (`:3392-3396`) | seconds = 15 |
| input | button is act-on-press; `clicked` toggles `show_popup` (`:3499`, `:3515-3517`) | popup shown |
| input | `if filters.show_popup` body runs in the same pass (`:3518`); `ui.textbox` inserts an empty entry and returns `""` | text empty |
| input | `"".parse::<u32>()` fails; `secs_str.len() == 0` | seconds = 0 |
| input | roster and bars below evaluate with the local `filters` (`:3645`, `:3672`, `:3760`) | all with an entry are online |
| input | `filters` written back (`:3992-3994`; `filters_modified` is initialised `true` and never cleared) | `WalletState.filters.filter_seconds_since_connected = 0` |
| render | same again from the stored 0; textbox draws the hint `∞` because the text is empty (`:896-897`) | user sees `∞`, never `15` |

So the overwrite happens in the input pass of the click frame itself. There is no frame in which the popup shows 15.

**After the popup closes.** The block at `:3518` is skipped, so nothing writes the seconds value again. It stays at 0 in `WalletState`. `FinalizerFilters` derives only `Debug, Clone, Copy` and is not saved anywhere, so a restart restores 15.

**What 0 does** (`zebra-gui/src/ui.rs:1380-1403`):

```rust
fn finalizer_is_online_ex(f: &FinalizerRecencyStatus, bft_status: &wallet::TFLRecencyStatus, filters: &FinalizerFilters) -> bool {
    let mut ok: bool = true;
    if filters.filter_height {
        ok &= f.no_yes_votes_in_my_height[0][0]
            + f.no_yes_votes_in_my_height[0][1]
            + f.no_yes_votes_in_my_height[1][0]
            + f.no_yes_votes_in_my_height[1][1] > 0;
    }

    if filters.filter_seconds_since_connected != 0 {
        let Some(last_direct_connection_utc) = f.last_direct_connection_utc else {
            return false;
        };

        if bft_status.now_utc < last_direct_connection_utc {
            println!("Saw direct connection in future: now: {}; seen: {}", bft_status.now_utc, last_direct_connection_utc);
            return false;
        }

        let secs_since_direct_connection = bft_status.now_utc - last_direct_connection_utc;
        ok &= secs_since_direct_connection <= filters.filter_seconds_since_connected as i64;
    }
    ok
}
```

With `filter_height == false` and the seconds value 0, neither block runs and the function returns `true` without looking at `f`. The `None` check that makes a never-connected finalizer offline is inside the block that 0 disables.

**Every reader of the filter values:**

| Site | What it draws | Extra gate |
| :- | :- | :- |
| `ui.rs:3645` | `online_stake`, which sizes the online/offline bar (`Right Pane Ratio Bar 1`, `:3670`) | terminated finalizers excluded (`:3633-3637`) |
| `ui.rs:3672` then `:1443` | per-finalizer colours in `Right Pane Ratio Bar 2` | terminated excluded |
| `ui.rs:3760` | roster row: wifi or attention icon, text colour, chip colour (`:3765-3783`) | terminated forced offline (`:3757-3758`); no entry is offline (`:3761-3762`) |
| `ui.rs:2569` then `:1443` | `Left Pane Ratio Bar` in the Edit Stake modal | none |
| `ui.rs:2592` | per-finalizer mark beside the user's bonds in the Edit Stake modal | no entry is offline |

The left pane reads `state.filters` at `:1615-1618`, so it picks up the 0 one pass later. `Right Pane Ratio Bar 1` is passed `FinalizerFilters::default()` (`:3670`), but with `is_real_finalizers == false` that argument is unused; the bar's two segments are `online_stake` and `offline_stake`, which were computed with the live `filters`.

**What a status entry means for a finalizer that never connected** (`zebra-state/src/new_network/bft.rs:1550-1583`):

```rust
for round in &bft_state.rounds_data {
    let is_my_height = round.height == bft_state.height;

    // The vote arrays are sized to the *active* roster (the top ACTIVE_ROSTER_MAX_N by
    // stake); members past that have no slot.
    let active_n = round.msg_val_sigs.len().min(round.msg_nil_sigs.len());
    for (roster_i, member) in round.roster.iter().take(active_n).enumerate() {
        let st = if let Some(v) = finalizer_statuses.iter_mut().find(|(key, _st)| *key == member.pub_key) {
            v
        } else {
            let last_i = finalizer_statuses.len();
            finalizer_statuses.push((member.pub_key, FinalizerRecencyStatus::default()));
            &mut finalizer_statuses[last_i]
        };
```

An entry is pushed for every member of the active roster of every round the BFT state holds, before any evidence about that member is looked at. For a member that has never sent this node a packet, `last_packet_utcs` has no key, and `st.1.last_direct_connection_utc` stays `None` (`:1579-1580`); if it has not voted at this height the vote counts stay zero. So "has a status entry" means only "is in the active roster of a round this node knows". It carries no liveness information. With both filters off, that is the whole test for ONLINE.

Finalizers outside the active roster, or any finalizer while `rounds_data` is empty, have no entry and stay offline at every reader (`finalizer_is_online` returns `false` at `:1407-1410`; the two `_ex` call sites have an `else { false }`).

**Is it deliberate?** The semantics "0 or empty means ignore connection time" are deliberate: the popup's tooltip says "Set to 0/empty to ignore connection time" (`ui.rs:3571-3573`, from `166a8d9f`). The loss of the default is not: `76c73104` changed the default to 15 and in the same commit replaced `or_default()` with the `['1', '5']` seed on this line, which shows the author wanted the box to open reading 15. Before that commit the default was 0, so an empty box and the default agreed and opening the popup changed nothing.

## Recommendations

1. **Seed the textbox from the current filter value before `ui.textbox` runs, and delete the dead seed.** In `ui_right_pane`, `zebra-gui/src/ui.rs:3549-3556`. Seeding from `filters.filter_seconds_since_connected` instead of a literal keeps the box and the filter in agreement whatever the default becomes.

   ```rust
   let textbox_id = ui::id("Seconds since connected Textbox");
   // ui.textbox creates a missing entry empty, and empty parses as "no time filter",
   // so the entry must exist with the current value before the first call.
   if !data.textboxes.contains_key(&textbox_id.id) {
       let mut text_buf = Vec::new();
       if filters.filter_seconds_since_connected != 0 {
           text_buf.extend(filters.filter_seconds_since_connected.to_string().chars());
       }
       data.textboxes.insert(textbox_id.id, TextboxState { text_buf, ..Default::default() });
   }
   let secs_string = ui.textbox(
       data,
       textbox_id,
       "∞",
       TextDecl { h: ui.scale(14.0), colour: WHITE, align: AlignX::Left, ..TextDecl },
   );
   let textbox_state = data.textboxes.entry(textbox_id.id).or_default();
   ```

   The rest of the block (`retain`, selection clamp, parse) is unchanged. With the entry seeded, the first pass parses `15` and writes 15 back: opening the popup becomes a no-op.

2. **Show the active filter on the popup button, so the meaning of ONLINE is visible with the popup closed.** At `ui.rs:3500`, build the label from `filters`: `Finalizer Filters (15 s)`, plus `, voted` when `filter_height` is on, and `Finalizer Filters (off)` when both are off. This is the piece that makes any future drift between the stored filter and what the user believes visible. It is separately landable and optional; item 1 alone removes the defect.

3. **Decide what ONLINE means when both filters are off (owner's call, not a bug fix).** Today it means "in the active roster". Two coherent options: keep it, on the grounds that the tooltip documents 0 as "ignore"; or move the `last_direct_connection_utc == None` check out of the seconds block in `finalizer_is_online_ex` so a finalizer never heard from is never ONLINE. The second changes documented behaviour, so it should not ride along with item 1.

4. **Considered alternatives, not chosen.**
   - Give `ui.textbox` an initial-text parameter. Right place in principle, but it changes the signature for every caller (Jump, Convert Commission, the address boxes) to serve one.
   - Only write the filter when the text changed since the last pass. Fixes the overwrite but leaves the box showing `∞` while the filter is 15, which is a different lie.
   - Treat empty as "keep the current value" and require an explicit `0` or `∞` to disable. Contradicts the tooltip and makes clearing the box do nothing visible.

5. **Tests.**
   - What exists: `zebra-gui` has only in-file unit tests (`src/lib.rs:2401`, `src/ui.rs:5327`). No test builds a `ui::Context` with a draw context, input and Clay arena, so `ui_right_pane` cannot be driven mechanically today; that would need new infrastructure.
   - Cheapest real check for item 1: a unit test in `ui.rs` that seeds a `UiData` through the same lines (lifted into a small `fn seed_textbox(data: &mut UiData, id: Id, text: &str)` if a seam is wanted), then asserts the entry's `text_buf` is `['1', '5']` and that `entry(id).or_default()`, which is what `ui.textbox` does, leaves it intact.
   - A unit test pinning the rule the display depends on, using the real `finalizer_is_online_ex`: a `FinalizerRecencyStatus::default()` (never connected) is offline under `FinalizerFilters::default()`, and online only when `filter_seconds_since_connected == 0` and `filter_height == false`. If item 3 is taken, this test changes with it.
   - Manual check: start the GUI with an unreachable roster finalizer shown offline. Click `Finalizer Filters`. Expected: the box reads `15`, and no row, chip or bar changes. Clear the box: expected `∞` hint and the finalizer turns ONLINE. Type `15`: it turns offline again. Close the popup: no change.

6. **UX after the fix.** Opening the popup shows `Connected less than: 15 seconds ago` with `15` as editable text. Nothing in the roster or the ratio bars changes on open or close. Clearing the box shows the dim `∞` hint and disables the time filter, as the tooltip says. With item 2, the button itself reads `Finalizer Filters (15 s)` at all times.

## Validation Information

**Verdict: CONFIRMED. Severity: Medium.**

| Claim | Verified at |
| :- | :- |
| `ui.textbox` inserts the entry with `or_default()` before the popup code reaches its `or_insert_with` | `ui.rs:784`, `:3550-3556` |
| No other code inserts this id into `data.textboxes` | all `textboxes` uses: `ui.rs:784`, `:1932`, `:1955`, `:2080`, `:2099`, `:2197`, `:2220`, `:3556`, `:5039`, `:5151`, `:5201` |
| Empty text sets the filter to 0 | `ui.rs:3561-3567` |
| The write happens in the input pass of the click frame | `ui.rs:3499` (act on press), `:3515-3518`, `:5260-5261` |
| The value is stored every pass and never restored on close | `ui.rs:3398`, `:3518`, `:3992-3994` |
| Default is 15 with `filter_height` off; not persisted | `wallet/src/lib.rs:1010-1026` |
| Both filters off returns `true` unconditionally | `ui.rs:1380-1403` |
| Readers: right pane bars and roster, left pane Edit Stake bar and marks | `ui.rs:3645`, `:3672`, `:3760`, `:2569`, `:2592`, `:1443` |
| A status entry is created for every active-roster member of every held round, with `last_direct_connection_utc: None` until a packet is seen | `zebra-state/src/new_network/bft.rs:1550-1583`; `zcash_primitives/src/bft.rs:1226-1233` |
| The seed was dead in the commit that added it; the earlier default was 0 | `git show 76c73104` (diff of `ui.rs` and `wallet/src/lib.rs`); `git show 76c73104:zebra-gui/src/ui.rs` line 753 |

All of the above was verified by reading. Not run: the GUI was not launched, per the brief. The claim about how participants use the ONLINE mark when staking is an inference.

**Severity justification.**

- *Why not High:* no balance, bond amount or transaction is wrong, and nothing crashes. The wrong figure is a liveness indicator and an online share of stake, both advisory. While the popup is open the box honestly shows `∞`, the tooltip explains it, and the roster tooltip still reports "Never connected". Typing `15` restores the filter.
- *Why not Low:* this is not merely confusing. A status display that participants use to judge finalizer health and network liveness becomes wrong for the rest of the session after one click on a control that implies no change, with no indication once the popup is closed. On a new public testnet, where unreachable finalizers are the normal case, the affected rows are exactly the ones the display exists to flag.

**Corrections made during validation.**

1. The review said the filter is zeroed "on the first frame the popup is shown". More precisely it is zeroed in the input pass of the frame of the click, before anything is drawn, and stored to `WalletState` in that same pass.
2. "Every finalizer that has a status entry" is right, and is narrower than "every finalizer": an entry exists only for members of the active roster of a round the BFT state holds. Finalizers without an entry stay offline. Terminated finalizers stay offline in the roster list and are left out of the right pane bars.
3. Added: the effect also reaches the left pane's Edit Stake modal (`ui.rs:2569`, `:2592`), not only the right pane.
4. Added: the "0 or empty means ignore" semantics are deliberate and documented in a tooltip; only the lost default is the defect. The popup shows `∞` while open, so the state is disclosed there and hidden only after closing.
5. Added: it requires `filter_height` to be off (the default). With that box ticked, opening the popup removes the time filter but not the vote filter.

**Cross-references.**

- `esc-cannot-close-a-modal-after-textbox-focus-and-jump-gives-no-feedback.md` (G10): same `ui.textbox` widget and its focus behaviour. If G10 changes `ui.textbox`'s signature or state handling, apply item 1 on top of that; otherwise independent.
- `clipboard-has-no-windows-or-wayland-backend-so-paste-only-flows-cannot-complete.md` (G2): the `Copy finalizer data as JSON` button in the same popup (`ui.rs:3576-3580`) is one of the silent clipboard callers. No code overlap with this fix.
- `faucet-wallet-tab-resets-every-modal-so-jump-and-convert-rewards-never-open.md` (G5): unrelated code; listed only because both concern controls in the Finalizers pane.
