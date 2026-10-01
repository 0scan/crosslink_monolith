# The Send modal enables its amount buttons for any non-empty pasted string, and `WalletState::send_to_address` only `println!`s when the address does not decode, so a send to anything but a testnet unified address with an Ironwood receiver does nothing and tells the user nothing

**Severity**: Medium
**Validation Status**: Confirmed
**Location**: `zebra-gui/src/ui.rs:1917-1925` (placeholder and paste), `:1992` (`can`), `:2008-2012` (amount buttons and the call); `wallet/src/lib.rs:1141-1156` (`send_to_address`), `:5034-5044` (`WalletAction::SendToAddress` handler), `:5081-5085` (failed action is logged and dropped), `:2122-2139` (`send_ironwood_to_ironwood_zats`), `:2047-2048` (the "can't afford" exit), `:4804`, `:4834` (`waiting_for_send` recomputed); `librustzcash/zcash_keys/src/encoding.rs:180-200` (`UnifiedAddress::decode`), `librustzcash/zcash_keys/src/address.rs:32-99` (`TryFrom<unified::Address>`); comparison: `zebra-gui/src/ui.rs:2244-2250` (Stake), `:2352-2358` (Retarget), `librustzcash/zcash_primitives/src/bft.rs:805-807`, `:834-856` (`FinalizerAddress::verify`, `decode`); the RPC path that already reports the same errors, `wallet/src/lib.rs:4944-4968`
**Found by agent:** /code-review high, GUI/UX (Claude Fable 5.1), 2026-09-30; validated 2026-10-01 at dev d00b6a44ef594
**In scope of audit?** Yes. Send is one of the wallet's primary flows. `send_to_address` and its `println!` date from `d5cbc152` / `1eccd0c2` ("wallet gui: basic send screen", Judah, 2025-12-01). The Stake modal's cached decode-and-verify came later, in `32e882b8` ("Crosslink: Finalizer addresses as capabilities", 2026-09-02); Send was not brought up to the same standard.

## Description

The Send modal has one gate on its amount buttons (`ui.rs:1992`):

```rust
let can = !waiting_for_send && data.send_address.len() != 0;
```

Any non-empty string passes. The string is whatever the clipboard held when the user clicked Paste Address. Clicking an amount hands that raw string to the wallet, which is where the address is first parsed, and where four different things can go wrong. None of them reaches the screen:

| What goes wrong | Where | What the code does | What the user sees |
| :- | :- | :- | :- |
| String is not a testnet unified address | `wallet/src/lib.rs:1142-1145` | `println!`, return | Nothing |
| Unified address decodes but has no Orchard (Ironwood) receiver | `wallet/src/lib.rs:5035`, `:5041-5043`, `:5081-5083` | action yields `false`, `println!`, action dropped | Buttons grey out for a moment, then come back |
| Same address and amount already queued | `wallet/src/lib.rs:1147-1152` | silent return | Nothing |
| Address is fine but the wallet cannot assemble the spend | `wallet/src/lib.rs:5037-5040`, `:2047-2048` | `println!`; the handler returns `true` regardless | Buttons grey out for a moment, then come back |

The modal does not close on a click and shows no status line, so success and all four failures look the same for the first moments. Only a successful send later adds a pending row to the transaction history.

The Stake and Retarget modals are stricter. They decode and verify the pasted string once, cache the result, and enable their buttons only when it is a verified `FinalizerAddress`. They, too, show no message when the string is rejected, but they cannot be made to submit garbage.

The display adds to the confusion. With nothing pasted, or with a paste shorter than 16 bytes, the modal shows `[00000000..00000000]`, which looks like a real destination.

## Attack Scenario and Steps

No attacker. A participant makes an ordinary mistake.

1. Fund the wallet from the faucet and open **Send**.
2. Copy an address that is not a testnet unified address with an Ironwood receiver. Realistic examples: a transparent `tm...` address from another wallet; a finalizer's `zfinv1...` identity copied from the roster a moment ago; a mainnet `u1...` address; a unified address with a character lost in copying.
3. Click **Paste Address**. The modal shows `[first8..last8]` of the string. The amount buttons `0.1` to `141` are enabled, limited only by balance.
4. Click `5`. Nothing changes: the modal stays open, the buttons stay enabled, no error appears, no transaction appears in the history.
5. The user cannot tell whether 5 cTAZ left the wallet. Clicking again repeats the same nothing.

