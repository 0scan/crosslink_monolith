# `get_from_clipboard` and `send_to_clipboard` only shell out to `pbpaste`/`pbcopy`, `xclip` and `xsel`, so on Windows (native or APE) and on any Linux desktop without those tools the paste-only Send, Stake and Retarget modals cannot be completed and every Copy button silently does nothing

**Severity**: High
**Validation Status**: Confirmed
**Location**: `zebra-gui/src/lib.rs:918-934` (`get_from_clipboard`), `:936-956` (`send_to_clipboard`), `:1383-1392` (`EVENT_COPYPASTE` / `CP_PASTE`, the Ctrl+V path); paste callers `zebra-gui/src/ui.rs:1924` (Send), `:2189` (Stake), `:2341` (Retarget), dead textbox variants `:1927-1961`, `:2192-2226`; copy callers `zebra-gui/src/ui.rs:854`, `:2108`, `:2794`, `:2798`, `:3578`, `:3614`, `:3616`, `:3776`, `:4801`, `:4822`, `:4842`; backend selection `patches/softer_gui-3.0.1/src/lib.rs:195-232`; Win32 bindings `patches/softer_gui-3.0.1/src/sys_win.rs:288-324`, `:368-409`; Windows build evidence `zebra-gui/build.rs:12-13`, `:48-59`, `zebra-gui/Cargo.toml:35-36`, `zebra-gui/build.bat`
**Found by agent:** /code-review high, GUI/UX (Claude Fable 5.1), 2026-09-30; validated 2026-10-01 at dev d00b6a44ef594
**In scope of audit?** Yes. Clipboard access is the only way to enter an address in three wallet modals. The gap was introduced by `eda288f9` ("APE build, softer_gui 3.0.2, and device-normalized input for zebra-gui", 2026-09-28), which moved `zebra-gui` from winit to `softer_gui` and deleted the `copypasta` fallback that had covered Windows and Wayland. `41594328` (2026-09-29) added `pbpaste`/`pbcopy` back for macOS only.

## Description

The GUI has exactly two clipboard functions, both on `InputCtx`. Each walks a fixed table of external programs and runs the first one that works:

| Function | Programs tried, in order | Result when none works |
| :- | :- | :- |
| `get_from_clipboard` (`lib.rs:918`) | `pbpaste` (macOS and APE builds only), `xclip`, `xsel` | empty `String` |
| `send_to_clipboard` (`lib.rs:936`) | `pbcopy` (macOS and APE builds only), `xclip`, `xsel` | `false` |

There is no Win32 path, no `wl-paste`/`wl-copy` path, and no in-process path of any kind. `softer_gui` itself exposes no clipboard API: it reports the copy/paste chord as an intent (`EVENT_COPYPASTE`) and leaves fetching to the application (`lib.rs:1384-1385`).

Three consequences follow, all verified by reading:

1. **Paste returns nothing where no helper program exists.** `Command::new("xclip")` fails to spawn, the loop falls through, and the function returns `String::new()`. This is every Windows machine, and every Linux machine that has neither `xclip` nor `xsel` installed.
2. **Three modals have no other input.** Send, Stake and Retarget take their address only from a `Paste Address` / `Paste Identity` button. The textbox variants are behind `if true { ... } else { ... }` and never run. With an empty paste, Stake and Retarget stay disabled forever, and Send stays disabled too (its guard is `send_address.len() != 0`).
3. **Nobody is told.** All eleven `send_to_clipboard` callers discard the returned `bool`. `get_from_clipboard` cannot even express failure: an empty clipboard and a missing backend both yield `""`. No "Copied", no "Clipboard unavailable", on any platform.

Native Windows is a real target of this repo, not a hypothetical one (evidence in Technical Details), and the Cosmopolitan APE build of `zebrad` is explicitly meant to run on Windows (`d00b6a44`, the commit under review, fixes its Windows startup).

## Attack Scenario and Steps

No attacker. This is the ordinary first-use path of a testnet participant on Windows.

1. Start `zebrad` on Windows 11 (native build or the APE). The window opens through `softer_gui`'s Win32 backend.
2. Get cTAZ from the faucet, then click **Stake**.
3. Copy a finalizer's `zfinv1...` address (from chat, a web page, or the roster's own copy icon; the roster icon does nothing either, see step 6).
4. Click **Paste Identity**. The line above the button still reads `[00000000..00000000]`.
5. Every `+N cTAZ` button stays greyed out. There is no message. Ctrl+V does nothing because the modal has no textbox to receive it.
6. Open **Receive** and click **Copy Address**. Nothing is copied; the previous clipboard content is still there. The same holds for Copy Seed, Copy Viewing Key, Copy Identity, the roster copy icon, "Copy finalizer data as JSON", and the three copy buttons in the block inspector.

