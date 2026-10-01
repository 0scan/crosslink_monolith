# The `new_network` BFT code has 39 `as` casts without a safety comment, and `finish_decision` and `restore` collapse a missing `aggregated_stakes` record into "zero stake" with `unwrap_or_default()`, so a damaged stake cache is carried on as a node-local ghost roster instead of stopping the node the way `s1_dev` does

**Severity**: Low
**Validation Status**: Partially confirmed
**Location**: `zebra-crosslink/AGENTS.md:198` and `:203` (the two rules); `zebra-crosslink/zebra-state/src/new_network/bft.rs:1139-1176` (`finish_decision` stake read and ghost roster), `:1279-1297` (`restore` stake read), `:1084-1089` (`decide` validates against `read_state`), `:298`, `:301`, `:304`, `:338`, `:339`, `:345`, `:400`, `:411`, `:431`, `:468`, `:559`, `:569`, `:746`, `:759`, `:772`, `:826`, `:832`, `:860`, `:899`, `:900`, `:994`, `:1078`, `:1091`, `:1097`, `:1116`, `:1182`, `:1183`, `:1208`, `:1271`, `:1282`, `:1327`, `:1328`, `:1335`, `:1372`, `:1373`, `:1481` (casts), `:573-580` (null-pointer fallbacks); `zebra-crosslink/zebra-state/src/new_network/fin.rs:136`, `:145` (casts); `zebra-crosslink/zebra-state/src/service/non_finalized_state/chain.rs:672-677`, `:2228-2237` and `zebra-crosslink/zebra-state/src/service/finalized_state/zebra_db/delegation.rs:194-198`, `:399-462`, `zebra-crosslink/zebra-state/src/service/finalized_state/zebra_db/block.rs:699-701` (where stake records come from); `zebra-crosslink/zebra-state/src/service/write.rs:411-437` and `zebra-crosslink/zebra-state/src/service/non_finalized_state.rs:357-365` (read view lags the writer after `finalize()`); `zebra-crosslink/zebra-state/src/service/stake_fixup.rs:1-9` (torn rows)
**Found by agent:** /code-review high (Claude Fable 5.1), 2026-09-29; validated 2026-09-29 at dev e99404e3de7cc
**In scope of audit?** Yes. `new_network/bft.rs` and `new_network/fin.rs` do not exist at `64046aeb`; both were added on `dev`, and the stake handling feeds the BFT roster, which is hybrid-consensus state. Two qualifications: 28 of the 39 casts were moved from `s1_dev`'s `zebra-crosslink/zebra-crosslink/src/lib.rs` (including all three the review cites), and `AGENTS.md` itself only arrived on `dev` with the upstream merge `2060039d`. The silent ghost-roster behaviour is new on `dev`: `s1_dev` also calls `unwrap_or_default()`, but then panics. ClT0 is not affected by the stake issue and the carried-over casts are harmless there.

## Description

`zebra-crosslink/AGENTS.md` contains two rules this module does not follow. Quoted verbatim:

- Line 198, under "Error handling": "Don't turn invariant violations into misleading `None`/default values"
- Line 203, under "Numeric Safety": "All `as` casts must have a comment explaining why the cast is safe"

**The cast rule.** `bft.rs` has 37 `as` casts to a primitive type and `fin.rs` has 2. None of them has a comment explaining why the cast is safe. The comments near `:772` and `:1091` explain what the value *means* (the 0-based height is the chain index), not why the conversion cannot lose information. Every cast is safe today, for one of four reasons:

- it widens without loss;
- its value came from the target type;
- a guard nearby rules out truncation;
- or it could only truncate with an absurd configured σ.

So the cast finding is a compliance and hardening issue, not a live bug. The full list is under Technical Details.

**The misleading-default rule.** This is the part that matters. Both `NonFinalizedState::aggregated_stakes_at` and `ZebraDb::aggregated_stakes` return `Option<Vec<([u8; 32], u64)>>`, and the two cases mean different things:

- **`Some(vec![])`** is a real record that says "no stake". It is legitimate: every bond is unbonded and every reward bank is empty.
- **`None`** means there is no record. For a block the node holds, the code's own invariants say that cannot happen (see below).

`finish_decision` (`bft.rs:1139-1143`) and `restore` (`bft.rs:1284-1288`) both end the lookup with `.unwrap_or_default()`. That turns `None` into `Some(vec![])`. The empty-stakes branch then deliberately keeps the previous roster as a "ghost roster" (`bft.rs:1151-1176`). That design is right for genuinely zero stake, because every node computes the same empty set and carries the same roster forward. For a missing record it is wrong: only this node carries the old roster forward, while its peers adopt the real stakes.

