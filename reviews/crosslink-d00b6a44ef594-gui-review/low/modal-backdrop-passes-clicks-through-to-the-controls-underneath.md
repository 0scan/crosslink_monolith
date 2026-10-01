# Every `Floating::Root` element is declared with Clay's `PASSTHROUGH` pointer capture mode, so the full-window backdrop of the Jump To Height modal and the block inspector do not block the widgets beneath them: a click that dismisses the Jump modal also fires the `button_ex` control under the cursor, and scrolling over the inspector also scrolls the pane list behind it

**Severity**: Low
**Validation Status**: Confirmed
**Location**: `zebra-gui/src/ui.rs:404-413` (`decl`, the `Floating::Root` arm; `PASSTHROUGH` at `:412`), `:1674-1702` (the "Modal Container" declaration; `Floating::Root(0.0, 0.0)` for `Modal::Jump` at `:1675-1676`), `:1716-1734` ("Modal Contents"), `:1764-1773` (close button, Esc, and the click-background-to-exit branch), `:643-659` (`button_ex`; the unconditional `mouse_pressed_id` write at `:656`), `:554-557` (`hovered_raw`, `hovered`), `:907-914` (`slider`), `:987` (`scroll_container` wheel gate), `:2873-2913` (Send, Receive, Stake, Edit Stake, User buttons), `:3005-3025` (transaction row), `:3982-3986` (`Receive cTAZ`), `:4149-4157` and `:4622-4630` (pane dividers), `:4269-4272` (`MINING` pill), `:4507-4533` (`Jump To Height...` button), `:4667-4702` (block inspector, `Floating::Root` at `:4696`), `:4980-4987` (tooltip, `Floating::Root`), `:5260-5261` (the two `run_ui` passes); `clay-rs/clay.h:3971-4028` (`Clay_SetPointerState`), `:1889-1898` (floating elements are not children of their parent), `:2105-2110` (each floating element becomes a tree root), `:2659-2671` (root sort by `zIndex`), `:4308-4316` (`Clay_PointerOver`); `zebra-gui/src/lib.rs:1596-1600` (viz runs before `ui_update`)
**Found by agent:** /code-review high, GUI/UX (Claude Fable 5.1), 2026-09-30; validated 2026-10-01 at dev d00b6a44ef594
**In scope of audit?** Yes. It is pointer routing in `zebra-gui`: a modal that lets clicks reach the controls it covers. `PASSTHROUGH` was added to `Floating::Root` by `05df7c89` ("Gui: Tooltips", 2026-01-27), when the tooltip was the only root-attached element. The Jump modal started using `Floating::Root` as a backdrop in `8bd28e08` (2026-05-05), and the block inspector in `2ea1c163` (2026-05-26).

## Description

Clay keeps one layout tree per floating element. `Clay_SetPointerState` walks those trees from the topmost down and records every element under the pointer in `pointerOverIds`. After each tree it stops, unless that tree's root is a floating element in `PASSTHROUGH` mode, in which case it carries on to the trees below. `Clay_PointerOver(id)`, which is all that `ui.hovered_raw(id)` calls, is a lookup in that list.

`decl()` sets `PASSTHROUGH` on every `Floating::Root` element (`ui.rs:412`). Three elements use `Floating::Root`:

| Element | Declared at | Is passthrough wanted? |
| :- | :- | :- |
| Tooltip | `ui.rs:4986` | Yes: a tooltip must not steal hover from the thing it describes |
| Jump To Height modal backdrop ("Modal Container" when `ui.modal == Modal::Jump`) | `ui.rs:1675-1676`, `:1695` | No: it is a dimmed, full-window backdrop |
| Block inspector ("Block Inspector Outer") | `ui.rs:4696` | No: it is an opaque panel with its own buttons and scroll area |

So while the Jump modal is open, the pointer is "over" the backdrop and, at the same time, over whatever pane widget lies beneath it. `button_ex` asks only `hovered_raw(id)` and then takes the press unconditionally (`ui.rs:656`), so the widget beneath acts on the same click that dismisses the modal.

The click-background-to-exit branch tries to consume the press by writing `ui.mouse_pressed_id = id` (`ui.rs:1772`). That stops code that asks `ui.hovered()` (which requires `mouse_pressed_id == Id::default()`), but not `button_ex`, `slider`, the pane dividers, or the inspector resize handle, which all use `hovered_raw` and overwrite `mouse_pressed_id`.

