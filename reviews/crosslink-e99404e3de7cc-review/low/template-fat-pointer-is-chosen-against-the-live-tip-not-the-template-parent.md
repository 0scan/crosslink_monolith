# `fat_pointer_for_template` judges the block template's BFT pointer against `read_state.best_tip()` at request time instead of against the template's own parent from `chain_info`, so a reorg deeper than sigma between the two reads can hand the miner a pointer that admission rejects permanently

**Severity**: Low
**Validation Status**: Partially confirmed
**Location**: `zebra-crosslink/zebra-state/src/new_network/bft.rs:544-584` (`fat_pointer_for_template`; the tip read at `:551`, the ancestry test at `:562-568`, the fallback at `:577-583`); `zebra-crosslink/zebra-state/src/service.rs:1116-1126` (the `ReadRequest::CrosslinkFatPointerToBftChainTip` handler); `zebra-crosslink/zebra-state/src/request.rs:1533` (the request carries only a height); `zebra-crosslink/zebra-rpc/src/methods.rs:3448` and `:3717-3727` (main `getblocktemplate` path: `chain_info` read, then the pointer request), `:3596` and `:3613-3635` (long-poll tip-change path, same pattern); `zebra-crosslink/zebra-rpc/src/methods.rs:3811-3878` (`submit_block`, no tip check); `zebra-crosslink/zebra-rpc/src/methods/types/get_block_template.rs:878-880` (test networks skip `check_synced_to_tip`); `zebra-crosslink/zebra-state/src/new_network.rs:3034` (a `Reject` is dropped as permanent); the rules the stale pointer then fails, `zebra-crosslink/zebra-state/src/new_network/bft.rs:442-446` (ordering), `:466-471` (sigma depth), `:482-486` (Last Final Snapshot); `zebra-crosslink/zebrad/src/components/miner.rs:406-441`, `:611-633`, `:667-669` (internal miner: long poll, solver cancellation, ignored `submit_old`)
**Found by agent:** /code-review high (Claude Fable 5.1), 2026-09-29; validated 2026-09-29 at dev e99404e3de7cc
**In scope of audit?** Yes. It is template construction for the hybrid chain and the race between two state reads. The tip-dependent selection was introduced on `dev` by `ae9bc5e6` ("Crosslink: Enforce Last Final Snapshot, Linearity and Tail Confirmation", 2026-09-18) and moved into `zebra-state` by `379170fe`. `s1_dev` (`64046aeb`) selects the template pointer in `zebra-crosslink/zebra-crosslink/src/lib.rs:1895-1910` by `do_not_include_until_bc_height` alone, with no tip read and no Last Final Snapshot rule in admission, so **ClT0 is not affected**.

## Description

`getblocktemplate` reads the chain tip once, into `chain_info`, and builds the whole template on `chain_info.tip_hash` at height `chain_info.tip_height + 1`. It then asks the state for the BFT fat pointer with a separate request, `ReadRequest::CrosslinkFatPointerToBftChainTip(height)`. That request carries only the proposed height. `fat_pointer_for_template` fills in the parent itself, from whatever the best tip is when it runs (`bft.rs:551`):

```rust
let parent_hash = read_state.best_tip().map(|(_, hash)| hash);
```

Every suitability test that depends on the chain (the Last Final Snapshot ancestry test at `:562-568`, and the "parent's own context" fallback at `:577-583`) is therefore evaluated against the live tip, while the depth and `do_not_include_until_bc_height` tests use the height derived from `chain_info`. When the tip moves between the two reads, the pointer is chosen for one chain and placed in a block that extends another.

The original finding is right that the race exists, that nothing downstream catches it, and that passing the parent into the request removes it. It overstates when the race matters. Walking the admission rules through the three ways a tip can move shows:

| Tip movement between the two reads | Pointer still valid for the template's parent? |
| :- | :- |
| Forward along the same chain, one or more blocks | **Yes**, whenever the template parent carries a non-null pointer |
| Reorg whose abandoned branch is at most sigma blocks deep | **Yes** |
| Reorg whose abandoned branch is deeper than sigma, across a BFT decision | **No**: permanent `Reject` |
| Multi-block forward advance while the template parent's pointer is still null (first blocks after activation) | **Sometimes no**: the fallback can hand over a too-new pointer |

