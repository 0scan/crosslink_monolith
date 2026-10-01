# Once `crosslink_conflict_hold` gives up and commits past a `bft_final_snapshot` that sits on a side chain, the node drops that snapshot for good, so `validate()` answers `Indeterminate` to every later proposal, `propose()` declines forever, a decision already cached as `Pass` by tenderlink trips the `assert_eq!` in `decide()` and aborts the process (`panic = "abort"`), and every restart afterwards exits with "Delete and resync", although the hold's own documentation promises the node "keeps running; it never resyncs"

**Severity**: High
**Validation Status**: Partially confirmed (every mechanical claim holds; "the node stops syncing" is overstated, and two consequences the review missed are added: the restart exit and the silent hold)
**Location**: `zebra-crosslink/zebra-state/src/service/write.rs:135-137` (`conflict_abandoned`, memory only), `:210-263` (`crosslink_conflict_hold`, give-up at `:256-262`, hold log at `:249-252` is `debug!`), `:421-439` (the reorg-depth commit loop that calls it); `zebra-crosslink/zebra-state/src/constants.rs:142-150` (`CONFLICT_HOLD_DEPTH = 999`); `zebra-crosslink/zebra-state/src/service/non_finalized_state.rs:357-365` (`finalize` drops every side chain whose root is not the committed block), `:822-840` (`commit_would_drop`), `:861-878` (`parent_chain`, no fork below the finalized tip); `zebra-crosslink/zebra-state/src/service/write.rs:45-60` (`validate_and_commit_non_finalized`); `zebra-crosslink/zebra-state/src/service.rs:545-600` (`known_block`, `is_ancestor_of`); `zebra-crosslink/zebra-state/src/new_network/bft.rs:107-116` (`needs_pow_block`, `missing_pow_blocks`), `:802-819` (`propose` Linearity), `:1012-1017` (snapshot lookup in `validate`), `:1034-1054` (Linearity in `validate`), `:1083-1084` (the `assert_eq!` in `decide`), `:1089` (`known_block(..).unwrap()`), `:1304-1321` (`restore` exit), `:1483`, `:1497-1521` (tenderlink closures; `rx.await.expect` at `:1520`); `tenderlink/src/lib.rs:328-339` (`proposal_is_valid` caches `Pass`), `:1210-1230` (condition 49 pushes the decision on the cached `Pass`); `zebra-crosslink/zebra-state/src/new_network.rs:1583`, `:3064`, `:3168` (tick order), `:2853-2862` (by-hash download dropped as "already finalized"); `zebra-crosslink/Cargo.toml:201-202`, `:280-281` (`panic = "abort"` in dev and release); `zebra-crosslink/zebrad/Cargo.toml:79`, `:136` (`release_max_level_info` in the default release binaries); `crosslink_book/src/FINALITY.md:349-357`, `:696-704`, `:726-729`; `IMPLEMENTATION.md:311-316`, `:374-402`
**Found by agent:** /code-review high (Claude Fable 5.1), 2026-09-29; validated 2026-09-29 at dev e99404e3de7cc
**In scope of audit?** Yes. The hold, the `bft_final_snapshot`/`fin` split it serves, the Linearity check and the new `BftRunner::validate`/`decide` are all introduced on `dev` (IMPLEMENTATION.md stage 7): `new_network/bft.rs` does not exist at `64046aeb`, and `write.rs` there has no conflict hold. This is BFT/PoW hybrid consensus liveness and process safety. ClT0 (`s1_dev`) is **not affected** by the dropped-snapshot state, because a decision there commits its snapshot immediately. `s1_dev` does carry the same re-validation `assert_eq!` in `zebra-crosslink/zebra-crosslink/src/lib.rs:733` under the same `panic = "abort"`; whether anything on `s1_dev` can flip that re-validation was not checked.

## Description

A BFT decision on `dev` no longer commits anything. It only moves `bft_final_snapshot`, which may name a block on a chain that is not this node's best chain (FINALITY.md §4.3, `bft.rs:1119`). To stay able to switch to that decision, the reorg-depth commit in `handle_commit` asks `crosslink_conflict_hold` before each commit whether committing the best chain's root would drop the chain holding the decided block. If it would, the commit waits at the fork point.

The wait is bounded. Once the best chain's non-finalized part is longer than `MAX_BLOCK_REORG_HEIGHT + CONFLICT_HOLD_DEPTH` (99 + 999), the hold records the decided hash in `conflict_abandoned`, logs one error, and returns `false`. The loop then commits every held block in one go, and `NonFinalizedState::finalize` drops every chain whose root is not the block being committed, which includes the chain holding the decided snapshot. From that moment this node has no copy of `bft_final_snapshot`, and it can never get one back: that branch forks below its finalized tip, and Zebra cannot commit such a block.