## Attack Scenario and Steps

There is no attacker. This is a reproduction.

1. Open the Jump modal: press Ctrl+J, or click `Jump To Height...` (bottom of the centre column).
2. Click the dimmed area where it covers a control in a pane. Three examples:
   - the `MINING` pill in the Network Info box (top right of the centre column);
   - the `Send` icon in the left pane;
   - a row in the transaction history.
3. Observe: the modal closes, and in the same frame mining toggles, or the Send modal opens in the left pane, or the chain camera jumps to that transaction's block with zoom reset to 2.0 and follow-tip off.
4. Variant, act on release: click the dimmed area over `Receive cTAZ` (right pane, Faucet section) and release. The modal closes on press, and the faucet request is sent on release.
5. Variant, inspector: click a block to open the block inspector so that it overlaps the right pane, then roll the wheel (or two-finger scroll) with the pointer over the overlap. Both the inspector and the finalizer list scroll.

**Attack Requirements and Assumptions:**

- Any platform: the logic is in `ui.rs` and Clay, with no backend dependence.
- "Your Wallet" tab selected. On the Faucet Wallet tab the Jump modal cannot open at all (see `faucet-wallet-tab-resets-every-modal-so-jump-and-convert-rewards-never-open.md`).
- For step 4, the faucet button must be enabled (`miner_shielded_spendable_funds` above 5.01 cTAZ and no request in flight, `ui.rs:3980-3984`).
- For step 5, the inspector must overlap a pane. It is placed 16 px right and below the inspected block and is up to 480 px wide (`ui.rs:4678`, `:4696-4699`). Worked example, not measured: at a 1600 px wide window, zoom 2.0 and scale 1.0, a BFT block sits near x = 872, so the inspector spans roughly 888 to 1368, and the right pane starts at 1216.

## Impact on Users

- **Unintended actions on dismiss.** Clicking outside a modal to dismiss it is a common habit, and the two panes cover a little over half of the window. Depending on where the click lands, the user also:
  - toggles mining on or off (`wallet::GUI_ENABLE_MINE`), the most consequential case, since the node stops or starts mining without the user asking;
  - opens Send, Receive, Stake, Edit Stake or User;
  - moves the chain camera to a transaction's block;
  - requests funds from the faucet;
  - toggles mute, moves the volume slider, opens Finalizer Filters or Convert Rewards, hits Reset View or Follow Tip;
  - starts a pane-divider drag, which resizes the pane until release.
- **Jump modal reopens.** A dismiss click that lands on `Jump To Height...` closes the modal and reopens it in the same frame, so the click appears to do nothing.
- **Double scroll and double click through the inspector.** Wheel input over the part of the inspector that overlaps a pane scrolls both. A click on `Copy hash` in that overlap also presses any pane button underneath.
- No money moves and no figure is displayed wrongly. None of the reachable controls sends funds in one click.

## Technical Details / Code Analysis

**`PASSTHROUGH` is set for every root-attached floating element** (`zebra-gui/src/ui.rs:404-413`):

```rust
Floating::Root(x, y) => {
    decl.floating.attachTo = clay::Clay_FloatingAttachToElement_CLAY_ATTACH_TO_ROOT;
    decl.floating.offset.x = x;
    decl.floating.offset.y = y;
    decl.floating.attachPoints = clay::Clay_FloatingAttachPoints {
        element: clay::Clay_FloatingAttachPointType_CLAY_ATTACH_POINT_LEFT_TOP,
        parent:  clay::Clay_FloatingAttachPointType_CLAY_ATTACH_POINT_LEFT_TOP,
    };
    decl.floating.pointerCaptureMode = clay::Clay_PointerCaptureMode_CLAY_POINTER_CAPTURE_MODE_PASSTHROUGH;
},
```

`Floating::Parent` leaves the zero-initialised default, `CAPTURE` (`ui.rs:346`, and the `TODO` at `:402`).

**What the two modes do in the vendored Clay.** `Clay_SetPointerState` loops over `layoutElementTreeRoots` from the last index down to 0 (`clay-rs/clay.h:3979`). For each root it walks the tree depth first, appends every element whose box contains the pointer to `pointerOverIds`, and sets `found` (`:3985-4021`). The loop body then ends with the only place the capture mode is read (`:4023-4027`):