So the waste is real, but confined to deep reorgs across a BFT decision and a narrow post-activation window. It is not "any reorg to a sibling branch".

## Attack Scenario and Steps

This is mainly a non-adversarial race. The trigger is an ordinary event: the best tip changes during the few milliseconds between the `ChainInfo` read and the fat-pointer read of one `getblocktemplate` call. An adversary with enough hash power on a low-difficulty testnet can make the one harmful case, a deep reorg, happen at will. They cannot choose when a victim's RPC call falls inside the window, though.

1. A miner (the internal miner or an external one) calls `getblocktemplate`. The loop reads `chain_info` with tip `A` at height `n` (`methods.rs:3448`). On the long-poll tip-change path the read is at `:3596`.
2. Before the pointer request is served (`methods.rs:3717-3727`, or `:3623-3635`), the node's best chain switches from branch `Y` (holding `A`) to branch `X`, forking at `F` (height `f`), with `n - f > sigma`. This node's BFT chain holds a decided block whose snapshot `S` lies on `X` above `F`. That is the usual reason a node's best chain jumps that far: it had been on a minority branch while BFT finalized the other one.
3. `fat_pointer_for_template(n + 1)` scans from the BFT tip. The block citing `S` passes every test (`S` is known, `S + sigma + 1 <= n + 1`, `S` is an ancestor of the new tip on `X`), so it is chosen.
4. The template (parent `A`, height `n + 1`, pointer to the `S` block) is returned. On a test network `check_synced_to_tip` is skipped (`get_block_template.rs:878-880`), so nothing refuses to serve it.
5. The miner solves it. The internal miner's next long-poll call returns a fresh template almost at once, because the tip hash differs from the long-poll id. The solver is only cancelled at its next `cancel_fn` check (`miner.rs:611-633`), however, and at low difficulty a solve can land first. The miner ignores `submit_old = false` (`miner.rs:667-669`, a standing TODO) and submits.
6. `submit_block` has no tip or staleness check (`methods.rs:3845-3849`). The block reaches `admit_fat_pointer` with `parent_hash = A`. `is_ancestor_of(S, A)` is `Some(false)`, so Last Final Snapshot returns `Some(CrosslinkVerdict::Reject)` (`bft.rs:482-486`). `new_network` drops it as permanent (`new_network.rs:3034`).

The mirror case goes the other way: the node reorgs from a branch holding a decided snapshot to a heavier branch without it. There the chosen pointer ranks below `A`'s own pointer, and the ordering rule (`bft.rs:445`) rejects instead.

**Attack Requirements and Assumptions:**

- A reorg deeper than sigma blocks below the template parent, across a BFT decision, must land in the window between two consecutive state reads of one RPC call. That window is roughly one mempool round trip on the main path and one `spawn_blocking` round trip on the long-poll path. This is an inference from the code; the window was not measured.
- The block built from the stale template sits on the branch the node just abandoned. Without this bug it would still be a stale side-chain block. The bug changes its fate from "orphaned, could in principle be reorged back in" to "permanently rejected".
- The post-activation case needs the template parent's pointer to be null and the tip to advance by at least two blocks inside the window.

## Impact on Users

- **Miners**: can lose a block that was already built on a parent the node had just abandoned. The loss is one stale-template solve per occurrence. The next template is correct.
- **Node operators**: see an occasional "fat pointer regressed or is too early" rejection of their own submitted block after a deep reorg. This is noise that looks like a consensus bug.
- **Finalizers, wallet users, light clients**: unaffected. The rejected block never enters any chain, and admission's verdict on it is correct for the pointer it carries. No node diverges.

## Technical Details / Code Analysis

**The request carries a height and no parent** (`request.rs:1533`), and the handler passes only that (`service.rs:1116-1126`):

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

**The template's parent and height come from an earlier read** (`methods.rs:3442-3448`, then `:3715-3727`):

