# `ZcashCrosslinkParameters::bootstrap_is_valid` bounds only `activation_height - roster_height` and ignores `bc_confirmation_depth_sigma`, so a network configured with a narrow bootstrap gap lets `build_bootstrap_genesis` read the carried headers `h1+1 ..= h1+sigma` from the non-finalized best chain, where a deep enough reorg gives nodes that bootstrap before and after it different BFT genesis hashes; the parameters the new testnet actually ships leave those headers 97 blocks inside every node's finalized state, so the launch network is not exposed

**Severity**: Low
**Validation Status**: Partially confirmed
**Location**: `librustzcash/zcash_primitives/src/bft.rs:391-419` (`BftBootstrap` and its documented guarantee), `:512-525` (`bootstrap_is_valid`), `:533-544` (`PROTOTYPE_PARAMETERS` and its `const` assert); `librustzcash/zcash_primitives/src/transaction/mod.rs:1542` (`STAKING_PERIOD = 150`); `librustzcash/components/zcash_protocol/src/consensus.rs:713` (`MAX_BLOCK_REORG_HEIGHT = 100 - 1`); `zebra-crosslink/zebra-state/src/new_network/bft.rs:650-660` (`tick` triggers `bootstrap` at `tip >= activation_height`), `:885-895` (`validate` linkage check), `:1084` (`decide` asserts `Pass`), `:1352-1398` (`build_bootstrap_genesis`), `:1402-1423` (`bootstrap`); `zebra-crosslink/zebra-state/src/service.rs:705-722` (`ReadState::block_header` prefers the non-finalized best chain); `zebra-crosslink/zebra-state/src/service/write.rs:421-437` (depth finalization loop); `zebra-crosslink/zebra-chain/src/parameters/network/testnet.rs:545`, `:862-873`, `:1095` (default and validation of the parameters); `zebra-crosslink/zebra-network/src/config.rs:624-665`, `:1068-1072` (`DCrosslinkParameters`, the config path that can change them); `zebra-crosslink/zebrad/src/config.rs:309-327` (the new testnet `ClT1` definition: no crosslink override, `clear_checkpoints()`)
**Found by agent:** /code-review high (Claude Fable 5.1), 2026-09-29; validated 2026-09-29 at dev e99404e3de7cc
**In scope of audit?** Yes. The chain-built BFT bootstrap (`BftBootstrap`, `bootstrap_is_valid`, `build_bootstrap_genesis`) was introduced on `dev` after the split (commits `8eb72a5d` "bootstrap BFT from the chain instead of a configured roster" and `fb0dd8f6` "make the BFT bootstrap a per-network parameter"; `8eb72a5d` is not an ancestor of `64046aeb`, and `64046aeb:librustzcash/zcash_primitives/src/bft.rs` has no `BftBootstrap`). It concerns the correctness of the hybrid PoW/BFT handover. ClT0 (`s1_dev`) is not affected: it has no chain-built bootstrap.

## Description

On `dev`, BFT genesis is not received from peers and is not pinned in configuration. Every node builds it locally from its own PoW chain the first time its best tip reaches the activation height `h2` (`bft.rs:650-660`). The genesis block carries the `sigma` headers above the roster height `h1`, that is `h1+1 ..= h1+sigma`, and its identity is the BLAKE3 hash of its full serialization, headers included (`librustzcash/zcash_primitives/src/bft.rs:356-360`). BFT height 1 must name that hash in `previous_block_fat_ptr`, and `validate` fails any block whose previous pointer is not this node's own tip (`bft.rs:888`). So all nodes must read byte-identical headers at `h1+1 ..= h1+sigma`, whenever they bootstrap.

The code relies on those headers being beyond reorg reach, and says so twice:

- `BftBootstrap` (`librustzcash/zcash_primitives/src/bft.rs:404-406`): "`h2 - h1` must exceed the reorg limit, so by the time any `h2` block is accepted the `h1` ancestor is the same on every chain".
- `build_bootstrap_genesis` (`bft.rs:1362-1364`): "the caller only asks once the tip is at or past the activation height, where h1 is finalized-by-depth, so the headers are the same on every node".