**Attack Requirements and Assumptions:**

- Platform: Windows (native or APE), or Linux with neither `xclip` nor `xsel` on `PATH`. Stock desktop installs of the major distributions do not ship either tool by default (from general knowledge, not checked against a specific distribution image for this report).
- On Linux Wayland sessions `softer_gui` opens the Wayland backend first (`softer_gui/src/lib.rs:213-217`). `xclip` and `xsel` are X11 clients, so they reach the clipboard only through XWayland and only if installed. Whether the compositor bridges the Wayland clipboard to an unfocused X client varies; this was not tested.
- macOS is not affected: `pbpaste` and `pbcopy` are part of the base system.

## Impact on Users

| Platform | Paste (Send / Stake / Retarget) | Copy buttons, Ctrl+C in textboxes | Ctrl+V into textboxes (Jump, Convert Commission, filters) |
| :- | :- | :- | :- |
| Windows native | **Never works** | Never works | Never works |
| Windows, APE | **Never works** | Never works | Never works |
| macOS native or APE | Works | Works | Works |
| Linux X11 with `xclip` or `xsel` | Works | Works | Works |
| Linux X11 without them | **Never works** | Never works | Never works |
| Linux Wayland | Only via XWayland plus `xclip`/`xsel`; not verified | Same | Same |

- **Staking is unreachable from the GUI on the affected platforms.** Stake and Retarget need a verified `FinalizerAddress`, and paste is the only source. Staking is what the testnet exists to exercise.
- **Sending is unreachable** for the same reason.
- **Receiving is impaired.** The Receive modal shows only `[first8..last8]` of the unified address and relies on Copy Address to hand over the full string. With copy broken the user has no way to read their own address out of the GUI. The same applies to the seed phrase and viewing key in the User modal: they are never rendered, only copied.
- **No funds are at risk and nothing is displayed wrong.** The flows are blocked, silently.
- A workaround exists outside the GUI: the wallet RPCs (`BASIC_SEND_STAGE`, the staking action requests) accept addresses as strings.

## Technical Details / Code Analysis

**The paste function** (`zebra-gui/src/lib.rs:926-933`, inside the loop over the program table at `:921-925`):

```rust
            if program == "pbpaste" && !cfg!(any(target_os = "macos", cosmo)) { continue; }
            if let Ok(output) = std::process::Command::new(program).args(args).output() {
                if output.status.success() {
                    return String::from_utf8_lossy(&output.stdout).into_owned();
                }
            }
        }
        return String::new();
```

The comment at `:919-920` states the design: "An APE is built for cosmo even when it runs on macOS; try both clipboard families there. Native Linux keeps X11 first." Windows is not mentioned.

**The copy function returns a bool that no caller reads** (`lib.rs:945-955`):

```rust
            if let Ok(mut child) = std::process::Command::new(program).args(args).stdin(std::process::Stdio::piped()).spawn() {
                let wrote = match child.stdin.take() {
                    Some(mut stdin) => stdin.write_all(text.as_bytes()).is_ok(),
                    None => false,
                };
                if let Ok(status) = child.wait() {
                    if wrote && status.success() { return true; }
                }
            }
        }
        return false;
```

A representative caller (`ui.rs:2107-2109`):

```rust
                            if button(ui, "Copy Address", true) {
                                ui.input().send_to_clipboard(&ua);
                            }
```

**Paste is the only input in the three modals.** Send (`ui.rs:1916-1927`):

```rust
                        if true {
                            let mut send_address = "0000000000000000";
                            if data.send_address.len() >= 16 {
                                send_address = &data.send_address;
                            }

                            ui.text(frame_strf!(data, "[{}..{}]", &send_address[..8], &send_address[send_address.len() - 8..]), TextDecl { font: Mono, h: ui.scale(20.0), colour: WHITE, align: AlignX::Center, ..TextDecl });
                            if button(ui, "Paste Address", true) {
                                data.send_address = ui.input().get_from_clipboard().trim().to_string();
                            }

                        } else {
                            // New version: TODO: finish
```

Stake has the same shape at `ui.rs:2181-2193` with the comment `// new version: TODO: finish`. Retarget (`ui.rs:2334-2342`) has no textbox variant at all, dead or alive.