A second route needs no bad address at all:

1. Paste a valid address while the balance is 5.0 cTAZ.
2. Click `5`. The button is enabled because the check is `balance >= amount` with no allowance for the fee (`ui.rs:2009`), and because `user_balance()` counts pending and unshielded funds (`wallet/src/lib.rs:1082`).
3. The wallet logs `tx build error: can't afford ...` and drops the action. On screen the buttons flicker and return. That the fee is what pushes `min_spend` above the balance is an inference from the message at `:2047`; the fee arithmetic was not traced.

**Attack Requirements and Assumptions:**

- **Platform:** any platform where paste returns text (macOS; Linux with `xclip` or `xsel`). Where paste returns nothing (finding G2) the buttons stay disabled, which is the one case the current gate handles.
- **Wallet state:** a balance of at least 0.1 cTAZ, so that at least one amount button is enabled.
- **Stdout is not visible.** The `println!` lines go to the terminal `zebrad` was started from. A user who launched from a desktop shortcut, or on Windows without a console, never sees them.

## Impact on Users

- **Uncertainty about whether money moved.** This is the main harm. The send did not happen, so no funds are lost, but the GUI gives the user no way to know that.
- **Some address kinds cannot be sent to, and the GUI does not say so.** The wallet only builds Ironwood outputs for sends. Transparent, Sapling and transparent-only unified addresses are all refused, some at decode and some later.
- **A misleading placeholder.** `[00000000..00000000]` is shown before anything is pasted and again after a too-short paste.
- **No wrong figures.** Balances and history remain correct throughout.

## Technical Details / Code Analysis

**From click to failure.** The button (`zebra-gui/src/ui.rs:2008-2012`):

```rust
                                for send in sends {
                                    if button(ui, send.0, can && (balance as u64) >= send.1) {
                                        wallet_state.lock().unwrap().send_to_address(data.send_address.clone(), send.1);
                                    }
                                }
```

The wallet entry point (`wallet/src/lib.rs:1141-1156`):

```rust
    pub fn send_to_address(&mut self, address: String, amount: u64) {
        let Ok(address) = UnifiedAddress::decode(&TEST_NETWORK /* @todo */, &address) else {
            println!("Invalid address for send: {}", address);
            return;
        };

        if self.actions_in_flight.iter().filter(|a| match a {
            WalletAction::SendToAddress(addr, amt) if amt.into_u64() == amount && addr.eq(&address) => true,
            _ => false
        }).count() != 0 {
            return;
        }

        self.waiting_for_send = true;
        self.actions_in_flight.push_back(WalletAction::SendToAddress(address, Zatoshis::from_u64(amount).expect("Invalid amount given to stake_to_finalizer")));
    }
```

The function returns `()`, so the GUI has nothing to react to.

**The wallet loop** (`wallet/src/lib.rs:5034-5044`):

```rust
                WalletAction::SendToAddress(address, amount) => {
                    if let Some(orchard_address) = address.orchard() {
                        let memo = MemoBytes::from_bytes("send from user wallet".as_bytes()).unwrap();
                        let ok = user_wallet.send_ironwood_to_ironwood_zats(network, &mut proposed_send, &mut client, &user_usk, amount.into_u64(), &orchard_tree, *orchard_address, memo).is_some();
                        just_init_new_tx |= ok;
                        if DUMP_ACTIONS { println!("Try user send: {ok:?}"); }
                        true // ALT ok
                    } else {
                        false
                    }
                }
```

Two things to note. A unified address without an Orchard receiver yields `false`, which leads only to `println!("** Failed to process action: ...")` at `:5082` and a `pop_front` at `:5085`. And when the spend cannot be prepared, `ok` is false but the arm returns `true` anyway, so not even that log line is printed; the specific reason was already printed deeper down (for example `:2047`).

**Why the buttons flicker.** `send_to_address` sets `waiting_for_send = true` (`:1154`). The wallet loop recomputes the flag from whether a proposed send is in progress (`:4804`) and writes it back on every state push (`:4834`). After a failed action there is no proposed send, so the flag returns to `false`.

**What `send_to_address` accepts.** `UnifiedAddress::decode` here is the `AddressCodec` implementation (`zcash_keys/src/encoding.rs:187-199`):