The first sentence argues only about `h1`. The second silently extends the argument from `h1` to the `sigma` headers above it. The check that enforces it, `bootstrap_is_valid`, requires only `activation_height - roster_height > MAX_BLOCK_REORG_HEIGHT` (99). It never looks at `bc_confirmation_depth_sigma`. That is the defect: the validator admits parameter sets in which some carried headers are still in the non-finalized state when nodes bootstrap, and `ReadState::block_header` then serves them from the non-finalized best chain (`service.rs:709-714`).

**Where the review overstated.** The review presented the failure as a property of the new testnet. It is not. The new testnet (`ClT1`, `zebrad/src/config.rs:309-327`) sets no crosslink override, so it runs `PROTOTYPE_PARAMETERS` (`testnet.rs:545`, `:1095`): `h1 = 150 / 2 = 75`, `h2 = 75 + 200 = 275`, `sigma = 4`. The carried headers are heights 76 to 79. The depth finalization loop keeps at most 99 blocks non-finalized (`write.rs:421-437`), so when any node's tip first reaches 275 its finalized tip is at least 176. All four headers are then already in that node's finalized database, 97 blocks below the finalization boundary, and no reorg the node will accept can replace them. The review's scenario uses a hypothetical `h2 = h1 + 100`; that parameter set passes `bootstrap_is_valid` but is not what launches.

What remains is a latent parameter-validation gap: `[network.testnet_parameters.crosslink]` (`zebra-network/src/config.rs:633-640`) lets any network set `bootstrap_roster_height`, `bootstrap_activation_height` and `bc_confirmation_depth_sigma` independently, and `bootstrap_is_valid` will accept a combination that breaks the documented guarantee. `IMPLEMENTATION.md` "5. `MAX_BLOCK_REORG_HEIGHT` 99 → 999" also plans to choose new bootstrap heights, which is exactly when a margin that ignores sigma could be cut too fine.

## Attack Scenario and Steps

The scenario needs a network whose parameters pass today's check but leave carried headers non-finalized at bootstrap. The shipped `ClT1` parameters do not.

1. A test network is defined with, for example, `bootstrap_roster_height = h1`, `bootstrap_activation_height = h1 + 100`, `bc_confirmation_depth_sigma = 4`. `bootstrap_is_valid` accepts it (`100 > 99`).
2. The honest chain reaches `h2`. On each running node the finalized tip is `h2 - 99 = h1 + 1`, so `h1+2 ..= h1+4` are non-finalized. Each node's `tick` calls `bootstrap`, `build_bootstrap_genesis` reads headers A at `h1+1 ..= h1+4` from the best chain, and the node decides genesis `G_A` (`bft.rs:1412-1421`).
3. An adversary with majority hashrate has been mining privately from a fork point at or above `h1+1` and at or below `h1+3`. It publishes a heavier branch B while the early nodes' finalized tip is still at or below the fork point (within about two blocks of `h2` for a fork at `h1+3`).
4. Early nodes reorg to B. Nothing stops them: `crosslink_conflict_hold` protects only the decided snapshot, which is `h1` and is on both branches (`write.rs:220-252`), and the network has no checkpoints past genesis (`clear_checkpoints()`, `zebrad/src/config.rs:317`). The early nodes keep their stored `G_A` (`restore` never re-derives a stored genesis, `bft.rs:1227-1248`).
5. A node that syncs after the reorg reaches `h2` on branch B, reads headers B, and decides `G_B`, whose hash differs.
6. The early nodes' BFT height 1 names `G_A`. On the late node `validate` fails it at `bft.rs:888-895`, so BFT on every late joiner never advances past genesis. (Inference, not traced into tenderlink: if tenderlink ever hands `decide` a block without a prior `Pass` from `validate`, the `assert_eq!` at `bft.rs:1084` aborts the process instead of stalling.)

With parameters where `sigma` approaches `h2 - h1` (for example `sigma = 99`, `h2 - h1 = 100`) the last carried header is one block deep at bootstrap, and an ordinary one-block orphan race at the activation height is enough; no adversary is needed.

**Attack Requirements and Assumptions:**