Everything BFT does next depends on that block:

1. **Every later proposal is `Indeterminate`, forever.** An honest proposal extends the decided snapshot (Linearity), so its own snapshot is on the dropped branch and `validate` returns `needs_pow_block(new_final_hash)` at `bft.rs:1014-1016`. A proposal whose snapshot this node does hold reaches Linearity, where `is_ancestor_of(parent_snapshot, ..)` returns `None` because the ancestor is unknown, and `validate` returns `needs_pow_block(parent_snapshot_hash)` at `bft.rs:1049-1051`. Neither answer can ever change.
2. **This node never proposes again.** `propose` declines unless `is_ancestor_of(parent_snapshot_hash, candidate_hash) == Some(true)` (`bft.rs:809-818`).
3. **A decision already in flight aborts the process.** Tenderlink caches the first non-`Indeterminate` validation result for a proposal (`tenderlink/src/lib.rs:328-339`) and decides on that cached `Pass` (`:1214-1220`). `decide` validates again under the write lock and `assert_eq!`s `Pass` (`bft.rs:1084`). If the give-up lands between the two, the re-validation is `Indeterminate`, the assertion panics, and with `panic = "abort"` the whole node dies.
4. **Every restart afterwards exits.** `restore` resolves the stored BFT tip's snapshot with `known_block`; it is gone, so the node logs "Delete ... and resync" and calls `process::exit(1)` (`bft.rs:1308-1321`). `conflict_abandoned` is a memory-only field, so nothing on disk records why.

The defect is the gap between what this path promises and what it does. `write.rs:218-219` says: "Until then the operator is told and the node keeps running; it never resyncs." IMPLEMENTATION.md stage 7 (`:311-316`) repeats "the node never requires a resync", and FINALITY.md:728-729 makes it a requirement: "A node never requires a resync to resume bft-block validation." In practice the node stops validating BFT at the give-up, can abort at that moment, and needs a resync after any restart.

## Attack Scenario and Steps

**The attack as the code allows it.** FINALITY.md:349-357 notes that "a reorganization slightly deeper than `σ` reaches this case". σ is 4 in the prototype parameters (`librustzcash/zcash_primitives/src/bft.rs:534`).

1. Π_bft decides a bft-block `B` whose snapshot `P` sits at roughly best tip minus σ.
2. An attacker who can briefly outmine the public testnet (cheap at low difficulty) publishes a branch that forks below `P` and has more work. Sticky fork choice only protects `fin`, which trails `P`, so every node switches to it (FINALITY.md:731-734). `P`'s branch stays in view as a side chain. **(Verified by reading)**
3. Honest proposals on the new branch fail Linearity (`is_ancestor_of(P, new) == Some(false)`), so BFT stalls. Honest miners build templates on the best tip (`fat_pointer_for_template`, `bft.rs:551`), so they extend the attacker's branch, and no one extends `P`'s branch. **(Verified by reading)**
4. Each node holds its depth commit at the fork point. After 1,098 non-finalized blocks it gives up, commits past, and drops `P`. The attacker does no further work: honest mining delivers the 1,098 blocks. **(Verified by reading for one node; that every node reaches the same point is inferred, since each runs the same deterministic rule over the same heavier chain.)**
5. From then on no node can validate or propose, so BFT is halted network-wide. Any node that was holding a `Pass` for the in-flight height aborts when the decision arrives. Every node that restarts exits. **(Inferred from items 1 to 4 of the Description.)**

The same end state can be reached without any reorganization, through `tail-confirmation-checks-carried-headers-against-their-own-nbits-so-a-proposer-can-fake-confirmations.md`. There, a single byzantine proposer makes a stale side-chain block the decided snapshot, and the give-up follows 1,098 blocks later.

**Attack Requirements and Assumptions:**

- For the adversarial path: enough hash rate to win a race about σ + 1 blocks deep once, then patience for about 1,100 blocks. No stake is needed. On a low-difficulty public testnet this is cheap.
- A **non-adversarial trigger also exists**: any network partition or eclipse in which this node holds the decided branch as a side chain while a heavier branch runs 1,098 blocks past the fork.
- The abort (item 3) also needs a timing coincidence: tenderlink must be holding a cached `Pass` for the current height when the give-up commit lands. The sync thread handles `Validate` and `Decided` requests in `tick()` and `wait()` around the commit loop (`new_network.rs:1583`, `:3064`, `:3168`), so a validation in one tick and a decision in the next straddle any commit. That window opens again at every BFT height while proposals are in flight. How often it hits was not measured.
- The node must hold `P` in its non-finalized state. `commit_would_drop` returns `false` for a decided block the node never saw (`non_finalized_state.rs:822-826`), so an eclipsed node that never saw `P` skips the hold and never reaches this state.