**The node is told this is probably fine.** The warning at `bft.rs:1166-1175` fires only if the previous roster had a member with `voting_power > 1`. It then says the situation is "Expected when every bond has been unbonded" and names a torn cache only as a fallback suspicion. The review's word "silently" overstates it, since a warning is logged. The warning still points the operator toward the benign reading.

## Attack Scenario and Steps

**No adversary is needed.** The trigger is ordinary damage to local state. One inferred path also involves an ordinary chain event.

**Path 1: a torn stakes row (verified mechanism, ordinary trigger)**

1. On this branch, a finalized block's stakes row is written in the block's own batch (`block.rs:699-701`). A row can therefore only go missing through an OS-level loss of the write-ahead log tail, such as power loss or a host crash. `stake_fixup.rs:108-111` describes contiguous missing heights as exactly that signature. A database written by an older, pre-atomic binary is the other source, but the new testnet starts from a fresh genesis, so that source does not apply there.
2. The node restarts. `restore` replays the stored BFT chain. At the decision whose snapshot lost its row, `bft.rs:1288` yields an empty vector, and `bft.rs:1291` keeps the previous roster for the next height.
3. The node runs tenderlink with that roster (`bft.rs:1329`, `:1348`). Its peers use the real stakes. Live decisions go through the same conflation at `bft.rs:1143`.
4. The operator sees a warning that calls this expected, and the node keeps running.

**Path 2: the read view lags the writer (mechanism verified by reading; reachability inferred)**

1. `handle_commit` publishes the non-finalized state to the read view at `write.rs:414-419`, *before* its finalize loop at `write.rs:421-437`.
2. `NonFinalizedState::finalize` drops every side chain whose root is not the finalized block (`non_finalized_state.rs:358-365`). The read view is not republished afterwards, so it still holds those chains until the next commit.
3. The BFT runner serves requests between commits (`new_network.rs:1583`, `:3064`, `:3168`). `decide` validates and resolves the snapshot through `read_state` (`bft.rs:1084`, `:1089`). `finish_decision` reads stakes from `block_writer` (`bft.rs:1139-1142`).
4. If a decided snapshot sits on a side chain that the writer just dropped, `read_state` still finds it while the writer does not. `aggregated_stakes_at` and the database both return `None`, and the ghost roster is kept.
5. *Inference, not traced end to end:* reaching step 4 needs BFT finality to lag about `MAX_BLOCK_REORG_HEIGHT` blocks behind the tip, plus a competing branch that forks below the height being finalized. On a low-difficulty testnet with the stalls described in the other findings, that is narrow but not impossible.

**Attack Requirements and Assumptions:**

- Path 1: a host crash or power loss that drops the rocksdb WAL tail. No attacker is involved.
- Path 2: a long BFT stall together with a deep competing branch. An adversary who can mine cheaply could help build the branch, but no concrete exploit is claimed.
- No cast path is exploitable. The only casts that could truncate need either a 32-bit target or a configured σ of at least 2^32.

## Impact on Users

- **Finalizers and stakers.** A node on Path 1 or 2 runs tenderlink with a roster its peers do not share. It weighs votes by stakes that no longer exist, may count signatures from members who are no longer on the real roster, and may fail to count real quorums. *Inferred from how tenderlink uses the roster; not exercised.* The likely visible result is a finalizer that stops contributing, or that disagrees on what has been decided, while its log says the situation is expected.
- **Node operators.** The repair already exists: the `fixup-db-stake` command (`zebrad/src/commands/fixup_db_stake.rs`, which drives `stake_fixup.rs`). The operator is not sent to it with any urgency. On `s1_dev` the same condition stops the node with instructions to run that repair (`64046aeb:zebra-crosslink/zebra-crosslink/src/lib.rs:807-822`).
- **Miners, wallets, light clients.** Not directly affected. The roster only shapes BFT voting on the affected node.
- **Casts.** No user impact today.

## Technical Details / Code Analysis

### What a stake record is, and when it can be missing

**Records are positional and written for every block.** `Chain::push` appends one entry per block, even when the set is empty (`chain.rs:2228-2237`). `pop_root` and `pop_tip` remove entries in step (`chain.rs:511-513`, `:2337`). The lookup is by index from the chain's root:

```rust
    pub fn aggregated_stakes_at(&self, hash: block::Hash) -> Option<Vec<([u8; 32], u64)>> {
        let height = self.height_by_hash(hash)?;
        let root_height = *self.blocks.keys().next()?;
        let index = height.0.checked_sub(root_height.0)? as usize;
        self.aggregated_stakes.get(index).cloned()
    }
```

