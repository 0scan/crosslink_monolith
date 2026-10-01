# The Send, Stake and Retarget modals slice the pasted clipboard string with `&s[..8]` and `&s[s.len() - 8..]` after only a byte-length check, so pasting non-ASCII text panics on the next frame, and with `panic = "abort"` that takes down the whole `zebrad` process

**Severity**: Medium
**Validation Status**: Confirmed
**Location**: `zebra-gui/src/ui.rs:1917-1922` (Send: guard and slice), `:1924` (Send: paste stored unvalidated); `:2182-2187` (Stake), `:2189`; `:2334-2339` (Retarget), `:2341`; `zebra-gui/src/lib.rs:918-934` (`get_from_clipboard`, lossy UTF-8, no filtering); `zebra-crosslink/Cargo.toml:201-202`, `:280-281` (`panic = "abort"` in both profiles); `zebra-crosslink/zebra-crosslink/src/viz2.rs:37-42` (GUI runs on the main thread of the node process)
**Found by agent:** /code-review high, GUI/UX (Claude Fable 5.1), 2026-09-30; validated 2026-10-01 at dev d00b6a44ef594
**In scope of audit?** Yes. It is a crash reachable from a button in three wallet modals. The Retarget and Stake slices date from `1be5038d` (Sam H. Smith, 2026-01-25); the Send lines were last rewritten by `c9231740` ("GUI: temp revert stake/send amount UI", 2026-05-15), which restored the same pattern.

## Description

Each of the three modals shows the pasted address abbreviated as `[first8..last8]`. The code that does so is the same in all three places:

1. Start with the placeholder `"0000000000000000"` (16 ASCII bytes).
2. If the stored string has **byte length** at least 16, use it instead.
3. Slice the first 8 **bytes** and the last 8 **bytes**.

Step 3 uses `&str` range indexing, which panics unless both cut points fall on UTF-8 character boundaries. The guard in step 2 checks length only. The stored string is whatever the clipboard held: `get_from_clipboard` returns `String::from_utf8_lossy(stdout)`, the caller applies `.trim()`, and nothing else looks at it before it is displayed.

So any clipboard text of 16 or more bytes where byte 8, or byte `len - 8`, falls inside a multi-byte character crashes the program one frame after the user clicks **Paste Address** or **Paste Identity**.

The consequence is larger than the original finding said. The GUI is not a separate program: since `b67c73ad` it runs on the main thread of `zebrad`, and the workspace sets `panic = "abort"` for both the dev and release profiles. A panic here aborts the node, the wallet, the miner and the finalizer together.

## Attack Scenario and Steps

No attacker is needed; this is a slip any user can make. An attacker's only lever is social: "copy this and paste it as the address".

1. On macOS, or on Linux with `xclip` or `xsel` installed, copy any text containing non-ASCII characters. A line of chat in Chinese, Japanese or Korean is enough: `你好世界你好` is 18 bytes, three bytes per character, so byte 8 is in the middle of the third character.
2. In the GUI, open **Send** (or **Stake**, or **Retarget** from Edit Stake).
3. Click **Paste Address** (or **Paste Identity**). The click stores the text in `data.send_address` (or `data.stake_address`). The abbreviated line was already drawn earlier in this frame, from the old value.
4. On the next frame the modal body runs again, the length guard passes, and `&send_address[..8]` panics with `byte index 8 is not a char boundary`.
5. The process aborts. The window closes and the node stops.

**Attack Requirements and Assumptions:**

- **Platform:** a platform where paste returns text. That is macOS, and Linux with `xclip` or `xsel`. On Windows, and on Linux without those tools, paste returns an empty string today (finding G2), so the crash cannot be reached there **until G2 is fixed**.
- **Clipboard content:** at least 16 bytes after trimming, with a multi-byte character straddling byte 8 or byte `len - 8`.
  - Text made only of 3-byte characters (CJK) always qualifies, because 8 is not a multiple of 3.
  - Text made only of 2-byte characters (Cyrillic, Greek, Hebrew, Arabic) cuts cleanly at byte 8 and at `len - 8`, so it does not panic. Mixed text depends on where the characters land.
  - Mostly-ASCII text with one curly quote, accented letter or emoji panics only if that character happens to sit across one of the two cut points.
- **Wallet state:** none. The paste buttons are always enabled (`button(ui, "Paste Address", true)`); no balance, sync state or Staking Day is required. Retarget needs an existing bond to open the modal.

## Impact on Users