## Impact on Users

- **Finalizers/stakers:** Once the give-up has happened on nodes holding one third or more of the stake, BFT cannot decide any block. No finalizer can propose. Rewards tied to finality stop. There is no in-protocol recovery: the only way back is a coordinated, out-of-protocol intervention.
- **Node operators:** A node can abort at the give-up. After that, every start exits with "the decided BFT chain finalizes block ..., which this database does not hold. Delete ... and resync". A resync does not help. A fresh node has to validate bft-block `B` to catch up, which needs `P`, and peers no longer serve `P`'s branch. During the 999-block hold operators get almost no warning. The hold's only per-commit message is `debug!` (`write.rs:249`), which the default release binaries compile out (`zebrad/Cargo.toml:79`, `:136`). The one generic signal is "BFT-Finality is falling behind", once the gap exceeds 512 (`bft.rs:668-669`).
- **Miners:** PoW continues on the node's own branch; blocks there can only cite bft-blocks at or below `B` (Last Final Snapshot), which this node holds, so admission is unaffected. Mining keeps working, but no new finality is created.
- **Wallet users and light clients:** `fin` is frozen at or below the fork point. It is not reverted: `fin` lies at or below an older snapshot, which is an ancestor of `P`, so it is on both branches. Transactions stop becoming final. No finalized transaction is reversed.
- **Bandwidth (inferred):** in this state every proposal inserts its snapshot into `MISSING_POW_BLOCKS`, and those hashes never become known, so they are requested from peers every tick forever (`bft.rs:112-116`). See `missing-pow-blocks-grows-without-bound-from-unverified-proposal-snapshots.md`.

## Technical Details / Code Analysis

**The give-up** (`write.rs:244-262`). The hold only protects a decided block that some chain holds and that the root commit would drop. After the bound, it flags the hash and lets the commit through:

```rust
        if !self.non_finalized_state.commit_would_drop(decided_hash, root_hash) {
            return false;
        }

        if held_len <= MAX_BLOCK_REORG_HEIGHT + CONFLICT_HOLD_DEPTH {
            tracing::debug!(
                "holding the commit at {root_hash}: it would drop the chain holding the decided block at height {}",
                decided_height.0,
            );
            return true;
        }

        self.conflict_abandoned = Some(decided_hash);
        tracing::error!(
            "crosslink: committing past the block decided at height {} after holding {} blocks; this node can no longer follow that decision",
            decided_height.0, CONFLICT_HOLD_DEPTH,
        );

        false
```

Because `conflict_abandoned == Some(decided_hash)` makes the hold return `false` at `:229-231`, the `while` loop at `write.rs:421-439` then commits every held block down to `MAX_BLOCK_REORG_HEIGHT` in the same call.

**The drop** (`non_finalized_state.rs:357-365`). Each commit removes every side chain that does not share the committed root:

```rust
        for mut side_chain in side_chains.rev() {
            if side_chain.non_finalized_root_hash() != best_chain_root.hash {
                // If we popped the root, the chain would be empty or orphaned,
                // so just drop it now.
                drop(side_chain);

                continue;
            }
```

**No way back.** A block on the dropped branch cannot be committed again. Its parent is neither the finalized tip nor on any held chain (`write.rs:53-57`), so `parent_chain` fails with `NotReadyToBeCommitted` (`non_finalized_state.rs:866-874`). A by-hash download of `P` itself, requested through `MISSING_POW_BLOCKS`, is dropped before that, when its height is at or below the finalized tip (`new_network.rs:2853-2862`):

```rust
                if !height_is_alleged {
                    // Deferred height checks for a by-hash request (see `height_is_alleged`).
                    if height.0 < min_height {
                        warning!("Block at height {} is below our near-tip-chain height {min_height}", height.0);
                        continue 'process_packets;
                    }
                    if height.0 <= finalized_height {
                        warning!("Block at height {} is already finalized", height.0);
                        continue 'process_packets;
                    }
                }
```

After the give-up the finalized tip is best tip minus 99, so `P` normally lands in that branch (inferred from the heights; not traced for every shape of fork).

**`validate` after the drop** (`bft.rs:1012-1017`, `:1034-1054`):

