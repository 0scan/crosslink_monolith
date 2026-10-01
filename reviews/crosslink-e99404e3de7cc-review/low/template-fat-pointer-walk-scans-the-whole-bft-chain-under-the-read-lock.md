# `fat_pointer_for_template` holds the `BFT_CHAIN` read lock while it pays two `NonFinalizedState` clones and several RocksDB reads per BFT block from the BFT tip down to the first block it may cite, so when this node's best chain and the BFT decisions disagree every block template costs work proportional to the decisions made since the fork, and `decide()` on the sync thread waits that long for its write lock

**Severity**: Low
**Validation Status**: Partially confirmed
**Location**: `zebra-crosslink/zebra-state/src/new_network/bft.rs:544-584` (`fat_pointer_for_template`; loop at `:553-572`); `zebra-crosslink/zebra-state/src/service.rs:1116-1126` (the handler takes `bft_chain().read()` for the whole call), `:546-549` (`known_block`, clones the non-finalized state), `:562-603` (`is_ancestor_of`), `:1696` (read requests run on `spawn_blocking`); `zebra-crosslink/zebra-state/src/service/watch_receiver.rs:77-115` (`with_watch_data` and `cloned_watch_data` both clone); `zebra-crosslink/zebra-state/src/new_network/bft.rs:82` (`BFT_CHAIN` is a `std::sync::RwLock`), `:1083` (`decide` takes the write lock), `:1145` (`finish_decision` takes it again), `:1037-1052` (Linearity allows equal snapshots); `zebra-crosslink/zebra-state/src/service/write.rs:210-262` (`crosslink_conflict_hold`); callers `zebra-crosslink/zebra-rpc/src/methods.rs:2189-2201`, `:3623-3635`, `:3717-3727`
**Found by agent:** /code-review high (Claude Fable 5.1), 2026-09-29; validated 2026-09-29 at dev e99404e3de7cc
**In scope of audit?** Yes. It concerns lock contention between the RPC path and the sync thread, which decides BFT blocks and commits PoW blocks. The per-block state lookups inside the loop were introduced on `dev` by `ae9bc5e6` (2026-09-18) and moved to `zebra-state` by `379170fe`. `s1_dev` (`64046aeb`) scans its BFT list in memory, comparing only `do_not_include_until_bc_height`, under the TFL service's own mutex (`zebra-crosslink/zebra-crosslink/src/lib.rs:1895-1909`), so **ClT0 is not affected**.

## Description

`fat_pointer_for_template` walks the decided BFT chain from its tip downward and returns the first block a template may cite. For each block that passes the cheap in-memory tests, it asks the state two questions: `known_block(snapshot)`, and `is_ancestor_of(snapshot, parent)`. The whole walk runs while the caller holds `bft_chain().read()` (`service.rs:1117`).

The original finding is right about three things:

- the work per block is expensive;
- it happens under the `BFT_CHAIN` read lock;
- `decide()` needs the write lock on the sync thread.

It is wrong about how far the walk goes, about who calls it, and about what the proposed early stop would save:

- **The walk does not scan the whole chain in the scenario given.** The loop already `break`s at the first qualifying block from the top. The parent's own pointer qualifies whenever the proposed height is the parent's height plus one and ancestry is judged against that parent. The display RPC's `u64::MAX` query also satisfies this. So the loop already stops at or above the parent pointer's index `p`, and costs at most `len - p` iterations. That is exactly the bound the review proposes. **The proposed early stop saves nothing when the inputs are consistent.**
- **A walk of the whole chain needs the parent's pointer to be null or unresolvable**, which with 9.5k decided blocks does not happen on a synced node. It can also happen through the race in finding 8, where the height and the parent disagree.
- **No GUI frame calls this function.** The in-tree callers are the two `getblocktemplate` sites and the RPC `get_tfl_fat_pointer_to_bft_chain_tip`. `zebra-crosslink/src/viz2.rs` takes `BFT_CHAIN` read locks for its own scans (`:540`, `:653`, `:963`), and `zebra-gui` and `wallet` do not call that RPC.
- **The per-iteration cost is higher than the review said.** Both `known_block` and `is_ancestor_of` clone the whole `NonFinalizedState` on every call.

What remains is real but bounded. **`len - p` is the number of BFT blocks this node has decided since the last one whose snapshot is on the template's chain.** It stays small in normal operation. It grows without a fixed bound while this node's best chain disagrees with the BFT decisions: the `crosslink_conflict_hold` state, and after it has given up. A low-hash-power adversary on a low-difficulty testnet can create that state.

