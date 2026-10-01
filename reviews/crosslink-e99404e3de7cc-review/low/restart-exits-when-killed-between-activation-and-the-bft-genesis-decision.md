# On restart, `BftRunner::restore()` calls `std::process::exit(1)` whenever the PoW tip is above the BFT activation height and no decided BFT chain is stored, but a node legitimately reaches that state if it dies after a tick's commit loop crosses activation and before the next tick's `bootstrap()`, or if storing the genesis decision fails, so the node refuses to start and demands a resync even though BFT genesis is deterministic and could simply be derived again

**Severity**: Low
**Validation Status**: Confirmed (with one precondition added: the window only opens when a block above activation carries a null fat pointer)
**Location**: `zebra-crosslink/zebra-state/src/new_network/bft.rs:1229-1248` (the exit), `:650-660` (`tick` bootstraps at the start of a tick), `:1402-1423` (`bootstrap`), `:1365-1398` (`build_bootstrap_genesis`), `:1428-1451` (`finish_bootstrap`), `:1195-1202` (a failed `write_bft_decision` is only logged), `:375-402` (`admit_fat_pointer`: null pointers are admitted above activation, and unresolved pointers defer), `:885-1057` (`validate` has no dependence on the tip height); `zebra-crosslink/zebra-state/src/new_network.rs:731` (`IDLE_MS = 100`), `:1583` (`tick` before the commit loop), `:2939-3138` (the commit loop), `:3168` (`wait` to the end of the tick); `zebra-crosslink/zebra-rpc/src/methods.rs:3595-3635` (a template is built on tip change with whatever pointer BFT has at that moment); `zebra-crosslink/zebra-state/src/service/finalized_state/zebra_db/bft.rs:56-74` (`bft_chain()` stops at the first missing height); `zebra-crosslink/zebra-state/src/constants.rs:61-91` (database format history)
**Found by agent:** /code-review high (Claude Fable 5.1), 2026-09-29; validated 2026-09-29 at dev e99404e3de7cc
**In scope of audit?** Yes. Bootstrapping BFT from the chain at an activation height, and storing the decided BFT chain in the finalized database, were both introduced on `dev` after the split (`0e0ad0d1`, "Store the decided BFT chain in the finalized database", added this exit). It is a restart-safety defect in the hybrid consensus start-up path. ClT0 (`s1_dev`) is not affected: `s1_dev` has no `BftBootstrap` and no activation-height bootstrap.

## Description

`restore()` opens with a guard. If the stored BFT chain is empty, the network bootstraps BFT from the chain, the best tip is strictly above the activation height, and BFT is launched, it logs "Delete ... and resync" and calls `std::process::exit(1)` (`bft.rs:1229-1248`). The guard's comment gives the rationale: such a database "predates BFT storage", and "re-deriving genesis over a chain that already ran BFT would put this node on a different decided chain than its peers."

Neither half of that rationale holds on `dev`.

**First, a current database reaches this state without being old or damaged.** The mechanics are all in the code:

- The genesis decision is made by `bootstrap()`. That runs only at the *start* of a tick (`bft.rs:655-660`).
- The same tick's commit loop runs afterwards (`new_network.rs:1583` then `:2943`), and commits every queued block whose parent is known, in height order.
- A block above activation whose fat pointer is null passes `admit_fat_pointer`. The function only rejects a null pointer after a non-null parent pointer (`bft.rs:390-392`).

So when a tick finds the tip below activation and its queue holds activation, activation+1 and so on with null pointers, it commits past activation with no genesis decided. The window then stays open until the next tick's `bootstrap()`, roughly the rest of the commit loop plus up to `IDLE_MS = 100` ms of `wait`. A kill or crash inside that window leaves a database with the tip above activation and no stored BFT chain.

The second path is quieter. `finish_decision` only logs a failed `write_bft_decision` (`bft.rs:1195-1202`). If the genesis row fails to write, the node keeps running, commits blocks past activation, and exits on its next start. The same happens if row 0 alone is missing, because `bft_chain()` stops at the first gap and returns nothing (`zebra_db/bft.rs:62-66`).

**Second, re-deriving genesis is what every node does anyway.** `build_bootstrap_genesis` is a pure function of:

- the best chain's headers at h1+1 through h1+sigma;
- the hardforks scheduled at BFT height 0;
- the parameters.