**The textbox paths are dead code: confirmed.** `git blame` attributes the `if true` wrappers to `c9231740` ("GUI: temp revert stake/send amount UI", Andrew Reece, 2026-05-15), which reverted `7a9489ad` ("UI: refactored wallet modals with textbox inputs and amount selectors", 2026-05-05). The commit title says "amount UI", but the address textboxes were switched off in the same revert. Even if they were live, Ctrl+V into them goes through the same `get_from_clipboard` (`lib.rs:1386-1389`), so they would allow typing a 134-character address by hand and nothing more.

**Windows is a real GUI target.** Evidence, all read directly:

| Evidence | Where |
| :- | :- |
| `softer_gui` ships a Win32 backend, compiled for `target_os = "windows"` and for `cosmo` | `patches/softer_gui-3.0.1/src/lib.rs:64-71`, `:158-159`, `:227-231` |
| An APE picks the Win32 backend at run time when the host is Windows | `patches/softer_gui-3.0.1/src/lib.rs:206-210` |
| `zebra-gui`'s build script embeds a Win32 icon resource under `#[cfg(windows)]` | `zebra-gui/build.rs:12-13`, `:48-59`; `Cargo.toml:35-36` |
| A Windows batch build script and Visual Studio project sit in the crate | `zebra-gui/build.bat`, `zebra-gui.sln`, `zebra-gui.vcxproj` |
| The audio module has a `target_os = "windows"` backend | `zebra-gui/src/audio.rs:253`, `:262` |
| Dev-only Win32 window arrangement in the node's GUI launcher | `zebra-crosslink/zebra-crosslink/src/viz2.rs:60-65` |
| HEAD fixes the zebrad APE's startup on Windows 11 | `d00b6a44` commit message |

**What "one binary" and the APE mean for clipboard access.** Since `b67c73ad` every `zebrad` opens the window on the main thread when a display exists (`viz2.rs:20-44`), so the GUI is what a default start shows on a desktop. In an APE, `cfg!(cosmo)` is true on every host, so the loop tries `pbpaste`, `xclip`, `xsel` in that order on Windows as well; none exists there. The APE cannot name a DLL at link time, but `softer_gui` already solves that: its `win32!` macro resolves `user32`/`kernel32` exports through `cosmo_dlsym` in a cosmo build and through a normal `#[link]` block otherwise (`sys_win.rs:288-324`). A clipboard implementation written against that macro works in both build shapes with one call site.

**Why this regressed.** Before `eda288f9` both functions ended with a `copypasta::ClipboardContext` fallback, which has Windows, macOS, X11 and Wayland backends. The migration removed it with winit. The commit message records: "Not verified here: ... Linux/Windows runs of softer_gui 3."

## Recommendations

1. **Add an in-process Win32 clipboard to the vendored `softer_gui` and call it first on Windows.** This fixes native Windows and the APE on Windows with one implementation.
   - In `patches/softer_gui-3.0.1/src/sys_win.rs`, extend the existing `win32!` block. All of these predate Vista, so the file's compatibility rule (`sys_win.rs:11-17`) is respected: `user32`: `OpenClipboard`, `CloseClipboard`, `EmptyClipboard`, `GetClipboardData`, `SetClipboardData`; `kernel32`: `GlobalAlloc`, `GlobalLock`, `GlobalUnlock`, `GlobalFree`.
   - Add two public functions to `softer_gui` (for example in `win.rs`, re-exported from `lib.rs`), compiled under `#[cfg(any(target_os = "windows", cosmo))]`:

   ```rust
   const CF_UNICODETEXT: u32 = 13;
   const GMEM_MOVEABLE: u32 = 0x0002;

   pub fn clipboard_get() -> Option<String> {
       unsafe {
           if OpenClipboard(NULL) == 0 { return None; }
           let mut text = None;
           let handle = GetClipboardData(CF_UNICODETEXT);
           if !handle.is_null() {
               let ptr = GlobalLock(handle) as *const u16;
               if !ptr.is_null() {
                   let mut len = 0;
                   while *ptr.add(len) != 0 { len += 1; }
                   text = Some(String::from_utf16_lossy(core::slice::from_raw_parts(ptr, len)));
                   GlobalUnlock(handle);
               }
           }
           CloseClipboard();
           text
       }
   }
   ```

   - `clipboard_set(text: &str) -> bool` is the mirror: encode to UTF-16 with a trailing nul, `GlobalAlloc(GMEM_MOVEABLE, bytes)`, copy, `OpenClipboard`, `EmptyClipboard`, `SetClipboardData(CF_UNICODETEXT, handle)`, `CloseClipboard`. On `SetClipboardData` failure, `GlobalFree` the block; on success the system owns it.
   - In `zebra-gui/src/lib.rs`, at the top of both functions: on a native Windows build call these unconditionally; in a cosmo build call them when `softer_gui::sys_win::cosmo::is_windows()` is true. Fall through to the program table otherwise.
   - `OpenClipboard` can fail transiently while another process holds the clipboard. Retry a few times a millisecond apart before giving up.
   - Send the change upstream to `ShieldedLabs/softer_gui` so the patch directory does not grow a permanent fork.