```rust
    fn decode(params: &P, address: &str) -> Result<Self, String> {
        unified::Address::decode(address)
            .map_err(|e| format!("{e}"))
            .and_then(|(network, addr)| {
                if params.network_type() == network {
                    UnifiedAddress::try_from(addr).map_err(|e| e.to_owned())
                } else {
                    Err(format!(
                        "Address {address} is for a different network: {network:?}"
                    ))
                }
            })
    }
```

Combined with the Orchard check in the wallet loop, the outcome by address kind is:

| Pasted string | `decode` | Wallet loop | Net result |
| :- | :- | :- | :- |
| Testnet unified address (`utest1...`) with an Orchard receiver | Ok | Ironwood send attempted | **Works** if funds suffice |
| Testnet unified address with Sapling and/or transparent receivers only | Ok (`try_from` at `address.rs:32-99` does not require Orchard) | `false`, dropped | Silent failure, after a flicker |
| Mainnet (`u1...`) or regtest (`uregtest1...`) unified address | Err, "different network" | not reached | Silent failure |
| Transparent `tm...`, Sapling `ztestsapling...`, TEX | Err, not a unified encoding | not reached | Silent failure |
| Finalizer address `zfinv1...` | Err | not reached | Silent failure |
| Truncated or mistyped unified address, arbitrary text | Err (Bech32m checksum or padding) | not reached | Silent failure |

The network is hard-coded to `TEST_NETWORK` with an `@todo` at `:1142`; the wallet loop uses the same constant (`:3529`), so the two agree today.

**How Stake validates, for comparison** (`zebra-gui/src/ui.rs:2244-2250`):

```rust
                        if data.stake_address != data.stake_address_checked {
                            data.stake_address_checked = data.stake_address.clone();
                            data.stake_address_cap = wallet::bft::FinalizerAddress::decode(&data.stake_address).filter(|a| a.verify());
                        }
                        let hex_dest = data.stake_address_cap;

                        let can = is_staking_day && !waiting_for_stake_to_finalizer && hex_dest.is_some();
```

| | Send | Stake / Retarget |
| :- | :- | :- |
| Parsed in the GUI before enabling buttons | No | Yes: `FinalizerAddress::decode`, exact length, `zfinv1` prefix, base64url alphabet |
| Cryptographic check | None possible for a UA beyond its checksum | Yes: `verify()` checks the ed25519 signature the address carries |
| Result cached per pasted string | No | Yes: `stake_address_checked` / `stake_address_cap` |
| Buttons enabled on | non-empty string | `hex_dest.is_some()` |
| Wallet call takes | `String` | the decoded `FinalizerAddress` |
| Message when invalid | None | None |

So the review's statement that Stake "already caches a decode and verify result" is correct, and Send lacks it. Neither modal tells the user why its buttons are disabled.

**The RPC path already has the right error strings** (`wallet/src/lib.rs:4963-4967`):

```rust
                } else if ua.is_err() {
                    let _ = sender.send(Err(format!("invalid unified address: {address}")));
                } else {
                    let _ = sender.send(Err(format!("unified address has no Ironwood receiver: {address}")));
                }
```

The same two conditions exist for RPC callers, with messages. The GUI path decodes separately and drops them.

## Recommendations

1. **Decode once, in one wallet function, and make `send_to_address` take the decoded address.** This mirrors how `stake_to_finalizer` takes a `FinalizerAddress`, and gives the GUI, the RPC path and the wallet loop a single rule.
   - In `wallet/src/lib.rs`, add:

   ```rust
   /// The one definition of "an address this wallet can send to": a unified address for
   /// the wallet's network that carries an Ironwood receiver.
   pub fn decode_send_address(address: &str) -> Result<UnifiedAddress, String> {
       let ua = match UnifiedAddress::decode(&TEST_NETWORK /* @todo */, address) {
           Ok(ua) => ua,
           Err(_) => return Err("Not a testnet unified address".to_string()),
       };
       if ua.orchard().is_none() {
           return Err("Address has no Ironwood receiver".to_string());
       }
       Ok(ua)
   }
   ```

   - Change the signature to `pub fn send_to_address(&mut self, address: UnifiedAddress, amount: u64)` and delete the `let Ok(..) else { println!; return }` block. Re-export `UnifiedAddress` from `wallet` so the GUI can name the type.
   - Use `decode_send_address` at `:4949-4950` in the RPC path too, keeping its existing error strings if RPC clients depend on them.
   - `zebra-crosslink/AGENTS.md` asks for `expect` messages that say why the invariant holds; while here, the message at `:1155` names the wrong function ("stake_to_finalizer").