Its fat pointer carries no signatures (`bft.rs:1365-1398`). `bootstrap()` already runs on any tick where the tip is at or above activation (`bft.rs:657`), not only exactly at activation. `validate` has no rule that depends on the tip height, so deciding genesis late passes the `assert_eq!` in `decide` (`bft.rs:1084`, `:885-1057`). A node on the new testnet that deletes its database and resyncs derives exactly the genesis it would derive by re-bootstrapping in place.

The "predates BFT storage" case is also out of reach for the new network. `dev`'s database format history says 29.0.0, 30.0.0 and 31.0.0 each "Requires a resync" (`constants.rs:61-91`), so no ClT0 (format 27) database can be opened by `dev`. Only internal `dev` databases written before `0e0ad0d1` fit the description, and even for those, re-deriving genesis is correct on a `FromChain` network.

## Attack Scenario and Steps

The trigger is not adversarial. It is an ordinary crash or kill during a narrow window, or a failed disk write. An adversarial miner can make the window more likely to occur, but cannot make it fatal alone.

**Scenario A: a syncing node crosses activation in one tick, and dies before the next.**

1. The PoW chain around activation contains a block above activation with a null fat pointer. This is likely for activation+1 even with only honest miners (inferred, not measured):
   - the miner's own node commits the activation block in tick T's commit loop;
   - the long-poll template for activation+1 is built as soon as the tip changes (`methods.rs:3595-3635`);
   - `fat_pointer_for_template` then finds an empty BFT chain and falls back to the parent's pointer, which is null (`bft.rs:573-580`);
   - genesis is decided only at the start of tick T+1;
   - nothing rebuilds the template when BFT decides, because the long-poll id covers the tip and time only (`methods.rs:3598-3604`).
2. A node syncing from behind receives activation-1, activation and activation+1 in the same batch. In one tick, `bootstrap()` does nothing because the tip is below activation (`bft.rs:657`). The commit loop then commits all three: the null pointer at activation+1 is admitted (`bft.rs:375-392`). The next block, carrying the genesis pointer, defers because genesis is not in `hash_to_height` (`bft.rs:396-402`).
3. The tip is now activation+1, and nothing is stored. The node is killed, crashes, or is stopped before the next tick's `bootstrap()`.
4. On restart, `restore()` finds an empty store, a tip above activation, and `launch` set (zebrad always passes `Some(bft_launch)`, `zebrad/src/commands/start.rs:634-651`). The node exits with "Delete ... and resync".
5. Every later start repeats step 4. Every resync crosses activation again, so it can hit the same window again.

**Scenario B: storing the genesis decision fails.** `write_bft_decision` for height 0 returns an error, for example on a full disk. `finish_decision` logs it and carries on (`bft.rs:1195-1202`). The node runs normally, mines or syncs past activation, and exits on its next start.

**Attack Requirements and Assumptions:**

- No attacker is required. Scenario A needs a crash, kill or stop inside a sub-second window that each node passes through once per sync. Scenario B needs a failed rocksdb write.
- **The window is short.** It runs from the first commit above activation to the start of the next tick: the remainder of the commit loop, whose length grows with how many queued blocks it verifies, plus `wait` of at most `IDLE_MS = 100` ms (`new_network.rs:731`, `:3168`).
- **An adversarial miner can make the window certain to occur, not longer-lived.** On a low-difficulty testnet they can mine a run of null-pointer blocks above activation, which are valid (`bft.rs:390-392`). That guarantees every later syncing node crosses activation in one tick, and makes that tick's commit loop longer. It still needs the victim to die inside the window.

## Impact on Users

- **Node operators:** the node refuses to start, and demands a full resync that is not needed. The message misdiagnoses a current database as one that "predates BFT storage".
- **Finalizers and stakers:** a finalizer hit by this is absent from BFT for the resync. The window sits right at activation, when the network's first rosters form, but it is one node at a time and short.
- **Miners, wallet users, light clients:** loss of service from the affected node until resync.
- **No consensus impact.** No chain split, no invalid block accepted, no funds at risk.

## Technical Details / Code Analysis

**The exit** (`bft.rs:1229-1248`):