`chain.rs:672-677`.

**The finalized path writes the row in the block's own batch.** It does so unconditionally, after the bond branch, at `block.rs:699-701`. `prepare_aggregated_stakes_batch` (`delegation.rs:399-462`) always inserts a row. When nothing is at stake that row is an empty vector, because `aggregate_stakes` drops zero banks (`delegation.rs:208-223`).

**Conclusion.**

- **Zero stake is `Some(vec![])`.** It is legitimate, and it is the case the ghost roster was designed for.
- **`None` means one of three things:**
  1. The block is not held at all.
  2. The block is held in the database but its row is missing: WAL-tail loss or a legacy database.
  3. The block is held in a `Chain` whose positional vector is out of step: a code bug.
- **In `finish_decision`, case (a) should be impossible,** because `decide` has just validated the snapshot as known. It is reachable only through the view lag of Path 2. Cases (b) and (c) are invariant violations. **None of the three means "zero stake".**

### The conflation

`finish_decision` (`bft.rs:1135-1151`):

```rust
        // The roster the next height votes with is the stake at this block's snapshot, read from
        // the chain rather than taken from the finalize result (FINALITY.md §7). The finalize
        // above put the snapshot in the finalized database, which is the only place that holds
        // aggregated stakes.
        let got_stakes = block_writer
            .non_finalized_state
            .aggregated_stakes_at(new_final_hash)
            .or_else(|| block_writer.finalized_state.db.aggregated_stakes(&new_final_hash))
            .unwrap_or_default();

        let mut chain = BFT_CHAIN.write().unwrap();
        if !got_stakes.is_empty() {
            chain.roster = got_stakes
                .into_iter()
                .map(|s| RosterMember { pub_key: s.0, voting_power: s.1, txids: Vec::new(), finalizer_address: None })
                .collect();
        } else {
```

The comment above it is also stale:

- A decision no longer finalizes anything (`bft.rs:1122-1124`).
- `Chain` holds stakes too (`chain.rs:672`).

`restore` repeats the same pattern (`bft.rs:1279-1297`):

```rust
            if !block.headers.is_empty() {
                let snapshot = Hash(block.snapshot_block_hash().0);
                if let Some(known) = read_state.known_block(snapshot) {
                    prev_finalized_bc_height = known.height.0 as u64;
                }
                let stakes = block_writer
                    .non_finalized_state
                    .aggregated_stakes_at(snapshot)
                    .or_else(|| block_writer.finalized_state.db.aggregated_stakes(&snapshot))
                    .unwrap_or_default();
                // Empty stakes are the ghost roster of `finish_decision`: the previous roster is
                // carried forward rather than replaced, so replay reproduces what voted.
                if !stakes.is_empty() {
                    unsorted_roster = stakes
                        .into_iter()
                        .map(|s| RosterMember { pub_key: s.0, voting_power: s.1, txids: Vec::new(), finalizer_address: None })
                        .collect();
                }
            }
```

**In `restore`, a snapshot the node does not hold keeps both stale values.** Both the watermark at `:1281-1283` and the roster are carried forward. Today this is masked, and the chain of reasoning is:

- Linearity makes every decided snapshot an ancestor of the last one.
- So an intermediate snapshot can only be missing if the last one is missing too.
- If the last one is missing, `restore` exits at `bft.rs:1306-1321`.

If finding 1's fix relaxes that exit, this carry-forward becomes live. A torn row, where the block is held but its row is missing, is not masked by anything.

**`s1_dev` had the same `unwrap_or_default()` but failed loudly.** It is at `64046aeb:zebra-crosslink/zebra-state/src/service/write.rs:254-260`, followed by the check at `64046aeb:zebra-crosslink/zebra-crosslink/src/lib.rs:807-822`:

```rust
    if got_stakes.len() > 0 {
        internal.finalizers_at_current_height = got_stakes.into_iter().map(|s| RosterMember { pub_key: s.0, voting_power: s.1, txids: Vec::new() }).collect();
    } else {
        let mut any_non_zero = false;
        for val in &internal.finalizers_at_current_height {
            if val.voting_power > 1 {
                any_non_zero = true;
            }
        }
        if any_non_zero {
            panic!(
```

**`s1_dev` erred the other way.** It also panics on a legitimate all-unbonded zero. `dev` fixed that by adding the ghost roster, but in doing so it also silenced the torn case.