2. **Add `wl-paste` / `wl-copy` for Wayland sessions.** In the program tables, when `WAYLAND_DISPLAY` is set, try `wl-paste -n` (paste) and `wl-copy` (copy) before `xclip`. Keep `xclip` and `xsel` as fallbacks. This is two table rows and an environment check.

3. **Make failure visible, on every platform.** This item touches the same call sites as findings G3 and G6 and should land with them.
   - Change `get_from_clipboard` to return `Option<String>`: `None` when no backend produced an answer, `Some("")` for a genuinely empty clipboard.
   - Paste buttons: on `None`, set a per-modal notice shown under the button in the existing warning colour `(0xff, 0xaf, 0x0e, 0xff)`. Exact text on Linux: `Clipboard unavailable. Install wl-clipboard, xclip or xsel.` On other platforms: `Clipboard unavailable.` On `Some("")`: `Clipboard is empty.`
   - Copy buttons: read the returned `bool`. On success show `Copied` for about 1.5 s next to the pointer through the existing tooltip mechanism (`set_tooltip_text!`), driven by one `Option<(Instant, bool)>` on `UiData`; on failure show `Copy failed: clipboard unavailable` the same way.
   - Where copy is the only way to see a value (Receive address, seed, viewing key), a failed copy should also reveal the full string in the modal so the user can transcribe it. The dead Receive textbox variant at `ui.rs:2075-2105` is the intended read-only display; it can be enabled for this case.

4. **Enable the address textboxes so paste is not the only input.** Turn on the dead `else` branches for the address field only in Send (`ui.rs:1927-1961`) and Stake (`ui.rs:2192-2226`), keep the current amount buttons, and give Retarget the same textbox. This needs finding G3's display fix first (the textbox variant does not byte-slice, so it also removes two of G3's three panic sites) and benefits from G6's validation line. Before doing this, ask why `c9231740` reverted it: the commit carries no explanation, and the reason may still apply.

5. **Considered alternatives, not chosen:**
   - *Shell out to `powershell -NoProfile -Command Get-Clipboard` and `clip.exe`.* No FFI, but PowerShell start-up blocks the UI thread for hundreds of milliseconds per click, `clip.exe` reads its input in the console code page so non-ASCII text is mangled, and whether a cosmo build can spawn them by bare name was not verified. Acceptable as a one-day stopgap only.
   - *Bring back `copypasta`.* It cannot build for the `*-unknown-cosmo` targets and pulls in the X11 and Wayland client libraries `softer_gui` exists to avoid.
   - *Declare the Win32 imports in `zebra-gui` under `#[cfg(windows)]`.* Works for native builds only; the APE on Windows has `target_os = "linux"` and would stay broken.
   - *Speak the X11 selection and Wayland data-device protocols in `softer_gui`.* The right long-term answer for Linux (no helper programs), but it is a substantial piece of protocol work. Items 2 and 3 make the present state usable and honest in the meantime.

6. **Tests.**
   - *Harness that exists:* `cargo test -p zebra-gui` runs plain `#[test]` functions in `zebra-gui/src/lib.rs:2400-2620` (input-snapshot logic on a bare `InputCtx`) and `zebra-gui/src/ui.rs:5326-5353` (pure helper functions). Nothing drives the UI with synthetic input, and nothing opens a window. `ZEBRA_TEST_GUI` (`zebrad/tests/crosslink.rs:110-112`) opens the real window during node tests but injects no clicks.
   - *Cheapest real check for item 1:* a `#[cfg(windows)] #[test]` in `softer_gui` that calls `clipboard_set("zfinv1-test-äß")` then asserts `clipboard_get() == Some(...)`. It touches the real clipboard, so mark it `#[ignore]` and run it by hand or in a dedicated CI step.
   - *Item 3:* factor the program-table walk so it takes the table as a parameter; a unit test passes a table naming a program that does not exist and asserts `None` / `false`.
   - *Manual, per platform (Windows native, Windows APE, macOS, Linux X11, Linux Wayland):* (a) Receive, Copy Address, paste into a text editor: the full `utest1...` string appears and the GUI shows `Copied`. (b) Copy a roster member's address with the copy icon, open Stake, Paste Identity: the line shows `[zfinv1AA..last8]` and the stake buttons enable on a Staking Day. (c) On Linux with `xclip`, `xsel` and `wl-clipboard` all absent: both actions show the "Clipboard unavailable" text.

