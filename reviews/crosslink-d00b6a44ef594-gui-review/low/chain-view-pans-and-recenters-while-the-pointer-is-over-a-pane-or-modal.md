# `viz_gui_draw_the_stuff_for_the_things` applies touchpad `scroll_delta` to the chain camera and recenters on the hovered block without checking `ui.capture`, so scrolling a pane list also pans the chain view and switches off `follow_tip`, and clicking a pane or inspector button over a hidden block moves the camera and resets zoom

**Severity**: Low
**Validation Status**: Confirmed
**Location**: `zebra-gui/src/viz_gui.rs:1780-1788` (touchpad pan, not gated), `:1890-1912` (`hovered_block`, not gated), `:1961-1966` and `:2014-2019` (click to recenter, not gated), `:1723-1735` (wheel and pinch zoom, gated), `:1842-1846` (`VIZ_GUI` press claim, gated), `:2343-2348` (inspect, gated), `:2350-2358` (`suppress_scroll_for_clay`), `:2125-2128` and `:2271` (inspector position follows the inspected block), `:1278-1294` (`hovered_hash_in_world`); `zebra-gui/src/ui.rs:4047` (`ui.capture` reset), `:4144`, `:4657`, `:4674-4676`, `:4512-4514`, `:4548-4550`, `:4426` (hover sites that set it), `:1704-1706` and `:1741-1744` (modal container; `:1744` tests the wrong variable), `:5009-5014` (set while a UI widget holds the press), `:987-998` (`scroll_container`), `:5288` (field); `zebra-gui/src/lib.rs:16-17` (`mod viz_gui; pub use viz_gui::*;`), `:1414-1417` (axis events to `scroll_delta` and `wheel_delta`), `:1596-1600` (viz runs before `ui_update`); `patches/softer_gui-3.0.1/src/mac.rs:260`, `wayland.rs:515`, `win.rs:893`, `x11.rs:552` (emitters of `AXIS_SCROLL_*`)
**Found by agent:** /code-review high, GUI/UX (Claude Fable 5.1), 2026-09-30; validated 2026-10-01 at dev d00b6a44ef594
**In scope of audit?** Yes. It is pointer routing between the chain view and the panes in `zebra-gui`. The pan lines were last rewritten by `eda288f9` ("APE build, softer_gui 3.0.2, and device-normalized input for zebra-gui", 2026-09-28), which split touchpad `scroll_delta` from mouse `wheel_delta`. Click to recenter is from `9c849fc0` (2026-01-26). The `ui.capture` gates on zoom, press claim and inspect are from `596c4231` and `c50e0167` (2026-05-02 and 05-03).

## Description

**Is `viz_gui.rs` still built?** Yes. Commit `b67c73ad` ("One binary: GUI when there is a display, headless otherwise; drop viz_gui") dropped the `viz_gui` Cargo feature in `zebrad` and `zebra-crosslink`, replacing `cfg(feature = "viz_gui")` with a run-time `gui_active()`. It did not touch `zebra-gui/src/viz_gui.rs`. `zebra-gui/src/lib.rs:16-17` still has `mod viz_gui; pub use viz_gui::*;`, and the frame loop calls `viz_gui_init` (`lib.rs:1191`), `viz_gui_anything_happened_at_all` (`:1497`) and `viz_gui_draw_the_stuff_for_the_things` (`:1596`). `main.rs:99` enters that loop through `zebra_gui::main_thread_run_program`.

**What `ui.capture` means.** It is the UI layer's statement that the pointer belongs to a Clay widget and not to the chain view. `run_ui` clears it at the start of each pass (`ui.rs:4047`) and sets it when:

- the pointer hovers "Left Pane" (`ui.rs:4144`) or "Right Pane" (`:4657`);
- the pointer hovers the block inspector (`:4674-4676`);
- the pointer hovers one of the centre-column buttons: `Jump To Height...` (`:4512`), `Reset View` or `Follow Tip` (`:4548`), the debug Test Format buttons (`:4426`);
- the pointer hovers a modal's container (`:1704-1706`);
- any UI widget holds the mouse press, that is `mouse_pressed_id` is neither default nor `VIZ_GUI` nor a minimap id (`:5009-5014`).