```rust
        // A node that has mined past the activation height has decided BFT genesis, so an empty
        // BFT chain there is a database written before the chain was stored. It is not migrated
        // and not re-bootstrapped: re-deriving genesis over a chain that already ran BFT would
        // put this node on a different decided chain than its peers.
        if stored.is_empty() {
            if let (Some(activation_height), Some((tip, _))) =
                (self.params.bootstrap.activation_height(), read_state.best_tip())
            {
                if tip.0 > activation_height && self.launch.is_some() {
                    tracing::error!(
                        "this database is past the BFT activation height ({}) but holds no decided \
                         BFT chain, so it predates BFT storage in the finalized database. Delete \
                         {} and resync.",
                        activation_height,
                        block_writer.finalized_state.db.path().display(),
                    );
                    std::process::exit(1);
                }
            }
        }
```

The first sentence of that comment is false. "Mined past the activation height" does not imply "decided BFT genesis": commits and the genesis decision are separate steps in separate phases of the tick.

**Bootstrap runs at the start of the tick** (`bft.rs:655-660`):

```rust
        self.finish_bootstrap();
        if let (Some(_), Some(activation_height)) = (self.launch.as_ref(), self.params.bootstrap.activation_height()) {
            if read_state.best_tip().is_some_and(|(tip, _)| tip.0 >= activation_height) {
                self.bootstrap(read_state, block_writer);
            }
        }
```

**The commit loop runs after it in the same tick** (`new_network.rs:1583`, `:2939-2953`):

```rust
        bft_runner.tick(&read_state, &mut block_writer);
```

```rust
        blocks_to_commit.sort_by_key(|(_, block)| block.coinbase_height().expect("all blocks in the commit queue should already have been confirmed to have a height"));

        // @Temporary: just pull one and wait to commit it.
        let mut any_blocks_in_the_queue_can_make_progress = false;
        blocks_to_commit.retain(|(hash, block_arc)| {
            let hash = *hash;
            let block_arc = block_arc.clone();

            let parent_hash = block_arc.header.previous_block_hash;

            if read_state.known_block(parent_hash).is_none() {
                return true; // keep
            }
```

Despite the `@Temporary` comment, the `retain` visits every queued block in height order. Each successful commit makes the next block's parent known within the same pass.

**A null pointer above activation is admitted; a genesis pointer defers** (`bft.rs:381-402`):

```rust
    if let Some(activation_height) = params.bootstrap.activation_height() {
        if !child_is_null && pow_block_height.0 <= activation_height {
            return Some(CrosslinkVerdict::Reject);
        }
    }

    // PERMANENT, decided purely from the (immutable) pointer values, without resolving either
    // block: the child reverts to no BFT pointer while its parent had one. A null pointer can
    // never be "as new or newer" than a real one, so this is a certain regression.
    if child_is_null && !parent_is_null {
        return Some(CrosslinkVerdict::Reject);
    }
```

```rust
    let child_index = if child_is_null {
        None
    } else {
        match chain.hash_to_height.get(&child_fat_pointer.points_at_block_hash()) {
            Some(&h) => Some(h as usize),
            None => return None,
        }
    };
```

The second excerpt is `bft.rs:396-403`: an unresolved non-null pointer returns `None`, which the sync loop treats as "defer".

**Genesis is a deterministic function of the chain** (`bft.rs:1372-1396`, abridged to the inputs):

```rust
        for h in roster_height + 1..=roster_height + params.bc_confirmation_depth_sigma as u32 {
            let (header, ..) = read_state.block_header(Height(h).into())?;
            headers.push(bc_hdr_to_lrz(&header));
        }
        let mut block = match BftBlock::try_from(params, 0, FatPointerToBftBlock::null(), headers) {
```

```rust
        let fat_pointer = FatPointerToBftBlock::from_parts(block.blake3_hash(), 0, 0, &[]);
        Some((block, fat_pointer))
```

**A failed genesis write is only logged** (`bft.rs:1195-1202`):

```rust
        if let Err(err) = block_writer.finalized_state.db.write_bft_decision(
            block.height,
            &block,
            &fat_pointer,
            &proposal_sigs,
        ) {
            tracing::error!("could not store BFT decision at height {}: {err}", block.height);
        }
```

**A missing row 0 hides every later row** (`zebra_db/bft.rs:62-66`):