2. **Validate in the Send modal with the same cache pattern as Stake, and gate the buttons on it.** In `UiData` (`ui.rs:38`), next to the stake fields, add `send_address_checked: String`, `send_address_ua: Option<wallet::UnifiedAddress>` and `send_address_err: String`. In the modal:

   ```rust
   if data.send_address != data.send_address_checked {
       data.send_address_checked = data.send_address.clone();
       match wallet::decode_send_address(&data.send_address) {
           Ok(ua) => { data.send_address_ua = Some(ua); data.send_address_err.clear(); }
           Err(err) => { data.send_address_ua = None; data.send_address_err = err; }
       }
   }
   let can = !waiting_for_send && data.send_address_ua.is_some();
   ```

   and the click becomes `send_to_address(data.send_address_ua.clone().unwrap(), send.1)`.

3. **Show why, in all three modals.** Directly under the abbreviated address, in the existing warning colour `(0xff, 0xaf, 0x0e, 0xff)` at `h: ui.scale(16.0)` (the style the Convert Commission modal already uses at `ui.rs:2147`):
   - Send, nothing pasted: grey `No address pasted` in place of `[00000000..00000000]` (this replacement is item 1 of finding G3).
   - Send, pasted and rejected: `Not a testnet unified address` or `Address has no Ironwood receiver`, from `send_address_err`.
   - Stake and Retarget, pasted and rejected: `Not a valid finalizer address (expected zfinv1...)`. If the string decodes but `verify()` fails: `Finalizer address signature does not verify`. That needs the cache line split into decode then verify instead of the current `.filter(|a| a.verify())`.
   - Send, valid: the abbreviated address in white, as now.

4. **Report the outcome of a click.** After a click the user should see one of two things within a second or so.
   - *Accepted:* close the modal (`ui.modal = Modal::None`), as Retarget and Convert already do (`ui.rs:2363`, `:2169`). The pending row in the transaction history is then the confirmation.
   - *Failed to prepare:* add `pub send_error: String` to `WalletState`. In the `SendToAddress` arm, when `send_ironwood_to_ironwood_zats` returns `None`, set it to `Could not build the send. Check that funds are confirmed and cover the fee.`; clear it in `send_to_address`. The wallet tab shows it in the warning colour until the next send. Return the real `ok` from the arm instead of `true // ALT ok` so the existing "Failed to process action" log line fires as well. Check first why the `ALT` was chosen; the same idiom is used for the faucet at `:5024`.
   - *Fee headroom:* enable an amount button only when `user_shielded_spendable_funds` covers amount plus fee, not when `user_balance()` covers the amount. This is a separate, smaller change and can follow.

5. **Considered alternatives, not chosen:**
   - *Have `send_to_address` return `Result<(), String>` and show the error after the click.* Fixes the silence but still lets the user click a button that cannot work. Validating before enabling matches Stake and is no more code.
   - *Validate in the GUI by calling `zcash_keys` directly.* `zebra-gui` depends on `wallet`, not on `zcash_keys`, and a second copy of the rule could drift from the wallet's.
   - *Support transparent and Sapling destinations.* `TxOutput::Transparent` exists (`wallet/src/lib.rs:1172-1176`), so transparent sends may be close, but that is a wallet feature decision, not a fix for a silent failure.

6. **Tests.**
   - *Harness that exists:* `#[test]` functions in `zebra-gui/src/ui.rs:5326` and `zebra-gui/src/lib.rs:2400` (free functions only; no modal rendering or click injection). Whether the `wallet` crate has its own unit tests was not checked; `decode_send_address` is a free function and can be tested wherever that crate's tests live, or from the GUI test module.
   - *Unit test for item 1*, on the real function: (a) a unified address produced by the wallet's own `user_ua.encode(&TEST_NETWORK)` decodes `Ok`; (b) the same string with its last character changed is `Err`; (c) a `tm...` address is `Err`; (d) `FinalizerAddress::create(&key).encode()` is `Err`; (e) a testnet unified address built with `UnifiedAddress::from_receivers(None, Some(sapling), None)` is `Err("Address has no Ironwood receiver")`; (f) the empty string is `Err`.
   - *End-to-end:* the node test harness in `zebra-crosslink/zebrad/tests/crosslink.rs` drives wallet sends through the RPC path, which after item 1 shares the decode function. A scenario that submits a send to a Sapling-only unified address and asserts the RPC error covers the shared rule.
   - *Manual, for items 2 to 4:* paste a `zfinv1...` address into Send. Expected: `Not a testnet unified address`, all amount buttons grey. Paste your own Receive address. Expected: no warning, buttons enabled; click `0.1`; the modal closes and a pending row appears.