The viz runs before `ui_update` in each frame (`lib.rs:1596`, then `:1600`), so it reads the value left by the previous frame's second `run_ui` pass. That is a one-frame lag by construction, and the existing gates already accept it.

**What the viz does with it.** Four pointer inputs feed the chain view. Two are gated, two are not:

| Input | Code | Gated on `!ui.capture`? |
| :- | :- | :- |
| Wheel zoom, pinch zoom | `viz_gui.rs:1723-1735` | Yes |
| Press claims `Id::VIZ_GUI` (starts a drag pan) | `:1842-1846` | Yes |
| Press sets `inspecting_block_hash` | `:2343-2348` | Yes |
| Touchpad `scroll_delta` pans the camera, vertical motion clears `follow_tip` | `:1780-1788` | **No** |
| Press on `hovered_block` recenters the camera and sets `zoom = 2.0` | `:1961-1966`, `:2014-2019` | **No** |

`hovered_block` itself is computed from the pointer's world position with no regard to what is drawn on top (`:1890-1912`); it is only cleared inside a minimap.

One further gap, found during validation: over the Jump modal's contents box `ui.capture` is false, because `ui.rs:1744` tests `container_hovered` where it should test `contents_hovered`. In that spot even the gated inputs reach the chain view.

## Attack Scenario and Steps

There is no attacker. These are reproductions.

*A. Touchpad scroll over a pane.*

1. On a laptop, leave `Follow Tip` locked (the default view).
2. Put the pointer over the transaction history (left pane) or the finalizer list (right pane) and two-finger scroll.
3. Observe: the list scrolls, the chain view behind it pans by the same finger distance, and the `Follow Tip` lock opens. A horizontal component pans the chain sideways as well.

*B. Click a UI button that has a block behind it.*

1. Click a PoW block to open the block inspector. It opens 16 px right and below the block.
2. Click `Copy hash` (or any inspector row) at a point where a block of the other column lies underneath.
3. Observe: the hash is copied, and the camera also recenters on the hidden block with `zoom = 2.0` and `follow_tip` off. The inspector, which is positioned from the inspected block's screen position, moves away from the cursor.
4. The same happens for any pane button when the chain has been panned or zoomed so that a block lies under the pane.

*C. Pointer over the Jump modal's box.*

1. Press Ctrl+J. Put the pointer on the modal box and roll the mouse wheel. The chain zooms.
2. Click the textbox to focus it. If an inspector was open, it closes, or if a block lies under the cursor, an inspector opens for it and the camera recenters.
3. Press on the modal's padding and drag. The chain pans.

**Attack Requirements and Assumptions:**

- A: a device that `softer_gui` reports on `AXIS_SCROLL_V` or `AXIS_SCROLL_H`, which `lib.rs:1414-1415` routes to `scroll_delta`. All four backends have an emitter (`mac.rs:260`, `wayland.rs:515`, `win.rs:893`, `x11.rs:552`), so macOS, Windows, Linux X11 and Linux Wayland are affected. This is verified by locating the emit sites; the device classification inside each backend was not read. A plain mouse wheel arrives on `AXIS_WHEEL_*` and goes to `wheel_delta`, which only zooms, and that is gated.
- B: any platform and any pointer device. A block must lie within 1.0 world unit of the pointer's world position (`viz_gui.rs:1289`). Blocks sit at world x = -5 (PoW) and x = +5 (BFT), so at the default camera they are under the centre column. They end up under a pane after a horizontal pan or a zoom-in, and they are under the inspector routinely, since the inspector opens beside the column it was opened from. Worked example, not measured: at zoom 2.0 and scale 1.0, `screen_unit` is 14.4 px, so the BFT column is 144 px right of the PoW column, inside the 480 px wide inspector that opens 16 px right of a PoW block.
- C: any platform. The Jump modal is centred in the window, which is where the chain columns are drawn.

## Impact on Users

