# Code review status: all 21 findings

Tracking table. One row per finding, named by its report file. Two reviews feed it:

- **Consensus review** (`C` rows): 11 reports in `crosslink-e99404e3de7cc-review/`, validated at `dev` @ `e99404e3de7cc`.
- **GUI review** (`G` rows): 10 reports in `crosslink-d00b6a44ef594-gui-review/`, validated at `dev` @ `d00b6a44ef594`.

Status is assessed against `dev` @ `d00b6a44ef594` on 2026-10-01. Every assessment is from reading code. Nothing was built or run for either review or for this table. Narrative, quotes and fix plans are in the report files. The structure every report follows, and the row formats used below, are in [`SECURITY-ISSUE-TEMPLATE.md`](SECURITY-ISSUE-TEMPLATE.md).

`C` numbers are the ones the consensus reports use to cross-reference each other, so they are kept as they are and the rows are sorted by severity.

### Column meanings

**Sev**: the containing folder: `high`, `medium` or `low`.

**Fires**: what it takes to trigger.

- Consensus rows: `honest` = an ordinary event such as a crash, a restart, a network split or a tip change · `adversary` = needs a dishonest proposer or miner · `config` = not reachable with the shipped parameters.
- GUI rows: every one fires in ordinary use, so the column gives the platforms: `all`, or the ones named.

**Gate**: when it has to land.

- `fork` = the fix changes consensus rules. It is free to land until BFT activation, which is height 36,288 on the shipped parameters. After that it needs a coordinated upgrade. This is a fact about the fix.
- `launch` = node-local or GUI-local, and it should be in before the public launch. This is a recommendation.
- `any` = no deadline.

**Status**: `FIXED` (with commit) · `PARTIAL` (bounded, mechanism survives) · `CLEAR` (obvious fix, not yet done) · `DECIDE` (needs a person) · `MEASURE` (needs a number first) · `EXTERNAL` (cannot be fixed in this repo).

**Test**: which harness exercises it. Three exist:

- `test-format`: the scenario tests in `zebra-crosslink/zebrad/tests/crosslink.rs`, about 45 of them. One in-process node, regtest only, proof of work disabled, ephemeral state, no restart.
- `dilated`: `zebra-crosslink/dilated_regtest/run.sh`. Two child-process nodes with real signatures, real templates and a restart past activation. It asserts liveness only.
- `cargo`: ordinary unit tests. `zebra-gui` has two small test modules and nothing that drives `run_ui` or the chain view.

No finding is covered by any of them today, so every row shows a route not yet taken: `unit` = easy pure-logic test · `intg` = easy with a harness that already exists · `hard` = needs new infrastructure · `none` = manual only.

**Owner**: the area that can close it. `node` = `zebra-state` and its callers · `spec+node` = a consensus rule has to be decided first, then coded · `gui` · `gui+wallet` · `gui+softer_gui` = needs a change in the vendored windowing patch.

---

## Consensus review (11)