## Attack Scenario and Steps

1. An adversary mines a branch `Y` that is heavier than the branch `X` holding the latest decided snapshot, and a victim node adopts `Y` as its best chain. The hold at `write.rs:210-262` only stops finalization commits; `Y` keeps growing in the non-finalized state.
2. BFT keeps deciding. This node validates each proposal by `known_block` and Linearity (`bft.rs:1014-1052`), not against its best chain, so it keeps deciding blocks whose snapshots lie on `X` and appending them. Linearity allows a snapshot equal to the parent's (`bft.rs:1037`), so BFT heights can advance faster than PoW heights. The rate depends on tenderlink round timing (round-0 propose timeout 2 s, `tenderlink/src/lib.rs:454`). This rate is an inference; it was not measured.
3. Every block on `Y` must carry a pointer whose snapshot is on `Y`, so `Y`'s tip pointer stays at the last pre-fork BFT block `p`.
4. Each `getblocktemplate` on the victim (internal miner on every tip change, external miners on every poll) walks from the BFT tip down to `p`. For each of the `len - p` blocks it pays one `NonFinalizedState` clone and at least one RocksDB read for `known_block`, and one clone plus one to three RocksDB reads for `is_ancestor_of`. It holds the `BFT_CHAIN` read lock throughout.
5. If a decision arrives during a walk, `decide()` blocks at `bft.rs:1083` until the walk ends. `decide()` runs on the sync thread, which also commits PoW blocks and answers tenderlink.

**Attack Requirements and Assumptions:**

- Enough hash power to make a victim prefer a branch without the decided snapshot. That is cheap on a low-difficulty public testnet.
- The attacker cannot call the RPC on a victim unless its RPC port is exposed. The cost is paid at the rate the victim's own miners poll.
- The gap grows while BFT keeps deciding and this node can still validate. Finding 6 argues that validation stops once the decided snapshot is dropped, which would freeze the gap from then on.
- A second, benign trigger: a node whose PoW tip is behind its BFT tip (catching up, with `check_synced_to_tip` skipped on test networks, `get_block_template.rs:878-880`). This skips the top blocks cheaply: one clone, the chain lookups and one DB miss each, no ancestry query.

## Impact on Users

- **Miners and pool operators on the affected node**: each template in the conflict state costs `O(len - p)` state lookups instead of a handful. The absolute cost was not measured. At a few microseconds per cached RocksDB point read plus two small allocations per iteration, a gap of 1,000 blocks is probably on the order of milliseconds, and a gap of 9,500 tens of milliseconds. This is an inference from the operation count, not a measurement.
- **Finalizers on the affected node**: a decision that lands during a walk is delayed by the rest of that walk, and so is every PoW commit queued behind it on the sync thread. The delay is bounded by one walk per concurrent request. The sync thread is delayed, not starved: each request takes the lock once and releases it.
- **Everyone else**: no correctness impact, no divergence, nothing persisted.

## Technical Details / Code Analysis

**The lock is held across the whole selection** (`service.rs:1116-1126`):

```rust
ReadRequest::CrosslinkFatPointerToBftChainTip(proposed_pow_height) => {
    let chain = crate::new_network::bft::bft_chain().read().unwrap();
    Ok(ReadResponse::CrosslinkFatPointerToBftChainTip(
        crate::new_network::bft::fat_pointer_for_template(
            &chain,
            &state.network.crosslink_parameters(),
            &state,
            proposed_pow_height,
        ),
    ))
}
```

**The loop stops at the first qualifying block** (`bft.rs:553-572`):

```rust
for (i, b) in chain.blocks.iter().enumerate().rev() {
    if b.headers.is_empty() || b.do_not_include_until_bc_height > proposed_pow_height {
        continue;
    }
    let snapshot_hash = Hash(b.snapshot_block_hash().0);
    let Some(known) = read_state.known_block(snapshot_hash) else { continue; };
    if known.height.0 as u64 + sigma + 1 > proposed_pow_height {
        continue;
    }
    let on_the_template_chain = match parent_hash {
        Some(parent_hash) => read_state.is_ancestor_of(snapshot_hash, parent_hash) == Some(true),
        None => true,
    };
    if on_the_template_chain {
        suitable_height = Some(i as u64 + 1);
        break;
    }
}
```

**Each state question clones the non-finalized state.**