- The network's operators chose crosslink parameters with `h2 - h1 - sigma` at most 98. Every node on the network must run the same parameters, so this is a network-design choice, not something an attacker can impose.
- For the adversarial variant, majority hashrate sustained for roughly `h2 - h1` blocks, released in a narrow window around `h2`. On a low-difficulty test network this is cheap.
- For the non-adversarial variant, only a natural reorg at the activation height, and only when `sigma` is close to `h2 - h1`.
- None of this is reachable with `PROTOTYPE_PARAMETERS`, which the new testnet uses.

## Impact on Users

For a network configured inside the gap:

- **Node operators and finalizers who join after the reorg**: their BFT chain never gets past genesis. They cannot validate or vote on any BFT block, so finalizer stake they run is idle, and their nodes never advance `bft_final_snapshot`. Recovery needs a manual database swap or a code change; a resync reproduces the same genesis from the same chain.
- **Miners on late nodes**: templates never carry a fat pointer past genesis, and PoW blocks from the rest of the network whose pointers name BFT blocks this node cannot resolve defer (`admit_fat_pointer` returns `None` for an unresolved hash, `bft.rs:399-402`), so the node falls behind the tip.
- **Wallet users and light clients** on those nodes see no BFT finality.
- The network splits by join time. Nodes that bootstrapped before the reorg are unaffected.

For the new testnet as shipped: no user impact.

## Technical Details / Code Analysis

**The check** (`librustzcash/zcash_primitives/src/bft.rs:512-525`) compares only `h2` with `h1`:

```rust
impl ZcashCrosslinkParameters {
    /// Whether a chain-built bootstrap puts `h1` beyond reorg reach before any `h2` block can be
    /// accepted. Always true for [`BftBootstrap::Supplied`].
    pub const fn bootstrap_is_valid(&self) -> bool {
        match self.bootstrap {
            BftBootstrap::FromChain { roster_height, activation_height } => {
                activation_height > roster_height
                    && activation_height - roster_height
                        > zcash_protocol::consensus::MAX_BLOCK_REORG_HEIGHT
            }
            BftBootstrap::Supplied => true,
        }
    }
}
```

It runs for every network: `with_crosslink_parameters` returns `InvalidCrosslinkBootstrap` when it fails (`zebra-chain/src/parameters/network/testnet.rs:862-868`), and both the config path (`zebra-network/src/config.rs:1068-1072`) and the regtest path (`testnet.rs:1095`) call it.

**The shipped parameters** (`librustzcash/zcash_primitives/src/bft.rs:533-544`, with `STAKING_PERIOD = 150` at `transaction/mod.rs:1542`):

```rust
pub const PROTOTYPE_PARAMETERS: ZcashCrosslinkParameters = ZcashCrosslinkParameters {
    bc_confirmation_depth_sigma: 4,
    bootstrap: BftBootstrap::FromChain {
        roster_height: crate::transaction::STAKING_PERIOD / 2,
        activation_height: crate::transaction::STAKING_PERIOD / 2 + 200,
    },
    staking: PROTOTYPE_STAKING,
};
const _: () = assert!(
    PROTOTYPE_PARAMETERS.bootstrap_is_valid(),
    "the bootstrap roster block must be below the reorg limit when any activation-height block is accepted"
);
```

The new testnet definition (`zebrad/src/config.rs:309-317`) sets no crosslink parameters and clears checkpoints, so `h1 = 75`, `h2 = 275`, `sigma = 4`, and nothing but depth finalization pins pre-activation blocks:

```rust
                network: testnet::Parameters::build()
                    // .with_network_name("Crosslink_Nightly_0")
                    .with_network_magic(Magic([b'C', b'l', b'T', b'1']))
                    .expect("Crosslink testnet magic is not a reserved value")
                    .with_slow_start_interval(Height(0))
                    .with_genesis_hash("05a60a92d99d85997cce3b87616c089f6124d7342af37106edc76126334a2c38")
                    .expect("Crosslink testnet genesis hash is well-formed")
                    .clear_checkpoints()
                    .expect("Crosslink genesis-only checkpoint list is valid")
```