- **Follow-tip is lost by scrolling a list.** A touchpad user who scrolls their transaction history or the finalizer roster finds the chain view somewhere else afterwards, with `Follow Tip` unlocked, so new blocks no longer bring the view along. Nothing tells them why. This is the most frequent of the effects: it happens on every touchpad scroll over a pane.
- **Camera jumps on unrelated clicks.** Copying a hash, or pressing any pane button, can throw the camera to another block and reset the zoom, and the inspector the user was working in slides away.
- **The Jump modal does not shield the chain.** Wheel, click and drag over the modal box act on the chain view behind it. A click meant to focus the height textbox can open or close a block inspector, and that inspector is drawn above the modal (root order, see the G4 file).
- No funds, balances or stake figures are involved, and nothing is blocked: the list still scrolls and the button still acts.

## Technical Details / Code Analysis

**The gated zoom, for contrast** (`zebra-gui/src/viz_gui.rs:1723-1731`):

```rust
if !ui.capture {
    let dxm = (input_ctx.mouse_pos().0.clamp(0, draw_ctx.window_width) - draw_ctx.window_width/2) as f32;
    let dym = (input_ctx.mouse_pos().1.clamp(0, draw_ctx.window_height) - draw_ctx.window_height/2) as f32;
    let old_screen_unit = SCREEN_UNIT_CONST * (ZOOM_FACTOR.powf(viz_state.zoom) * ui.dpi_scale);
    viz_state.zoom += input_ctx.zoom_delta as f32 * PINCH_ZOOM_STEPS;
    if !inside_any_minimap {
        viz_state.zoom += input_ctx.wheel_delta.1 as f32 * WHEEL_ZOOM_STEP;
    }
    viz_state.zoom = viz_state.zoom.min(26.0);
```

**The ungated touchpad pan** (`viz_gui.rs:1780-1788`):

```rust
// A touchpad pans the camera 1:1 with the fingers: scroll_delta is logical pixels and
// screen_unit is physical pixels per world unit, so the dpi scale bridges them.
viz_state.camera_x -= input_ctx.scroll_delta.0 as f32 * ui.dpi_scale / screen_unit;
if !inside_any_minimap || !minimap_wheel_scrubbed {
    if input_ctx.scroll_delta.1 != 0.0 {
        viz_state.follow_tip = false;
    }
    viz_state.camera_y -= input_ctx.scroll_delta.1 as f32 * ui.dpi_scale / screen_unit;
}
```

The horizontal line is unconditional. The vertical lines skip only a minimap scrub.

**The same `scroll_delta` scrolls the list** (`zebra-gui/src/ui.rs:987-998`):

```rust
if self.hovered(id) && !self.suppress_scroll_for_clay {
    let shift = self.input().key_held(KEY_LEFTSHIFT) || self.input().key_held(KEY_RIGHTSHIFT);
    // A touchpad pans by the distance the fingers moved; a wheel steps a
    // fixed WHEEL_LINE per click. Which device is which was decided upstream.
    let dy = self.input().scroll_delta.1 as f32 + self.input().wheel_delta.1 as f32 * WHEEL_LINE;
    let dx = self.input().scroll_delta.0 as f32 + self.input().wheel_delta.0 as f32 * WHEEL_LINE;
    if shift {
        scroll_container_state.scroll_x -= dy / self.scale;
    } else {
        scroll_container_state.scroll -= dy;
    }
    scroll_container_state.scroll_x -= dx / self.scale;
```

`suppress_scroll_for_clay` is the viz telling Clay not to scroll (`viz_gui.rs:2355-2358`). It is the one-directional half of an arbitration: the viz can stop the lists from scrolling when it owns the gesture, but nothing stops the viz from panning when a list owns it.

```rust
ui.suppress_scroll_for_clay = ((inside_any_minimap && wheelish) || minimap_wheel_scrubbed)
    || ((ui.mouse_pressed_id == ui::Id::CHAIN_MINIMAP_POW || ui.mouse_pressed_id == ui::Id::CHAIN_MINIMAP_POS)
        && input_ctx.mouse_held(BTN_LEFT))
    || (in_center_column && wheelish && !ui.capture);
```

Trace for scenario A, pointer over the transaction history:

- Previous frame: `ui.hovered(id("Left Pane"))` set `ui.capture = true` (`ui.rs:4144`).
- This frame, viz: zoom block skipped. Pan block runs: `camera_y` moves, `follow_tip = false`. `suppress_scroll_for_clay` is false, because `ui.capture` is true.
- This frame, `run_ui` pass 1: the history's `scroll_container` is hovered and not suppressed, so it scrolls by the same delta.

