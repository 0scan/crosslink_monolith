# Tail Confirmation in `BftRunner::validate` checks each carried header only against the `nBits` it declares for itself (anything up to PoWLimit), never against the difficulty the chain requires at that height, so one byzantine proposer can attach sigma near-free fake confirmations to a side-chain snapshot, get it decided as `bft_final_snapshot`, and halt BFT finality on every honest node

**Severity**: High
**Validation Status**: Confirmed
**Location**: `zebra-crosslink/zebra-state/src/new_network/bft.rs:187-212` (`header_pow_is_valid`), `:990-1032` (Tail Confirmation in `validate`), `:1012-1017` (snapshot only has to be held), `:1034-1054` (Linearity), `:1083-1084` and `:1119` (`decide` re-validates and sets `bft_final_snapshot`), `:778-785` and `:802-819` (`propose` declines against the decided snapshot); `zebra-crosslink/zebra-state/src/service.rs:546-549` (`known_block`) and `zebra-crosslink/zebra-state/src/service/read/find.rs:159-184` (side chains count as held); `zebra-crosslink/zebra-state/src/service/write.rs:220-262` and `:421-432` (`crosslink_conflict_hold`); `zebra-crosslink/zebra-state/src/service/non_finalized_state.rs:288-315` (the decided chain is exempt from fork eviction); `zebra-crosslink/zebra-state/src/service/check/difficulty.rs:183-205` and `zebra-crosslink/zebra-state/src/service/check.rs:268-325` (the contextual rule that exists but is not applied); `zebra-crosslink/zebra-chain/src/parameters/network_upgrade.rs:253`, `:264`, `:269`, `:275`, `:450-500` (testnet minimum-difficulty rule); `zebra-crosslink/zebra-chain/src/parameters/network/testnet.rs:525-534` (default testnet PoWLimit); `librustzcash/zcash_primitives/src/bft.rs:533-541` (`PROTOTYPE_PARAMETERS`, sigma = 4); `crosslink_book/src/FINALITY.md:315-323` and `:1012-1016` (the design statement)
**Found by agent:** /code-review high (Claude Fable 5.1), 2026-09-29; validated 2026-09-29 at dev e99404e3de7cc
**In scope of audit?** Yes. `header_pow_is_valid` and the four-part Tail Confirmation were introduced on dev (commit `af76dddd`, "Tail Confirmation by header PoW"); `bft.rs` does not exist on `s1_dev`. ClT0 is affected by a strictly weaker check: `s1_dev`'s `validate_bft_block` (`zebra-crosslink/zebra-crosslink/src/lib.rs:890-1030` at `64046aeb`) performs no proof-of-work, linkage or count check on the carried headers at all, only that `headers[0]` hashes to a held block.

## Description

Tail Confirmation is the BFT validity rule that is supposed to stop a proposer from finalizing a PoW block that the honest chain has not buried. FINALITY.md §3.4 states it as "`B.headers_bc` form a `σ`-header proof-of-work tail on `snapshot(B)`", and the Book's honest validator "downloads the bc-blocks for `P.headers_bc` and checks their bc-block validity". Zebra Crosslink deliberately checks headers rather than blocks (FINALITY.md §8.1 records the sync deadlock that motivated this), and implements the proof-of-work part in `header_pow_is_valid`:

- the header's own `difficulty_threshold` must expand and be at or below `network.target_difficulty_limit()` (PoWLimit);
- the header hash must be at or below that same self-declared threshold;
- the Equihash solution must verify.

Nothing compares the declared `nBits` with `ThresholdBits(height)`, the value every full block at that position must carry (`check.rs:311-321`). So the only cost the rule imposes per header is one PoWLimit-difficulty solve, whatever the chain's real difficulty is. On a network whose real difficulty sits above PoWLimit, sigma "confirmations" are almost free to manufacture.

The remaining parts of Tail Confirmation do not compensate:

- the count and linkage checks (`bft.rs:995-1011`) constrain only the shape of the proposer-supplied headers;
- the snapshot (`headers[0].prev_block`) only has to be a block this node holds on **any** chain (`known_block`, which returns side-chain blocks, `find.rs:173-179`), not one on the node's best chain;
- the carried headers never have to be known to anyone.

Linearity (`bft.rs:1036-1054`) then requires only that the snapshot descends from the parent bft-block's snapshot, again on any chain.

Once such a proposal is decided, `decide` stores its snapshot as `bft_final_snapshot` (`bft.rs:1119`). If that snapshot is on a side chain the honest network will never extend, every honest proposal from then on fails Linearity against it, honest proposers decline to propose (`bft.rs:809-818`), and BFT finality stops.

## Attack Scenario and Steps

1. **Pick a snapshot.** The attacker picks a block `S` that descends from the current decided snapshot `P` but is not on the honest best chain. Two ways to get one:
   - a natural orphan above `P` that honest nodes still hold in their non-finalized state (side chains survive until the best chain's non-finalized part exceeds `MAX_BLOCK_REORG_HEIGHT` = 99 or the chain is evicted as one of more than `MAX_NON_FINALIZED_CHAIN_FORKS` = 10);
   - or, more reliably, one block the attacker mines itself on top of `P` (or on top of any best-chain block above `P`) at the chain's real contextual difficulty, and publishes. Honest nodes admit it as a side-chain block.
2. **Forge the tail.** The attacker builds sigma = 4 headers with `nBits` = PoWLimit, `headers[0].prev_block = S`, each linked to the previous, each with a valid Equihash solution under PoWLimit. With the ClT0 deploy config's `target_difficulty_limit = 0x0f0f…0f` a random hash passes about one time in 17, so the whole tail is on the order of 70 Equihash solver runs (inference from the target value; solver speed not measured).
3. **Propose.** When tenderlink schedules the attacker as proposer for some round of the current BFT height, it proposes a v2 `BftBlock` with the correct parent fat pointer, height, version, hardforks and `do_not_include_until_bc_height`, carrying the forged tail.
4. **Honest validation passes.** On each honest roster member, `validate` passes every check (`bft.rs:888-1057`). A validator that does not yet hold `S` returns `Indeterminate` with `NeedsBlock`, the sync loop requests `S` by hash (`new_network.rs:2127-2152`), the attacker's node serves it (`new_network.rs:2647-2655` serves from any chain), and the next re-validation passes.
5. **Decision.** Honest finalizers prevote and precommit a valid proposal (tenderlink `lib.rs:1111-1127`, `:1168-1182`); with 2f+1 precommits `decide` runs, re-validates (Pass), and sets `bft_final_snapshot = (height(S), S)` (`bft.rs:1083-1119`).
6. **Honest BFT stops.** Every honest proposer's candidate is `tip - sigma` on its best chain, which does not descend from `S`; `propose` logs "candidate snapshot … does not extend the parent bft-block's snapshot" and returns `None` (`bft.rs:809-818`). Any proposal that does not extend `S` fails Linearity on every honest validator (`bft.rs:1040-1045`). Sticky fork choice does not move nodes onto `S`'s chain: it selects by work among chains containing `fin`, and `fin` has not moved (FINALITY.md §4.3). Only a chain through `S` that outgrows the honest chain, which needs the attacker's hash rate, would restart honest proposals.
7. **Commit hold, then abandonment.** Because `S`'s chain holds `bft_final_snapshot`, fork eviction exempts it (`non_finalized_state.rs:300-313`) and `crosslink_conflict_hold` refuses the reorg-depth commit that would drop it while the best chain's non-finalized part is at most `MAX_BLOCK_REORG_HEIGHT + CONFLICT_HOLD_DEPTH` = 99 + 999 = 1,098 blocks long (`write.rs:248-253`). Past that it records `conflict_abandoned`, logs "can no longer follow that decision", and commits (`write.rs:256-261`), which enters the state described in `dropped-decided-snapshot-leaves-validate-indeterminate-forever-and-decide-can-abort.md`.

**Attack Requirements and Assumptions:**

- The attacker is a finalizer in the active roster (stake bonded) and is scheduled as proposer for at least one round. Proposer selection is stake-weighted per (height, round) (tenderlink `lib.rs:650-675`), so any roster member is scheduled eventually; it does not need more than f of the stake, because honest finalizers vote for the proposal.
- Honest validators hold `S` or can fetch it by hash. Step 1's self-mined variant makes this the attacker's choice.
- The chain's real difficulty is materially above PoWLimit, or the attacker has less hash rate than the honest network. If the network's real difficulty is at PoWLimit anyway, the contextual check gains nothing and the attack costs the same as mining sigma real blocks, which the protocol model already admits.
- Parameters actually in force: sigma is 4 (`PROTOTYPE_PARAMETERS`, `librustzcash/zcash_primitives/src/bft.rs:533-534`, overridable through the `[crosslink]` network parameters). PoWLimit is operator configuration, not pinned in the dev tree: the built-in default testnet PoWLimit is `2^251 - 1` (`testnet.rs:525-533`); the ClT0 deploy config in the working copy (`testnet_online.toml`, untracked) and the tracked test configs use `0f0f…0f`. The dev tree does not contain the new testnet's config, so the exact value for the new network has to be read from its deployment config.

## Impact on Users

- **Finalizers and stakers:** one byzantine roster member halts BFT finality for the whole network with a single decided proposal. Honest finalizers cannot recover inside the protocol; the decided chain is persisted (`write_bft_decision`), so a restart does not help. Recovery needs an out-of-protocol action (a hardfork or a coordinated reset of the BFT chain).
- **Node operators:** each node holds up to 1,098 non-finalized best-chain blocks in memory before giving up on the decision, then enters the abandoned-decision state of finding 6 (validation `Indeterminate` forever, possible abort in `decide`).
- **Wallet users and anything reading `fin`:** `fin` stops advancing, so finality-gated flows (staking rewards keyed to finality, "final" balances in wallets and the GUI) freeze. PoW blocks keep being mined and accepted, so ordinary spends continue with an unbounded finality gap.
- **Safety, secondary:** even when the chosen `S` is on the best chain (for example the current tip), the forged tail finalizes it with zero real confirmations. A later ordinary 1 or 2 block reorg then lands in the same stalled state. Inference, not traced: the next roster is read from the stakes at `S` (`bft.rs:1139-1143`), so an attacker-mined `S` also chooses which staking state the next height's roster comes from.

## Technical Details / Code Analysis

**The only proof-of-work check on a carried header** (`bft.rs:187-212`):

```rust
fn header_pow_is_valid(header: &BcBlockHeader, network: &zebra_chain::parameters::Network) -> Result<(), String> {
    use zebra_chain::work::difficulty::ParameterDifficulty as _;
    let mut bytes = Vec::new();
    BcBlockHeaderWrap::write_data(header, &mut bytes).map_err(|e| e.to_string())?;
    let header = Header::zcash_deserialize(&*bytes).map_err(|e| e.to_string())?;
    let hash = header.hash();
    let threshold = header
        .difficulty_threshold
        .to_expanded()
        .ok_or_else(|| format!("invalid difficulty threshold {:?}", header.difficulty_threshold))?;
    if threshold > network.target_difficulty_limit() {
        return Err(format!("difficulty threshold {threshold:?} is above the network limit"));
    }
    if network.disable_pow() {
        return Ok(());
    }
    if hash > threshold {
        return Err(format!("hash {hash} does not meet difficulty threshold {threshold:?}"));
    }
    header.solution.check(&header).map_err(|e| e.to_string())
}
```

`threshold` is the header's own field. This mirrors the context-free `difficulty_threshold_is_valid` in `zebra-consensus/src/block/check.rs:76-103`, as the doc comment says. For a full block that is only the first half: the contextual half, `difficulty_threshold_and_time_are_valid` (`check.rs:311-321`), later requires `nBits == ThresholdBits(height)`. Carried headers never reach the second half.

**The snapshot only has to be held somewhere** (`bft.rs:1012-1017`, `service.rs:546-549`):

```rust
let new_final_hash = Hash(new_block.snapshot_block_hash().0);
if read_state.known_block(new_final_hash).is_none() {
    tracing::warn!("Didn't have hash available for confirmation: {}", new_final_hash);
    return needs_pow_block(new_final_hash);
}
```

```rust
pub fn known_block(&self, hash: block::Hash) -> Option<KnownBlock> {
    read::find::non_finalized_state_contains_block_hash(&self.latest_non_finalized_state(), hash)
        .or_else(|| read::find::finalized_state_contains_block_hash(&self.db, hash))
}
```

`non_finalized_state_contains_block_hash` returns `KnownBlockLocation::SideChain` for a block on any non-best chain (`find.rs:173-179`), and `validate` ignores the location.

**Linearity accepts a side-chain snapshot** (`bft.rs:1036-1054`): `is_ancestor_of` answers "across every chain this state holds" (`service.rs:551-600`), so `P ⪯ S` holds whenever `S`'s side chain passes through `P`.

**The decision and the stall** (`bft.rs:1119`, `:809-818`):

```rust
chain.bft_final_snapshot = Some((new_final_height, new_final_hash));
```

```rust
if let Some(parent_snapshot_hash) = parent_snapshot_hash {
    if parent_snapshot_hash != candidate_hash
        && read_state.is_ancestor_of(parent_snapshot_hash, candidate_hash) != Some(true)
    {
        tracing::info!(
            "not proposing: candidate snapshot {} does not extend the parent bft-block's snapshot {}",
            candidate_hash, parent_snapshot_hash,
        );
        return None;
    }
}
```

FINALITY.md §3.4 notes that honest proposal would instead repeat the parent's `headers_bc`; `propose` does not implement that (IMPLEMENTATION.md design question 3), and repeating would not move finality past `S` anyway.

**The commit hold** (`write.rs:248-261`): held while `held_len <= MAX_BLOCK_REORG_HEIGHT + CONFLICT_HOLD_DEPTH` (99 + 999, `constants.rs:150`), then `conflict_abandoned = Some(decided_hash)` and the commit proceeds.

**What a contextual check can and cannot close.** Everything `ThresholdBits` needs is available at validation time without the blocks behind the headers:

- `AdjustedDifficulty` needs the `(nBits, time)` of the previous 28 blocks (`POW_ADJUSTMENT_BLOCK_SPAN`, `difficulty.rs:22-27`, `:60-73`).
- For `headers[0]` those are `S` and its 27 ancestors. `S` is held, and a held block's ancestors are held too: every non-finalized chain carries its whole history above the finalized tip, and the finalized database holds the rest (`service.rs:559-561`). `ReadStateService::any_chain_block_header` (`service.rs:620-631`) reads them from any chain.
- For `headers[i]`, `i > 0`, the context is `headers[i-1..=0]` followed by `S` and its ancestors, all of which are in hand.
- `AdjustedDifficulty::new_from_header_time` (`difficulty.rs:130-161`) already exists "for use when validating block headers, where the full block has not been downloaded yet", and `bft.rs` is in the same crate.

The check stays objective in FINALITY.md's sense: it is a function of the carried headers and the snapshot's ancestry, not of the validator's best chain or clock. The median-time-past lower bound (`check.rs:290-295`) is objective too and should come with it.

It does **not** close the gap by itself on a `Network::Testnet` above height 299,188:

- `expected_difficulty_threshold` returns PoWLimit whenever a block's time is more than `6 × target_spacing` after its parent's (`difficulty.rs:196-205`, `network_upgrade.rs:450-500`), and that rule is active from `TESTNET_MINIMUM_DIFFICULTY_START_HEIGHT` = 299,188 (`network_upgrade.rs:269`).
- With the Crosslink post-Blossom spacing of 25 s (`network_upgrade.rs:253`) the gap is 150 s. The only objective upper bound on a header's time, `median-time-past + 90 min`, is enforced on testnets only from height 653,606 (`network_upgrade.rs:275`, `check.rs:304-309`).
- So a proposer who spaces the forged headers' timestamps more than 150 s apart makes every one of them a legitimate minimum-difficulty header.
- ClT0 is far past 299,188. The new testnet would reach it after about 299,188 × 25 s ≈ 87 days (inference, assuming spacing holds from genesis).

Regtest is also exempt: `expected_difficulty_threshold` pins every regtest block at PoWLimit (`difficulty.rs:193-195`), so the check is a no-op there, which matters for tests.

**ClT0 comparison** (`s1_dev`, `zebra-crosslink/zebra-crosslink/src/lib.rs:1003-1030` at `64046aeb`):

```rust
let new_final_hash = ZebBlockHash(BlockHash::from_header_data(new_block.headers.first().expect("at least 1 header")).0);
let new_final_pow_height =
    if let Some(new_final_height) = block_height_from_hash(&call, new_final_hash).await {
        new_final_height.0
    } else {
```

On `s1_dev` the finalized block is `headers[0]` itself, it must be held on any chain, and the other headers are not examined. No work is needed to fake the tail. The consequence differs: `s1_dev` finalizes the decided block directly through `CrosslinkFinalizeBlock` (`lib.rs:787`), which FINALITY.md §4.2 and §5.2 describe as collapsing onto the decided branch, so a known side-chain hash becomes canonical. Not traced further here.

## Recommendations

1. **Check every carried header against the difficulty its position requires.** In `bft.rs`, add a helper next to `header_pow_is_valid` and call it from `validate` right after the snapshot is known (after `bft.rs:1017`, before the per-header PoW loop):
   - the snapshot's height comes from `known_block`;
   - the context for header 0 is `S` and up to 27 ancestors read with `any_chain_block_header`; genesis ends the walk early;
   - each header is checked with `AdjustedDifficulty::new_from_header_time` plus `difficulty_threshold_and_time_are_valid` (make that function `pub(crate)`), then pushed onto the front of the context for the next header.

   This restores the invariant "each carried header could head a bc-valid block at its height", which is what the Book's honest validator gets from downloading the blocks, while keeping FINALITY.md §8.1's "headers, not blocks" property. Sketch:

   ```rust
   fn carried_headers_meet_required_difficulty(
       read_state: &ReadState,
       snapshot_hash: Hash,
       snapshot_height: Height,
       headers: &[Header],
   ) -> Result<(), String> {
       let network = read_state.network();
       let mut context: Vec<(CompactDifficulty, DateTime<Utc>)> = Vec::with_capacity(POW_ADJUSTMENT_BLOCK_SPAN);
       let mut cursor = snapshot_hash;
       while context.len() < POW_ADJUSTMENT_BLOCK_SPAN {
           let header = read_state
               .any_chain_block_header(HashOrHeight::Hash(cursor))
               .expect("a held block's ancestors are held: each chain carries its history above the finalized tip");
           context.push((header.difficulty_threshold, header.time));
           if header.previous_block_hash == GENESIS_PREVIOUS_BLOCK_HASH {
               break;
           }
           cursor = header.previous_block_hash;
       }

       let mut previous_height = snapshot_height;
       for (i, header) in headers.iter().enumerate() {
           let adjustment = AdjustedDifficulty::new_from_header_time(
               header.time,
               previous_height,
               network,
               context.iter().copied(),
           );
           if let Err(error) = check::difficulty_threshold_and_time_are_valid(header.difficulty_threshold, adjustment) {
               return Err(format!("carried header {i}: {error}"));
           }
           context.insert(0, (header.difficulty_threshold, header.time));
           context.truncate(POW_ADJUSTMENT_BLOCK_SPAN);
           previous_height = (previous_height + 1).expect("sigma headers above a held block stay below Height::MAX");
       }
       Ok(())
   }
   ```

   `propose` must apply the same helper to its own window and decline on failure, as it already does for linkage (`bft.rs:839-846`), so an honest node never proposes a tail its peers reject.

2. **Close the testnet minimum-difficulty hole for the new testnet.** Without this, step 1 stops protecting the network at height 299,188. Preferred: make the minimum-difficulty start height a `testnet::Parameters` field (the upstream TODO at `network_upgrade.rs:457`, Zebra #8364) and set it to "never" for the new testnet, so `ThresholdBits` always applies. This is a PoW consensus parameter and must be fixed before genesis. Fallback, if the testnet must keep minimum-difficulty blocks: in the carried-header helper only, reject a header that qualifies through the minimum-difficulty exemption (`NetworkUpgrade::is_testnet_min_difficulty_block` against the previous context time). Cost of the fallback: when the honest chain contains a legitimate minimum-difficulty block, no proposal can finalize a window containing it, so BFT waits sigma blocks longer; `propose` must mirror the rule.

3. **Considered alternatives.**
   - *Download and fully validate the tail blocks* (the Book's honest validator): rejected by FINALITY.md §8.1 for the catch-up deadlock after the tail is orphaned; the header-context check gives the same difficulty guarantee without it.
   - *Require the snapshot to be on the validator's best chain with sigma real confirmations*: not objective; a node catching up on decided bft-blocks could never validate one whose snapshot was later reorged, which is exactly the deadlock above.
   - *Require cumulative tail work of at least `sigma × work(nBits(S))`*: simpler, needs no 28-block context, but it is not the chain's rule, it drifts with difficulty adjustment in both directions, and it still admits minimum-difficulty timestamps games unless combined with item 2.

4. **Tests** (end to end, crosslink test-format scenarios):
   - *Forged tail is rejected.* On a configured testnet (not regtest, which pins every block at PoWLimit) with a mined history whose `ThresholdBits` is below PoWLimit, `LoadPoS` force-feeds a BFT block whose sigma headers declare PoWLimit and link onto a held best-chain block. Assert the force-feed fails with `PoS validation = Fail` and `bft_final_snapshot` is unchanged. `disable_pow = true` is fine for this test: the `nBits` equality check does not depend on Equihash, so no mining is needed.
   - *Side-chain variant.* Same, with the snapshot on a one-block side chain; assert rejection and that a following honest proposal is decided.
   - *Honest tail passes.* Control case: the honest proposer's own window is decided, including a window whose timestamps were produced by the internal miner.
   - *Minimum-difficulty case* (only if item 2's fallback is chosen): a tail containing a timestamp-gapped header is rejected, and `propose` declines rather than proposing it.
   - Unit test only for the pure helper: context assembly across a side chain that forks from the finalized prefix.

5. **Rollout.** Items 1 and 2 change BFT and PoW validity rules. Every finalizer must run the same rule or honest proposals split the vote, so both must land before the new testnet's genesis; after genesis they need a coordinated upgrade at a scheduled BFT height. ClT0 (`s1_dev`) has no header checks at all; a backport would be a coordinated upgrade of the live network, which is worth doing only if ClT0 is to stay live alongside the new testnet. Otherwise record the weakness in ClT0's known issues.

## Validation Information

**Verdict: CONFIRMED. Severity: High.**

| Claim | Verified at |
|-|-|
| Carried headers are checked only against their own `nBits`, bounded by PoWLimit | `bft.rs:187-212`, called at `:1027-1032` |
| No contextual `ThresholdBits` comparison anywhere in `validate` | `bft.rs:885-1057` read in full |
| Snapshot may be on any chain; carried headers need not be known | `bft.rs:1012-1017`, `service.rs:546-549`, `find.rs:159-184`, FINALITY.md:319-323 |
| Linearity accepts a side-chain snapshot descending from the parent's | `bft.rs:1036-1054`, `service.rs:562-600` |
| Decision sets `bft_final_snapshot` to the snapshot | `bft.rs:1083-1119` |
| Honest proposers then decline; honest validators reject non-extending proposals | `bft.rs:809-818`, `:1040-1045` |
| Commit hold up to 99 + 999 blocks, then abandonment | `write.rs:248-261`, `constants.rs:150`, `zcash_protocol/src/consensus.rs:713` |
| Decided chain is exempt from fork eviction | `non_finalized_state.rs:300-313` |
| Only the scheduled proposer's proposal is accepted | tenderlink `lib.rs:2317-2322`, `:785` |
| sigma = 4 | `librustzcash/zcash_primitives/src/bft.rs:533-534` |
| PoWLimit: default testnet `2^251 - 1`; ClT0 deploy config and test configs `0f0f…0f`; the in-tree new testnet (ClT1) config sets no limit, so it gets the default unless a deployment TOML overrides it | `testnet.rs:525-533`; `zebrad/src/config.rs:309-327` (no `with_target_difficulty_limit`); `testnet_online.toml:64` (untracked); `zebrad/tests/common/configs/v2.5.0-funding-streams.toml:62` |
| Testnet minimum-difficulty rule from height 299,188, gap 6 × 25 s | `network_upgrade.rs:253`, `:264`, `:269`, `:450-500`; `difficulty.rs:196-205` |
| Testnet max-time rule only from 653,606 | `network_upgrade.rs:275`, `network.rs:274-280`, `check.rs:304-309` |
| `s1_dev` checks no header PoW, linkage or count | `64046aeb:zebra-crosslink/zebra-crosslink/src/lib.rs:890-1030` |

**Severity justification.**

*Why not Critical:* this is a public test network with no real value at stake. The attacker must hold bonded stake and be scheduled as proposer. The outcome is loss of finality liveness plus the abandoned-decision state of finding 6, not theft, a PoW-chain fork, or silent acceptance of invalid transactions. The effect is also loud: "not proposing: candidate snapshot … does not extend" on every honest node, and "can no longer follow that decision" after the hold.

*Why not Medium:* one byzantine roster member with any nonzero stake halts BFT finality for the whole network with one decided proposal, at a cost of roughly one real block plus about 70 PoWLimit Equihash runs (inference). There is no in-protocol recovery: the decision is persisted and every later honest proposal is invalid against it. Adversarial participants are expected on a public testnet, and the attack needs no hash-rate majority, no stake beyond roster membership, and no network position.

**Corrections made during validation.**

1. "Over any block the validators know about" was too broad. The snapshot must also descend from the parent bft-block's snapshot (Linearity, `bft.rs:1036-1054`), so it must be on a chain through the last decided snapshot. The self-mined one-block side chain in step 1 satisfies this at will, so the scenario stands.
2. "On the public testnet PoWLimit is trivial": the in-tree new testnet (ClT1) parameters in `zebrad/src/config.rs:309-327` set no limit, so they use the default `2^251 - 1`, but a deployment TOML can override it, as ClT0's did. Stated the values actually found (default `2^251 - 1`, ClT0 config and test configs `0f0f…0f`) and made the attack's dependence on "real difficulty above PoWLimit" an explicit assumption.
3. Added that the contextual check alone is not a complete fix: past height 299,188 the testnet minimum-difficulty rule lets a proposer earn PoWLimit legitimately by spacing forged timestamps more than 150 s apart, so the fix plan includes item 2.
4. Added that ClT0 is affected by a strictly weaker check (no header PoW at all), with a different downstream consequence (direct finalization rather than a Linearity stall).

**Cross-references.**

- `dropped-decided-snapshot-leaves-validate-indeterminate-forever-and-decide-can-abort.md` (finding 6): this attack drives nodes into that state once `crosslink_conflict_hold` gives up.
- `missing-pow-blocks-grows-without-bound-from-unverified-proposal-snapshots.md` (finding 3): the attack uses the same by-hash fetch path to deliver `S` to validators that do not hold it. That finding's fix (scoping the missing set to live proposals) keeps this delivery working, because the proposal stays live while it is being voted on.
- `admit-fat-pointer-accepts-a-certificate-by-hash-without-checking-signatures.md` (finding 7) and `template-fat-pointer-is-chosen-against-the-live-tip-not-the-template-parent.md` (finding 8): both assume decided snapshots are on a chain honest miners can build on; this finding breaks that assumption from the BFT side.