**This same code was hurt by a misleading default before.** `64046aeb:zebra-crosslink/zebra-crosslink/src/lib.rs:1328-1331` records that an earlier `unwrap_or(0)` in the replay watermark "activated the entire blacklist across the whole replay, nondeterministically by DB-availability race".

### The read view lags the writer

`write.rs:411-437`:

```rust
        // Committing blocks to the finalized state keeps the same chain, so we can update the
        // chain seen by the rest of the application now.
        let tip_block_height = update_latest_chain_channels(
            &self.non_finalized_state,
            &mut self.chain_tip_sender,
            &self.non_finalized_state_sender,
            None,
        );

        while self
            .non_finalized_state
            .best_chain_len()
            .expect("just successfully inserted a non-finalized block above")
            > MAX_BLOCK_REORG_HEIGHT
        {
            if self.crosslink_conflict_hold() {
                break;
            }
            tracing::trace!("finalizing block past the reorg limit");
            let contextually_verified_with_trees = self.non_finalized_state.finalize();

            self.prev_finalized_note_commitment_trees = self.finalized_state
                        .commit_finalized_direct(contextually_verified_with_trees, self.prev_finalized_note_commitment_trees.take(), "commit contextually-verified request")
                        .expect(
                            "unexpected finalized block commit error: note commitment and history trees were already checked by the non-finalized state",
                        ).1.into();
        }
```

"Keeps the same chain" is true of the best chain. It is not true of the side chains that `finalize()` drops (`non_finalized_state.rs:358-365`). `handle_crosslink_finalize` has the same ordering (`write.rs:332-338`, then its loop).

### The 39 casts

**Origin legend:**

- **carried:** the same expression exists in `64046aeb:zebra-crosslink/zebra-crosslink/src/lib.rs` at the line given.
- **reworked:** an equivalent cast exists there.
- **new:** grep found no counterpart.

Unless marked `fin.rs`, lines are in `bft.rs`.

| Line | Cast | Why it is safe today | Origin | Planned change |
|-|-|-|-|-|
| 298, 301, 304 | `at_height as usize` (u64 to usize) | Lossless on 64-bit; `at_height == 0` is tested first, so `- 1` cannot underflow; the only caller (`:574`) passes `i + 1` for an index into `chain.blocks` | carried, 1038/1042/1046 | `usize::try_from`; on failure return `None`, which is the documented meaning ("the chain has no such block") |
| 338, 339 | `signatures.len() as u64` | usize is at most 64 bits on every target Zebra builds | carried, 1340/1341 | `u64::try_from(..).expect(..)` |
| 345 | `round as u32` (i32 to u32) | `Vote::from_bytes` masks the round to 31 bits (`librustzcash/zcash_primitives/src/bft.rs:1199`), so it is never negative | carried, 1347 | `u32::try_from(..).expect("Vote::from_bytes masks the round to 31 bits")` |
| 400, 431 | `h as usize` (u64 to usize) | Every value was written from a usize (`:1116`, `:1335`) | carried, 338/359 | Store `usize` in `hash_to_height` |
| 411 | `pow_block_height.0 as u64` | Widening | carried, 349 | `u64::from` |
| 468 (two) | `pow_block_height.0 as u64`, `snapshot_height.0 as u64` | Widening | new | `u64::from` |
| 559 | `known.height.0 as u64 + sigma + 1` | Widening; the sum overflows only for σ near `u64::MAX` | new | `u64::from`, plus the σ bound (step 4) |
| 569 | `i as u64 + 1` | usize to u64 is lossless | reworked, 1904 | Keep the index as `usize` (step 2) |
| 746 | `tip_height.0 as u64` | Widening | new | `u64::from` |
| 759 | `sigma as u32` | `:746` returned unless tip ≥ σ, so σ ≤ `u32::MAX` and the subtraction cannot underflow | new | Say so in a comment, or `u32::try_from` once σ is bounded |
| 772, 899, 1182, 1271, 1327, 1481 | `len() as u64` | usize to u64 is lossless | carried or reworked, 666/910/848/1324/1375/1407 | One helper holding the single index-to-height cast |
| 826, 832, 994, 1372 | `sigma as usize` | Truncates only on a 32-bit target with σ ≥ 2^32 | 826 carried (660); others new | σ bound, then a comment citing it |
| 860 | `bft_height as u32` | A BFT chain of 2^32 blocks is unreachable, and a truncated height fails `validate` (`:900`) | carried, 683 | `u32::try_from`; decline to propose on failure |
| 900, 1078, 1183, 1208, 1282, 1328 | `u32 as u64` | Widening (`:1208` adds 1 to a widened u32, so it cannot overflow) | carried or reworked, 913/727/849/1455/1335/1376 | `u64::from` |
| 1091 | `new_block.height as usize` | `validate` (`:899-903`), asserted at `:1084`, already made it equal to `chain.blocks.len()` | carried, 742 | `let insert_i = chain.blocks.len();` |
| 1097 | `i as u32` | Given `:1084`, the placeholder loop runs once, with `i == new_block.height` | carried, 751 | Delete the loop (step 3) |
| 1116, 1335 | `insert_i as u64`, `i as u64` | usize to u64 is lossless | carried, 776/1382 | Store `usize` in `hash_to_height` |
| 1373 | `sigma as u32` | A σ ≥ 2^32 truncates, and a large `roster_height + σ` overflows; either way `BftBlock::try_from` rejects the header count, so bootstrap logs and never happens | new | σ bound, then `u32::try_from` |
| fin.rs:136 | `bft_height as usize` | Written from a usize | new | Store `usize` in `hash_to_height` |
| fin.rs:145 | `sigma as u32` | Truncation would shrink the prune_σ clamp. It is unreachable because with σ ≥ 2^32 nothing is proposed (`:746`) and admission rejects every real pointer (`:469`), so `candidate` returns `None` at `fin.rs:132-133` (read, not run) | new | σ bound |