The PoW genesis is pinned (`with_genesis_hash`); the BFT genesis has no equivalent.

**When bootstrap runs** (`zebra-state/src/new_network/bft.rs:655-660`): on the first tick at which the best tip is at or past `h2`. The tip is never below `h2` at that point, so `h2` is the worst case for header depth.

```rust
        self.finish_bootstrap();
        if let (Some(_), Some(activation_height)) = (self.launch.as_ref(), self.params.bootstrap.activation_height()) {
            if read_state.best_tip().is_some_and(|(tip, _)| tip.0 >= activation_height) {
                self.bootstrap(read_state, block_writer);
            }
        }
```

**What it reads** (`bft.rs:1365-1376`): the best chain's headers at `h1+1 ..= h1+sigma`, through `ReadState::block_header`, which answers from the non-finalized best chain first and falls back to the database only for heights that chain does not hold (`service.rs:709-719`):

```rust
    fn build_bootstrap_genesis(&self, read_state: &ReadState) -> Option<(BftBlock, FatPointerToBftBlock)> {
        let params = &self.params;
        let BftBootstrap::FromChain { roster_height, .. } = params.bootstrap else {
            return None;
        };
        // h1 is the snapshot, so the carried headers are the sigma blocks above it:
        // h1+1 ..= h1+sigma (see BftBlock).
        let mut headers: Vec<BcBlockHeader> = Vec::with_capacity(params.bc_confirmation_depth_sigma as usize);
        for h in roster_height + 1..=roster_height + params.bc_confirmation_depth_sigma as u32 {
            let (header, ..) = read_state.block_header(Height(h).into())?;
            headers.push(bc_hdr_to_lrz(&header));
        }
```

Both `as` casts are uncommented (AGENTS.md "All `as` casts must have a comment explaining why the cast is safe"), and `sigma as u32` silently truncates a configured `sigma` above `u32::MAX`; `roster_height + sigma` can also overflow. Neither is reachable with shipped parameters.

**Where finalization-by-depth sits** (`zebra-state/src/service/write.rs:421-437`): after each commit, blocks are moved to the finalized database until the best non-finalized chain holds at most `MAX_BLOCK_REORG_HEIGHT` (99, from `zcash_protocol`, re-exported at `zebra-state/src/constants.rs:43`) blocks. So at tip `T` the finalized tip is `T - 99`. Before genesis is decided `bft_final_snapshot` is `None`, so `crosslink_conflict_hold` returns `false` immediately (`write.rs:220-226`) and never delays this.

```rust
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
```

The invariant the design needs is therefore `h1 + sigma <= h2 - 99`, that is `h2 - h1 - sigma >= 99`. The review's proposed `h2 - (h1 + sigma) > 99` is one block stricter, which matches the one block of slack today's `h1` check already keeps (`h2 - h1 > 99` puts `h1` at or below `h2 - 100`). The shipped values give `275 - 75 - 4 = 196`.

**Why a different genesis stalls BFT** (`bft.rs:885-895`):

```rust
    fn validate(&self, chain: &BftChain, read_state: &ReadState, new_block: &BftBlock) -> (TMStatus, TMStatusReason) {
        let fail = (TMStatus::Fail, TMStatusReason::None);

        if new_block.previous_block_fat_ptr.points_at_block_hash() != chain.fat_pointer_to_tip.points_at_block_hash() {
            tracing::warn!(
                "Block has invalid previous block fat pointer hash: was {} but should be {}",
                new_block.previous_block_fat_ptr.points_at_block_hash(),
                chain.fat_pointer_to_tip.points_at_block_hash(),
            );
            return fail;
        }
```

**Is genesis received or pinned anywhere?** No. `build_bootstrap_genesis`'s own doc says every node "constructs identically from its own PoW chain instead of receiving" it (`bft.rs:1352-1354`). The only external entry for BFT blocks, `force_feed_bft_block` (`bft.rs:162-172`), is the test-format `LoadPoS` path and goes through the same `validate`. No config field names a BFT genesis hash (`DCrosslinkParameters`, `zebra-network/src/config.rs:633-640`). No PoW checkpoint pins `h1+1 ..= h1+sigma` on `ClT1` (`clear_checkpoints()`). The one thing that pins them is depth finalization, and it pins them only when the parameters leave enough room, which is what `bootstrap_is_valid` fails to require.