```c
Clay_LayoutElement *rootElement = Clay_LayoutElementArray_Get(&context->layoutElements, root->layoutElementIndex);
if (found && Clay__ElementHasConfig(rootElement, CLAY__ELEMENT_CONFIG_TYPE_FLOATING) &&
        Clay__FindElementConfigWithType(rootElement, CLAY__ELEMENT_CONFIG_TYPE_FLOATING).floatingElementConfig->pointerCaptureMode == CLAY_POINTER_CAPTURE_MODE_CAPTURE) {
    break;
}
```

Roots are appended in declaration order (`clay.h:2105`) and sorted by `zIndex` with a stable bubble sort (`:2659-2671`). `decl()` never sets `zIndex`, so the order is declaration order, and the walk above goes from the last declared root to the first. With the Jump modal open the roots are, bottom to top: the main tree, "Modal Container" (`PASSTHROUGH`), "Modal Contents" (`Floating::Parent`, so `CAPTURE`), the inspector if open (`PASSTHROUGH`), the tooltip if shown (`PASSTHROUGH`).

- Pointer inside "Modal Contents": the walk stops there. Nothing beneath is hovered. The modal's own widgets are safe.
- Pointer on the backdrop outside the contents: the container is found, it is `PASSTHROUGH`, the walk continues into the main tree, and every pane element under the pointer is added.

**The dismiss branch and its attempt to consume the press** (`ui.rs:1764-1767`, `:1770-1773`):

```rust
let (clicked, colour, _) = ui.button_ex(false, BUTTON_GREY, id, true, CursorIcon::Default);
if clicked || (ui.input().key_pressed(KEY_ESC) && !ui.nav_enable) {
    ui.modal = Modal::None;
}
```

```rust
if ui.hovered(container_id) && !ui.hovered(contents_id) && ui.input().mouse_pressed(BTN_LEFT) {
    ui.modal = Modal::None;
    ui.mouse_pressed_id = id;
}
```

**How `button_ex` records a press** (`ui.rs:645-658`):

```rust
let mouse_hover = self.hovered_raw(id);
let key_hover   = self.nav_enable && self.nav_id == id.id;

let mouse_held     = mouse_hover && self.input().mouse_held(BTN_LEFT);
let mouse_pressed  = mouse_hover && self.input().mouse_pressed(BTN_LEFT);
let mouse_released = mouse_hover && self.input().mouse_released(BTN_LEFT);

let key_held     = key_hover && self.input().key_held(KEY_ENTER);
let key_pressed  = key_hover && self.input().key_pressed(KEY_ENTER);
let key_released = key_hover && self.input().key_released(KEY_ENTER);

if mouse_pressed { self.mouse_pressed_id = id; }
if key_pressed   { self.key_pressed_id   = id; }
let mouse_activated  = enabled && self.mouse_pressed_id == id && if act_on_press { mouse_pressed } else { mouse_released };
```

It never checks whether another widget already owns the press, and it does not look at `ui.modal`.

**One click on the backdrop over the `Send` icon, frame by frame.** Each rendered frame runs `viz_gui_draw_the_stuff_for_the_things` (`lib.rs:1596`), then `ui_update` (`lib.rs:1600`), which calls `run_ui` twice: pass 1 with the real input and no drawing, pass 2 with a copy of the input that has no press or release edges, and drawing (`ui.rs:5250-5261`).