```rust
let chain_info @ zebra_state::GetBlockTemplateChainInfo {
    tip_hash,
    tip_height,
    max_time,
    cur_time,
    ..
} = fetch_chain_info(read_state.clone()).await?;
```

```rust
let height = chain_info.tip_height.next().map_misc_error()?;

let fat_pointer = {
    let ret = self
        .read_state
        .clone()
        .oneshot(ReadRequest::CrosslinkFatPointerToBftChainTip(height.0 as u64))
        .await;
    match ret {
        Ok(ReadResponse::CrosslinkFatPointerToBftChainTip(fp)) => fp,
        _ => zcash_primitives::bft::FatPointerToBftBlock::null(),
    }
};
```

Between the two sits at least the `fetch_mempool_transactions` await (`:3460-3462`). On the long-poll path (`:3596` to `:3623`) only the request's own scheduling separates them.

**The selection reads its own parent** (`bft.rs:550-583`):

```rust
let sigma = params.bc_confirmation_depth_sigma;
let parent_hash = read_state.best_tip().map(|(_, hash)| hash);
let mut suitable_height = None;
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
if let Some(h) = suitable_height {
    return fat_pointer_to_block_at_height(&chain.blocks, &chain.fat_pointer_to_tip, h)
        .unwrap_or_else(FatPointerToBftBlock::null);
}
parent_hash
    .and_then(|parent_hash| read_state.any_chain_block_header(parent_hash.into()))
    .map(|hdr| hdr.fat_pointer_to_bft_block.clone())
    .unwrap_or_else(FatPointerToBftBlock::null)
```

**Why a forward advance is harmless.** This was verified by walking the rules. Let the template parent be `A` at height `n` and the live tip `T` descend from `A`. Every candidate the loop accepts has snapshot height at most `n - sigma` and is an ancestor of `T`. Any ancestor of `T` at height `n` or below is an ancestor of `A`, so Last Final Snapshot (`bft.rs:482`) holds for `A`. `A`'s own pointer passed admission against `A`'s parent (depth at `:469`, ancestry at `:482`, `do_not_include_until_bc_height` at `:411`), so it also qualifies here against `T` at height `n + 1`. The loop stops at the first qualifying block from the top, so it returns a block ranked at or above `A`'s pointer, and the ordering rule at `:445` holds. The fallback is reached only if nothing qualifies, which needs `A`'s pointer to be null.

**Why a reorg of depth at most sigma is harmless.** With fork point `F` at height `f >= n - sigma`, every candidate snapshot (height at most `n - sigma`) is at or below `f`. It is therefore a common ancestor of both branches. `A`'s pointer's snapshot is at most `n - 1 - sigma`, so it is common too, and it qualifies against `T`. The argument above then goes through unchanged.

**Why a deeper reorg is not.** When `f < n - sigma`, a BFT block whose snapshot lies on the new branch between `f + 1` and `n - sigma` passes all of the loop's tests against `T`, and fails Last Final Snapshot against `A`. Linearity (`bft.rs:1037-1052`) makes the BFT snapshots one bc-linear sequence. In the mirror case (the node leaves the branch holding the decided snapshot), every BFT block ranked at or above `A`'s pointer therefore has its snapshot off the new branch. The loop falls below `A`'s pointer's index, and the ordering rule rejects.

**Nothing downstream catches it.** `submit_block` hands the block straight to `submit_block_to_new_network` (`methods.rs:3845-3849`), with no comparison against the current tip. Admission's `Reject` is final: `new_network.rs:3034` maps it to a non-retryable failure.

**A related misleading default.** Both call sites turn any error from the request into `FatPointerToBftBlock::null()` (`methods.rs:3633`, `:3725`). Once the parent's pointer is non-null, a null child pointer is a certain permanent `Reject` (`bft.rs:390`). AGENTS.md: "Don't turn invariant violations into misleading `None`/default values."

## Recommendations