## Recommendations

1. **Make `bootstrap_is_valid` bound the last carried header, not `h1`.** In `librustzcash/zcash_primitives/src/bft.rs`, require that `h1 + sigma` is more than `MAX_BLOCK_REORG_HEIGHT` below `h2`, and reject a `sigma` of zero (genesis would carry no headers, and `decide` asserts `!new_block.headers.is_empty()` at `bft.rs:1115`) or one that does not fit in `u32`. This restores the invariant that every header `build_bootstrap_genesis` reads is in the finalized database of every node whose tip is at or past `h2`.

   ```rust
   impl ZcashCrosslinkParameters {
       /// Whether a chain-built bootstrap puts `h1` and the sigma headers BFT genesis carries,
       /// `h1+1 ..= h1+sigma`, in every node's finalized state before any node builds genesis.
       /// Always true for [`BftBootstrap::Supplied`].
       pub const fn bootstrap_is_valid(&self) -> bool {
           match self.bootstrap {
               BftBootstrap::FromChain { roster_height, activation_height } => {
                   // `as`: widening u32 to u64 is lossless.
                   if self.bc_confirmation_depth_sigma == 0
                       || self.bc_confirmation_depth_sigma > u32::MAX as u64
                   {
                       return false;
                   }
                   // `as`: the check above keeps sigma within u32.
                   let sigma = self.bc_confirmation_depth_sigma as u32;
                   let Some(last_carried) = roster_height.checked_add(sigma) else {
                       return false;
                   };
                   activation_height > last_carried
                       && activation_height - last_carried
                           > zcash_protocol::consensus::MAX_BLOCK_REORG_HEIGHT
               }
               BftBootstrap::Supplied => true,
           }
       }
   }
   ```

   Update the doc on `BftBootstrap` (`bft.rs:404-406`) to state the bound on `h1 + sigma`, and the `const` assert message at `:541-544` to match. The existing `const _: () = assert!(PROTOTYPE_PARAMETERS.bootstrap_is_valid(), ...)` then also guards the bootstrap heights that `IMPLEMENTATION.md` item 5 will choose when `MAX_BLOCK_REORG_HEIGHT` moves to 999.

2. **Read the genesis headers from the finalized database, so the invariant is enforced where it is used.** In `zebra-state/src/new_network/bft.rs`, pass the finalized database handle (`block_writer.finalized_state.db`, already available in `bootstrap`) to `build_bootstrap_genesis` and read `h1+1 ..= h1+sigma` with `ZebraDb::block_header` (`zebra-crosslink/zebra-state/src/service/finalized_state/zebra_db/block.rs:129`) instead of `ReadState::block_header`. With step 1 in place these heights are always finalized once the tip is at or past `h2`, so a missing one is an invariant violation: log it with `tracing::error!` naming the height and the finalized tip before returning `None`, rather than deferring silently (AGENTS.md "Don't turn invariant violations into misleading `None`/default values"). Replace the two uncommented casts:

   ```rust
   let sigma = u32::try_from(params.bc_confirmation_depth_sigma)
       .expect("bootstrap_is_valid keeps sigma within u32 for every FromChain network");
   let last_carried = roster_height
       .checked_add(sigma)
       .expect("bootstrap_is_valid keeps h1 + sigma below the activation height");
   let mut headers: Vec<BcBlockHeader> = Vec::new();
   for h in roster_height + 1..=last_carried {
       let Some(header) = db.block_header(Height(h).into()) else {
           tracing::error!(
               "crosslink bootstrap: header {} is not finalized although the tip is past the activation height",
               h,
           );
           return None;
       };
       headers.push(bc_hdr_to_lrz(&header));
   }
   ```

   This also makes genesis a pure function of finalized data, which bears on finding 4 (see cross-references).