- **The whole node goes down, not just the window.** A participant running a finalizer stops voting until they restart. A miner stops mining. This is inferred from the profile settings and the thread layout, both read directly; the abort itself was not reproduced because the brief forbids building.
- **No persistent damage.** The pasted string lives only in `UiData` in memory. After a restart the modal is back to its placeholder. The clipboard still holds the same text, though, so a user who does not understand what happened will crash again on the next paste.
- **No funds are involved.** Nothing is sent or staked by the paste.
- **Likelihood is moderate to low for most users, high for CJK-locale users.** The usual way to hit it is to click Paste while the clipboard still holds something else. If that something else is ordinary CJK text, the crash is certain.

## Technical Details / Code Analysis

**Send** (`zebra-gui/src/ui.rs:1917-1925`):

```rust
                            let mut send_address = "0000000000000000";
                            if data.send_address.len() >= 16 {
                                send_address = &data.send_address;
                            }

                            ui.text(frame_strf!(data, "[{}..{}]", &send_address[..8], &send_address[send_address.len() - 8..]), TextDecl { font: Mono, h: ui.scale(20.0), colour: WHITE, align: AlignX::Center, ..TextDecl });
                            if button(ui, "Paste Address", true) {
                                data.send_address = ui.input().get_from_clipboard().trim().to_string();
                            }
```

**Stake** (`ui.rs:2182-2190`) and **Retarget** (`ui.rs:2334-2342`) are the same code on `data.stake_address`, which the two modals share:

```rust
                    let mut stake_address = "0000000000000000";
                    if data.stake_address.len() >= 16 {
                        stake_address = &data.stake_address;
                    }

                    ui.text(frame_strf!(data, "[{}..{}]", &stake_address[0..8], &stake_address[stake_address.len()-8..]), TextDecl { font: Mono, h: ui.scale(20.0), colour: WHITE, align: AlignX::Center, ..TextDecl });
                    if button(ui, "Paste Identity", true) {
                        data.stake_address = ui.input().get_from_clipboard().trim().to_string();
                    }
```

`str::len` is a byte count, and `&s[a..b]` on a `str` panics when `a` or `b` is not a character boundary. Both are standard-library behaviour.

**Order within the frame.** The `ui.text` call precedes the button, so the frame in which the click lands still draws the old value. The slice of the new value happens on the following frame, and it happens **before** the validation that Stake and Retarget do run (`FinalizerAddress::decode` at `ui.rs:2246` and `:2354`). The validation would reject the string, but the code never gets there.

**Nothing filters the paste.** The Ctrl+V path into textboxes drops control characters (`lib.rs:1388`, `c >= ' ' && c != '\u{7f}'`), and textboxes store `Vec<char>`, so they are safe. The button path stores the raw string. There is also no length cap: the clipboard could hold megabytes, which would be stored and compared every frame.

**Why the process dies** (`zebra-crosslink/Cargo.toml:201-202` and `:280-281`):

```toml
[profile.dev]
panic = "abort"
```

```toml
[profile.release]
panic = "abort"
```

`zebrad` is built from this workspace, and `viz2::run_node` calls `zebra_gui::main_thread_run_program` on the process's main thread with the node on a spawned thread (`viz2.rs:37-40`). There is no `catch_unwind` anywhere in `zebra-gui/src` or `viz2.rs`. The standalone `zebra-gui` binary has its own profiles without `panic = "abort"`, but a panic on its main thread ends that process too.

**Every other byte-index slice of a string in the GUI, checked.** The sweep covered every range-index expression in `ui.rs`, `lib.rs` and `viz_gui.rs`:

| Site | String sliced | Source | Safe? |
| :- | :- | :- | :- |
| `ui.rs:1922` | `data.send_address` | clipboard | **No** |
| `ui.rs:2187` | `data.stake_address` | clipboard | **No** |
| `ui.rs:2339` | `data.stake_address` | clipboard | **No** |
| `ui.rs:2074` | `user_recv_ua` | wallet's own `user_ua.encode(network)` (`wallet/src/lib.rs:3636`, `:3648`), guarded by `len() != 0` | Yes: Bech32m is ASCII and far longer than 16 |
| `ui.rs:1365` | `FinalizerAddress::encode()` output | base64url, fixed 134 chars | Yes |
| `ui.rs:3594` | this node's own encoded finalizer address | same encoder | Yes |
| `ui.rs:3244` | `tx.txid.to_string()` | hex, 64 chars | Yes |
| `ui.rs:3254` | transaction memo, sliced at `find(":") + 1` | **sender-controlled**, but filtered to ASCII at `:3250` before `find`, and `:` is one byte | Yes |
| `ui.rs:1229` | `format_stake_amount` digits | formatted integer | Yes for boundaries (its arithmetic is finding G1) |
| `viz_gui.rs:2206` | chrono-formatted time, strips 4 bytes | ASCII format string | Yes |
| `ui.rs:850`, `:859-860`, `:5161`, `:5167` | textbox `text_buf` | `Vec<char>`, not `str` | Yes |
| `ui.rs:1209`, `:1213`, `viz_gui.rs:2406`, `:2410` | `[u8]` hash bytes | byte arrays | Yes |