1. **Make the template pointer a function of the template's parent, and derive the height from that parent.** One change fixes both this finding and the precondition finding 9 depends on.
   - Replace `ReadRequest::CrosslinkFatPointerToBftChainTip(u64)` with two requests:
     - `CrosslinkFatPointerForTemplate { parent_hash: block::Hash }` for block templates;
     - `CrosslinkFatPointerToBftChainTip` for the display RPC `get_tfl_fat_pointer_to_bft_chain_tip` (`methods.rs:2189-2201`), which keeps today's "best tip, `u64::MAX`" meaning.
   - `fat_pointer_for_template` takes `parent_hash` and derives `proposed_pow_height` from the parent's own height, so the two inputs can never disagree:

   ```rust
   pub fn fat_pointer_for_template(
       chain: &BftChain,
       params: &ZcashCrosslinkParameters,
       read_state: &ReadState,
       parent_hash: Hash,
   ) -> Result<FatPointerToBftBlock, BoxError> {
       let Some(parent) = read_state.known_block(parent_hash) else {
           return Err(format!("template parent {parent_hash} is not in this node's state").into());
       };
       let Some(parent_header) = read_state.any_chain_block_header(parent_hash.into()) else {
           return Err(format!("template parent {parent_hash} has no stored header").into());
       };
       // Block heights are u32, so widening to u64 cannot overflow.
       let proposed_pow_height = parent.height.0 as u64 + 1;
       select_fat_pointer(chain, params, read_state, parent_hash, &parent_header.fat_pointer_to_bft_block, proposed_pow_height)
   }
   ```

   - `select_fat_pointer` is today's loop, with `parent_hash` passed in instead of read from `best_tip()`, and bounded below as finding 9's plan describes. The display request calls it with the best tip and `u64::MAX`.
   - The invariant this restores: every chain-dependent test in the selection is evaluated against the block the template actually extends.

2. **Use `chain_info` at both `getblocktemplate` call sites.** At `methods.rs:3623-3635` and `:3717-3727`, send `CrosslinkFatPointerForTemplate { parent_hash: chain_info.tip_hash }`. On an error, return an RPC error instead of substituting `FatPointerToBftBlock::null()`: a null pointer under a non-null parent can only produce a block that `bft.rs:390` rejects permanently. A short retry (the next long-poll iteration) is the honest response.

3. **Considered alternatives, not chosen:**
   - Re-read the tip after the pointer request, and loop until the two reads agree. This narrows the window without closing it. It also still lets the height and the parent disagree.
   - Have `submit_block` refuse blocks whose parent is no longer the best tip. That throws away legitimate side-chain blocks, and the wasted solve has already happened.
   - Have the internal miner honour `submit_old = false` (the TODO at `miner.rs:667`). Worth doing separately, but it does not help external miners, and it leaves the state request ambiguous.

4. **Tests** (end-to-end, in `zebrad/tests/crosslink.rs`, using the test-format harness):
   - *Deep reorg across a decision, then mine.* Start from the setup of `crosslink_reject_pow_block_citing_a_snapshot_off_its_own_chain`: two branches, a BFT decision whose snapshot is on branch B, sigma plus two blocks deep. Add a harness instruction, `MINE_FROM_TEMPLATE_ON(parent_hash)`, that builds a template on an explicit parent through the new request and submits it. Issue it on branch A's tip while branch B is best.
   - *Assert:* the block is accepted as a side-chain block. It is not rejected with "fat pointer regressed or is too early". `EXPECT_NODE_ALIVE` also holds.
   - *Regression guard on the normal path:* extend `crosslink_mine_from_template` to decide BFT blocks between `MINE_FROM_TEMPLATE` steps. Assert that every mined block is accepted and that the chain length grows by exactly the number mined.
   - The race itself cannot be driven deterministically through the harness. The explicit-parent instruction tests the property the fix establishes: the pointer depends only on the parent.

5. **Rollout.** This changes no consensus or validity rule: admission is untouched, and only which valid pointer an honest template picks changes. It can land at any time, before or after the new testnet's genesis, without coordination. No backport to `s1_dev` is needed, because ClT0 has neither the tip-dependent selection nor the Last Final Snapshot rule.

## Validation Information

**Verdict: PARTIALLY CONFIRMED. Severity: Low.**