3. **Considered alternatives, not chosen.**
   - *Pin the BFT genesis hash in network configuration*, like `with_genesis_hash` for PoW. It cannot be known until the chain reaches `h1 + sigma`, so it would need a coordinated config change after launch. Useful later as a checkpoint; not a launch-time fix.
   - *Carry headers at or below `h1` in genesis.* Changes what `snapshot_block_hash` (`parent(headers[0])`) names and how Tail Confirmation reads genesis; a larger consensus change for no gain over step 1.
   - *Only step 2, without step 1.* A misconfigured network would then fail to bootstrap at run time instead of being rejected when the parameters load. Step 1 gives the operator the error at the earliest point.
   - *Add `CONFLICT_HOLD_DEPTH` to the margin.* Not needed: the hold applies only once a decided snapshot exists, and none exists before genesis.

4. **Tests.**
   - Integration (config load): in `zebra-network/src/config/tests/vectors.rs`, next to `should_allow_unshielded_coinbase_spends_rejected_on_testnet`, deserialize a testnet config with `[network.testnet_parameters.crosslink]` set to `bootstrap_roster_height = 75`, `bootstrap_activation_height = 176`, `bc_confirmation_depth_sigma = 4` and assert it is rejected with `InvalidCrosslinkBootstrap`; the same config with `bootstrap_activation_height = 180` loads. This drives the real operator path.
   - Integration (crosslink test format): in `zebrad/tests/crosslink.rs`, a scenario with `SET_PARAMS` at the boundary (`FromChain { roster_height: h1, activation_height: h1 + sigma + 100 }`, harness `sigma = 3`) that mines to `h2`, asserts BFT genesis is decided (`EXPECT_POS_CHAIN_LENGTH` 1), then loads a heavier branch forking at `h1 + sigma - 1` and asserts the PoW tip did not switch to it. This shows the carried headers are pinned at bootstrap. It depends on the harness running the bootstrap path with the parameters from `SET_PARAMS`; `crosslink_reject_fat_pointer_below_bootstrap_activation` (`zebrad/tests/crosslink.rs:815`) already runs `BOOTSTRAP_HARNESS_PARAMETERS` and is the model to follow (not verified that it reaches `bootstrap()`).
   - Unit (pure leaf, `bootstrap_is_valid` only): the boundary `h2 - h1 - sigma` of 99 rejected and 100 accepted; `sigma = 0` rejected; `sigma = u64::from(u32::MAX) + 1` rejected; `roster_height` near `u32::MAX` rejected rather than overflowing.

5. **Rollout.** Step 1 tightens which network parameter sets load; it does not change block validity on any network whose parameters already satisfy it. `PROTOTYPE_PARAMETERS` gives a margin of 196, so `ClT1` is unaffected and the fix can land before or after its genesis without coordination. Step 2 changes only where the node reads bytes that are identical in both places under step 1, so it is not a consensus change either. It should land before anyone defines a network with shrunk bootstrap heights, and in the same change that picks new heights for `MAX_BLOCK_REORG_HEIGHT = 999`. No `s1_dev` backport: ClT0 has no chain-built bootstrap.

## Validation Information

**Verdict: PARTIALLY CONFIRMED. Severity: Low.**