**Totals:** 39 casts; 28 carried or reworked from `s1_dev`, 11 new on `dev`.

**σ is operator-configurable.** It comes from `zebra-network/src/config.rs:632`, and `with_crosslink_parameters` (`zebra-chain/src/parameters/network/testnet.rs:862-873`) checks the bootstrap and the staking calendar but not σ. σ = 0 is worse than any cast: `validate` accepts zero headers (`bft.rs:995`), and then `snapshot_block_hash()` panics on the empty vector (`bft.rs:1013`; `librustzcash/zcash_primitives/src/bft.rs:318-320`).

### Other `None`/default conversions in the new code

| Site | Conversion | Verdict |
|-|-|-|
| `bft.rs:1143` | Missing stake record becomes zero stake | **Violation** (above) |
| `bft.rs:1281-1288` | Unheld snapshot keeps the stale watermark and roster; missing row becomes zero stake | **Violation**; the unheld part is masked today by the exit at `:1306-1321` |
| `bft.rs:573-575` | `fat_pointer_to_block_at_height(..).unwrap_or_else(FatPointerToBftBlock::null)` | **Violation, unreachable today.** `h = i + 1` always resolves. If it ever failed, the miner would get a null pointer, and admission rejects that permanently when the parent has one (`bft.rs:390-392`). Carried from `s1_dev` (lib.rs:1905). Replace with `expect` |
| `bft.rs:577-580` | Parent header not readable becomes a null pointer | **Violation in the race case.** `parent_hash: None` (empty state, GUI query) correctly gives null. A tip hash that cannot be read back is finding 8's race, and it should become an error returned to `getblocktemplate`, not a doomed template |
| `bft.rs:327` | Missing signature becomes `[0u8; 64]` | Honest: that value is `TMSig::NIL` (`librustzcash/zcash_primitives/src/bft.rs:726`), which tenderlink counts as absent (`tenderlink/src/lib.rs:402`). Spell it `TMSig::NIL.0` |
| `bft.rs:443-444`, `:775`, `:906-907`, `:778` | Null pointer ranks 0; no parent gives version and `do_not_include` 0; no final snapshot means "improved" | Honest and documented; 0 is the v1 implicit value |
| `bft.rs:1496`, `:1508-1510` | Closed reply becomes "no proposal" or `Indeterminate` | Honest: both are tenderlink's "cannot answer". The reply can only close if the sync thread is gone, and under `panic = "abort"` (`zebra-crosslink/Cargo.toml:281`) the process goes with it |
| `fin.rs:93-95`, `:121-153` | Unseen block gives `CantBeFinalized`; `candidate` gives `None` | Honest; documented in FINALITY.md §7.2 and in the doc comment |

**Adjacent, same spirit, outside the two rules:**

- `bft.rs:1195-1202` only logs a failed `write_bft_decision`. See finding 4.
- `bft.rs:1089` uses a bare `.unwrap()`, where the AGENTS.md error-handling rules ask for an `expect` saying why the invariant holds.
- **Unvalidated, for the code owner:** `bft.rs:338-339` store a signature *count* in `ConsensusCounts`. Tenderlink fills those fields with *stake-weighted* sums (`tenderlink/src/lib.rs:416-421`). I did not check whether anything reads the counts of an ingested round.

## Recommendations