| Step | Where | What happens |
| :- | :- | :- |
| Frame N-1, pass 2 | `ui.rs:1704-1706` | Pointer rests on the backdrop. `ui.hovered(container_id)` is true, so `ui.capture = true` is left set for the next frame's viz |
| Frame N (press), viz | `viz_gui.rs:1842-1846`, `:2343` | `ui.capture` is true, so the viz does not claim the press and does not change the inspected block. The click-to-recenter at `:1961` and `:2014` is not gated and fires if a block lies under the cursor (see the G9 file) |
| Frame N, pass 1, start | `ui.rs:4068` | `clay.pointer_state` rebuilds `pointerOverIds` from the previous layout: "Modal Container", plus "Main", "Left Pane", "Main Contents", "Buttons Container", and the `Send` button |
| Frame N, pass 1, tab bar | `ui.rs:1598-1599` | The two wallet tabs are declared before the modal. Not under the pointer here, so nothing happens |
| Frame N, pass 1, modal | `ui.rs:1770-1773` | `mouse_pressed_id` is still default, container hovered, contents not hovered, button pressed: `ui.modal = Modal::None`, `ui.mouse_pressed_id` = the close button's id |
| Frame N, pass 1, left pane | `ui.rs:2875`, `:2909` | `button_ex(true, .., id("Send"), ..)`: `hovered_raw` is true through the passthrough root, `mouse_pressed` is true, so `mouse_pressed_id` is overwritten with the `Send` id and `mouse_activated` is true. `ui.modal = Modal::Send` |
| Frame N, pass 2 | `ui.rs:1690` | `ui.modal` is `Send`, so the Send modal is laid out and drawn. The user sees Jump replaced by Send |
| Frame N+k (release) | `ui.rs:4996-4998` | `mouse_pressed_id` is cleared. Nothing else fires for an act-on-press button |

For an act-on-release control such as `Receive cTAZ` (`button_ex(.., act_on_press = false, ..)` at `ui.rs:3944`, `:3984`): on the press frame the modal closes and `mouse_pressed_id` becomes the faucet button's id (`:656`); on the release frame `mouse_released && mouse_pressed_id == id` holds, and `request_from_faucet()` runs.

**Controls declared before the modal behave the other way round.** The wallet tabs run their `button_ex` first (`ui.rs:741`, called from `:1598-1599`). A backdrop click over a tab sets `mouse_pressed_id` to the tab id, so `ui.hovered(container_id)` at `:1770` is false and the modal is not dismissed by that branch. Clicking "Faucet Wallet" through the backdrop switches the tab, and the modal then disappears through the reset at `:1621-1623`. Clicking "Your Wallet" does nothing visible and the modal stays open.

**Every modal and popup, and whether it is affected.**

| Surface | How it is declared | Backdrop | Affected? |
| :- | :- | :- | :- |
| `Modal::Jump` | Container `Floating::Root(0, 0)`, window-sized, `PASSTHROUGH`; contents `Floating::Parent`, centred (`ui.rs:1675-1689`, `:1717`) | Full window, `(0, 0, 0, 0xC0)` | **Yes**: backdrop clicks reach everything declared after it |
| `Modal::Send`, `Receive`, `Stake`, `Unstake` (Edit Stake), `Retarget`, `ConvertRewards`, `User` | Container `Floating::Parent` of "Left Pane", `grow!()`, so `CAPTURE`; contents is an ordinary child (`ui.rs:1678`, `:1717`) | Left pane only | No: the walk stops at the container. The centre column and right pane have no backdrop and stay live, which reads as intended |
| Block inspector | `Floating::Root`, `PASSTHROUGH` (`ui.rs:4696`) | None | **Yes**: clicks and wheel input reach pane widgets under the overlap |
| Block inspector resize handle | `Floating::Parent` of the inspector (`ui.rs:4965`) | None | No |
| Tooltip | `Floating::Root`, `PASSTHROUGH` (`ui.rs:4986`) | None | Passthrough is correct here |
| Finalizer Filters popup | Inline element in the right pane, not floating (`ui.rs:3518-3530`) | None | No |
| Network Info box | Inline (`ui.rs:4225-4257`) | None | No |

All eight modals are `closeable` (`title_bar(ui, true, ..)` at `ui.rs:1824`, `1901`, `2059`, `2117`, `2179`, `2332`, `2368`, `2783`).

**The inspector's double scroll** follows from `scroll_container` (`ui.rs:987`):

```rust
if self.hovered(id) && !self.suppress_scroll_for_clay {
```

Both the inspector's scroll container and the pane's are in `pointerOverIds`, so both apply the same `scroll_delta` and `wheel_delta`.

**A related ordering observation (inferred from the sort, not observed on screen).** The inspector is declared after the modal (`ui.rs:4667` versus `:1690`), so its root sits above both the Jump backdrop and the Jump contents. An open inspector is drawn on top of the Jump modal wherever they overlap.

## Recommendations