| Claim | Verified at |
| :- | :- |
| `bootstrap_is_valid` requires only `h2 - h1 > MAX_BLOCK_REORG_HEIGHT` and ignores sigma | `librustzcash/zcash_primitives/src/bft.rs:515-524`, read |
| `MAX_BLOCK_REORG_HEIGHT` is 99 in the constant both the check and the finalize loop use | `librustzcash/components/zcash_protocol/src/consensus.rs:713`; `zebra-state/src/constants.rs:43`; `write.rs:11` |
| `build_bootstrap_genesis` reads `h1+1 ..= h1+sigma` from the best chain once `tip >= h2` | `bft.rs:1370-1376`, `:655-660`; `service.rs:709-719` |
| Genesis hash covers the carried headers | `librustzcash/zcash_primitives/src/bft.rs:356-360` (hash of the full serialization) |
| BFT genesis is built locally, not received or configured | `bft.rs:1352-1354`; `force_feed_bft_block` is test-format only (`bft.rs:141-172`); no field in `DCrosslinkParameters` (`zebra-network/src/config.rs:633-640`) |
| No PoW checkpoint pins pre-activation blocks on `ClT1` | `zebrad/src/config.rs:317` (`clear_checkpoints()`) |
| A different genesis fails BFT height 1 on the late node | `bft.rs:888-895` |
| At tip `T` the finalized tip is `T - 99`, and nothing holds it before genesis | `write.rs:421-437`; `write.rs:220-226` (`bft_final_snapshot` is `None`) |
| The new testnet uses `PROTOTYPE_PARAMETERS` (75, 275, sigma 4) | `zebrad/src/config.rs:309-327` sets no crosslink override; `testnet.rs:545`, `:1095`; `bft.rs:533-540`; `transaction/mod.rs:1542` |
| With those values the carried headers (76 to 79) are finalized at bootstrap, 97 blocks below the boundary (finalized tip at least 176) | arithmetic from the rows above |
| Parameters can be changed per network by config | `zebra-network/src/config.rs:633-665`, `:1068-1072` |
| Bootstrap is dev-only | `git show 64046aeb:librustzcash/zcash_primitives/src/bft.rs` has no `BftBootstrap`; `8eb72a5d` is not an ancestor of `64046aeb` |
| A late node might abort rather than stall via `decide`'s `assert_eq!` | `bft.rs:1084` read; whether tenderlink can reach `Decided` without a `Pass` from `Validate` was **not** traced (inference) |

**Severity justification: Low, and why not Medium.** The mechanism is real and its consequence on an affected network is severe: every node that joins after the split has a permanently stalled BFT chain, with no automatic recovery. But it is unreachable on the network being launched: the shipped parameters leave a 196-block margin, and the only way in is for the network's designers to configure a gap of 98 or less for every node. That is a latent validation gap in a parameter checker, not an exploitable path on `ClT1`. Medium would require a realistic path to the failure on the network as it will run. Low is the floor, and the gap merits a fix because the code documents a guarantee (`bft.rs:1362-1364`) that the validator does not enforce, and because `IMPLEMENTATION.md` item 5 plans to re-choose the bootstrap heights.

**Corrections made during validation.**

1. The review framed this as a live exposure of the new testnet ("with activation = h1+100"). `ClT1` uses `h1 = 75`, `h2 = 275`, `sigma = 4`; the scenario's parameters are hypothetical. Recorded as Partially confirmed for that reason.
2. The exact bound the design needs is `h2 - h1 - sigma >= 99` (finalized tip at `h2` is `h2 - 99`). The review's `h2 - (h1 + sigma) > 99` is one block stricter, which matches the existing check's convention; kept.
3. Added what the review did not check: the reorg in its scenario must also be accepted by the early nodes, which it is (the conflict hold protects only `h1`), and it must be revealed while their finalized tip is still at or below the fork point, which narrows the window to about two blocks after `h2` for the review's numbers.
4. Added the non-adversarial variant: with `sigma` close to `h2 - h1`, a one-block natural reorg at `h2` suffices.
5. Added the uncommented and truncating `as u32` / `as usize` casts in `build_bootstrap_genesis`.

**Cross-references.**

- `restart-exits-when-killed-between-activation-and-the-bft-genesis-decision.md` (finding 4): its fix proposes re-bootstrapping instead of exiting. That is safe only if genesis is a pure function of data every node agrees on, which is what steps 1 and 2 here guarantee. `IMPLEMENTATION.md` stage 6 refuses to re-derive genesis because "a node whose PoW chain reorged below the bootstrap roster height" would get a different one; with step 2, the inputs are finalized, so that concern no longer applies to heights up to `h1 + sigma`. Land this fix first, or together.
- `dropped-decided-snapshot-leaves-validate-indeterminate-forever-and-decide-can-abort.md` (finding 6): shares the `assert_eq!` at `bft.rs:1084` that could turn a genesis mismatch into an abort.
- `new-bft-code-breaks-agents-md-cast-comment-and-misleading-default-rules.md` (finding 10): the two uncommented casts at `bft.rs:1372-1373` belong to the same AGENTS.md rule and are fixed by step 2 here.