### 1. Stop conflating a missing stake record with zero stake

**The change.** Add one helper in `bft.rs` and use it at both call sites. It keeps `Option` with a single honest meaning, "this node does not hold the block", so no new type is needed. It panics on a held block without a record, because that is a damaged state cache.

```rust
/// The validator set at `snapshot`, or `None` when this node holds no such block.
fn snapshot_stakes(block_writer: &WriteBlockWorkerTask, snapshot: Hash) -> Option<Vec<([u8; 32], u64)>> {
    let non_finalized = &block_writer.non_finalized_state;
    if non_finalized.any_chain_contains(&snapshot) {
        let stakes = non_finalized.aggregated_stakes_at(snapshot).expect(
            "Chain::push appends one stakes entry per block and pop_root and pop_tip remove them \
             in step, so every block a chain holds has one",
        );
        return Some(stakes);
    }
    let db = &block_writer.finalized_state.db;
    if !db.contains_hash(snapshot) {
        return None;
    }
    let stakes = db.aggregated_stakes(&snapshot).expect(
        "a finalized block's stakes row is written in the block's own batch, so a finalized block \
         without one is a damaged state cache: stop the node and run the zebrad fixup-db-stake repair",
    );
    Some(stakes)
}
```

**In `finish_decision`:**

- Replace `:1139-1143` with a call to the helper, ending in `.expect("decide() validated this snapshot against the same state on this thread")`.
- Keep the `is_empty()` branch as it is. It now sees only a genuine zero.
- Reword the warning so it no longer suggests a torn cache.
- Fix the stale comment at `:1135-1138`.

**In `restore`:**

- Move both the watermark read and the stake read under one `match snapshot_stakes(..)`.
- On `None`, stop instead of carrying stale values forward: until finding 1 lands, exit with the same message as `:1314-1320`.
- Finding 1's fix should then replace both exits together.

**The invariant this restores:** the ghost roster happens only when the chain itself says there is no stake. That makes it the same on every node.

### 2. Make `decide` and the writer read the same state

**The change.** In `WriteBlockWorkerTask::handle_commit`, publish the non-finalized state *after* the finalize loop instead of before it (`write.rs:414-419` moves below `:437`). Do the same in `handle_crosslink_finalize` (`write.rs:332-338`). Then `read_state` never holds a chain the writer has dropped while BFT requests are served. That is what makes the `expect` in step 1 an invariant rather than a race.

**What this does not fix.** A decided snapshot whose chain really was dropped then shows up as `Indeterminate` in `validate` and trips the `assert_eq!` at `bft.rs:1084`. That is finding 6, and its fix owns that case.

*Alternative:* have `validate` and `decide` resolve blocks through `block_writer` instead of `read_state`. Rejected: it changes every consensus read in the module. Moving one publish is smaller and also helps finding 8.

### 3. Casts: remove what can be removed, justify the rest, and enforce it mechanically

Changes, in order:

1. **Widening u32 to u64** (`:411`, `:468` ×2, `:559`, `:746`, `:900`, `:1078`, `:1183`, `:1208`, `:1282`, `:1328`): use `u64::from(x)`. No cast, no comment needed.
2. **`BftChain::hash_to_height`:** make it a `HashMap<Blake3Hash, usize, ..>`. It is in-memory only and rebuilt at `:1335`. This removes the casts at `:400`, `:431`, `:1116`, `:1335` and `fin.rs:136`, plus the matching casts in `zebra-crosslink/src/lib.rs:232-233` and `viz2.rs:864`. Keep `suitable_height` at `:569` as a `usize` as well.
3. **`decide`:** set `let insert_i = chain.blocks.len();` and replace the placeholder loop at `:1094-1106` and the assignment at `:1117` with `chain.blocks.push(new_block.clone())`. `validate` requires `height == chain.blocks.len()` (`:899-903`) and `decide` asserts `Pass` (`:1084`), so the loop can only push the one entry it immediately overwrites. This removes `:1091` and `:1097`. The placeholder guards elsewhere become dead code and can go in a follow-up.
4. **σ:** bound it once, in `ParametersBuilder::with_crosslink_parameters` (`testnet.rs:862-873`). Reject `σ == 0`, and reject any σ above a small cap (for example `MAX_BLOCK_REORG_HEIGHT`; the owner picks the number, but it must be at most `u32::MAX`). Add a `const _: () = assert!(..)` for `PROTOTYPE_PARAMETERS`. Every σ cast (`:759`, `:826`, `:832`, `:994`, `:1372`, `:1373`, `fin.rs:145`) then carries one comment: "σ ≤ cap, checked by `with_crosslink_parameters`".
5. **Other narrowing casts:**
   - `:298-304` use `usize::try_from` and return `None`.
   - `:860` uses `u32::try_from` and declines to propose, with a log line.
   - `:345` and `:338-339` use `try_from(..).expect(..)` with the reasons given in the table.
   - The remaining `len() as u64` sites (`:772`, `:899`, `:1182`, `:1271`, `:1327`, `:1481`) go through one `fn bft_height_of_index(index: usize) -> u64`, which holds the only cast and its comment.