| # | Finding | Sev | Fires | Gate | Status | Test | Owner | Notes |
|-|-|-|-|-|-|-|-|-|
| C2 | [Tail Confirmation trusts each header's own `nBits`](crosslink-e99404e3de7cc-review/high/tail-confirmation-checks-carried-headers-against-their-own-nbits-so-a-proposer-can-fake-confirmations.md) | high | adversary | fork | DECIDE | hard | spec+node | One byzantine proposer halts finality. Two decisions: how carried headers prove their binding, jointly with C11, and what to do about the testnet minimum-difficulty rule. The contextual check itself is CLEAR |
| C11 | [Fat pointer sits outside the Equihash input](crosslink-e99404e3de7cc-review/high/fat-pointer-sits-outside-the-equihash-input-so-miners-can-grind-the-block-hash-with-sha256d.md) | high | adversary | fork | DECIDE | intg | spec+node | Found during validation of C7. One decision: carry the ZIP 244 value in the header, or bind the digest in `hashBlockCommitments` only. Also on `s1_dev` |
| C6 | [Dropped decided snapshot halts BFT and can abort](crosslink-e99404e3de7cc-review/high/dropped-decided-snapshot-leaves-validate-indeterminate-forever-and-decide-can-abort.md) | high | honest | launch | DECIDE | intg | node | The give-up is the documented interim before stage 9. The abort and the restart exit are bugs against FINALITY.md's "never requires a resync". Decision: ship the interim detached state, or go straight to stage 9. C6.3 and C6.4 are CLEAR either way |
| C7 | [Fat pointer certificate accepted by hash only](crosslink-e99404e3de7cc-review/medium/admit-fat-pointer-accepts-a-certificate-by-hash-without-checking-signatures.md) | medium | adversary | fork | DECIDE | intg | spec+node | Decision: checked certificates, or declare the header pointer hash-only. Finality is not at risk either way. Also on `s1_dev`, where a TODO for it was dropped |
| C3 | [`MISSING_POW_BLOCKS` never expires](crosslink-e99404e3de7cc-review/medium/missing-pow-blocks-grows-without-bound-from-unverified-proposal-snapshots.md) | medium | adversary | launch | CLEAR | unit | node | About 21 bogus hashes fill the 42-slot download budget. The design overview lists the expiry rule as an open question. Also fires for honest proposals once C6's state is reached |
| C1 | [Restart exits when the decided snapshot was only in memory](crosslink-e99404e3de7cc-review/medium/restart-exits-when-the-decided-snapshot-is-still-only-in-the-non-finalized-state.md) | medium | honest | launch | CLEAR | intg | node | **Narrowed by `8cc3f5b7`**: a clean stop now flushes the backup. A crash, a hard kill and a disabled backup still strand the node. The dilated run already restarts; the missing step is deleting the backup directory between stop and start |
| C4 | [Restart exits between activation and genesis](crosslink-e99404e3de7cc-review/low/restart-exits-when-killed-between-activation-and-the-bft-genesis-decision.md) | low | honest | launch | CLEAR | hard | node | Delete one guard. Since `8cc3f5b7` a clean stop can land in the window too. The test needs a tool that deletes BFT rows between stop and start |
| C5 | [Bootstrap check ignores sigma](crosslink-e99404e3de7cc-review/low/bootstrap-genesis-headers-can-still-be-reorged-so-late-joiners-build-a-different-genesis.md) | low | config | any | CLEAR | unit | node | Not exposed: the margin is 1,724 blocks against a need of 99. The heights were re-chosen in `ca131a7d` and `00565513` without the fix |
| C8 | [Template pointer chosen against the live tip](crosslink-e99404e3de7cc-review/low/template-fat-pointer-is-chosen-against-the-live-tip-not-the-template-parent.md) | low | honest | any | CLEAR | intg | node | Costs one mined block, and only after a reorg deeper than sigma across a decision. Needs one new test-format instruction that names the template parent |
| C9 | [Template pointer walk cost under the read lock](crosslink-e99404e3de7cc-review/low/template-fat-pointer-walk-scans-the-whole-bft-chain-under-the-read-lock.md) | low | honest | any | MEASURE | intg | node | No timing has been taken. The loop bound is CLEAR and rides on C8. The clone-per-iteration cost is the part to measure |
| C10 | [`AGENTS.md` cast and misleading-default rules](crosslink-e99404e3de7cc-review/low/new-bft-code-breaks-agents-md-cast-comment-and-misleading-default-rules.md) | low | honest | launch | CLEAR | unit | node | 39 uncommented casts, all safe today. The part that matters: a missing stake record is treated as zero stake at two sites. Sigma of 0 is still accepted by config validation |

## GUI review (10)

| # | Finding | Sev | Fires | Gate | Status | Test | Owner | Notes |
|-|-|-|-|-|-|-|-|-|
| G1 | [Edit Stake amounts shown wrong](crosslink-d00b6a44ef594-gui-review/high/edit-stake-modal-shows-bond-amounts-wrong-because-the-fraction-is-not-zero-padded.md) | high | all | launch | CLEAR | unit | gui | 0.001, 0.01 and 0.1 cTAZ all read `0.100`. Display only. The same bug was fixed in `str_from_ctaz` by `5b20864f`; this inline copy was missed |
| G2 | [No clipboard backend on Windows or Wayland](crosslink-d00b6a44ef594-gui-review/high/clipboard-has-no-windows-or-wayland-backend-so-paste-only-flows-cannot-complete.md) | high | Windows; Linux without `xclip` or `xsel` | launch | CLEAR | hard | gui+softer_gui | Send, Stake and Retarget cannot be completed. Regression from `eda288f9`, which removed the `copypasta` fallback. **Must not land before G3** |
| G3 | [Non-ASCII paste aborts the node](crosslink-d00b6a44ef594-gui-review/medium/pasted-non-ascii-text-panics-the-gui-on-a-byte-index-slice.md) | medium | macOS, Linux | launch | CLEAR | unit | gui | The GUI runs on zebrad's main thread and the build is `panic = "abort"`, so the whole node dies. Unreachable on Windows only because paste returns nothing there |
| G6 | [Send never validates the address](crosslink-d00b6a44ef594-gui-review/medium/send-modal-never-validates-the-address-and-a-failed-send-is-silent.md) | medium | all | any | CLEAR | unit | gui+wallet | Only a testnet unified address with an Orchard receiver works; everything else fails with a `println!`. The RPC path already produces proper errors for the same cases |
| G7 | [Finalizer Filters zeroes the seconds filter](crosslink-d00b6a44ef594-gui-review/medium/opening-finalizer-filters-zeroes-the-seconds-filter-and-marks-everyone-online.md) | medium | all | any | CLEAR | unit | gui | One click marks every active-roster finalizer ONLINE for the rest of the session, in the stake bars, the roster and Edit Stake. The seed added in `76c73104` was never reachable |
| G8 | ["claimed @" label uses the wrong bond](crosslink-d00b6a44ef594-gui-review/medium/claimed-at-label-in-transaction-history-uses-the-wrong-bonds-claim.md) | medium | all | any | CLEAR | unit | gui | An unclaimed bond can read "claimed" with another bond's height and amount, and the label changes while scrolling. Display only |
| G4 | [Modal backdrop passes clicks through](crosslink-d00b6a44ef594-gui-review/low/modal-backdrop-passes-clicks-through-to-the-controls-underneath.md) | low | all | any | CLEAR | unit | gui | Worst single outcome: an unrequested MINING toggle. Only the Jump modal and the block inspector are affected; the other seven modals capture |
| G5 | [Faucet tab resets every modal](crosslink-d00b6a44ef594-gui-review/low/faucet-wallet-tab-resets-every-modal-so-jump-and-convert-rewards-never-open.md) | low | all | any | CLEAR | unit | gui | The reset is deliberate for wallet modals. Jump and Convert Rewards were added later and were caught by it |
| G9 | [Chain view reacts under panes and modals](crosslink-d00b6a44ef594-gui-review/low/chain-view-pans-and-recenters-while-the-pointer-is-over-a-pane-or-modal.md) | low | all | any | CLEAR | none | gui | Fires most often of the three input findings: every touchpad scroll over a pane. Three one-line gates |
| G10 | [Esc dead after textbox focus; Jump gives no feedback](crosslink-d00b6a44ef594-gui-review/low/esc-cannot-close-a-modal-after-textbox-focus-and-jump-gives-no-feedback.md) | low | all | any | CLEAR | unit | gui | After one Tab, Esc is dead in all eight modals. Half of a two-stage Esc: the clearing handler is commented out |

---

## Roll-up

| Status | High | Medium | Low | Total |
|-|-|-|-|-|
| FIXED | 0 | 0 | 0 | **0** |
| PARTIAL | 0 | 0 | 0 | 0 |
| CLEAR (obvious fix, pending) | 2 | 6 | 8 | **16** |
| DECIDE | 3 | 1 | 0 | **4** |
| MEASURE | 0 | 0 | 1 | 1 |
| total | 5 | 7 | 9 | **21** |

By trigger, consensus rows: **6 honest**, **4 adversary**, **1 config**. All 10 GUI rows fire in ordinary use.

By gate: **3 fork** (C2, C7, C11), **8 launch** (C1, C3, C4, C6, C10, G1, G2, G3), **10 any**.

By test route, what CI would actually catch on a bad push:

| Route | Findings | Runs in CI? |
|-|-|-|
| `test-format`, `dilated`, `cargo` | 0 | n/a |
| **covered subtotal** | **0** | |
| `unit` (easy, unwritten) | 11 | no |
| `intg` (easy, unwritten) | 6 | no |
| `hard` | 3 | no |
| `none` | 1 | never |

**0 of 21 are covered by something that runs automatically.** 17 of them have a cheap route.

## What the table makes obvious

- **Nothing is fixed, and 16 of 21 are waiting on effort, not on a decision.** No recommendation from either review has landed, apart from filing C11 as its own report.
- **All four decisions are consensus design.** C2 and C11 share one of them: how a header carried in a BFT proposal proves its pointer binding.
- **Three findings have a hard deadline** (C2, C7, C11). The deadline moved in their favour: activation went from height 275 to 36,288, about 10.5 days after genesis at 25 second blocks.
- **Five consensus findings are one piece of startup design**, not five fixes: C1, C4, C6, C10's stake default and C3's re-fetch all meet in `restore()`.
- **The GUI's worst outcome is a node abort, not a wrong pixel.** G3 kills a finalizer or miner through a paste, and fixing G2 first would extend that to Windows.
- **Eleven unit tests would pin eleven findings.** Most need a pure function extracted first, and the reports say which.
- **On a testnet with no dishonest participants, C6, C1 and C4 are the consensus rows that matter**, plus every GUI row.

---

# Recommendation-level evaluation

The table above tracks the 21 **findings**. This one tracks the 78 individual **recommendations** inside them. A finding moves to PARTIAL when some of these land and others do not, and the ones that do not are where the remaining work is.

Numbering follows each report's own Recommendations section, so `C6.3` is item 3 in the C6 report. Items that only list rejected alternatives or rollout timing are left out; rollout is the Gate column above. `T` marks a report's unnumbered test list.

**Verdicts.** `DONE` (implemented, commit named) · `PARTIAL` · `OPEN` (not done, still applies) · `SUPERSEDED` (the facts changed under it) · `DECLINED` (considered and not doing, reason given).

**A deliberate bias:** anything not verified against the tree is marked `OPEN`, not assumed done.

| Verdict | Count |
|-|-|
| DONE | 1 |
| PARTIAL | 0 |
| SUPERSEDED / DECLINED | 0 |
| **OPEN** | **77** |
| total | **78** |

## Consensus findings

| # | Recommendation | Verdict | Note |
|-|-|-|-|
| C1.1 | Truncate the replay at the first stored decision whose snapshot is not held; delete the exit | OPEN | Exit still at `bft.rs:1308-1320` |
| C1.2 | Correct the five stale comments about write ordering | OPEN | `zebra_db/bft.rs:53-55` and four in `bft.rs` |
| C1.3 | Optional: flush the snapshot to the backup at decision time | OPEN | `8cc3f5b7` gave the writer a `backup_task` handle, which removes the report's stated obstacle. It is used only at shutdown |
| C1.T | Tests: snapshot lost with the backup; backup disabled; two-node catch-up after truncation | OPEN | Route: `dilated` plus deleting the backup directory |
| C2.1 | Contextual difficulty check on every carried header in `validate`; `propose` uses the same helper | OPEN | `header_pow_is_valid` unchanged |
| C2.2 | Make the minimum-difficulty start height a `testnet::Parameters` field and turn it off for the new testnet | OPEN | Still hardcoded at 299,188. **Must land with C2.1** |
| C2.4 | Tests: forged tail rejected, side-chain variant, honest tail passes, minimum-difficulty case, helper unit test | OPEN | End-to-end needs a non-regtest harness |
| C3.1 | Entries expire unless a live proposal refreshes them, plus a hard cap | OPEN | Still a plain `HashSet`. The 30 s and 16 are suggestions, not measured |
| C3.2 | Run the header PoW check before the snapshot lookup in `validate` | OPEN | Does not bound the set on its own |
| C3.3 | Cap by-hash requests at a fixed share of the download budget | OPEN | |
| C3.5 | Tests: bogus snapshots do not stall sync; a real missing snapshot is still fetched; expiry and cap unit test | OPEN | |
| C4.1 | Remove the empty-store exit in `restore`; log and let the first tick re-derive genesis | OPEN | Guard at `bft.rs:1233-1245` |
| C4.2 | Rewrite the guard comment and `restore`'s doc comment | OPEN | |
| C4.3 | Keep the failed genesis write at `error`, and say the height is re-decided on next start | OPEN | |
| C4.T | Tests: empty store above activation; row 0 missing; one tick crosses activation | OPEN | Needs a row-deletion tool. Same tool as C10.4 |
| C5.1 | `bootstrap_is_valid` bounds `h1 + sigma` and rejects sigma of 0 | OPEN | The function was edited in `ca131a7d` for `h0` and still ignores sigma |
| C5.2 | `build_bootstrap_genesis` reads headers from the finalized database and logs a miss | OPEN | Also removes two of C10.3's casts |
| C5.4 | Tests: config-load rejection, boundary scenario, unit boundary cases | OPEN | The report's example heights now also need a valid `bootstrap_staking_height` |
| C6.1 | Persist the abandonment as a hazard row before the first commit past the fork | OPEN | `conflict_abandoned` is still memory only. Rides on the C6 decision |
| C6.2 | Add a detached state to `BftChain`: `validate` returns `Indeterminate`, `propose` declines | OPEN | Rides on the C6 decision |
| C6.3 | Replace `decide`'s `assert_eq!` with a match that parks an `Indeterminate` decision | OPEN | CLEAR regardless of the decision |
| C6.4 | Keep the tenderlink handle and abort it on detach; the push closure parks, never panics | OPEN | CLEAR regardless. Also covers the shutdown path noted under "Facts that moved" |
| C6.5 | `restore` starts detached when the hazard row explains the missing snapshot | OPEN | Same branch as C1.1 |
| C6.6 | Log the hold at `warn!`, repeat an `error!` while detached, add two gauges | OPEN | The hold log is still `debug!`, which release builds compile out |
| C6.T | Tests: test-format give-up scenario; decide-race integration test; dilated restart after give-up | OPEN | First is `intg`; the other two are `hard` |
| C7.1 | Record the voting roster per decided height; `certificate_is_valid` in `admit_fat_pointer` and on `previous_block_fat_ptr` | OPEN | Rides on the C7 decision. Worst case is now a 12-signature batch, since `3351fc4c` |
| C7.3 | Tests: malformed-certificate matrix; migrate the scenes that pass `sigs = &[]` | OPEN | The harness has no signing helper yet |
| C7.4 | File the Equihash grinding issue separately | **DONE** | It is report C11 |
| C8.1 | The request carries the template parent's hash; the height is derived from it | OPEN | |
| C8.2 | Both `getblocktemplate` sites send `chain_info.tip_hash`; return an RPC error instead of a null pointer | OPEN | Same null fallback as C9.1 and C10.3 |
| C8.4 | Tests: a template-on-named-parent instruction and a deep-reorg scenario | OPEN | |
| C9.1 | Bound the loop at the parent pointer's index; errors, not null defaults | OPEN | A correctness guard, not a speedup. Lands with C8 |
| C9.2 | One `NonFinalizedState` snapshot per selection | OPEN | |
| C9.3 | Add a tracing span with the iteration count; shorten the lock hold only if it matters | OPEN | This is the measurement the MEASURE status waits on |
| C9.5 | Tests: 64 extra decisions then three templates; null-parent case | OPEN | |
| C10.1 | `snapshot_stakes` helper that panics on a missing record for a held block; reword the warning | OPEN | The `restore` half lands with C1.1 and C6.5 |
| C10.2 | Publish the non-finalized state after the finalize loop, in both commit paths | OPEN | |
| C10.3 | Casts: `u64::from`, `usize` in `hash_to_height`, a sigma bound, `try_from`, clippy cast lints | OPEN | 39 casts still uncommented |
| C10.4 | Tests: zero-stake scenario; restart with a deleted stakes row; cast leaf unit test | OPEN | |
| C11.1 | Bind a pointer digest into the ZIP 244 terminator of `hashBlockCommitments`; add `fatpointerdigest` to the template | OPEN | Waits on C11.2 |
| C11.2 | Decide the carried-header path with C2 before implementing C11.1 | OPEN | This is the DECIDE |
| C11.4 | Tests: tampered pointer, pre-activation hash stability, template round trip, digest unit test, regenerate vectors | OPEN | The grind test itself is `hard`: the harness disables PoW |

## GUI findings

| # | Recommendation | Verdict | Note |
|-|-|-|-|
| G1.1 | Delete `format_stake_amount`; both rows call `str_from_ctaz` | OPEN | |
| G1.2 | Unit test `str_from_ctaz` with a table of zero-padded cases | OPEN | |
| G2.1 | In-process Win32 clipboard in the vendored `softer_gui` | OPEN | One implementation covers native and APE builds. **After G3.1** |
| G2.2 | Add `wl-paste` and `wl-copy` when `WAYLAND_DISPLAY` is set | OPEN | |
| G2.3 | Make failure visible: "Clipboard unavailable", "Copied", "Copy failed" | OPEN | Shares the notice line with G6.3 |
| G2.4 | Enable the address textboxes in Send and Stake; add one to Retarget | OPEN | First ask why `c9231740` reverted them |
| G2.6 | Tests: Windows round trip in `softer_gui`; program-table unit test; manual matrix | OPEN | |
| G3.1 | One char-boundary-safe `abbreviate_ends` helper for all three modals | OPEN | |
| G3.2 | One `pasted_address` function: trim, drop control characters, cap the length | OPEN | |
| G3.4 | Unit test for `abbreviate_ends`; manual CJK paste check | OPEN | |
| G4.1 | `Floating::Root` captures; a new `RootPassthrough` variant for the tooltip only | OPEN | Same code as G9.3 |
| G4.2 | A `z` field on `Decl` so the Jump modal sits above the block inspector | OPEN | The stacking claim is inferred from Clay's root sort |
| G4.4 | Clay-only unit test through the real `decl()`; six manual steps | OPEN | |
| G5.1 | `Modal::acts_on_user_wallet()`; reset only when it is true | OPEN | |
| G5.3 | Unit test pinning the classification; manual check of three entry points | OPEN | |
| G6.1 | `wallet::decode_send_address` as the single rule, shared with the RPC path | OPEN | Independent; can land any time |
| G6.2 | Cache the decode as Stake does and gate the amount buttons on it | OPEN | On top of G3.1 |
| G6.3 | Show the reason under the address in all three modals | OPEN | Shares the notice line with G2.3 |
| G6.4 | Report the outcome of a click: close on accept, show a send error, return the real result | OPEN | |
| G6.6 | Tests: decode across address kinds; RPC scenario for a Sapling-only address | OPEN | |
| G7.1 | Seed the textbox from the stored filter before calling `ui.textbox`; delete the dead seed | OPEN | |
| G7.2 | Optional: show the active filter on the popup button | OPEN | |
| G7.3 | Owner's call: may a never-connected finalizer be ONLINE when both filters are off | OPEN | A product decision, not a bug fix |
| G7.5 | Unit tests: the seed survives; `finalizer_is_online_ex` for a never-connected status | OPEN | |
| G8.1 | Replace the positional search with `claim_for_bond`, matching on `bond_key()` | OPEN | |
| G8.2 | Include `ConvertReward` in `sent_stake` | OPEN | |
| G8.3 | Label unclaimed `BeginUnstake` rows from the unbonded roster | OPEN | Improvement, not needed for correctness |
| G8.4 | Unit test `claim_for_bond` with a two-bond list | OPEN | |
| G9.1 | Gate the touchpad pan on `!ui.capture` | OPEN | |
| G9.2 | Clear `hovered_block` when `ui.capture` is true | OPEN | Removes the recenter, the highlight and the hover sound together |
| G9.3 | Fix `ui.rs:1744` to test `contents_hovered` | OPEN | Found during validation. Same code as G4.1 |
| G9.5 | Six manual steps | OPEN | No mechanical route |
| G10.1 | Esc always closes the modal; a live handler clears focus | OPEN | **Before G10.2** |
| G10.2 | Focus the Jump textbox on open | OPEN | |
| G10.3 | `parse_jump_height(text, tip)`; disable Jump and show a warning for bad or out-of-range input | OPEN | |
| G10.4 | Unit test for `parse_jump_height`; seven manual steps | OPEN | |

---

# How the recommendations interact

The recommendations are not independent. Treating the 77 open ones as a flat backlog loses the pairs that must land together and the orderings that prevent a regression.

## Must land together (splitting them causes harm)

| Pair | Why | State |
|-|-|-|
| **C2.1 + C2.2** | The contextual check alone is bypassable. From height 299,188 the minimum-difficulty rule lets a proposer earn PoWLimit legitimately by spacing fake timestamps 150 s apart | Both OPEN |
| **C1.1 + C4.1 + C6.5 + C10.1** | All four change what `restore()` does when state is missing. Done separately, each fix rewrites the branch the previous one added | All OPEN |
| **C8.1 + C8.2 + C9.1** | The loop bound is only correct once the height and the parent agree, which C8 provides | All OPEN |
| **G2.3 + G6.2 + G6.3** | They share the notice line under the paste button. Clipboard failure takes priority over a validation error | All OPEN |
| **G4.1 + G9.3** | Both edit the modal hover code in `ui_left_pane` | Both OPEN |

## Ordering rules

| Rule | Source | State |
|-|-|-|
| **G3.1 before G2.1** | A working Windows clipboard makes the G3 node abort reachable on Windows | Not yet at risk: neither landed |
| **G10.1 before G10.2** | With focus on open and no Esc fix, Esc is dead from the first frame | Not yet at risk |
| **C11.2 before C11.1** | The digest definition depends on how carried headers prove their binding | Decision pending |
| **C11.1 and C7.1 before BFT activation** | Both change block validity after the first non-null pointer | Activation is height 36,288 on the shipped parameters |
| **C5.2 before or with C4.1** | Re-deriving genesis is only safe if genesis is a pure function of finalized data | Both OPEN |
| **C3.1 must keep a node's own stored decisions fetchable** | C1.1 relies on re-fetching the missing snapshot through this set | Design constraint on C3.1 |
| **Choose the template pointer before computing default roots** | After C11.1 the template's commitments depend on the pointer, and C8 edits the same `getblocktemplate` path | Coordinate C8 with C11 |
| **G3 before G6.2 and G6.3** | Same lines; the validation sits on top of the safe abbreviation | Both OPEN |

## Anti-correlated / mutually exclusive

| Tension | Resolution |
|-|-|
| **C6: re-anchor BFT on the node's own branch** vs **halt BFT on this node** | **Re-anchoring DECLINED in the report.** It allows two conflicting final snapshots. Halting the whole process is also declined: one cheap reorg would then shut every node down |
| **C6: interim detached state** vs **stage 9's second chain state** | **Open, this is the C6 decision.** The interim adds a database row that stage 9 would later replace |
| **C7: checked certificates** vs **hash-only pointer with empty signatures** | **Open, this is the C7 decision.** The design overview says the signatures are in the header so a link can be verified from it |
| **C11: exclude the pointer from the difficulty hash** | **DECLINED in the report.** It unbinds the pointer, so a relay could swap it for free |
| **C11: make the pointer canonical** | **DECLINED in the report.** Honest nodes hold different signature sets, and a roster key holder can re-sign |
| **C9: the original review's early stop as a speedup** | **SUPERSEDED.** The loop already stops there. The bound stays, as a correctness guard under C9.1 |
| **C3.2 alone** | Moving the PoW check earlier does not bound the set. It needs C3.1 |

## The same work, counted twice or more

| Work | Appears as |
|-|-|
| What `restore()` does when a snapshot or a row is missing | C1.1, C4.1, C6.5, C10.1 |
| Replace the null fat pointer fallbacks in `fat_pointer_for_template` with errors | C8.2, C9.1, C10.3 |
| Bound sigma in parameter validation | C5.1, C10.3 |
| Casts in `build_bootstrap_genesis` and `admit_fat_pointer` | C5.2, C7.1, C10.3 |
| `decide` must never abort the process on a lifecycle race | C6.3, C6.4, and the shutdown path under "Facts that moved" |
| A test tool that deletes database rows between stop and start | C4.T, C10.4 |
| A restart scenario in the dilated run | C1.T, C6.T |
| The notice line under the paste button | G2.3, G6.3 |
| Sanitising and abbreviating a pasted address | G3.1, G3.2, G6.2 |
| The Jump modal's hover and capture code | G4.1, G9.3 |

## Clusters: one decision or design unlocks many

| Cluster | Members | Blocked on |
|-|-|-|
| **Startup recovery** | C1.1 to C1.3, C4.1 to C4.3, C6.1, C6.2, C6.5, C10.1 | The C6 decision: interim detached state or stage 9 |
| **Carried-header binding** | C2.1, C2.2, C11.1, C11.2 | One decision, taken jointly, plus the minimum-difficulty policy |
| **Header certificate** | C7.1, C7.3 | One decision: checked or hash-only |
| **Template pointer** | C8.1, C8.2, C9.1, C9.2 | Nothing. Coordinate with C11.1 on the template roots |
| **Paste-an-address flows** | G2.1 to G2.4, G3.1, G3.2, G6.1 to G6.4 | Nothing. The order is in the GUI folder's README |
| **Pointer and keyboard routing** | G4.1, G4.2, G9.1 to G9.3, G10.1, G10.2 | Nothing |
| **Wrong figures** | G1.1, G7.1, G8.1, G8.2 | Nothing. Each is independent and unit-testable |

## Facts that moved under the reports

The consensus reports were written at `e99404e3de7cc`. Eight commits later, at `d00b6a44ef594`, these statements in them are out of date. The reports themselves have not been edited.

| Report | What it says | What is true now |
|-|-|-|
| C1 | A graceful stop is covered by the scenario, because nothing flushes the backup at shutdown | `8cc3f5b7` flushes the backup on a clean stop. Crash, hard kill and backup-off are unchanged |
| C1, C4, C6 | Restart tests need a new child-process harness | The dilated run already restarts both nodes past activation |
| C4, C5 | Roster height 75, activation height 275, gap 200 | Roster height 34,560, activation 36,288, gap 1,728, since `ca131a7d` and `00565513` |
| C5 | Margin of 196 blocks | Margin of 1,724 blocks. Still unexposed, and still unexposed if the reorg limit goes to 999 |
| C7, C11 | Up to 100 signatures in a pointer | Up to 12, since `3351fc4c` |
| C11 | After C7's fix, quorum-subset freedom reaches about 88.8 bits at 100 members | About 10.6 bits at 12 members. A roster key holder can still re-sign without limit, so the conclusion stands |
| all `write.rs` and `new_network.rs` citations | Line numbers | `write.rs` moved down about 10 lines, `new_network.rs` about 46. `bft.rs` line numbers are still exact |

One item is new and belongs to no report yet. `sync()` can now return at shutdown, which drops the BFT request receiver. A decision arriving at that moment may reach the `panic!` at `bft.rs:1517` or the `expect` at `:1520`, and the build aborts on panic. **This is inferred from reading and was not traced end to end.** C6.4 would cover it.

## Not tracked here

Both reviews cut lower-priority candidates to stay within their limit of ten. None was validated, so none has a row.

- **Consensus:** tenderlink's replies are answered only between ticks on the sync thread, so a burst of blocks may cause round timeouts; `validate()` compares hardforks byte by byte although the type derives `Eq`; `bft.rs:338-339` fills `ConsensusCounts` with signature counts where tenderlink uses stake-weighted sums.
- **GUI:** the nine items listed at the end of `crosslink-d00b6a44ef594-gui-review/README.md`.
- **Unconfirmed deployment facts:** the PoWLimit the new testnet will actually deploy with, and whether the `staking3d` network has already started. C2 and C11 are priced against the in-tree default of `2^251 - 1`.