```rust
        let new_final_hash = Hash(new_block.snapshot_block_hash().0);
        if read_state.known_block(new_final_hash).is_none() {
            tracing::warn!("Didn't have hash available for confirmation: {}", new_final_hash);
            return needs_pow_block(new_final_hash);
        }
```

```rust
        if let Some(parent_snapshot_hash) = parent_snapshot_hash {
            if parent_snapshot_hash != new_final_hash {
                match read_state.is_ancestor_of(parent_snapshot_hash, new_final_hash) {
                    Some(true) => {}
                    Some(false) => {
                        tracing::warn!(
                            "BFT block violates Linearity: its snapshot {} does not extend its parent's snapshot {}",
                            new_final_hash, parent_snapshot_hash,
                        );
                        return fail;
                    }
                    // One of the two is not placed on a chain here yet; ask again rather than
                    // reject, exactly as a missing snapshot does above.
                    None => {
                        return needs_pow_block(parent_snapshot_hash);
                    }
                }
            }
        }
```

`is_ancestor_of` returns `None` as soon as the ancestor is unknown (`service.rs:565-572`):

```rust
                let ancestor_is_finalized = self.db.contains_hash(ancestor);
                let ancestor_is_known = ancestor_is_finalized
                    || non_finalized_state
                        .chain_iter()
                        .any(|chain| chain.height_by_hash(ancestor).is_some());
                if !ancestor_is_known {
                    return None;
                }
```

Its doc comment gives the assumption this state breaks: "an absent block may still arrive" (`service.rs:557-558`). Here it cannot.

**The cached `Pass` and the abort.** Tenderlink validates once and keeps the answer (`tenderlink/src/lib.rs:328-339`):

```rust
    // auto-caching
    async fn proposal_is_valid(&mut self, validate_closure: ClosureToValidateProposedBlock) -> TMStatus {
        // TODO: may want to start doing some of these on < proposal_chunks_n, i.e. shortcut known-invalid
        if self.proposal_checked_validity.0 == TMStatus::Indeterminate {
            if self.proposal_is_faulty {
                self.proposal_checked_validity = (TMStatus::Fail, TMStatusReason::None);
            } else if self.has_full_proposal() {
                self.proposal_checked_validity = validate_closure.0(&self.proposal).await;
            }
        }
        self.proposal_checked_validity.0
    }
```

It pushes the decision on that cached answer (`tenderlink/src/lib.rs:1214-1220`). This is the only call of `push_block_closure`:

```rust
            if (self.height == self.rounds_data[i].height && // any round
                has_enough_info_to_determine_validity &&
                big_threshold <= counts.yes_precommits &&
                self.rounds_data[i].proposal_is_valid(self.validate_closure.clone()).await == TMStatus::Pass)
            {
                if PRINT_BFT_CONDITIONS { println!("{ctx_str} {ANSI_GRY}BFT_CONDITIONS{ANSI_RST}: in condition 49: value decided"); }
                let (new_roster, new_vote_namespace) = self.push_block_closure.0(self.rounds_data[i].proposal.clone(), round_data_to_fat_pointer(&self.rounds_data[i], roster), self.rounds_data[i].proposal_sigs.clone()).await;
```

`decide` then re-validates against the current state and asserts (`bft.rs:1083-1089`):

```rust
        let mut chain = BFT_CHAIN.write().unwrap();
        assert_eq!(self.validate(&chain, read_state, &new_block), (TMStatus::Pass, TMStatusReason::None));

        // The `snapshot`: the parent of the deepest carried header, i.e. the block being
        // finalized. See `BftBlock::snapshot_block_hash`.
        let new_final_hash = Hash(new_block.snapshot_block_hash().0);
        let new_final_height = read_state.known_block(new_final_hash).unwrap().height;
```

Both profiles abort on panic (`zebra-crosslink/Cargo.toml:201-202` and `:280-281`):

```toml
[profile.dev]
panic = "abort"
```

```toml
[profile.release]
panic = "abort"
```

The push closure also panics if `decide` ever drops the reply without answering (`bft.rs:1520`, `rx.await.expect("new_network dropped a decided BFT block")`). A fix that simply returns early from `decide` would therefore move the abort, not remove it.

**The restart exit** (`bft.rs:1306-1321`):

```rust
        if let Some(new_block) = blocks.last() {
            new_final_hash.0 = new_block.snapshot_block_hash().0;
            match read_state.known_block(new_final_hash) {
                Some(known) => new_final_height = known.height,
                None => {
                    // The bc-chain is behind the BFT chain, which the two-stores split used to
                    // make possible. Both now live in the same database and commit together, so
                    // this is a damaged database rather than a configuration mistake.
                    tracing::error!(
                        "the decided BFT chain finalizes block {}, which this database does not \
                         hold. Delete {} and resync.",
                        new_final_hash,
                        block_writer.finalized_state.db.path().display(),
                    );
                    std::process::exit(1);
                }
            }
        }
```