```rust
        for (i, (height, block)) in blocks.into_iter().enumerate() {
            if height.0 as usize != i {
                break;
            }
            let Some(fat_pointer) = fat_pointers.get(&height) else { break; };
```

## Recommendations

The fix is shared with `restart-exits-when-the-decided-snapshot-is-still-only-in-the-non-finalized-state.md`: `restore()` should never exit for missing state that it, or the network, can supply again. Here that means letting the existing `bootstrap()` re-derive genesis. Both should land in one change to `restore()`.

1. **Remove the empty-store exit in `BftRunner::restore`** (`bft.rs:1229-1248`) and replace it with an informational log. No new bootstrap code is needed:
   - with an empty store, `restore` loads nothing and starts no tenderlink;
   - on the first tick, `tick` sees the tip at or above activation and calls `bootstrap()`;
   - `bootstrap()` builds genesis from the same headers, decides it (`validate` passes at any tip height), writes row 0, and `finish_bootstrap` starts tenderlink at height 1, which catches up from peers.

   Rows 1 and above, if they exist behind a missing row 0, are rewritten as catch-up re-decides them. On the next restart `bft_chain()` loads them contiguously, and replay's linkage check (`bft.rs:1265-1267`) guarantees they chain to the re-derived genesis.

   The invariant restored: a `FromChain` node's BFT genesis is always a function of its chain, never of whether a previous process happened to finish storing it.

   ```rust
        if stored.is_empty() {
            if let (Some(activation_height), Some((tip, _))) =
                (self.params.bootstrap.activation_height(), read_state.best_tip())
            {
                if tip.0 >= activation_height {
                    // Genesis is a pure function of the headers above h1 (`build_bootstrap_genesis`),
                    // so the first tick's `bootstrap` derives the block this node would have stored.
                    tracing::info!(
                        "crosslink restore: tip {} is at or above the BFT activation height {} and no \
                         BFT chain is stored; BFT genesis will be derived from the chain on the first tick",
                        tip.0,
                        activation_height,
                    );
                }
            }
        }
   ```

2. **Rewrite the comment above the guard**, and `restore`'s doc comment (`bft.rs:1217-1219`). "Mined past the activation height" does not imply "decided genesis". An empty store above activation is a state the current code produces itself.

3. **Keep the log level of a failed `write_bft_decision` at `error`, and add that the height is re-decided on the next start.** With item 1, and item 1 of the shared plan, a missing row is self-healing, so the operator needs to know that and nothing more.

**Considered alternatives, and why they were not chosen:**

- **Bootstrapping inside the commit loop the moment the tip reaches activation.** For example, `bootstrap()` could be called from the `retain` closure after the activation block commits. That shrinks the window to zero for Scenario A, but not for Scenario B (a failed write), and it moves BFT work into the commit loop. The exit is still wrong, so this is at most a complement to item 1.
- **Rejecting null fat pointers above activation.** This would change a consensus rule. It would also break the honest activation+1 case described above, where a template is built before genesis is decided locally, and needs its own design (for example, a grace window). It is out of scope for a start-up fix.
- **Keeping the exit, but only for `tip > activation + k`.** Any k is arbitrary. An adversarial null-pointer run, or a failed write, exceeds it.

**Tests:**

The test format cannot restart a node in-process. The BFT chain is a process-wide `LazyLock` and the request sender a `OnceLock` (`bft.rs:82`, `:152`), and the harness runs with `ephemeral = true` (`zebrad/tests/crosslink.rs:76`). These tests therefore need a child-process harness, as for the shared finding.

- **Empty store above activation** (end-to-end, deterministic).
  1. Run a single-finalizer crosslink regtest node with a `FromChain` bootstrap past activation until genesis and a few heights are decided. Record the genesis hash from the "deciding BFT genesis {hash}" log line (`bft.rs:1415-1418`).
  2. Stop the node.
  3. Remove all rows from the three `bft_*` column families with a test-only `ZebraDb` helper behind `#[cfg(any(test, feature = "proptest-impl"))]`.
  4. Restart.
  5. Assert that the process stays up, that it logs the same genesis hash again, and that BFT reaches at least the pre-restart height.