6. **Enforcement:** add `#![deny(clippy::cast_lossless, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]` at the top of `bft.rs` and `fin.rs`. Put `#[expect(clippy::..., reason = "...")]` on any cast that stays, so the reason is the comment the rule asks for and a new uncommented cast fails clippy.
7. **Null-pointer fallbacks:** replace `:575`'s `.unwrap_or_else(FatPointerToBftBlock::null)` with `.expect("h is i + 1 for an index into chain.blocks")`. Fold `:577-580` into finding 8's fix by passing the template's parent hash and header in, and returning an error when the header cannot be read.

**Considered alternatives:**

- **Go back to `s1_dev`'s rule (panic on any empty stakes once the roster had stake).** Rejected. It conflates the two cases the other way and kills every node when stake legitimately drains to zero, which the `dev` comment at `:1152-1161` identifies as reachable.
- **On a missing record, stop BFT but keep syncing PoW, instead of aborting.** Rejected for now. `spawn_tenderlink` keeps no handle to stop tenderlink (`bft.rs:1483`), and a node whose BFT is quietly dead is the same failure in a different form. Aborting with a repair instruction matches `restore`'s existing exits.
- **Comment every cast in place, with no refactor.** This is acceptable for compliance. It leaves 39 comments where fewer than ten casts remain after steps 1 to 3.

### 4. Tests

End-to-end first:

1. **Scenario test (test format, `zebrad/tests/crosslink.rs`).**
   - *Drives:* bonds before activation, BFT decides several heights, then every bond is unbonded (the `staking_tx_unbond` helper at `:1914`) and BFT decides past the point where stakes are empty.
   - *Asserts:* the node keeps deciding with the carried roster, and after a restart `restore` rebuilds the same roster for each height.
   - *Why:* it pins the legitimate zero-stake path, so step 1 cannot over-correct.
2. **Restart integration test (`zebrad/tests/integration/database.rs`).**
   - *Drives:* run a node to a few decisions, stop it, delete the `aggregated_stakes_by_hash` row of a decided snapshot below the finalized tip with a raw rocksdb write (as `stake_fixup.rs:245` does for its repair batch), then restart.
   - *Asserts:* the process exits non-zero, the log names the `fixup-db-stake` repair, and tenderlink does not start. Then run the repair, restart again, and assert the node resumes with the roster it had before.
3. **State integration test (zebra-state).**
   - *Drives:* commit enough blocks on two branches that `handle_commit` finalizes and drops a side chain.
   - *Asserts:* right after `handle_commit` returns, `read_state.known_block` and the writer agree for a block on the dropped branch.
   - *Why:* it pins step 2's invariant.
4. **One unit test, for a pure leaf.** `fat_pointer_to_block_at_height` at 0, 1, `len`, `len + 1` and `u64::MAX`.
5. **Build checks.** The existing crosslink scenario suite must still pass after step 3, and clippy with the new denies must be clean.

### 5. Rollout

- **No consensus rule changes.** Steps 1 to 3 change how a node reacts to its own damaged state and how it spells conversions.
- **The σ bound is network-parameter validation.** It rejects only configurations that would already crash (σ = 0) or misbehave (σ ≥ 2^32). Every real network uses σ = 4, which passes.
- **None of this needs to land before genesis.** It is best landed before launch so the testnet's operators get the loud failure.
- **No `s1_dev`/ClT0 backport is needed.** `s1_dev` already stops on a missing row (`lib.rs:807-822`), and its casts are harmless.
- **Separate observation for `s1_dev`, not validated for ClT0:** the same `s1_dev` code also panics if stake legitimately drains to zero.

## Validation Information

**Verdict: PARTIALLY CONFIRMED. Severity: Low.**