**The ungated recenter** (`viz_gui.rs:1952-1966`; the BFT loop at `:2004-2019` is the same):

```rust
for on_screen_bc in magic(&mut viz_state.on_screen_bcs).values_mut() {
    if on_screen_bc.block.this_hash == hovered_block || viz_blocks.contains(&on_screen_bc.block.this_hash) {
        on_screen_bc.t_roundness = 0.3;
        on_screen_bc.t_darkness = 0.2;
        if input_ctx.key_pressed(KEY_SPACE) {
            on_screen_bc.x = 0.0;
            on_screen_bc.y = 0.0;
            on_screen_bc.alpha = 0.0;
        }
        if input_ctx.mouse_pressed(BTN_LEFT) && !minimap_scrub_this_frame {
            viz_state.follow_tip = false;
            viz_state.camera_x = on_screen_bc.t_x;
            viz_state.camera_y = on_screen_bc.t_y;
            viz_state.zoom = 2.0;
        }
```

The comment at `viz_gui.rs:2013` says the recenter "should not piggyback on minimap press", so piggybacking on another widget's press was considered for the minimap and nowhere else.

**The gated inspect, a few hundred lines later** (`viz_gui.rs:2343-2348`):

```rust
if !ui.capture && input_ctx.mouse_pressed(BTN_LEFT) && !minimap_scrub_this_frame {
    viz_state.inspecting_block_hash = hovered_block;
    viz_state.inspecting_block_screen_x = hovered_block_screen_x;
    viz_state.inspecting_block_screen_y = hovered_block_screen_y;
    viz_state.block_inspection = None;
}
```

Trace for scenario B, a click on `Copy hash` with a BFT block under the cursor:

- Previous frame: `ui.hovered(inspect_outer_id)` set `ui.capture = true` (`ui.rs:4674-4676`).
- This frame, viz: `hovered_block` is the hidden BFT block. The press recenters: `camera_x = 5.0`, `camera_y` = that block's `t_y`, `zoom = 2.0`, `follow_tip = false`. Inspect is skipped, so the inspected block stays the same. Its screen position is recomputed from the new camera (`viz_gui.rs:2125-2128`), and the inspector is placed from it (`ui.rs:4696-4699`).
- This frame, `run_ui` pass 1: `Copy hash` (`ui.rs:4785`, act on press) fires normally.
- Net result: the copy happens, and the panel moves out from under the cursor.

The hover side effects are ungated as well: the hidden block is highlighted, its relatives stay bright while others fade (`viz_gui.rs:1981`, `:2025`), and the hover sound plays (`:2337-2341`).

**`ui.capture` is false over the Jump modal's box** (`zebra-gui/src/ui.rs:1703-1706`, `:1741-1744`):

```rust
let container_id = _elem.decl.id;
let container_hovered = ui.hovered(container_id);

if container_hovered { ui.capture = true; }
```

```rust
let contents_id = _elem.decl.id;
let contents_hovered = ui.hovered(contents_id);

if container_hovered { ui.capture = true; }
```

For `Modal::Jump` the contents are `Floating::Parent` (`ui.rs:1717`), which makes them a separate Clay root in `CAPTURE` mode. When the pointer is on the contents, Clay's pointer walk stops at that root (`clay-rs/clay.h:4024-4027`), so the container is not hovered, and neither is any pane. `contents_hovered` is computed and then not used for capture; the second `if` repeats the first. For the other seven modals the contents are an ordinary child of the container, both are hovered together, and the slip has no effect.

With `ui.capture` false on the press frame, the viz claims `Id::VIZ_GUI` (`viz_gui.rs:1843-1845`) and overwrites `inspecting_block_hash` with whatever is under the cursor, which is the zero hash when no block is there (`:2343-2344`), closing an open inspector. If the press landed on a modal widget, that widget's `button_ex` then overwrites `mouse_pressed_id` and acts normally. If it landed on padding, `VIZ_GUI` keeps the press and dragging pans the camera (`:1747-1753`).