- `known_block` (`service.rs:546-549`) goes through `latest_non_finalized_state()`, which is `cloned_watch_data()` (`service.rs:755-757`).
- `is_ancestor_of` (`service.rs:562-603`) goes through `with_watch_data`, which also clones before running the closure (`watch_receiver.rs:77-88`).
- The clone rebuilds `chain_set: BTreeSet<Arc<Chain>>` and clones `network` and the hardfork `Arc` (`non_finalized_state.rs:51-92`).
- The lookups themselves are one `HashMap` probe per chain (`chain.rs:680-682`), plus `db.height` / `db.contains_hash` RocksDB reads.

```rust
pub fn known_block(&self, hash: block::Hash) -> Option<KnownBlock> {
    read::find::non_finalized_state_contains_block_hash(&self.latest_non_finalized_state(), hash)
        .or_else(|| read::find::finalized_state_contains_block_hash(&self.db, hash))
}
```

**The claim that "the parent pointer always qualifies" was verified, with its precondition.** Every non-genesis block is committed only after `admit_fat_pointer` returns `Accept` (`new_network.rs:3008`, `:3037`, `:3064`; there is no other commit path apart from genesis at `write.rs:203-208`). Accept for a parent `P` at height `h` citing BFT block index `p` established four things.

| Test in the loop | Established at admission of `P` | Still true for a template at `h + 1` on `P`? |
| :- | :- | :- |
| `headers` non-empty | `bft.rs:419-425` (placeholder defers) | Yes: blocks are append-only, real blocks never overwritten (`bft.rs:1113-1117`) |
| `do_not_include_until_bc_height <= proposed` | `<= h` (`bft.rs:411`) | Yes: `h <= h + 1` |
| snapshot known, `snapshot + sigma + 1 <= proposed` | `h - snapshot >= sigma + 1` (`bft.rs:466-471`) | Yes: stricter at `h` than at `h + 1` |
| snapshot is an ancestor of the parent | ancestor of `P`'s parent (`bft.rs:482`) | Yes: ancestry is permanent, and `P` is kept while the template is built on it |

The claim holds exactly when `proposed_pow_height == height(parent) + 1` and ancestry is judged against that parent.

- It also holds on the display path, where `proposed_pow_height` is `u64::MAX` and the parent is the best tip.
- It fails under finding 8's race, where the height comes from `chain_info` but the parent is the live tip.
- It fails when a committed block's pointer is missing from `hash_to_height`. That can only happen if `write_bft_decision` failed or did not land before a crash (`bft.rs:1195-1202` only logs the error).