The three paste sites are the only places a user-supplied or pasted string is sliced by byte index. The memo is the only other externally controlled string that is sliced at all, and its ASCII filter makes it safe.

## Recommendations

1. **Replace the three guard-and-slice blocks with one boundary-safe helper.** Add a free function next to `parse_ctaz` in `zebra-gui/src/ui.rs` and call it from all three modals. This is the whole crash fix; it is small and should land first.

   ```rust
   /// "[first8..last8]" by characters, never by bytes: the input is whatever the clipboard held.
   pub fn abbreviate_ends(s: &str, edge: usize) -> String {
       let count = s.chars().count();
       if count < 2 * edge {
           return format!("[{s}]");
       }
       let mut head_end = s.len();
       let mut tail_start = s.len();
       for (i, (byte_o, _)) in s.char_indices().enumerate() {
           if i == edge { head_end = byte_o; }
           if i == count - edge { tail_start = byte_o; }
       }
       format!("[{}..{}]", &s[..head_end], &s[tail_start..])
   }
   ```

   Call sites become, for Send:

   ```rust
   if data.send_address.is_empty() {
       ui.text("No address pasted", TextDecl { h: ui.scale(20.0), colour: WHITE.mul(0.6), align: AlignX::Center, ..TextDecl });
   } else {
       ui.text(frame_strf!(data, "{}", abbreviate_ends(&data.send_address, 8)), TextDecl { font: Mono, h: ui.scale(20.0), colour: WHITE, align: AlignX::Center, ..TextDecl });
   }
   ```

   Stake and Retarget are identical with `data.stake_address` and the text `No identity pasted`. Dropping the `"0000000000000000"` placeholder is deliberate: finding G6 records that `[00000000..00000000]` reads like a real destination.

2. **Sanitise at the paste, in one place.** Add a small function used by all three paste buttons (and by the dead textbox variants if they are revived), so the stored string is bounded and printable:

   ```rust
   const PASTED_ADDRESS_MAX_CHARS: usize = 512;

   fn pasted_address(ui: &Context) -> String {
       let raw = ui.input().get_from_clipboard();
       let mut out = String::new();
       for c in raw.trim().chars().take(PASTED_ADDRESS_MAX_CHARS) {
           // Same filter as the Ctrl+V path in lib.rs: no control characters.
           if c >= ' ' && c != '\u{7f}' { out.push(c); }
       }
       out
   }
   ```

   512 comfortably exceeds a finalizer address (134 characters) and a unified address with three receivers. This is where finding G2's `Option<String>` return and finding G6's validation attach, so write it once for all three.

3. **Considered alternatives, not chosen:**
   - *Add `s.is_ascii()` to the existing guard.* One line and it stops the panic, but non-ASCII pastes then silently show the all-zero placeholder, which is the misleading display G6 complains about.
   - *Validate first and only display a decoded address.* Correct in the end state (G6 does this), but as the sole fix it leaves the user with no view of what they pasted when it is rejected, and it couples the crash fix to a larger change.
   - *Wrap the UI pass in `catch_unwind`.* Not possible under `panic = "abort"`, and it would hide the next bug of this kind instead of preventing it.
   - *Use `str::get(..8)` and fall back to the placeholder.* Same objection as the first alternative.