## Validation Information

**Verdict: CONFIRMED. Severity: Medium.**

| Claim | Verified at |
| :- | :- |
| Amount buttons are enabled for any non-empty string | `ui.rs:1992`, `:2009` |
| The GUI never parses the send address | no decode call between `ui.rs:1900` and `:2056`; the only one is in the wallet |
| `send_to_address` only `println!`s and returns on a decode failure | `wallet/src/lib.rs:1142-1145` |
| The modal is unchanged after a click | no `ui.modal = Modal::None` in the Send arm; compare `ui.rs:2169`, `:2363` |
| `[00000000..00000000]` with nothing pasted | `ui.rs:1917-1922` |
| Stake caches a decode-and-verify result and gates on it | `ui.rs:2244-2250`; Retarget `:2352-2358` |
| Accepted address kinds | `zcash_keys/src/encoding.rs:187-199`; `zcash_keys/src/address.rs:32-99`; `wallet/src/lib.rs:5035` |
| A unified address without an Orchard receiver fails later, also silently | `wallet/src/lib.rs:5041-5043`, `:5081-5085` |
| A failed spend preparation is silent and not even counted as a failed action | `wallet/src/lib.rs:5037-5040` (`true // ALT ok`), `:2047-2048` |
| `waiting_for_send` resets on the next state push | `wallet/src/lib.rs:4804`, `:4834` |
| On-screen behaviour | **Not run.** Read from the code only |

**Deliberate?** Partly. The duplicate guard and `true // ALT ok` are choices (the `ALT` tag marks a considered alternative). The missing validation looks like an older screen that was not revisited: Send is from December 2025, Stake's capability check from September 2026, and the modal still carries a `// New version: TODO: finish` branch. Nothing says the silence is intended.

**Severity justification.**

- *Why not High:* no funds move, no balance or stake figure is wrong, nothing crashes. The correct input, a unified address copied from another participant's Receive modal, works.
- *Why not Low:* this is the money-moving flow, and its failure mode is indistinguishable from its success for the first moments after the click. A user can come away believing a payment was made. It is reachable with ordinary inputs, including a fully valid address when the balance does not cover the fee. This is the borderline call of the three related findings; if only the wrong-address routes existed, Low would be defensible.

**Corrections made during validation.**

1. The review said clicking an amount "does nothing visible". True for decode failures. For a unified address without an Orchard receiver, and for a spend that cannot be prepared, `waiting_for_send` is set and the buttons grey out briefly before returning.
2. The review listed "a transparent address, a truncated UA or arbitrary text". Added three more routes: a decodable unified address with no Orchard receiver; a duplicate of an in-flight send; a valid address with funds that do not cover amount plus fee.
3. The review said "the Stake modal already caches a decode and verify result; Send needs the same plus an 'invalid address' line". Stake and Retarget have no such line either; the plan adds it to all three.
4. The placeholder also appears after a paste shorter than 16 bytes, not only when nothing was pasted.
5. The same two error conditions already have user-facing strings on the RPC path (`wallet/src/lib.rs:4963-4967`); the plan reuses that rule.

**Cross-references.**

- `pasted-non-ascii-text-panics-the-gui-on-a-byte-index-slice.md` (G3): the same lines. G3's item 1 replaces the abbreviation and removes the all-zero placeholder; G3's item 2 adds the single `pasted_address` function. Land G3 first, then items 2 and 3 here on top. The validation in item 2 must stay **after** G3's safe abbreviation in frame order, or be moved above the `ui.text` call; either way it must not rely on the string being ASCII.
- `clipboard-has-no-windows-or-wayland-backend-so-paste-only-flows-cannot-complete.md` (G2): G2's item 3 puts a "Clipboard unavailable" or "Clipboard is empty" notice in the same slot under the paste button as item 3 here. Implement them as one notice with a fixed priority: clipboard failure first, then validation error. G2's item 4 (address textboxes) makes the validation line more valuable, since typed input is far more error-prone than pasted input.
- Item 1 (wallet crate) is independent of G2 and G3 and can land at any time.