**Why going below `p` is never useful.** Every BFT block below `p` ranks below the parent's pointer. For a block extending that parent, the ordering rule rejects such a pointer permanently (`bft.rs:442-446`). So the only effect of today's loop continuing past `p` (which it does only under finding 8's race) is to pick a pointer that cannot be admitted. That makes the bound a correctness guard, not a speedup.

**Who else takes the lock.**

- The sync thread is the only writer: `decide` (`bft.rs:1083`), `finish_decision` (`:1145`), restore (`:1333`), activation (`:1468`) and peer strings (`:1533`).
- Readers include admission on the sync thread (`new_network.rs:3008`), `write.rs:221` and `:278`, `non_finalized_state.rs:300`, and the visualizer (`viz2.rs:540`, `:653`, `:963`).
- The visualizer's read at `:653` clones the whole `blocks` vector, and its fallback at `:963` computes `blake3_hash()` of every BFT block. Those are heavier than this walk. They are out of this finding's scope, but relevant when judging GUI contention.
- Whether new readers queue behind a waiting writer depends on the platform's `RwLock` implementation. This was not verified here.

**Logging would not reveal a slow walk.** The per-request `CodeTimer` (`service.rs:1050`) only logs at 5 and 9 minutes (`zebra-chain/src/diagnostic.rs:21-26`).

## Recommendations

1. **Land this with finding 8's fix and bound the loop at the parent's pointer.** Once the request carries the parent and the height is derived from it, the parent's pointer qualifies by construction. Scanning `(floor..len).rev()` then yields the same result as today, and makes it impossible to return a regressing pointer. Continuing the signature from finding 8's plan:

   ```rust
   fn select_fat_pointer(
       chain: &BftChain,
       params: &ZcashCrosslinkParameters,
       read_state: &ReadState,
       parent_hash: Hash,
       parent_fp: &FatPointerToBftBlock,
       proposed_pow_height: u64,
   ) -> Result<FatPointerToBftBlock, BoxError> {
       let floor = if *parent_fp == FatPointerToBftBlock::null() {
           0
       } else {
           match chain.hash_to_height.get(&parent_fp.points_at_block_hash()) {
               // `hash_to_height` holds indices into `blocks`, which fit in usize.
               Some(&h) => h as usize,
               None => return Err(format!("the BFT block cited by template parent {parent_hash} is not in this node's BFT chain").into()),
           }
       };
       let sigma = params.bc_confirmation_depth_sigma;
       for i in (floor..chain.blocks.len()).rev() {
           let b = &chain.blocks[i];
           if b.headers.is_empty() || b.do_not_include_until_bc_height > proposed_pow_height {
               continue;
           }
           let snapshot_hash = Hash(b.snapshot_block_hash().0);
           let Some(known) = read_state.known_block(snapshot_hash) else { continue; };
           // Block heights are u32, so widening to u64 cannot overflow.
           if known.height.0 as u64 + sigma + 1 > proposed_pow_height {
               continue;
           }
           if read_state.is_ancestor_of(snapshot_hash, parent_hash) != Some(true) {
               continue;
           }
           // `i` is an index into `blocks`, so `i + 1` fits in u64.
           let at_height = i as u64 + 1;
           return Ok(fat_pointer_to_block_at_height(&chain.blocks, &chain.fat_pointer_to_tip, at_height)
               .expect("at_height is one more than an index into chain.blocks"));
       }
       if floor == 0 && *parent_fp == FatPointerToBftBlock::null() {
           return Ok(FatPointerToBftBlock::null());
       }
       Err(format!("the pointer of template parent {parent_hash} no longer qualifies; the parent was likely dropped").into())
   }
   ```

   - An unresolvable parent pointer is an error, not a scan from zero. Admission would only defer the child (`bft.rs:427-434`), so a template built on it cannot be committed until the BFT block arrives.
   - Only a parent dropped mid-scan can make the floor fail. A template on a dropped parent is pointless, and an error says so, where a default pointer would hide it (AGENTS.md: "Don't turn invariant violations into misleading `None`/default values").
   - The `.unwrap_or_else(FatPointerToBftBlock::null)` at `bft.rs:575` becomes an `expect`, for the same reason.

2. **Answer the whole selection from one non-finalized snapshot.**
   - Take `read_state.latest_non_finalized_state()` once, needing a `pub(crate)` accessor.
   - Answer `known_block` with the existing `read::find::non_finalized_state_contains_block_hash` / `finalized_state_contains_block_hash`, which already take `&NonFinalizedState`.
   - Move the body of `ReadStateService::is_ancestor_of` into a free function over `(&NonFinalizedState, &ZebraDb)` that both the method and the selection call.
   - This removes two clones per iteration, and gives every answer in one selection the same view of the chains.

3. **Measure before doing more.** Add a `tracing` span with the iteration count around the scan, and drive it with the conflict scenario below. Only if the measured hold time matters, shorten the lock hold:
   - copy `(index, snapshot hash, do_not_include_until_bc_height)` for `floor..len` under the lock, evaluate outside it, and re-lock to read the chosen pointer;
   - `blocks` is append-only, so an index stays valid across the gap;
   - `viz2.rs:533-551` already uses this "lock for the scan, look up outside" pattern.

4. **Considered alternatives, not chosen:**
   - *The review's bound alone, without finding 8's fix.* It is unsound while the proposed height can disagree with the parent: the parent's pointer then need not qualify. With finding 8 fixed, it costs exactly what today's loop costs.
   - *Binary search over `floor..len`.* Linearity (`bft.rs:1037-1052`), the monotone `do_not_include_until_bc_height` (`:975`), and non-decreasing snapshot heights make "qualifies" a prefix property, so `O(log(len - p))` lookups would do. Not chosen now: its correctness silently depends on Linearity holding for every stored block, including those loaded by `restore` (whether restore re-validates Linearity was not verified). A violation would turn a slow answer into a wrong pointer and a permanent `Reject`. Revisit if step 3 measures gaps in the thousands.
   - *Caching the last answer by `(parent_hash, blocks.len())`.* Effective for repeated polls, but `getblocktemplate` asks once per template, so the hit rate is low.

5. **Tests** (end-to-end, `zebrad/tests/crosslink.rs`, test-format harness):
   - *Conflict with many decisions.* Start from `crosslink_reject_pow_block_citing_a_snapshot_off_its_own_chain`, keeping branch A best. `LOAD_POS` sixty-four more BFT blocks whose snapshots are on branch B, then `MINE_FROM_TEMPLATE` three times.
     - *Assert:* each mined block is accepted (`EXPECT_POW_CHAIN_LENGTH` grows by three) and `EXPECT_POS_CHAIN_LENGTH` is unchanged. A template that picked below the floor, or a stale fallback, would be rejected and the length would not grow.
   - *Null parent pointer.* `MINE_FROM_TEMPLATE` on the first block after bootstrap activation. Assert it is accepted with a non-null pointer when a qualifying BFT block exists, and with a null pointer otherwise.
   - Timing is measured with the span from step 3, not asserted in a test.

6. **Rollout.** No consensus or validity rule changes; only which valid pointer an honest template picks, and how fast. It can land before or after the new testnet's genesis. No `s1_dev` backport is needed.

## Validation Information

**Verdict: PARTIALLY CONFIRMED. Severity: Low.**

| Claim | Verified at |
| :- | :- |
| Per-block `known_block` and `is_ancestor_of` inside the loop | `bft.rs:558`, `:562-568` |
| Walk holds the `BFT_CHAIN` read lock | `service.rs:1117` |
| `decide()` takes the write lock on the sync thread | `bft.rs:1083`; `finish_decision` again at `:1145` |
| `is_ancestor_of` "iterates every chain plus DB reads" | `service.rs:562-603`: one hash probe per chain, one to three RocksDB reads; plus a full `NonFinalizedState` clone (`watch_receiver.rs:77-88`), which the review missed |
| Walks "the entire BFT chain" in the conflict case | **Not reproduced**: the loop breaks at the first qualifying block, which is at or above the parent pointer's index (`bft.rs:569-570`, argument above) |
| "Every GUI frame" calls it | **Not reproduced**: callers are `methods.rs:2193`, `:3627`, `:3721` only; `viz2.rs` reads `BFT_CHAIN` directly for other work |
| The parent pointer always qualifies | True when the height is the parent's plus one and ancestry is judged against that parent (admission table above); false under finding 8's race |
| Bounding at `hash_to_height[parent_fp]` gives the same result in `O(gap)` | Same result, yes; but today's loop is already `O(gap)` whenever the claim above holds |

**Severity justification.**

- *Why not Medium:*
  - There is no correctness impact.
  - The cost is bounded by the number of BFT decisions since the fork, and is paid only by template and display requests, only in the conflict or catch-up states.
  - It delays `decide()` by at most the remaining time of one walk per concurrent request; it cannot starve it.
  - The heavier `BFT_CHAIN` readers are the visualizer's, not this one.
- *Why not lower:* Low is the floor. The contention is real on the thread that commits blocks and decides BFT, and the gap that drives it is adversary-influenced on a low-difficulty testnet (AGENTS.md: "Bound all loops/allocations over attacker-controlled data").

**Corrections made during validation.**

1. "Walks all 9.5k+ BFT blocks back to the fallback": the fallback is reached only when nothing qualifies, which needs a null or unresolvable parent pointer (or finding 8's race). In the conflict case the walk stops at the parent pointer's index, and its length is the number of decisions since the fork.
2. "Every GUI frame": no GUI code calls this. The visualizer's own `BFT_CHAIN` reads are separate and heavier.
3. "It could stop at the parent pointer's index ... gives the same result in O(gap)": today's loop already stops there when the inputs are consistent, so the bound saves nothing. Its value is as a guard against returning a pointer the ordering rule rejects, and it is only sound after finding 8's fix.
4. The per-iteration cost includes two full `NonFinalizedState` clones, not only chain iteration and DB reads.

**Cross-references.**

- `template-fat-pointer-is-chosen-against-the-live-tip-not-the-template-parent.md` (finding 8): the same function, and the precondition for this plan's bound. Implement both as one change: parent in the request, height derived from the parent, loop bounded at the parent pointer's index.
- `dropped-decided-snapshot-leaves-validate-indeterminate-forever-and-decide-can-abort.md` (finding 6): the conflict and abandonment state that makes the gap large; finding 6 also bounds how long it can keep growing.
- `admit-fat-pointer-accepts-a-certificate-by-hash-without-checking-signatures.md` (finding 7): with the bound, the selection always returns the node's stored pointer for the chosen index, never the parent header's bytes, except for the null-parent case.
- `missing-pow-blocks-grows-without-bound-from-unverified-proposal-snapshots.md` (finding 3): another AGENTS.md "bound all loops" case on the same BFT path.