**Not traced further:** `viz_blocks.contains(..)` in the same condition means that with a non-default `ui.viz_op`, a press can recenter on a block selected by the viz op. `viz_op` is changed only in `dbg_ui` (`ui.rs:127`), so this looks debug-only; I did not follow it.

## Recommendations

1. **Pan from the touchpad only when the pointer is not on the UI.** In `viz_gui.rs:1780-1788`, wrap both axes:

   ```rust
   // A touchpad pans the camera 1:1 with the fingers: scroll_delta is logical pixels and
   // screen_unit is physical pixels per world unit, so the dpi scale bridges them.
   if !ui.capture {
       viz_state.camera_x -= input_ctx.scroll_delta.0 as f32 * ui.dpi_scale / screen_unit;
       if !inside_any_minimap || !minimap_wheel_scrubbed {
           if input_ctx.scroll_delta.1 != 0.0 {
               viz_state.follow_tip = false;
           }
           viz_state.camera_y -= input_ctx.scroll_delta.1 as f32 * ui.dpi_scale / screen_unit;
       }
   }
   ```

   - The minimaps are drawn by the viz, not by Clay, so `ui.capture` is false over them and their scrub path (`:1755-1778`) is unchanged.
   - UX: two-finger scrolling over a pane, the inspector or a modal scrolls only that list. The chain view and the `Follow Tip` lock do not change. Over the centre column the pan works as before.

2. **Treat a block as hovered only when the pointer is not on the UI.** At `viz_gui.rs:1910-1912`:

   ```rust
   if inside_any_minimap || ui.capture {
       hovered_block = Hash32::from_u64(0);
   }
   ```

   - This removes the recenter at `:1961` and `:2014`, the highlight and fade, and the hover sound for blocks hidden under a pane, the inspector or a modal, in one place. The inspect at `:2343` is already gated and is unaffected.
   - `ui.capture` is also true while a UI widget holds the press (`ui.rs:5009-5014`), so a drag that starts on a slider or divider and crosses the chain view does not highlight blocks. That is the wanted behaviour.
   - UX: clicking `Copy hash` copies and nothing moves. Blocks under a pane no longer light up as the pointer passes over the pane.

3. **Set `ui.capture` over the Jump modal's contents.** At `ui.rs:1744`:

   ```rust
   if container_hovered || contents_hovered { ui.capture = true; }
   ```

   - UX: with the Jump modal open, wheel, click and drag on the modal box do not zoom, inspect or pan the chain.

4. **Considered alternatives, not chosen:**
   - *Run `ui_update` before the viz so `ui.capture` is fresh.* The viz draws first so that the panes paint over it; reordering means splitting viz input from viz drawing. The one-frame lag is already accepted by the three existing gates and matters only when the pointer crosses a pane edge and clicks within one frame.
   - *Test pane rectangles in the viz* (as `in_center_column` does at `viz_gui.rs:2351-2352`). It uses the constant `PANE_PERCENT_*` widths, which no longer match after the user drags a divider (`ui.left_pane_width`), and it knows nothing of the inspector or modals. `ui.capture` already covers all of them.
   - *Gate only the two recenter sites.* It leaves the hover highlight and sound firing for hidden blocks.

5. **Tests.**
   - *What exists:* unit tests only (`lib.rs:2400`, `ui.rs:5326`). `viz_gui_draw_the_stuff_for_the_things` takes a `DrawCtx`, which is built only inside `main_thread_run_program` (`lib.rs:1280`), so no existing harness can call it. `zebrad`'s node tests can open the window with `ZEBRA_TEST_GUI` (`zebrad/tests/crosslink.rs:110-112`) but inject no pointer input (inferred from that comment; not traced).
   - *Mechanical check:* none without building a headless `DrawCtx` fixture, which is new infrastructure.
   - *Manual check:*
     1. Touchpad, `Follow Tip` locked: two-finger scroll over the transaction history, then over the finalizer list. Expected: list scrolls, chain still, lock stays closed.
     2. Two-finger scroll over the centre column. Expected: chain pans, lock opens (unchanged behaviour).
     3. Two-finger scroll over a minimap. Expected: scrub as before.
     4. Open the inspector on a PoW block, click `Copy hash` repeatedly at several heights of the panel. Expected: camera and inspector never move.
     5. Zoom in until a block column is under the left pane, click pane buttons over it. Expected: no camera movement, no block highlight.
     6. Ctrl+J, wheel over the modal box, click the textbox, drag on the padding. Expected: chain does not zoom, pan or change inspector.