| Claim | Verified at |
| :- | :- |
| Selection reads the live best tip, not the template's parent | `bft.rs:551` |
| The request carries only a height | `request.rs:1533`; `service.rs:1116-1126` |
| `chain_info` is read earlier, then the pointer is requested separately | `methods.rs:3448` then `:3717-3727`; long-poll path `:3596` then `:3623-3635` |
| No tip check at `submitblock` | `methods.rs:3811-3878` |
| A wrong pointer is a permanent reject | `bft.rs:445`, `:469`, `:482-486`; `new_network.rs:3034` |
| Long polling supersedes the stale template, but does not stop an in-flight solve from being submitted | `methods.rs:3484`; `miner.rs:406-441`, `:611-633`, `:667-669` |
| Test networks skip the synced-to-tip check | `get_block_template.rs:878-880` |
| Forward advances and reorgs of depth at most sigma yield a valid pointer | Derived by walking `bft.rs:406-486` against the loop at `:553-572` (argument in Technical Details) |
| ClT0 not affected | `git show 64046aeb:zebra-crosslink/zebra-crosslink/src/lib.rs` lines 1895-1910 select on `do_not_include_until_bc_height` only; `git grep "is_ancestor_of" 64046aeb` finds no admission ancestry rule |

**Severity justification.**

- *Why not Medium:*
  - No node diverges, no finality is lost, and nothing stalls: the next template is correct.
  - The only harmful triggers are a reorg deeper than sigma across a BFT decision, or a multi-block advance in the first blocks after activation, and either must land inside a millisecond-scale window.
  - The block lost in the reorg case was built on the branch the node had just abandoned, so it was already uncompetitive. The bug turns "orphaned" into "rejected".
  - An adversary gains nothing they did not already get by winning the reorg.
- *Why not lower:* Low is the floor of the scale. It is not a false positive: the race is real, it produces permanent rejections of honestly mined blocks, and it becomes easier on a low-difficulty public testnet, where deep reorgs are cheap and `check_synced_to_tip` is skipped.

**Corrections made during validation.**

1. The review said `chain_info` is "fetched earlier in getblocktemplate at methods.rs:3715". `:3715` is where the height is derived. `chain_info` is read at `:3448` (main path) and `:3596` (long-poll tip-change path), and both paths have the race.
2. The review said a reorg "to a sibling branch" leaves the chosen pointer off the template's chain. That holds only when the abandoned branch is deeper than sigma and a BFT decision sits on the new branch. For shallower reorgs and for forward advances, every candidate the loop can pick is a common ancestor, and the result is valid (argument above).
3. The review named only Last Final Snapshot as the failing rule. In the mirror reorg case the failing rule is the ordering rule (`bft.rs:445`). In the post-activation multi-block case it can be the sigma depth rule (`:469`) or `do_not_include_until_bc_height` (`:411`).
4. Added: long polling supersedes the stale template within one round trip, but the internal miner ignores `submit_old`, and `submitblock` has no tip check. So the stale block does get submitted.

**Cross-references.**

- `template-fat-pointer-walk-scans-the-whole-bft-chain-under-the-read-lock.md` (finding 9): the same function. Its proposed early stop is only sound once this finding is fixed, because "the parent's pointer always qualifies" needs the proposed height to be the parent's height plus one. The fix plans are written to land as one change.
- `admit-fat-pointer-accepts-a-certificate-by-hash-without-checking-signatures.md` (finding 7): the fallback at `bft.rs:577-583` copies the parent header's pointer bytes verbatim. Whenever the height and the parent agree (today, whenever the tip did not move; after this fix, always), the loop always finds a qualifying block at or above the parent's pointer's index (the parent's own pointer qualifies), and returns the node's own stored pointer for it (`fat_pointer_to_block_at_height`), never the miner-supplied bytes. The fallback runs only when nothing qualifies, which needs the parent's pointer to be null or unresolvable, or this race. That weakens finding 7's claim that forged pointers propagate to honest templates through this fallback.
- `dropped-decided-snapshot-leaves-validate-indeterminate-forever-and-decide-can-abort.md` (finding 6) and the conflict-hold path at `write.rs:210-262`: the deep-reorg-across-a-decision state that triggers this race is the same state that state machine manages.