1. **Make `Floating::Root` capture the pointer, and give passthrough to the tooltip only.**
   - In `zebra-gui/src/ui.rs`, add a variant next to `Root` on the existing `Floating` enum, and set `PASSTHROUGH` only for it:

   ```rust
   Floating::Root(x, y) | Floating::RootPassthrough(x, y) => {
       decl.floating.attachTo = clay::Clay_FloatingAttachToElement_CLAY_ATTACH_TO_ROOT;
       decl.floating.offset.x = x;
       decl.floating.offset.y = y;
       decl.floating.attachPoints = clay::Clay_FloatingAttachPoints {
           element: clay::Clay_FloatingAttachPointType_CLAY_ATTACH_POINT_LEFT_TOP,
           parent:  clay::Clay_FloatingAttachPointType_CLAY_ATTACH_POINT_LEFT_TOP,
       };
       // The tooltip is clamped to the window edge and can end up under the cursor;
       // if it captured, the widget it describes would lose hover and the tooltip would flicker.
       if let Floating::RootPassthrough(..) = item.floating {
           decl.floating.pointerCaptureMode = clay::Clay_PointerCaptureMode_CLAY_POINTER_CAPTURE_MODE_PASSTHROUGH;
       }
   },
   ```

   - Change the tooltip at `ui.rs:4986` to `Floating::RootPassthrough(tooltip_pos.0, tooltip_pos.1)`. Leave the Jump container (`:1676`) and the inspector (`:4696`) on `Floating::Root`.
   - Result: on the backdrop, `Clay_SetPointerState` stops at "Modal Container", so no pane element is in `pointerOverIds`, and `button_ex`, `slider`, the dividers and `scroll_container` underneath see no hover. The dismiss branch at `:1770` keeps working, because it asks about the container itself. `ui.capture` is still set through `ui.hovered(container_id)` (`:1706`) and `ui.hovered(inspect_outer_id)` (`:4674`).
   - UX: clicking the dimmed area closes the Jump modal and does nothing else. Scrolling over the inspector scrolls only the inspector. Clicking an inspector button presses only that button.

2. **Put the Jump modal above the block inspector.** With item 1 alone, an inspector that overlaps the Jump modal still sits above it, in drawing and in hit order. Add a `z: i16` field to `Decl` (default 0), copy it to `decl.floating.zIndex` in `decl()`, and set it to 1 on "Modal Container" and "Modal Contents" when `ui.modal == Modal::Jump`, and to 2 on the tooltip. Each floating element is its own root with its own `zIndex`, so both modal elements need it.
   - UX: with Jump open, the inspector is dimmed under the backdrop like everything else.

3. **Considered alternatives, not chosen:**
   - *First claim wins in `button_ex`* (`if mouse_pressed && self.mouse_pressed_id == Id::default()`). It breaks nested buttons, where the inner control relies on overwriting the outer one's claim (a transaction row is itself a `button_ex`, `ui.rs:3010`), and it would let the viz's `VIZ_GUI` claim (`viz_gui.rs:1844`) starve UI buttons on any frame where `ui.capture` is stale.
   - *Guard each pane widget with `ui.modal == Modal::None`.* Scattered, easy to miss a widget, and it does nothing for the inspector.
   - *A "press swallowed" flag on `Context`, checked in `button_ex`, `slider` and the dividers.* Workable, but it duplicates what Clay's capture mode already provides, and it leaves hover highlights and wheel scrolling leaking through.

4. **Tests.**
   - *What exists:* `zebra-gui` has unit tests only: `lib.rs:2400` (`mod tests`: render worker shutdown and `apply_mouse_snapshot` edge handling) and `ui.rs:5326` (`roster_identity_tests`). Nothing drives `run_ui`, which needs a `DrawCtx` that is only built inside `main_thread_run_program` (`lib.rs:1280`).
   - *Cheapest real check (inferred to be feasible, not compiled):* a unit test in `ui.rs` that needs only Clay. Create a `clay_layout::Clay`, begin a layout, declare a 100 by 100 element with a known id through the real `elem().decl(..)`, then a window-sized `Floating::Root(0.0, 0.0)` element, end the layout, call `pointer_state` inside the first element, and assert `Clay_PointerOver` is false for it. Repeat with `Floating::RootPassthrough` and assert true. This exercises the real `decl()` and the real Clay walk, with no text and no `DrawCtx`.
   - *Manual check:*
     1. Ctrl+J, click the dim area over the `MINING` pill. Expected: modal closes, pill unchanged.
     2. Ctrl+J, click the dim area over `Send`. Expected: modal closes, no Send modal.
     3. Ctrl+J, press and release on the dim area over `Receive cTAZ`. Expected: modal closes, no faucet request.
     4. Ctrl+J, click inside the modal box away from its buttons. Expected: modal stays open.
     5. Open the inspector over the right pane edge, wheel over the overlap. Expected: only the inspector scrolls.
     6. Hover a finalizer row near the window's right edge so the tooltip is clamped under the cursor. Expected: tooltip stays steady, no flicker.