The comment calls this state "a damaged database". After a give-up it is the expected state of a healthy database, and nothing persisted says so.

**A second route to the same abort (inferred, not traced end to end).** The hold protects only the chain holding `bft_final_snapshot`, and only while that block is above the finalized tip (`write.rs:233-236`). Suppose the latest decided snapshot is already in the finalized database, and a proposal's snapshot sits on a side chain that forks at the finalized tip. An ordinary depth commit then drops that side chain through the same `finalize` code. If this happens between tenderlink's `Pass` and the decision, it hits the same `assert_eq!`. It needs this node's best chain and the proposer's to diverge about 95 blocks deep, so it is rarer. It is also self-healing, because that branch forks at the finalized tip and can be downloaded again. The fix below covers it with the same code.

## Recommendations

**Decision: halt BFT loudly on this node, keep the process and PoW running, and do not re-anchor.** Consensus safety was weighed in both directions:

- **Re-anchoring BFT validation is consensus-unsafe.** Examples: letting Linearity pass when the parent snapshot is abandoned, or rebuilding the BFT chain from this node's own branch. If enough nodes vote `Pass` on proposals whose Linearity they cannot check, Π_bft can decide a snapshot that does not extend `P`. Two conflicting BFT-final snapshots then exist. That breaks "the snapshots of final bft-blocks are bc-linear", the premise behind every "With Linearity" guarantee in FINALITY.md §4.3, and the result is the "Without Linearity" outcome: nodes whose `fin` lies on different branches never reconverge, whatever the work. `restore` already rejects re-deriving BFT state for this reason (`bft.rs:1229-1232`). A permanent split with finality on both sides is strictly worse than a halt.
- **Halting the process** (exit at the give-up) is safe, but it is the wrong scope. FINALITY.md:696-700 requires that "the depth commit is never held back indefinitely" and that the node "remains a working PoW node with a frozen `fin`". Under the attack above, every honest node would exit together about 1,100 blocks after one cheap reorg, which gives any attacker a delayed kill switch for the whole network. It also recovers nothing: restart finds the same state.
- **Stopping BFT participation** is what the node already does in effect, since it can only answer `Indeterminate`. Not voting never violates safety. The fix makes that halt deliberate, persisted, visible, and free of aborts. The real remedy remains stage 9's second chain state (IMPLEMENTATION.md:374-402), whose done condition is that "bft-block validation on the held node never stopped".

**Implementation plan (interim, until stage 9 lands):**

1. **Persist the abandonment as the hazard record before the first commit past the fork.**
   - Add a row beside `crosslink_fin`, mirroring `zebra_db/fin.rs`: `crosslink_conflict_abandoned() -> Option<(Height, Hash)>` and `write_crosslink_conflict_abandoned(height, hash)`.
   - In `crosslink_conflict_hold`, write the row, then set `conflict_abandoned`. If the write fails, keep holding: one more tick of hold is safe, while committing without the record reproduces the restart exit.
   - Load the row in `WriteBlockWorkerTask::new`, so the field survives a restart.
   - This is the "persisted hazard record" that `write.rs:218` and FINALITY.md §4.3 name as the `@Todo`. Stage 9 keeps it, which satisfies IMPLEMENTATION.md's rule that no stage adds a mechanism a later stage deletes.

   ```rust
   if held_len <= MAX_BLOCK_REORG_HEIGHT + CONFLICT_HOLD_DEPTH {
       return true;
   }

   // Written before the first commit past the fork: after that commit the decided snapshot is
   // gone, and a restart needs this row to tell an abandoned decision from a damaged database.
   if let Err(err) = self.finalized_state.db.write_crosslink_conflict_abandoned(decided_height, decided_hash) {
       tracing::error!("crosslink: could not record the abandoned decision at height {}: {err}; still holding", decided_height.0);
       return true;
   }
   self.conflict_abandoned = Some(decided_hash);
   crate::new_network::bft::detach(decided_height, decided_hash);
   ```

2. **Add a detached state to `BftChain`**: `detached: Option<(Height, Hash)>`, set by `bft::detach` on the sync thread (the only writer, per `bft.rs:94`) and by `restore` from the row.
   - `validate` returns `(TMStatus::Indeterminate, TMStatusReason::None)` first thing when `detached` is set. It must not call `needs_pow_block`, because no hash on the abandoned branch can ever arrive.
   - `bft::detach` clears `MISSING_POW_BLOCKS` once.
   - `propose` returns `None` with one log line.
   - `Indeterminate` is the consensus-correct answer here. The node cannot evaluate Linearity, so it must vote neither for nor against the value, and tenderlink falls back to nil on timeout.

