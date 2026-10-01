# Crosslink review plans at dev e99404e3de7cc

One file per finding, in the format of the zeronym review files (`C:\zero\zeronym-22aa9851caf68-high-medium`). Each file's **Recommendations** section is the fix plan: the change, alternatives considered, tests, and rollout.

- **Target:** `dev` at `e99404e3de7cc`, clean tracked tree. `dev` split from `s1_dev` at `64046aeb` (the `s1_dev` tip, which ClT0 runs).
- **Source:** findings 1 to 10 come from `/code-review high` on 2026-09-29. Finding 11 was surfaced by the validator of finding 7.
- **Method:** every finding was validated by reading code only. Nothing was built, run, or tested, because the box is short on disk and RAM.
- **Not reproduced:** none. Every finding held at least in part, and most had parts corrected. See each file's "Corrections made during validation".

## Findings

| # | Severity | Verdict | Changes consensus rules? | ClT0 / `s1_dev` affected? | File |
|-|-|-|-|-|-|
| 2 | High | Confirmed | Yes | Weaker check there | [tail confirmation trusts self-declared nBits](high/tail-confirmation-checks-carried-headers-against-their-own-nbits-so-a-proposer-can-fake-confirmations.md) |
| 11 | High | Confirmed | Yes | Yes | [fat pointer outside the Equihash input allows sha256d grinding](high/fat-pointer-sits-outside-the-equihash-input-so-miners-can-grind-the-block-hash-with-sha256d.md) |
| 6 | High | Partially confirmed | No (adds a DB row) | Only the abort-on-assert shape | [dropped decided snapshot halts BFT and can abort](high/dropped-decided-snapshot-leaves-validate-indeterminate-forever-and-decide-can-abort.md) |
| 7 | Medium | Partially confirmed | Yes | Yes | [fat pointer certificate accepted by hash only](medium/admit-fat-pointer-accepts-a-certificate-by-hash-without-checking-signatures.md) |
| 3 | Medium | Partially confirmed | No | No | [`MISSING_POW_BLOCKS` unbounded, starves downloads](medium/missing-pow-blocks-grows-without-bound-from-unverified-proposal-snapshots.md) |
| 1 | Medium | Partially confirmed | No | No | [restart exits when the snapshot is only in non-finalized state](medium/restart-exits-when-the-decided-snapshot-is-still-only-in-the-non-finalized-state.md) |
| 4 | Low | Confirmed | No | No | [restart exits between activation and genesis](low/restart-exits-when-killed-between-activation-and-the-bft-genesis-decision.md) |
| 5 | Low | Partially confirmed | No (tightens config validation) | No | [genesis headers within reorg reach](low/bootstrap-genesis-headers-can-still-be-reorged-so-late-joiners-build-a-different-genesis.md) |
| 8 | Low | Partially confirmed | No | No | [template pointer chosen against the live tip](low/template-fat-pointer-is-chosen-against-the-live-tip-not-the-template-parent.md) |
| 9 | Low | Partially confirmed | No | No | [template pointer walk cost under the read lock](low/template-fat-pointer-walk-scans-the-whole-bft-chain-under-the-read-lock.md) |
| 10 | Low | Partially confirmed | No | No | [AGENTS.md cast and misleading default rules](low/new-bft-code-breaks-agents-md-cast-comment-and-misleading-default-rules.md) |

## Suggested landing order

**Before the new testnet's genesis (consensus changes):**

1. **Findings 2 and 11, designed together.** Finding 11's fix binds a pointer digest into `hashBlockCommitments`. That does not reach the carried headers in BFT proposals, which finding 2 checks from the header alone. Settle how carried headers prove their binding first, then implement both.
2. **Finding 2's second half:** turn the testnet minimum-difficulty rule off for the new testnet, or refuse minimum-difficulty exemptions in carried headers. Without it the contextual check is bypassable after height 299,188.
3. **Finding 7:** certificate checks in `admit_fat_pointer` and for `previous_block_fat_ptr`. The test harness scenes that pass empty signatures need migrating.

**Before launch (node-local, no consensus change):**

4. **Findings 6, 1 and 4 as one design.** They all change `restore()` and what a node does when state is missing or abandoned: a detached BFT state for 6, a truncated replay for 1, and re-bootstrap for 4. Finding 10's missing-stake handling in `restore` belongs in the same change.
5. **Finding 3:** expiring, capped `MISSING_POW_BLOCKS` with a fixed share of the download budget. It must still allow snapshots of decisions the node stored itself, which finding 1's fix relies on.
6. **Findings 8 and 9 as one change:** the template parent goes into the request, and the scan is bounded at the parent's pointer. Finding 11 makes the template's commitments depend on the chosen pointer, so coordinate the `getblocktemplate` edits.
7. **Finding 10:** the casts, `snapshot_stakes`, and the sigma bound. Findings 5 and 7 already fix some of the casts.
8. **Finding 5:** land it together with the new bootstrap heights planned in `IMPLEMENTATION.md` item 5.

## Open points for the owner

- **New testnet PoWLimit.** The in-tree ClT1 parameters (`zebrad/src/config.rs:309-327`) set no limit, so the default `2^251 - 1` applies unless the deployment TOML overrides it. Findings 2 and 11 are priced against that. Confirm the value the deployment will actually use.
- **ClT0 backports.** Findings 2, 7 and 11 also affect ClT0. Every fix there is a hard fork, so backport only if ClT0 stays live.
- **Unvalidated side notes from the validators:**
  - `bft.rs:338-339` fills `ConsensusCounts` with signature counts where tenderlink uses stake-weighted sums.
  - A sigma of 0 would panic in `snapshot_block_hash`.
  - `s1_dev` panics if stake legitimately drains to zero.
- **Cut by the original review, not planned here:**
  - Tenderlink's Propose, Validate and Decided replies are answered only between ticks on the sync thread, which also runs `verify_expensive`. A burst of blocks could cause round timeouts.
  - `validate()` compares hardforks byte by byte, although `HardForkConfig` derives `Eq`.
