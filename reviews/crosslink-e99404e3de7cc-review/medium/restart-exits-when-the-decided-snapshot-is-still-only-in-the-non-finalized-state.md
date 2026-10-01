# On restart, `BftRunner::restore()` calls `std::process::exit(1)` whenever the newest stored BFT decision finalizes a snapshot that the reopened state does not hold, and on `dev` that snapshot normally lives only in the non-finalized state, so a crash during catch-up sync, or any restart with the non-finalized backup disabled, leaves a node that refuses to start until its operator deletes the database and resyncs

**Severity**: Medium
**Validation Status**: Partially confirmed (the defect and its permanence are confirmed; the trigger window is measured from the snapshot's commit, not from the decision, and the ephemeral-state trigger is wrong)
**Location**: `zebra-crosslink/zebra-state/src/new_network/bft.rs:1304-1322` (the exit), `:1279-1297` (replay already tolerates an unknown snapshot for every row but the last), `:1122-1128` (a decision no longer commits), `:1135-1143` and `:1192-1202` (stale comments; `write_bft_decision` runs synchronously and its error is only logged), `:1013-1017` (`validate` requires the snapshot to be known), `:624-647` (`restore` runs inside `BftRunner::new`); `zebra-crosslink/zebra-state/src/new_network.rs:1450` (`BftRunner::new` runs before STP networking is set up at `:1466-1481`), `:3029-3032` and `:3113-3118` (blocks whose pointer is unresolved are deferred); `zebra-crosslink/zebra-state/src/new_network/fin.rs:121-153` (`candidate` follows the best tip's pointer); `zebra-crosslink/zebra-state/src/service/write.rs:332-356`, `:414-419`, `:421-439`, `:441-447` (what reaches the finalized database, and that the backup is never written synchronously); `zebra-crosslink/zebra-state/src/service/finalized_state/zebra_db/bft.rs:53-56` (stale doc comment), `:78-98` (`write_bft_decision`); `zebra-crosslink/zebra-state/src/service/non_finalized_state/backup.rs:26`, `:34-66`, `:110-145` (5 s rate-limited backup task and restore); `zebra-crosslink/zebra-state/src/service/non_finalized_state.rs:174-235` (`with_backup`); `zebra-crosslink/zebra-state/src/config.rs:120-129`, `:270-279` (backup switches); `zebra-crosslink/zebra-state/src/service.rs:353-389` (backup restore gated on the max checkpoint height)
**Found by agent:** /code-review high (Claude Fable 5.1), 2026-09-29; validated 2026-09-29 at dev e99404e3de7cc
**In scope of audit?** Yes. The defect is the interaction of three changes made on `dev` after the split from `s1_dev`: BFT storage in the finalized database (`0e0ad0d1`, "Store the decided BFT chain in the finalized database"), decoupling the decision from finalization (`aee1513a`, "Track node-local finality in fin, decoupled from the BFT decision"), and moving the BFT chain into `zebra-state` (`379170fe`). It is a restart-safety defect of the hybrid consensus node. ClT0 (`s1_dev`) is not affected: there the decision finalized its snapshot into the database before appending the decision to the pos file (see Technical Details).

## Description

When a node starts, `BftRunner::new` calls `restore()`, which replays the decided BFT chain stored in the finalized database. After the replay loop it resolves the snapshot of the last replayed block with `read_state.known_block`. If that lookup fails, the node logs "Delete ... and resync" and calls `std::process::exit(1)` (`bft.rs:1304-1322`).

The comment on that branch explains the intent: "Both now live in the same database and commit together, so this is a damaged database rather than a configuration mistake." That premise is false on `dev`:

- **The decision row is durable immediately.** `finish_decision` writes it synchronously through `ZebraDb::write_bft_decision` (`bft.rs:1195-1202`).
- **The snapshot is not.** "A decision no longer commits" (`bft.rs:1122-1124`). The snapshot sits `sigma` blocks below the tip that proposed it, far above the 99-block depth at which blocks move to the finalized database (`write.rs:421-439`). It reaches the finalized database only when `fin` passes it, and `fin` follows the snapshot of the bft-block that the *best tip's* fat pointer names (`fin.rs:121-153`), which lags the newest decision.
- **Its only durable copy is the non-finalized backup.** That copy is written by an asynchronous task at most once every 5 s (`backup.rs:26`, `:110-145`). The backup is not written at all when `should_backup_non_finalized_state = false` or `debug_skip_non_finalized_state_backup_task = true` (`config.rs:270-279`, `non_finalized_state.rs:233-235`).

So after any restart that loses the tail of the non-finalized state, the newest stored decision can name a snapshot the node no longer has. `restore()` then exits. Nothing between two starts changes the database, so the node exits again on every later start. The only remedy it offers is deleting the whole state and resyncing.

The replay loop itself already treats an unknown snapshot as recoverable for every row except the last ("An unresolvable snapshot keeps the last known values", `bft.rs:1276-1278`). Only the final lookup is fatal. Three comments still describe the old ordering: `zebra_db/bft.rs:53-55` ("a decision is written after its snapshot commits"), `bft.rs:1136-1138` ("The finalize above put the snapshot in the finalized database"), and `bft.rs:1311-1313`.

## Attack Scenario and Steps

The trigger is not adversarial. It is an ordinary crash, kill or restart combined with the ordinary state of a node on `dev`.

**Scenario A: a crash or restart during catch-up sync (default configuration).**

1. A node is behind. PoW blocks whose fat pointer names a bft-block it has not yet decided are deferred rather than committed (`admit_fat_pointer` returns `None` at `bft.rs:396-402`; the sync loop keeps them at `new_network.rs:3029-3032` and `:3113-3118`). The PoW tip therefore advances in step with local BFT decisions.
2. Tenderlink delivers the decided block for BFT height k. `validate` passes only once the snapshot S_k is in the local state (`bft.rs:1013-1017`), so S_k was typically committed moments earlier. `decide` then runs `finish_decision`, which writes row k to the finalized database at once (`bft.rs:1195`).
3. S_k exists only in the in-memory non-finalized state. The backup task has not yet run, because it runs at most every 5 s. Nothing on the write path flushes the backup synchronously: both calls to `update_latest_chain_channels` in `write.rs` pass `None` for the backup path (`write.rs:337`, `:418`), and `write_to_backup` has no other caller (verified by grep).
4. The process dies before the next backup pass. A crash, an OOM kill, or an operator's Ctrl-C all qualify. I found no shutdown hook that flushes the backup, so a graceful stop is covered too (inferred from the absence of other callers).
5. On restart, `restore_backup` restores whatever was backed up, which does not include S_k. `BftRunner::new` runs `restore()` (`new_network.rs:1450`) before STP networking exists (`:1466-1481`), so S_k cannot be fetched first. `known_block(S_k)` returns `None` and the node exits with "Delete ... and resync".
6. Every later start repeats step 5.

**Scenario B: the backup is disabled (supported configuration).**

With `should_backup_non_finalized_state = false`, every restart discards the whole non-finalized state. The node exits unless, at the moment it stopped, the newest decision's snapshot had already been moved into the finalized database by a `fin` advance.

That window is short. `propose` produces one bft-block per PoW block (`bft.rs:757-758`). The snapshot of decision k reaches the database only after a PoW block carrying a pointer to decision k commits (`fin.rs:131-152`, `write.rs:441-447`, `write.rs:332-356`). Decision k+1 then follows within about one block interval. So at a random stop time the newest decided snapshot is usually above the finalized tip, and the restart usually exits. This steady-state reasoning is an inference from reading the code; I did not measure it.

`debug_skip_non_finalized_state_backup_task = true` has the same effect, because nothing writes the backup when the task is skipped.

**Scenario C: a state directory moved or copied without its backup directory.** The backup lives under `cache_dir/non_finalized_state/<network>`, which is a sibling of the database directory, not inside it (`config.rs:277-278`). An operator who copies only `cache_dir/state/...` to a new machine gets a database whose newest decision names a missing snapshot. This is inferred operational practice, not verified in any deploy script.

**Attack Requirements and Assumptions:**

- No attacker is required. The triggers are a crash, kill or restart during catch-up sync; any restart with the backup disabled; or a state copy without the backup directory.
- An attacker who can crash a node indirectly raises the odds. For example, the `assert_eq!` in `decide` (see finding 6) aborts the process under `panic = abort`, and an abort during catch-up lands in Scenario A. That abort is itself finding 6; this finding turns it from a restart into a forced resync.
- A synced node on the default configuration is mostly safe. Its newest snapshot was committed `sigma` blocks before the decision, and at the 25 s post-Blossom target spacing constant (`zebra-chain/src/parameters/network_upgrade.rs:253`) that is well over 5 s. I did not check whether the new testnet overrides the spacing.

## Impact on Users

- **Node operators:** the node refuses to start, deterministically and permanently, until the state is deleted and resynced. The message calls the database "damaged" when it is not, which points operators at the wrong cause.
- **Finalizers and stakers:** the finalizer's voting power is absent for the whole resync. If several finalizers restart together (for example, after a coordinated binary upgrade on a network where operators disabled the backup), BFT liveness can drop below quorum for the resync duration. This is inferred; it depends on roster concentration.
- **Miners:** a mining node is down for the resync.
- **Wallet users and light clients** served by an affected node lose service for the same period.
- **No consensus impact.** No chain split, no invalid block accepted, no funds at risk. Other nodes are unaffected.

## Technical Details / Code Analysis

**The exit** (`bft.rs:1304-1322`):

```rust
        let mut new_final_hash = Hash([0; 32]);
        let mut new_final_height = Height(0);
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

**The same lookup inside the replay loop is tolerated** (`bft.rs:1279-1283`). This means intermediate rows with an unknown snapshot silently keep the previous roster, while the last row with an unknown snapshot kills the process:

```rust
            if !block.headers.is_empty() {
                let snapshot = Hash(block.snapshot_block_hash().0);
                if let Some(known) = read_state.known_block(snapshot) {
                    prev_finalized_bc_height = known.height.0 as u64;
                }
```

**A decision stores its row but does not commit its snapshot** (`bft.rs:1122-1124`, `:1192-1202`):

```rust
        // A decision no longer commits. It advances `bft_final_snapshot`, which may name a block
        // on a chain that is not `bc_best`; `fin` moves only where the best chain changes, and
        // only by the §3.2 rule (FINALITY.md §4.3).
```

```rust
        // The decision is stored after the chain lock is dropped, so readers never wait on a
        // disk write. A crash before the row lands leaves the BFT chain one height short and
        // that height is decided again on the next run.
        if let Err(err) = block_writer.finalized_state.db.write_bft_decision(
            block.height,
            &block,
            &fat_pointer,
            &proposal_sigs,
        ) {
            tracing::error!("could not store BFT decision at height {}: {err}", block.height);
        }
```

The comment above covers a crash *before* the row lands. The failure in this finding is a crash *after* the row lands, but before the snapshot is durable. Nothing handles that case.

**The stale storage comment** (`zebra_db/bft.rs:53-55`):

```rust
    /// The decided BFT chain, ascending from height 0. Stops at the first gap: a decision is
    /// written after its snapshot commits, so a crash in between leaves a short chain, which
    /// resumes by re-deciding that height rather than by loading past the hole.
```

**What moves a block into the finalized database.** There are two paths:

- depth past `MAX_BLOCK_REORG_HEIGHT` (`write.rs:421-439`);
- `fin` advancing to `candidate(bc_best)` (`write.rs:441-447` into `crosslink_update_fin` into `handle_crosslink_finalize`, `write.rs:316`, `:332-356`).

`candidate` starts from the best tip's own pointer (`fin.rs:127-135`):

```rust
    let best_chain = non_finalized_state.best_chain()?;
    let tip = best_chain.tip_block()?;
    let tip_height = tip.height;

    let fat_pointer = &tip.block.header.fat_pointer_to_bft_block;
    if *fat_pointer == FatPointerToBftBlock::null() {
        return None;
    }
    let bft_height = *chain.hash_to_height.get(&fat_pointer.points_at_block_hash())?;
```

A just-decided bft-block is not named by any committed PoW block yet, so its snapshot is not a `fin` candidate yet.

**The backup is asynchronous and rate limited** (`backup.rs:26`, `:113-121`):

```rust
pub(crate) const MIN_DURATION_BETWEEN_BACKUP_UPDATES: Duration = Duration::from_secs(5);
```

```rust
    let err = loop {
        let rate_limit = tokio::time::sleep(MIN_DURATION_BETWEEN_BACKUP_UPDATES);
        let backup_blocks: HashMap<block::Hash, PathBuf> = {
            let backup_dir_path = backup_dir_path.clone();
            tokio::task::spawn_blocking(move || list_backup_dir_entries(&backup_dir_path))
                .await
                .expect("failed to join blocking task when reading in backup task")
                .collect()
        };

        if let (Err(err), _) = tokio::join!(non_finalized_state_receiver.changed(), rate_limit) {
            break err;
        };
```

**The backup switches** (`config.rs:270-275`):

```rust
    pub fn non_finalized_state_backup_dir(&self, network: &Network) -> Option<PathBuf> {
        if self.ephemeral || !self.should_backup_non_finalized_state {
            // Ephemeral databases are intended to be irrecoverable across restarts and don't
            // require a backup for the non-finalized state.
            return None;
        }
```

**The restore runs before networking** (`new_network.rs:1449-1450`), so it cannot fetch what it lacks:

```rust
    // BFT restores its persisted chain against the bc-chain, so this runs after genesis is in.
    let mut bft_runner = bft::BftRunner::new(bft, config, &read_state, &mut block_writer, rt.clone());
```

**Why ClT0 (`s1_dev`) is not affected.** On `s1_dev` the decision path first awaited `CrosslinkFinalizeBlock(new_final_hash)` (`zebra-crosslink/zebra-crosslink/src/lib.rs:787` at `64046aeb`), which put the snapshot in the finalized database. Only after that did it append the decision to the pos file (`:836-842`). A stored decision therefore always had a durable snapshot. `s1_dev`'s load path `.unwrap()`s the same lookup (`:1365`), which panics only when the pos file outlives the database. That is a different, pre-existing condition, and it is not this finding.

## Recommendations

The fix is shared with `restart-exits-when-killed-between-activation-and-the-bft-genesis-decision.md`: `restore()` should never exit for missing state that the network can supply again. It should resume from the longest prefix it can fully re-derive, and let the normal catch-up path fill in the rest.

1. **Truncate the replay at the first stored decision whose snapshot this state does not hold, and delete the exit** (`BftRunner::restore`, `bft.rs:1263-1323`).

   - Replay stops *before* such a row, because the roster for the next height is the stake at that row's snapshot, which cannot be computed.
   - The loop records the last resolved `(Height, Hash)` and uses it for `bft_final_snapshot` and for the startup `terminated_finalizers_at`. The post-loop lookup and its exit are removed.
   - Rows above the truncation point stay in the database. When catch-up re-decides those heights, `finish_decision` overwrites them.

   This restores two invariants:
   - startup never depends on state that is not durable;
   - every replayed height, and the startup roster, is derived from the stakes at a snapshot this state actually holds.

   The second invariant is stronger than today's silent "keep the last known values" for intermediate rows.

   ```rust
        let mut last_final: Option<(Height, Hash)> = None;

        for decision in stored {
            let StoredDecision { block, fat_pointer, proposal_sigs } = decision;
            if block.previous_block_fat_ptr.points_at_block_hash() != fat_pointer_to_tip.points_at_block_hash() {
                break;
            }

            // `decide` refuses a block without headers, so every stored block names a snapshot.
            let snapshot = Hash(block.snapshot_block_hash().0);
            let Some(known) = read_state.known_block(snapshot) else {
                // The snapshot was only in the non-finalized state, which a restart can lose.
                // The decision is still final: resume BFT at this height and let catch-up
                // re-decide it once PoW sync has the snapshot again.
                tracing::warn!(
                    "crosslink restore: BFT height {} finalizes {}, which this state does not hold; \
                     resuming BFT at that height",
                    ingest.len(),
                    snapshot,
                );
                break;
            };

            // usize to u64 is lossless on every target Zebra supports (pointers are at most 64 bits).
            let this_bft_height = ingest.len() as u64;
            let this_terminated = terminated_finalizers_at(hardforks, this_bft_height, prev_finalized_bc_height);
            let roster = tenderlink_roster_from_internal(&unsorted_roster, &this_terminated);

            prev_finalized_bc_height = u64::from(known.height.0);
            let stakes = block_writer
                .non_finalized_state
                .aggregated_stakes_at(snapshot)
                .or_else(|| block_writer.finalized_state.db.aggregated_stakes(&snapshot))
                .unwrap_or_default();
            if !stakes.is_empty() {
                unsorted_roster = stakes
                    .into_iter()
                    .map(|s| RosterMember { pub_key: s.0, voting_power: s.1, txids: Vec::new(), finalizer_address: None })
                    .collect();
            }

            ingest.push(decided_round_data(hardforks, &block, &fat_pointer, roster, proposal_sigs, this_bft_height));
            blocks.push(block);
            fat_pointer_to_tip = fat_pointer;
            last_final = Some((known.height, snapshot));
        }
   ```

   After the loop, `chain.bft_final_snapshot = last_final;` and the startup watermark is `last_final.map_or(0, |(height, _)| u64::from(height.0))`. The 0 there means "nothing finalized yet", the value `prev_finalized_bc_height` already starts from; it is not a masked error. The `unwrap_or_default()` on stakes is unchanged here; whether it should stay is finding 10's question.

   **Why this cannot deadlock (reasoning from the code, not tested).** Suppose replay stops at height k, so heights 0 to k-1 are loaded.
   - S_k was mined before decision k existed, so its own fat pointer names a height of at most k-1. That pointer resolves, and S_k commits when PoW sync delivers it.
   - Every PoW block at or below S_k's height on S_k's chain was mined earlier still, so the same argument applies to each of them.
   - Once S_k is in the state, `validate` passes for the re-delivered decision k, catch-up continues, and blocks pointing at k and above stop deferring.
   - If catch-up delivers decision k before S_k arrives, `validate` asks for S_k by hash through `needs_pow_block` (`bft.rs:1016`).

   Resuming at a height below the network's is the path every freshly bootstrapped node already takes: `finish_bootstrap` starts tenderlink at height 1 and catches up (`bft.rs:1428-1451`).

2. **Correct the stale comments**, so the next reader does not rebuild the old assumption:
   - `zebra_db/bft.rs:53-55`: a decision is written before its snapshot is durable, and restore resumes below any row whose snapshot is missing.
   - `bft.rs:1136-1138`: the snapshot is not finalized by the decision.
   - `bft.rs:1192-1194`: add the crash-after-row case.
   - `bft.rs:1217-1219` and `:1311-1313`.

3. **Optionally, flush the snapshot to the backup at decision time**, purely to save the re-download. `finish_decision` would need the backup path, which `WriteBlockWorkerTask` does not hold today. This is a performance nicety on top of item 1, not the fix.

**Considered alternatives, and why they were not chosen:**

- **A synchronous backup flush before `write_bft_decision`** as the fix. It does not cover `should_backup_non_finalized_state = false`, the skipped backup task, or a state copied without its backup directory. It also adds directory listing and file I/O to every decision on the sync thread.
- **Delaying `write_bft_decision` until `fin` passes the snapshot.** This needs a pending-write queue. It also never writes a decision whose snapshot is on a chain the node does not follow, and after a restart it re-decides the same heights that item 1 truncates. It is more machinery for the same result.
- **Committing the snapshot to the finalized database at decision time.** This reverts `aee1513a`'s design: a decision may name a snapshot that is not on `bc_best`, and FINALITY.md §4.3 requires the node to stay able to follow either chain.
- **Keeping the exit with a better message.** The database is not damaged, and the data needed to continue is one PoW download away.

**Tests:**

The test format cannot restart a node in-process.
- The BFT chain is a process-wide `LazyLock` (`bft.rs:82`).
- The request sender is a `OnceLock` set once per process (`bft.rs:152`, `:632`).
- The harness runs with `ephemeral = true` (`zebrad/tests/crosslink.rs:76`).

The restart tests therefore need a child-process harness (a zebrad binary on crosslink regtest with a persistent `cache_dir`), or a new test-format restart instruction that re-executes the binary.

- **Snapshot lost with the backup** (end-to-end, deterministic). This test drives the decisive state directly instead of racing the 5 s timer.
  1. Run a single-finalizer node past activation until at least three BFT decisions are stored. Record its BFT height and each decided block's hash from the log.
  2. Stop the node.
  3. Delete `cache_dir/non_finalized_state/<network>`.
  4. Restart.
  5. Assert that the process is still running after a bounded number of ticks, and that the "resuming BFT at that height" warning names the expected height.
  6. Assert that, once PoW re-syncs from a second node, BFT reaches at least the pre-restart height, with the same decided block hashes at every height.
- **Backup disabled** (end-to-end). Same as above, with `should_backup_non_finalized_state = false` and no deletion step.
- **Two-node catch-up after truncation** (end-to-end). Node A keeps finalizing while node B restarts truncated. Assert that B re-decides the truncated heights from A, that its stored rows end identical to A's, and that PoW blocks pointing at the truncated heights stop deferring on B.
- No unit test is needed; `restore` is not a pure leaf.

**Rollout:**

- **No consensus change.** This is node-local startup behaviour; validity rules and stored formats are unchanged. It needs no coordinated upgrade.
- **Land it before the new testnet's genesis anyway.** Every operator on `dev` is exposed from the first restart onward.
- **No backport to `s1_dev` (ClT0):** the ordering there prevents the defect.

## Validation Information

**Verdict: PARTIALLY CONFIRMED. Severity: Medium.**

| Claim | Verified at |
|-|-|
| `restore()` exits with "Delete ... and resync" when the last replayed snapshot is unknown | `bft.rs:1304-1322`, read directly |
| The exit is permanent: nothing between starts changes the inputs | `restore` runs before networking, `new_network.rs:1450` vs `:1466-1481`; `bft_chain()` reads the same rows, `zebra_db/bft.rs:56-74` |
| `finish_decision` writes the row synchronously; a write error is only logged | `bft.rs:1195-1202` |
| A decision does not commit its snapshot | `bft.rs:1122-1124`; no finalize call in `decide`/`finish_decision`, `bft.rs:1060-1215` |
| The snapshot reaches the finalized database only by depth or by `fin` | `write.rs:421-439`, `:441-447`, `:316`, `:332-356`; `fin.rs:121-153` |
| The backup is written asynchronously, at most every 5 s, never synchronously on commit | `backup.rs:26`, `:110-145`; `write.rs:337`, `:418` pass `None`; no other `write_to_backup` caller |
| The backup is off with `should_backup_non_finalized_state = false` or when the task is skipped | `config.rs:270-279`; `non_finalized_state.rs:233-235` |
| `zebra_db/bft.rs:53` comment is stale | `zebra_db/bft.rs:53-55` vs `bft.rs:1122-1124` |
| Intermediate unknown snapshots are tolerated; only the last is fatal | `bft.rs:1276-1283` vs `:1308-1321` |
| ClT0 finalized the snapshot before storing the decision | `s1_dev` `zebra-crosslink/zebra-crosslink/src/lib.rs:787`, `:836-842` at `64046aeb` |
| During catch-up the newest snapshot was committed moments before its decision | Inferred from `validate` requiring the snapshot (`bft.rs:1013-1017`) and PoW commits deferring on unresolved pointers (`bft.rs:396-402`, `new_network.rs:3113-3118`); timing not measured |

**Severity justification: Medium.**

*Why not High:*
- No consensus property breaks, no funds are at risk, and no other node is affected.
- A synced node on the default configuration is usually safe, because its newest snapshot is `sigma` block intervals old at decision time.
- The damage is a resync, which is cheap on a young test network.
- An adversary cannot trigger it without first crashing the node by some other means.

*Why not Low:*
- The triggers are ordinary: a crash or Ctrl-C during catch-up sync, or a documented configuration switch.
- With the backup disabled, almost every restart fails, and fails forever.
- The error message misdiagnoses the database as damaged.
- Any other abort bug (finding 6) is converted from a restart into a forced resync.
- On a public testnet where finalizers restart nodes for upgrades, this directly costs BFT voting power.

**Corrections made during validation:**

1. **The window is measured from the wrong event.** The finding says "kill ... within 5 s of a decision". What matters is whether the backup ran after the *snapshot* was committed to the non-finalized state. On a synced node that happened about `sigma` blocks before the decision, so a kill right after a decision is usually safe. The dangerous case is catch-up sync, where snapshot commit and decision are close together. Scenario A states this precisely.
2. **The ephemeral state is not a trigger.** An ephemeral database is created in a fresh temporary directory on every run (`config.rs:106-118`, `:257-258`), so its BFT rows never outlive it.
3. **The max-checkpoint gate is probably not a trigger on crosslink networks.** The gate (`service.rs:367-389`) skips the backup restore only while the finalized tip is below the max checkpoint height. On a network whose checkpoint list is genesis-only, that height is 0. This is inferred: I did not trace `params.checkpoints()` for the new testnet's parameters.
4. **Two triggers were added:** `debug_skip_non_finalized_state_backup_task = true`, and copying the state directory without the sibling backup directory.
5. **Two more stale comments were added:** `bft.rs:1136-1138` and `bft.rs:1311-1313`, besides `zebra_db/bft.rs:53`.

**Cross-references:**

- `restart-exits-when-killed-between-activation-and-the-bft-genesis-decision.md` (finding 4) is the other exit in `restore()`. The fix is shared: `restore()` resumes from what it can re-derive instead of exiting. Both should land in one change.
- `dropped-decided-snapshot-leaves-validate-indeterminate-forever-and-decide-can-abort.md` (finding 6): its `assert_eq!` abort is one way to reach Scenario A. With this fix, a restored chain whose newest snapshot was *dropped* (rather than merely lost with the backup) truncates and then waits for a snapshot peers may no longer serve. The node stays up but its BFT stalls at that height, which is finding 6's problem to solve.
- `missing-pow-blocks-grows-without-bound-from-unverified-proposal-snapshots.md` (finding 3): after truncation, the missing snapshot is requested through `needs_pow_block`. A bound added for finding 3 must still admit snapshots of decisions this node has itself stored.
- `new-bft-code-breaks-agents-md-cast-comment-and-misleading-default-rules.md` (finding 10): the `unwrap_or_default()` on aggregated stakes in the same replay loop (`bft.rs:1288`) is that finding's subject.