3. **Make `decide` tolerate a re-validation that loses data, and keep it strict on real contradictions.**
   - Replace the `assert_eq!` with a `match`.
   - `Pass`: record the decision as today.
   - `Fail`: keep a panic. A value this node validated as `Pass` can only fail again if an immutable fact changed, which is an invariant violation. Say that in the message.
   - `Indeterminate`: park the decision in `BftRunner` with its reply sender kept alive, and return. `tick` retries the parked decision before draining new requests. That completes the self-healing case (the second route above), because the snapshot is re-downloaded through `MISSING_POW_BLOCKS`. While detached, the parked decision is never completed.
   - Do not "store it anyway with the last roster and height". The next height's roster is the stake at the snapshot (FINALITY.md §7), which this node no longer has. A guessed roster is the misleading default AGENTS.md:198 forbids ("Don't turn invariant violations into misleading `None`/default values").

   ```rust
   let mut chain = BFT_CHAIN.write().unwrap();
   match self.validate(&chain, read_state, &new_block) {
       (TMStatus::Pass, _) => {}
       (TMStatus::Indeterminate, reason) => {
           // Tenderlink decides only on a Pass from this node, so the snapshot was here at
           // validation and has been dropped since. The decision is final either way; it is
           // recorded once the snapshot is back, and never while this node is detached.
           drop(chain);
           tracing::warn!("parking decided BFT block {}: {reason:?}", new_block.blake3_hash());
           self.parked_decision = Some(ParkedDecision { block: new_block, fat_pointer, proposal_sigs, reply });
           return;
       }
       (TMStatus::Fail, reason) => panic!(
           "decided BFT block {} failed re-validation after passing it: an immutable fact changed ({reason:?})",
           new_block.blake3_hash(),
       ),
   }
   ```

4. **Stop tenderlink when detaching, without aborting the process.**
   - Keep the `JoinHandle` from `self.rt.spawn(tenderlink::entry_point(..))` (`bft.rs:1483`) and `abort()` it in `bft::detach`, before any reply sender is dropped.
   - Change the push closure at `bft.rs:1520` so that a dropped reply parks the task instead of panicking. The message "new_network dropped a decided BFT block" stops being an invariant once detaching is a legitimate state:

   ```rust
   let Ok(answer) = rx.await else {
       // The node detached from BFT (a decided snapshot it can no longer hold). Tenderlink is
       // being aborted; this height never completes here.
       return std::future::pending().await;
   };
   answer
   ```

5. **`restore` starts detached instead of exiting when the row explains the missing snapshot.**
   - If `known_block(new_final_hash)` is `None` and `crosslink_conflict_abandoned()` names that same hash, load the BFT chain, set `detached`, use the stored height as `bft_final_snapshot`'s height, skip `spawn_tenderlink`, and log an error.
   - Keep the exit only for a missing snapshot with no record, and coordinate that path with `restart-exits-when-the-decided-snapshot-is-still-only-in-the-non-finalized-state.md`, which changes the same branch.

6. **Make the hold and the detached state visible in release builds.**
   - Log the hold at `warn!` when it first engages for a decided hash, and then every 100 held blocks, stating how many blocks remain before the give-up. `debug!` is compiled out by `release_max_level_info`.
   - While detached, `tick` logs `error!` every `DIAGNOSTIC_INTERVAL`.
   - Add the gauges `state.crosslink.conflict_hold.held_blocks` and `state.crosslink.bft_detached` (AGENTS.md: metrics use the existing `state.*` prefix).

**Considered alternatives, not chosen:**

- *Only remove the `assert_eq!`.* The `unwrap` at `bft.rs:1089` panics next, and dropping the reply panics at `bft.rs:1520`.
- *Hold forever, or freeze the PoW tip at the bound.* This contradicts FINALITY.md:696 ("never held back indefinitely"). It also either grows the non-finalized state without bound, or turns the cheap reorg into a network-wide PoW halt, since honest miners are on the heavier branch.
- *Raise `CONFLICT_HOLD_DEPTH`.* FINALITY.md:726-728 already rules this out: "under a permanent conflict it is the same wall `CONFLICT_HOLD_DEPTH` blocks later."
- *Automatic resync or snapshot rollback onto the decided branch.* This is stage 9 itself. If stage 9 can land before the new testnet's genesis, do it and keep only items 3 and 4 above (decide must never abort on a data race, whatever the state design).
- *Finality-aware mining* (templates prefer the branch holding `bft_final_snapshot`). This would remove the cheap trigger in the attack, but it is the fork-choice change FINALITY.md §4.3 weighs against the Book's liveness analysis. It is a design question, not a fix for this finding.