4. **Tests.**
   - *Harness that exists:* plain `#[test]` modules in `zebra-gui/src/ui.rs:5326` and `zebra-gui/src/lib.rs:2400`, run with `cargo test -p zebra-gui`. They test free functions; there is no harness that renders a modal or injects a click.
   - *Unit test for item 1* in the existing `ui.rs` test module, driving the real function:

   ```rust
   #[test]
   fn abbreviate_ends_never_cuts_inside_a_character() {
       assert_eq!(abbreviate_ends("你好世界你好世界你好世界你好世界", 8), "[你好世界你好世界..你好世界你好世界]");
       assert_eq!(abbreviate_ends("0123456789abcdef", 8), "[01234567..89abcdef]");
       assert_eq!(abbreviate_ends("short", 8), "[short]");
       assert_eq!(abbreviate_ends("", 8), "[]");
       // One multi-byte character straddling byte 8 and one straddling len - 8.
       assert_eq!(abbreviate_ends("1234567é90123456é8901234", 8), "[1234567é..é8901234]");
   }
   ```

   - *Manual check (macOS or Linux with `xclip`):* copy `你好世界你好`, open Send, click Paste Address, wait one second. Expected: the modal shows `[你好世界你好]` (or blank boxes if the Mono font lacks the glyphs; glyph coverage was not checked) and the program keeps running. Repeat in Stake and Retarget. Before the fix the program exits on the first try.

5. **UX after the fix.** With nothing pasted: grey `No address pasted` / `No identity pasted` instead of `[00000000..00000000]`. With a short paste: the whole string in brackets. With a long paste: `[first8..last8]` counted in characters. Whether the paste is a valid address is G6's line, directly underneath.

## Validation Information

**Verdict: CONFIRMED. Severity: Medium.**

| Claim | Verified at |
| :- | :- |
| The guard is a byte-length check only | `ui.rs:1918`, `:2183`, `:2335` |
| The slices are byte-index `str` slices | `ui.rs:1922`, `:2187`, `:2339` |
| The pasted string is stored unvalidated, trimmed only | `ui.rs:1924`, `:2189`, `:2341`; `lib.rs:929` |
| The slice of a new paste runs on the next frame, ahead of any decode | order of `ui.text` and `button` in each block; decode at `ui.rs:2244-2247`, `:2352-2355` |
| A panic aborts the node process | `zebra-crosslink/Cargo.toml:201-202`, `:280-281`; `viz2.rs:37-42`; no `catch_unwind` in `zebra-gui/src` |
| No other pasted or user-supplied string is byte-sliced | table in Technical Details |
| The actual abort on a running build | **Not run.** Follows from standard-library slicing semantics and the profile settings |

**Deliberate?** No. There is no comment on the guard. The `>= 16` test shows the author meant to make the slice safe and thought in bytes; real addresses are ASCII, for which bytes and characters coincide.

**Severity justification.**

- *Why not High:* a crash ranks high on the scale in use, and this one stops the node. Against that: it needs the user to paste the wrong thing, the wrong thing must contain multi-byte characters at particular offsets, nothing is lost or corrupted, a restart fully recovers, and no money or stake figure is affected. It cannot be triggered remotely. On Windows it is unreachable today.
- *Why not Low:* it is a whole-process abort, including a finalizer's voting, from a single click on an always-enabled button, with no error message to explain it; for users whose everyday text is CJK the wrong-clipboard slip crashes every time; and fixing G2 extends its reach to Windows.

**Corrections made during validation.**

1. The review said the panic kills "the GUI thread". The GUI runs on the main thread of `zebrad` and both profiles set `panic = "abort"`, so the whole node process aborts.
2. The review said "any text of 16+ bytes with a multibyte char straddling byte 8 or len-8". Correct, with a sharper statement of which scripts qualify: pure 3-byte text always does; pure 2-byte text never does.
3. The review said "On macOS or Linux". Narrowed: Linux only where `xclip` or `xsel` is present, and Windows becomes affected once G2 is fixed.
4. Added the full sweep of other slice sites, including the sender-controlled memo at `ui.rs:3254`, which is safe because of the ASCII filter on the line above it.
5. Added: the paste is also unbounded in length and unfiltered for control characters.

**Cross-references.**

- `clipboard-has-no-windows-or-wayland-backend-so-paste-only-flows-cannot-complete.md` (G2): **this fix must land before, or with, G2's Windows clipboard backend.** Today the empty paste on Windows hides this crash there. `pasted_address` (item 2) is also where G2's `Option<String>` result is consumed.
- `send-modal-never-validates-the-address-and-a-failed-send-is-silent.md` (G6): the same three blocks of code. Item 1 here removes the `[00000000..00000000]` placeholder that G6 also objects to, and G6 adds the validity line beneath the abbreviated address. If both are implemented together, do item 1 and item 2 here first, then G6 on top; they do not conflict.
- G2's item 4 (enable the address textboxes) removes the byte slices in Send and Stake altogether, since the textbox variant does not abbreviate. Retarget would still need item 1.