- **Row 0 missing, rows 1 and above present** (end-to-end). Same as above, but delete only height 0. Assert re-derivation, and that on a further restart all rows load and replay (the linkage check passes).
- **One tick crosses activation** (test-format scenario, no restart). Build a PoW chain where activation+1 through activation+3 carry null pointers (the builders used by `crosslink_reject_fat_pointer_below_bootstrap_activation`, `zebrad/tests/crosslink.rs:815`, already make pointer-bearing chains around activation). Feed it in one batch, and assert the tip passes activation before genesis is decided. This pins the precondition, so a future change to the tick order is caught.

**Rollout:**

- **No consensus change.** This is start-up behaviour only. It needs no coordinated upgrade.
- **Land it before the new testnet's genesis.** The window is at activation, which every node crosses in its first sync.
- **No backport to `s1_dev`**, which has no activation bootstrap.

## Validation Information

**Verdict: CONFIRMED. Severity: Low.**

| Claim | Verified at |
|-|-|
| `restore()` exits when the store is empty, the tip is above activation, and BFT is launched | `bft.rs:1233-1246`, read directly |
| BFT is always launched in zebrad | `zebrad/src/commands/start.rs:634-651` (`Some(bft_launch)`) |
| `bootstrap()` runs at the start of the tick; the commit loop runs later in the same tick | `bft.rs:655-660`; `new_network.rs:1583`, `:2943` |
| One tick can commit several blocks in height order | `new_network.rs:2939-3138` (`retain` over the sorted queue) |
| Null pointers above activation are admitted; unresolved (genesis) pointers defer | `bft.rs:381-402` |
| An honest template for activation+1 can carry a null pointer | `methods.rs:3595-3635`, `bft.rs:573-580`; the timing is inferred, not measured |
| The window ends at the next tick's `bootstrap()`, at most one `IDLE_MS` of waiting after the commit loop | `new_network.rs:731`, `:3168` |
| A failed genesis write is only logged | `bft.rs:1195-1202` |
| A missing row 0 hides all later rows | `zebra_db/bft.rs:62-66` |
| Genesis is deterministic and `validate` accepts it at any tip height | `bft.rs:1365-1398`; `bft.rs:885-1057` has no tip-height rule |
| No ClT0 database can be opened by `dev` | `constants.rs:61`, `:77-86` (29, 30, 31 require a resync) |
| `s1_dev` has no activation bootstrap | `git grep BftBootstrap 64046aeb` finds nothing in `zebra-crosslink` |

**Severity justification: Low.**

*Why not Medium:*
- Scenario A needs a crash or kill inside a window of roughly one tick, and each node passes through that window once per sync.
- Scenario B needs a failed rocksdb write, which on most nodes means the disk is already failing.
- An adversary can make the window certain to occur, but cannot make the victim die inside it.
- The consequence is one node's resync on a young chain.

*Why not lower:* Low is the lowest level on this scale, and the defect is real. The exit rests on a false premise and turns a harmless, self-describing state into a forced resync.

**Corrections made during validation:**

1. **The finding omits a precondition.** "One tick commits blocks activation-1 through activation+3" is only possible when those blocks above activation carry null fat pointers. A block carrying the genesis pointer defers, because genesis is not yet in `hash_to_height` (`bft.rs:396-402`). If every block above activation carries it, the tick stops at tip == activation, and the guard's strict `tip.0 > activation_height` does not fire. The precondition is likely for activation+1 even with honest miners (inferred from the template path), and an adversarial miner can guarantee it.
2. **The window is at most one tick of waiting beyond the commit loop.** It is not "several blocks" of wall-clock time, although a large batch lengthens the commit loop itself.
3. **"Genesis is deterministic" holds given this node's headers at h1+1 through h1+sigma.** Whether every node holds the same headers there is finding 5's question, and re-deriving in place does not make it worse than a resync would.
4. **The guard's stated rationale does not apply to the new network.** Format 31 databases cannot come from ClT0.

**Cross-references:**

- `restart-exits-when-the-decided-snapshot-is-still-only-in-the-non-finalized-state.md` (finding 1) is the other exit in `restore()`. The fix is shared: `restore()` resumes from what it can re-derive, instead of exiting. Land both in one change.
- `bootstrap-genesis-headers-can-still-be-reorged-so-late-joiners-build-a-different-genesis.md` (finding 5) governs whether the re-derived genesis matches peers'. That question is identical for a node that resyncs from scratch, which is today's only remedy.