| Claim | Verified at |
|-|-|
| Rule "All `as` casts must have a comment explaining why the cast is safe" | `zebra-crosslink/AGENTS.md:203` |
| Rule "Don't turn invariant violations into misleading `None`/default values" | `zebra-crosslink/AGENTS.md:198` |
| `AGENTS.md` is new on `dev` | Absent at `64046aeb`; added by `2060039d` (2026-08-13) |
| Cast at 411, `(pow_block_height.0 as u64)`, uncommented | `bft.rs:411`; widening, so safe |
| Cast at 298, `at_height as usize`, uncommented | `bft.rs:298` (also `:301`, `:304`); safe by caller |
| Cast at 1091, `new_block.height as usize`, uncommented | `bft.rs:1091`; safe, and equal to `chain.blocks.len()` by `:899-903` and `:1084` |
| All three were carried from `s1_dev` | `64046aeb:zebra-crosslink/zebra-crosslink/src/lib.rs:349`, `:1038`, `:742` |
| 39 casts in total, none with a safety comment | grep of `bft.rs` and `fin.rs` for `as <primitive>`; each read in context |
| `.unwrap_or_default()` on aggregated stakes | `bft.rs:1143`; also `bft.rs:1288`, which the review did not mention |
| `None` and `Some(vec![])` are distinct, and both lookups preserve that until the default | `chain.rs:672-677`, `non_finalized_state.rs:815-820`, `delegation.rs:194-198` |
| Every held block has a record | `chain.rs:2228-2237`, `:511-513`, `:2337`; `block.rs:699-701`; `delegation.rs:399-462` |
| Empty stakes keep the previous roster | `bft.rs:1151-1176`, `:1289-1296` |
| `s1_dev` panicked on this condition instead | `64046aeb:.../zebra-crosslink/src/lib.rs:807-822`; `64046aeb:.../service/write.rs:254-260` |
| Read view can hold chains the writer dropped | `write.rs:414-437`; `non_finalized_state.rs:358-365`; `decide` reads through `read_state` (`bft.rs:1084`, `:1089`), `finish_decision` through `block_writer` (`:1139-1142`) |

**Severity justification: Low.**

- **Why not Medium.**
  - None of the 39 casts can lose information on any reachable input.
  - The stake conflation needs local state damage (WAL-tail loss) or a narrow, inferred view-lag path that also needs a long BFT stall.
  - The consequence is contained to the affected node's BFT participation. It does not produce an invalid chain or move funds.
  - A warning is logged, even if it is worded too softly.
- **Why not dismiss it.** Low is the floor of the scale, so the question is whether to drop the finding. It should stay:
  - The stake default turns a detectable corruption, which `s1_dev` stopped on, into a quiet node-local roster divergence, on code that feeds BFT voting.
  - This exact module has a documented history of a misleading default causing nondeterministic roster errors (`64046aeb:.../lib.rs:1328-1331`).
  - The fixes are cheap.

**Corrections made during validation:**

1. **"The new BFT code" is only partly right.** Both files are new on `dev`, but all three casts the review cites, and 28 of the 39 in total, were moved from `s1_dev`'s `lib.rs`. `AGENTS.md` arrived on `dev` after that code was written.
2. **"Silently keeps a ghost roster" overstates it.** `bft.rs:1166-1175` logs a warning. The problem is that the warning calls the situation expected, and that a missing record is treated as zero at all.
3. **The review missed the same conflation in `restore` (`bft.rs:1288`)** and the stale watermark next to it (`:1281-1283`).
4. **The review did not establish what a missing record means.** It is never zero stake. It is a damaged cache, a code bug, or the view-lag race described above.
5. **The review listed three casts; there are 39.** It also missed two null-pointer defaults (`:575`, `:577-580`) that fall under the same rule.

**Cross-references:**

- `restart-exits-when-the-decided-snapshot-is-still-only-in-the-non-finalized-state.md` (finding 1): `restore`'s `None` branch in step 1 must be designed together with that fix.
- `dropped-decided-snapshot-leaves-validate-indeterminate-forever-and-decide-can-abort.md` (finding 6): owns the case where a decided snapshot really is dropped. Step 2 routes the view-lag path there.
- `template-fat-pointer-is-chosen-against-the-live-tip-not-the-template-parent.md` (finding 8): the `:577-580` null fallback should be fixed as part of it.
- `template-fat-pointer-walk-scans-the-whole-bft-chain-under-the-read-lock.md` (finding 9): same function as the `:569` and `:575` changes; land them together.
- `restart-exits-when-killed-between-activation-and-the-bft-genesis-decision.md` (finding 4): the logged-only `write_bft_decision` failure at `:1195-1202`.
- `bootstrap-genesis-headers-can-still-be-reorged-so-late-joiners-build-a-different-genesis.md` (finding 5): `build_bootstrap_genesis`, where the `:1372-1373` σ casts live.