**Tests** (end to end first, as the test format allows):

1. **Test format, `zebrad/tests/crosslink.rs`**: `crosslink_conflict_hold_gives_up_and_detaches_bft_without_aborting`, modeled on `crosslink_pow_follows_the_heaviest_chain_until_fin_moves_to_the_decided_branch` (`crosslink.rs:992`).
   - Common prefix, then a short branch B. `LoadPoS` a bft-block whose snapshot is on B.
   - Extend branch A by 1,100 blocks with `BlockGen`.
   - Before the give-up: `LoadPoS` of a bft-block extending the decided snapshot on B succeeds, which asserts that the hold kept B.
   - After the give-up: `push_instr_load_pos(.., SHOULD_FAIL)` for a further bft-block, `push_instr_expect_node_alive`, `push_instr_expect_pow_chain_length` still advancing on A, and `push_instr_mine_from_template` still producing a committable block.
   - If the format cannot express "detached", report the gap as IMPLEMENTATION.md's stage rules require.
2. **The decide race, as an integration test inside `zebra-state`** (a `#[cfg(test)]` module in `new_network`, driving a real `WriteBlockWorkerTask` over an ephemeral state).
   - Send `BftRequest::Validate` for X and assert `Pass`. Commit branch-A blocks through `handle_commit` until the give-up. Send `BftRequest::Decided` for X.
   - Assert there is no panic, the decision is parked, `detached` is set, and the hazard row is written.
   - A second case drops X's snapshot through an ordinary depth commit with no give-up, then delivers the snapshot again. Assert that the parked decision completes on the next tick and tenderlink receives its roster.
3. **Restart, in the dilated two-node regtest** (`DILATED_REGTEST.md`, `zebra-crosslink/dilated_regtest/run.sh`). Drive one node through the give-up, restart it, and assert that it starts, logs the detached state, does not exit, and keeps syncing PoW.
4. No unit tests: there is no pure leaf function here.

**Rollout:**

- **No consensus rule changes.** Which bft-blocks and bc-blocks are valid is unchanged. The fix changes only local behavior: whether this node aborts, exits, or keeps running detached. Nodes with and without the fix interoperate, and no coordinated upgrade is needed.
- The new database row is a format addition. Bump the minor database format version in `zebra-state/src/constants.rs` per Zebra's migration rules.
- It should still land **before the new testnet's genesis**. Without it, one cheap reorg is enough to leave every node BFT-dead about 1,100 blocks later, and every restarted node exits.
- **`s1_dev`/ClT0:** no backport of items 1, 2, 5 and 6, because the hold and the dropped-snapshot state do not exist there. Item 3's pattern (`assert_eq!` on re-validation with `panic = "abort"`) exists at `s1_dev` `zebra-crosslink/zebra-crosslink/src/lib.rs:733`. Backport it only if a flip from `Pass` to not-`Pass` is shown to be reachable on `s1_dev`; that was not checked here.

## Validation Information

**Verdict: PARTIALLY CONFIRMED. Severity: High.**

| Claim | Verified at |
| - | - |
| The hold gives up after `MAX_BLOCK_REORG_HEIGHT + CONFLICT_HOLD_DEPTH` held blocks and commits past | `write.rs:248-262`, `:421-439`, `constants.rs:150` |
| The give-up drops the chain holding the decided snapshot | `non_finalized_state.rs:357-365` |
| The dropped branch can never be committed again | `write.rs:53-57`, `non_finalized_state.rs:861-878` |
| Linearity's `is_ancestor_of` returns `None` for an unknown ancestor, and `validate` then answers `Indeterminate` | `service.rs:565-572`, `bft.rs:1049-1051` |
| Honest proposals hit `Indeterminate` earlier, at the snapshot lookup | `bft.rs:1014-1016` |
| `propose` declines forever | `bft.rs:809-818` |
| Tenderlink caches `Pass` and decides on the cached value | `tenderlink/src/lib.rs:328-339`, `:1214-1220` |
| `decide` re-validates and `assert_eq!`s `Pass`, then `unwrap`s the snapshot lookup | `bft.rs:1084`, `:1089` |
| The process builds with `panic = "abort"` | `zebra-crosslink/Cargo.toml:201-202`, `:280-281` |
| A dropped decision reply panics in tenderlink's task | `bft.rs:1520` |
| Validation and decision straddle the commit loop within a tick | `new_network.rs:1583`, `:3064`, `:3168` |
| By-hash re-download of the snapshot is dropped as "already finalized" | `new_network.rs:2859-2861` |
| A restart after the give-up exits with "Delete and resync" | `bft.rs:1308-1321`; `conflict_abandoned` is memory only at `write.rs:137`, `:167` |
| The hold's progress log is compiled out of release binaries | `write.rs:249`, `zebrad/Cargo.toml:79`, `:136` |
| The documentation promises no resync and continuous BFT validation | `write.rs:218-219`, `IMPLEMENTATION.md:316`, `FINALITY.md:696-704`, `:728-729` |
| A reorganization slightly deeper than σ puts the decided snapshot on a side chain, and honest miners then extend the other branch | `FINALITY.md:349-357`, `bft.rs:551` |
| Same `assert_eq!` on re-validation exists on `s1_dev` | `git show 64046aeb:zebra-crosslink/zebra-crosslink/src/lib.rs`, line 733 |