## Validation Information

**Verdict: CONFIRMED. Severity: High.**

| Claim | Verified at |
| :- | :- |
| Clipboard access is only `pbpaste`/`pbcopy`, `xclip`, `xsel` | `zebra-gui/src/lib.rs:918-956`; `grep -i clipboard` over the repo finds no other implementation |
| `softer_gui` has no clipboard API, only the chord intent | `patches/softer_gui-3.0.1/src/event.rs:90`, `:100`, `:439-441`; no `clipboard` match in the crate |
| Every `send_to_clipboard` caller ignores the `bool` | all eleven call sites listed under Location, read individually |
| Missing backend and empty clipboard are indistinguishable on paste | `lib.rs:933` |
| Send, Stake, Retarget accept an address only by paste | `ui.rs:1916-1926`, `:2181-2190`, `:2334-2342` |
| Textbox variants are dead | `if true` at `ui.rs:1916`, `:2181`; blame to `c9231740` |
| Native Windows GUI build is a real target | table in Technical Details |
| The APE selects the Win32 backend on a Windows host | `patches/softer_gui-3.0.1/src/lib.rs:206-210` |
| The gap is a regression from the winit removal | `git show eda288f9`, restricted to `zebra-gui/src/lib.rs`, removes the `copypasta` fallback |
| Behaviour on a running Windows build | **Not run** (the brief forbids building). Inferred from the code: `Command::new` on a program that is not on `PATH` returns `Err`, and the loop falls through |

**Deliberate?** Not as far as the tree shows. No comment or TODO mentions Windows or Wayland clipboard. The only design note (`lib.rs:919-920`) covers macOS under the APE. `eda288f9` states its Windows run was not verified. The reading that fits is an unnoticed regression three days before this review.

**Severity justification.**

- *Why High rather than Medium:* by the scale in use a blocked flow is Medium, but this one blocks all three address-taking wallet flows at once (Send, Stake, Retarget) for an entire first-class platform, plus every copy action, including the only way to get one's own receive address out of the GUI. Staking is the activity the testnet is launched to exercise. It needs no unusual input: it is what happens on the first click.
- *Why not higher:* High is the top of the scale. It deserves the lower end of High: no figure is wrong, no funds move, nothing crashes, macOS and tooled-up Linux are unaffected, and an RPC workaround exists.
- *What would lower it to Medium:* a decision that Windows and tool-less Linux are unsupported for the testnet launch, stated to users.

**Corrections made during validation.**

1. The review framed Windows as "a native Windows build (softer_gui has a win backend)". The APE build is equally affected and is the more likely thing a testnet participant on Windows runs; added.
2. The review listed Wayland as having no backend. More precisely: there is no Wayland-native path, and `xclip`/`xsel` may still work through XWayland where installed. Not tested.
3. Added a platform the review missed: Linux X11 with neither `xclip` nor `xsel` installed fails exactly like Windows.
4. Added the cause: the `copypasta` fallback was removed in `eda288f9`, so this is a regression, not an original omission.
5. "Ctrl+V silently does nothing" needs a qualifier: in Send, Stake and Retarget it does nothing on any platform, because there is no textbox to receive it. In modals with a textbox it works wherever paste works.
6. The review counted the "textbox variants" for all three modals. Retarget has none.

**Cross-references.**

- `pasted-non-ascii-text-panics-the-gui-on-a-byte-index-slice.md` (G3): **ordering constraint.** Today an empty paste on Windows shields Windows users from G3. Fixing this finding makes G3 reachable there, so G3's fix must land before or together with item 1.
- `send-modal-never-validates-the-address-and-a-failed-send-is-silent.md` (G6): item 3 here (paste feedback) and G6's validation line are drawn in the same few lines of each modal and share one "notice under the paste button" slot. Land them as one change to those lines.
- Suggested landing order across the three: G3 item 1, then this finding's item 1 and 2, then one combined change for this item 3 plus G6, then item 4.