## Validation Information

**Verdict: CONFIRMED. Severity: Low.**

| Claim | Verified at |
| :- | :- |
| `viz_gui.rs` is still compiled and used | `zebra-gui/src/lib.rs:16-17`, `:1191`, `:1497`, `:1596`; `git show b67c73ad` (stat) lists `zebra-gui/src/lib.rs` and `main.rs` only |
| What `ui.capture` means and when it is set | `ui.rs:4047`, `:4144`, `:4657`, `:4674-4676`, `:4512`, `:4548`, `:4426`, `:1706`, `:5009-5014` |
| Viz reads the previous frame's `ui.capture` | `lib.rs:1596` then `:1600` |
| Touchpad pan is not gated | `viz_gui.rs:1780-1788` |
| The list scrolls from the same delta | `ui.rs:987-998`; `suppress_scroll_for_clay` false when `ui.capture` is true, `viz_gui.rs:2358` |
| `follow_tip` is switched off | `viz_gui.rs:1784-1786` |
| Click to recenter is not gated, sets `zoom = 2.0` | `viz_gui.rs:1961-1966`, `:2014-2019` |
| `hovered_block` ignores the UI | `viz_gui.rs:1890-1912`, `:1278-1294` |
| Inspector follows the inspected block | `viz_gui.rs:2125-2128`, `:2271`; `ui.rs:4696-4699` |
| Wheel zoom and inspect are gated | `viz_gui.rs:1723`, `:2343` |
| Touchpad input exists on every backend | emit sites in `patches/softer_gui-3.0.1/src/mac.rs:260`, `wayland.rs:515`, `win.rs:893`, `x11.rs:552` (located by search; classification logic not read) |
| `ui.capture` false over the Jump modal's contents | `ui.rs:1741-1744`, `:1717`; `clay.h:4024-4027` |

**Severity justification.**

- *Why not Medium:* no flow is blocked and no money or stake figure is wrong. The list scrolls, the button acts, and the view is restored with `Reset View` or `Follow Tip`. The rubric puts confusing behaviour at Low.
- *Why not lower:* Low is the floor. Among the Low findings this one fires most often: every touchpad scroll over a pane moves the chain view and silently unlocks follow-tip.

**Is it deliberate?** No. The comment at `viz_gui.rs:1780-1781` describes the pan as a chain-view gesture and says nothing about panes. Three neighbouring inputs carry a `!ui.capture` gate, added in `596c4231` and `c50e0167`. The pan lines were rewritten in `eda288f9` without one, and the recenter (`9c849fc0`) predates the gates. `git log` shows no message describing pan-under-pane as intended.

**Corrections made during validation.**

1. The review says wheel zoom and inspect "are gated correctly". They are gated, but the gate is open over the Jump modal's contents because of `ui.rs:1744`. Added as item 3.
2. The review's line numbers (1782 to 1787, 1961, 2014, 2343) are exact.
3. Added: the horizontal pan at `viz_gui.rs:1782` is unconditional, not even skipped for a minimap scrub.
4. Added: the hover highlight, fade and hover sound are ungated too, which is why the fix belongs on `hovered_block` and not on the two recenter sites.
5. Added: the confirmation that `viz_gui.rs` is still compiled, since `b67c73ad`'s subject line suggests otherwise.

**Cross-references.**

- `modal-backdrop-passes-clicks-through-to-the-controls-underneath.md` (G4): a click on the Jump backdrop over a block also recenters the camera through the ungated path here. G4's fix does not close that; item 2 here does. Item 3 here and G4's item 1 both edit the modal's hover handling in `ui_left_pane`; land them together. G4's item 1 also stops the inspector passing wheel input to the pane beneath, which is the Clay-side twin of scenario A.
- `esc-cannot-close-a-modal-after-textbox-focus-and-jump-gives-no-feedback.md` (G10): its item 2 focuses the Jump textbox on open, which removes the click described in scenario C step 2, but not the wheel and drag cases.