**Severity justification.** *Why not Critical:* nothing becomes final that conflicts with Π_bft. `fin` is frozen, not reverted, so no wallet sees a finalized transaction reversed and no funds move. The failure is liveness plus process availability, on a test network. The network-wide version needs an attacker to win a short PoW race and then about 1,100 blocks to pass. The abort also needs a timing coincidence. *Why not Medium:* the end state is permanent and has no in-protocol recovery: BFT is halted on every node that reaches it, nodes may abort, and every restart exits into a resync that cannot restore BFT either. On a low-difficulty public testnet the trigger costs one short reorg, or one byzantine proposer through the Tail Confirmation finding. The hold that should give operators 999 blocks of warning logs nothing in release builds.

**Corrections made during validation:**

1. *"From then on the Linearity check's is_ancestor_of returns None":* true, but most proposals never reach it. An honest proposal's own snapshot is on the dropped branch, so `validate` returns `Indeterminate` earlier, at `bft.rs:1014-1016`. The Linearity `None` path is reached only by proposals whose snapshot this node holds. Those are exactly the proposals that violate Linearity, and they get `Indeterminate` where `Fail` would be correct. The conclusion (every proposal `Indeterminate`) stands.
2. *"Every later PoW block pointing at a newer BFT block defers forever, so the node stops syncing":* overstated. Such a block does defer (`admit_fat_pointer` returns `None` at `bft.rs:399-402`). But a block on this node's own branch cannot cite a bft-block newer than `B`, because Last Final Snapshot requires the cited snapshot to be an ancestor. The node therefore keeps syncing and mining its own branch as a PoW node. What it has lost is the decided branch, which the depth commit had already made unreachable, and all BFT progress.
3. *"The by-hash re-download of the dropped snapshot is thrown away every tick as 'already finalized'":* correct when the snapshot's height is at or below the new finalized tip, which is the usual case after a give-up. If it is higher, the block is queued instead and fails `NotReadyToBeCommitted`. It is never re-committed either way.
4. *Added:* a restart after the give-up exits (`bft.rs:1308-1321`), which contradicts `write.rs:219` and FINALITY.md:728-729. The review did not mention it.
5. *Added:* the hold is silent in release builds, and the cheap adversarial trigger comes from FINALITY.md:349-357 together with honest miners templating on the best tip.

**Cross-references:**

- `tail-confirmation-checks-carried-headers-against-their-own-nbits-so-a-proposer-can-fake-confirmations.md`: a byzantine proposer can place `bft_final_snapshot` on a stale side chain with no reorg. The give-up then follows about 1,100 blocks later and produces this finding's end state. Fixing that finding removes the cheapest trigger but not the partition or reorg triggers.
- `restart-exits-when-the-decided-snapshot-is-still-only-in-the-non-finalized-state.md`: same `process::exit(1)` branch in `restore`. The two fixes must be designed together, since this one adds a recorded exception and that one addresses a transient absence.
- `missing-pow-blocks-grows-without-bound-from-unverified-proposal-snapshots.md`: in this state even honest proposals add hashes to `MISSING_POW_BLOCKS` that can never become known. Item 2 above stops that for the detached case only.
- `template-fat-pointer-walk-scans-the-whole-bft-chain-under-the-read-lock.md`: after the give-up, the decided snapshots are off this node's chain, so every template walk scans the whole BFT chain back to the fallback. This finding makes that the permanent condition.
- `new-bft-code-breaks-agents-md-cast-comment-and-misleading-default-rules.md`: the ghost-roster default in `finish_decision` is the reason item 3 parks the decision instead of recording it with the last roster.