## Validation Information

**Verdict: CONFIRMED. Severity: Low.**

| Claim | Verified at |
| :- | :- |
| Every `Floating::Root` element gets `PASSTHROUGH` | `ui.rs:404-413`; default is `CAPTURE` at `:346` |
| The Jump modal uses a full-window `Floating::Root` backdrop | `ui.rs:1675-1689`, `:1695-1700` |
| Clay keeps walking lower roots for a passthrough root | `clay.h:3979-4028`; root order `:2105`, `:2659-2671` |
| `hovered_raw` is `Clay_PointerOver`, a list lookup | `ui.rs:554`; `clay.h:4308-4316` |
| The backdrop click dismisses the modal | `ui.rs:1770-1773` |
| `button_ex` overwrites `mouse_pressed_id` unconditionally | `ui.rs:656` |
| Mining toggles | `checkbox_pill` calls `button_ex(true, ..)`, `ui.rs:611`; used at `:4269-4272` |
| The Send modal opens | `ui.rs:2875`, `:2909` |
| The camera jumps for a transaction row | `ui.rs:3010`, `:3017-3025` |
| `Receive cTAZ` fires on release | `ui.rs:3943-3944`, `:3982-3986`; it is in `ui_right_pane`, so it is visible on the "Your Wallet" tab |
| The inspector is `Floating::Root` and wheel input reaches the list beneath | `ui.rs:4696`, `:987` |
| Two `run_ui` passes per frame, edges only in the first | `ui.rs:5250-5261` |

**Severity justification.**

- *Why not Medium:* no flow is blocked, no money or stake figure is wrong, and nothing crashes. No control reachable through the backdrop moves funds in one click. The worst single outcome is an unrequested mining toggle, which is visible on the pill and reversed by one click. It needs the Jump modal open and a dismiss click that lands on a control.
- *Why not lower:* Low is the floor. It is not a false positive: the click-through is deterministic, and it changes state the user did not ask to change.

**Corrections made during validation.**

1. The review implies every control under the backdrop fires. Controls declared before the modal, which are the two wallet tabs, take the press first, and then the backdrop branch does not dismiss (`ui.hovered` needs an unclaimed press). Everything declared after the modal fires as described.
2. Added: a dismiss click over `Jump To Height...` closes and reopens the modal in one frame.
3. Added: besides `button_ex`, the `slider` (`ui.rs:911-914`) and the pane dividers (`:4151-4157`, `:4624-4630`) also take the press through the backdrop.
4. Added: `ui.mouse_pressed_id = id` at `ui.rs:1772` shows the author meant the dismiss click to be consumed. The behaviour is not deliberate.
5. Added: the inspector also passes clicks through, not only wheel input, and it is drawn above the Jump modal (inferred from root order).
6. The line cited for the Jump backdrop's passthrough, `ui.rs:412`, is right. The dismiss branch is at `:1770`, as cited.

**Cross-references.**

- `chain-view-pans-and-recenters-while-the-pointer-is-over-a-pane-or-modal.md` (G9): the same dismiss click also recenters the chain camera when a block lies under the cursor, because click-to-recenter is not gated on `ui.capture`. Item 1 here does not fix that; G9's item 2 does. G9 also covers `ui.rs:1744`, where the Jump modal's contents fail to set `ui.capture`. Land this file's item 1 and G9's item 3 together, since both edit the modal's hover handling.
- `esc-cannot-close-a-modal-after-textbox-focus-and-jump-gives-no-feedback.md` (G10): the other ways to close the same modal (Esc at `ui.rs:1765`, shown above). Users who cannot close with Esc fall back to the backdrop click, which is what triggers this finding.
- `faucet-wallet-tab-resets-every-modal-so-jump-and-convert-rewards-never-open.md` (G5): a backdrop click on the "Faucet Wallet" tab closes the modal through that reset.
