# Crosslink finality implementation plan

[`FINALITY.md`](./crosslink_book/src/FINALITY.md) defines the behavior. This file orders the work that brings the
code to it. Each stage names the FINALITY.md sections it implements, the code it touches, and
the condition under which it is done. Where this file and FINALITY.md disagree, FINALITY.md is
right and this file is corrected.

Stages 1 to 8 are done, so `zebra-crosslink/zebra-crosslink` holds no finality state and
answers no finality question. Their text is in this file's history
(`git show 30ac270a:IMPLEMENTATION.md`).
Stage 9 gives the node the second chain state that FINALITY.md §4.3 requires, so that a BFT
branch conflicting with the depth commit is recorded rather than abandoned. It is not ready to
start: questions 7 to 10 below block it.
Removing what is left of the crate afterwards is separate work, in
[`CRATE_REMOVAL.md`](./CRATE_REMOVAL.md); nothing depends on it, and it changes no behavior.

A stage ends the same way every time: its node tests pass, it is committed, and the dilated
two-node regtest (DILATED_REGTEST.md, `zebra-crosslink/dilated_regtest/run.sh`) passes against a
build of that commit before the next stage starts.

## Rules for every stage

- Read the FINALITY.md sections a stage cites before changing code. FINALITY.md labels each
  statement as **Book**, **Zebra Crosslink**, or **current tree**. Zebra Crosslink statements
  are requirements, Book statements are requirements wherever FINALITY.md does not record a
  Zebra Crosslink departure, and current-tree statements describe code that changes.
- No stage adds a mechanism a later stage deletes. Each stage says what it deletes, and a stage
  whose deletions all sit in a later stage is in the wrong place in the order. Moving code is
  not writing it: a stage may carry code across a crate boundary that a later stage removes.
- Nothing new is written in `zebra-crosslink/zebra-crosslink`. Work that would land there lands
  in `zebra-state` instead, even where the crate's existing structure would take it
  (FINALITY.md §7.1). That a change fits the TFL service's main loop is an argument against it.
- `σ` and the staking reward and payout code belong to other work. No stage changes
  `bc_confirmation_depth_sigma`, `pos_subsidy`, `update_bonds_with_pos_issuance`,
  `fixup_aggregated_stakes`, or the wallet reward projection.
- Databases written by an earlier derivation are deleted, not migrated. No stage adds code that
  loads them.
- A stage that changes code FINALITY.md describes as current tree updates those FINALITY.md
  statements in the same commit (FINALITY.md §§5, 6, 8).
- `VIZ_GUI_FINALITY_RULES.md` is untracked on purpose and is never committed.
- One agent at a time in one tree. The next stage starts only after the previous one is
  committed and its regtest has printed `PASS`.
- No stage pushes anything. An agent that believes a stage needs a push has misread it.
- Tests run through `phest.bat zebra-crosslink`, with a test-name filter as the fourth
  argument: `phest.bat zebra-crosslink Debug Win64 <filter>`. Never call `cargo` directly.
  The node tests in `zebrad/tests/crosslink.rs` run headless (FINALITY.md §8.1); a non-empty
  `ZEBRA_TEST_GUI` opens the visualizer window for them instead.
  Each node test boots a zebrad in the test process and ends it with `process::exit`, so a
  test run is one process per test, and the harness's capture is turned off so the
  runner's per-instruction dump survives an abort:
  `$env:RUST_TEST_THREADS=1; $env:RUST_TEST_NOCAPTURE=1; .\phest.bat zebra-crosslink Debug Win64 -p zebrad --test crosslink <test name>`.
  Both settings are environment variables because `phargo.bat` forwards `%4` through
  `%9` and splits `--test-threads=1` at the `=`, so a trailing `-- --nocapture
  --test-threads=1` never reaches the harness.
  A winit panic means the feature was left on; it is not worked around in the test code.
- After a stage is committed, `zebra-crosslink/dilated_regtest/run.sh` runs against a debug
  build of the commit and must print `PASS` (DILATED_REGTEST.md). It is the system test the
  node tests are not: wallet, staking, BFT bootstrap and finality under 90x time dilation.
- A block loaded with `SHOULD_FAIL` costs the harness's full 30-second submission deadline:
  a rejected block never gets an answer from the ingest queue. The fork-rejection tests and
  diagram scene 3 therefore take minutes, not seconds.
- `REGTEST_BLOCK_BYTES` and `REGTEST_POS_BLOCK_BYTES` are written by the `#[ignore]`d
  `regen_test_data` in `zebrad/tests/crosslink.rs`; the diagram scenes by
  `crosslink_write_finality_diagram_scenes`. Both are regenerated, never hand-edited, whenever
  the block format or the header count of a BFT block changes.
- The build uses `panic = abort`: a new `assert!`, `unwrap`, or `expect` on a consensus path
  terminates the node when it fails.
- A gap between what a stage asks for and what the test format can express is reported in the
  stage's commit message and left as an `@Todo` beside the test. It never stops the stage and it
  never changes the test format.
- Encoding of new database rows, and whether a derived value is stored or recomputed, are the
  implementing stage's call. Recomputing wins where the value is a function of the chain, so
  that one derivation exists (FINALITY.md §8.1).

## Corrections owed by the finished stages

- FINALITY.md §4.3 "Current tree" says the tree does not compute `fin` and describes the
  collapse onto a BFT-decided branch. Both ended with stage 7.
- Two doc comments name `CrosslinkFinalizeBlock`, which stage 7 deleted: the one on `VizScene`
  in `zebra-crosslink/zebra-crosslink/src/viz2.rs`, and the one on diagram scene 3 in
  `zebrad/tests/crosslink.rs`.
- The dilated regtest has no recorded `PASS` against the stage 7 and stage 8 commits.

## Stage 9: A second chain state for a conflicting BFT branch

Implements FINALITY.md §4.3 "Implementation in Zebra Crosslink" (the second finalized state) and
§7.1. It replaces the `CONFLICT_HOLD_DEPTH` interim in `zebra-state/src/service/write.rs`.
Blocked by questions 7 to 10; the text below is the mechanism question 7 calls C.

- The PoW state P keeps a snapshot of its finalized state at a height at or below `fin`, retaken
  as `fin` advances and left alone while finality lags. The snapshot must be openable as an
  independent, writable finalized state while P keeps writing.
- How the snapshot is taken is the storage engine's business and is chosen behind one interface,
  not spread through `zebra-state`: a hard-linked checkpoint under RocksDB, a persistent savepoint
  plus a reflink or byte clone under redb, or a logical copy into a fresh database. The last is
  the portable fallback and costs a full database of time and disk; a byte copy needs P's writer
  paused for its duration, which is one pause of the `new_network` block writer, reads
  unaffected. Zebra's move from RocksDB to redb must not change anything above this bullet.
- On a bc-block that forks below P's finalized tip but above `fin`, the node opens C from the
  snapshot, replays P's own stored blocks from the snapshot height to the fork point, and feeds C
  the conflicting chain from peers. C's fork-choice floor is `bft_final_snapshot`; C never
  depth-commits.
- `zebra-state` routes blocks and reads to both states and chooses the served best chain across
  both by the §4.3 switch rule. C is dropped when `fin` passes the fork; P's branch is recorded
  for as long as blocks arrive on it.
- The wallet's `REWIND_DISTANCE` and `CHECKPOINTS_N` cover a switch back to `fin`, which is
  deeper than `MAX_BLOCK_REORG_HEIGHT`.

Deletes: the `CONFLICT_HOLD_DEPTH` hold and its `@Todo`.

Done when a two-node regtest in which one node is held on a PoW fork more than
`MAX_BLOCK_REORG_HEIGHT` blocks long while the other finalizes a conflicting branch rejoins
without a resync, both branches are still recorded on the held node afterwards, and bft-block
validation on the held node never stopped.

## Needs design pass

A design session reads the cited FINALITY.md sections, re-checks the code facts listed, asks the
user, and records each answer in FINALITY.md as a Zebra Crosslink requirement. It then updates
the stage the question blocks and deletes the question. Question numbers are stable: an answered
question is removed and its number is not reused. Questions 3 to 5 block no stage; 7 to 10 block
stage 9; 11 changes how stage 9 and CRATE_REMOVAL.md are written.

### 3. Proposal cadence

FINALITY.md §3.4 says a proposal is always *possible*: when the tail does not extend the parent's
snapshot, an honest proposer repeats the parent's headers. It does not say when a proposer
*should* propose.

Code facts: Tenderlink calls the propose closure at the start of each round when it has no valid
value; the closure returns `None` unless the snapshot improves. The Book ties Linearity's
rationale to BFT liveness possibly requiring a minimum proposal rate
([construction.md lines 610–614](https://github.com/daira/tfl-book/blob/fe6e1d6f403f62da46c64e8f5a7db3cb188ffae2/src/design/crosslink/construction.md#L610-L614)).

To decide: whether a proposer repeats the parent's headers every round, only after some number
of empty rounds, or never; and whether a bft-block with an unchanged snapshot has any other
effect, such as carrying hardforks or `do_not_include_until_bc_height`.

### 4. `do_not_include_until_bc_height`

This is a Zebra-only bft-block field. A hardfork bft-block sets it to its greatest
`pow_activation_height`, later blocks carry it forward monotonically, and a bc-block may not
cite a bft-block whose value exceeds the bc-block's height.

To decide: whether it holds any consequence for Last Final Snapshot, Extension, `candidate`
monotonicity, or the roster at the previous decided bft-block's snapshot; and whether
FINALITY.md defines it as a Zebra Crosslink validity rule. Template selection keeps it as an
extra condition, and the template check and the admission check are the same code.

### 5. `MAX_BLOCK_REORG_HEIGHT` 99 → 999

FINALITY.md §4.3 records 999 as the intended value; the tree has 99 in
`zcash_protocol::consensus`.

Code facts: `ZcashCrosslinkParameters::bootstrap_is_valid` requires
`activation_height − roster_height > MAX_BLOCK_REORG_HEIGHT`, and a `const _: () = assert!` on
`PROTOTYPE_PARAMETERS` checks it while compiling. The prototype gap is 1,728 blocks, so 999
fits under that assertion. The wallet's `REWIND_DISTANCE` and `CHECKPOINTS_N` derive from the
constant. zebra-chain has a separate constant of 1000, and comments at
`zebra-state/src/request.rs` and `non_finalized_state.rs` say 1000. The non-finalized state holds
up to that many blocks per chain in memory. The depth commit is the second floor under `fin`
(FINALITY.md §8.1); a larger value makes the conflict hold and stage 9's second state rarer, and
is not the difference between recovering and needing a resync.

No implementation stage changes the constant. Increasing it is a separate policy change.

To decide: those bootstrap heights, and which stage carries the change once they are chosen.

### 7. Stage 9's mechanism

[`STAGE9_OPTIONS.html`](./STAGE9_OPTIONS.html) draws the three mechanisms and six scenarios.

| | A. no commit past `fin` | B. restore point and rewind | C. second database |
|---|---|---|---|
| How | the committed tip stops at `fin`, so both branches stay in the non-finalized state | one database; a checkpoint at or below `fin`; a decision that cannot be attached makes the node rewind to it, replay its own blocks to the fork, and take the BFT branch as an ordinary fork | each branch gets its own committed database (stage 9 as written above) |
| Memory grows during | any finality stall | a decided conflict only | never |
| Can a peer trigger it? | no | no: the trigger carries ≥⅔ of stake's signatures | no, if triggered by a decision |
| New code | small: remove the cap | medium: checkpoint, rewind, fetch the fork | large: two states, routing, role swap |
| FINALITY.md §4.3 | breaks "the depth commit is never held back indefinitely" | breaks none | breaks none |

Under B, the conflict hold keeps both branches in memory while the conflict is live. If the PoW
branch stays heavier past the hold's cap, the node commits it and drops the BFT branch but keeps
the restore point, and rewinds again once the BFT branch's headers show more work. B removes the
permanence of an unattachable decision, not the wait: finality still waits until the decided
branch is the heavier one (FINALITY.md §3.4).

To decide: A, B or C. B needs one thing settled that the drawing leaves out: the restore point
predates the bft rows and the `fin` row written after it, so a rewind has to carry them forward.
Choosing C needs answers to the following, none of which the stage text gives.

- **`fin` passing the fork on C's side.** "C is dropped when `fin` passes the fork" covers `fin`
  passing on P's branch. If the served chain is C's and `fin` advances past the fork there, P can
  never hold `fin` again. Either C becomes the primary state (depth commit, `fin` row, snapshot
  source) and P the side state that keeps recording its branch, or P is dropped.
- **Where `fin` and the BFT rows live.** The decided bft-chain rows and `fin` are in P's
  finalized database. C is opened from a copy of it and starts with its own copy of those rows,
  which is the two-stores pitfall of FINALITY.md §8.1. The stage has to name the authoritative
  database, where C's directory lives, whether C is reopened at startup, and whether C's
  non-finalized state gets a backup.
- **Which state writes.** `crosslink_update_fin` and `crosslink_conflict_hold` are methods on the
  one writer that owns one `finalized_state` and one `non_finalized_state`, and `bft_chain()` and
  `fin()` are process-wide statics in `new_network`.

### 8. Wallet rewind below `MAX_BLOCK_REORG_HEIGHT`

Stage 9 says the wallet's `REWIND_DISTANCE` and `CHECKPOINTS_N` cover a switch back to `fin`.
Both are constants derived from `MAX_BLOCK_REORG_HEIGHT`, and `fin` can lag any distance
(FINALITY.md §3.3), so no constant covers it.

To decide: the wallet rescans from a height when a reorganization is deeper than its
checkpoints, or FINALITY.md states a bound.

### 9. How often the snapshot is retaken

"Retaken as `fin` advances" is about once per block while BFT is live. A hard-linked checkpoint
can afford that; the portable fallback, a logical copy of the whole database, cannot. A staler
snapshot costs only replay time.

To decide: the retake rule. STAGE9_OPTIONS.html assumes every 100 blocks of `fin`, deleting the
previous one. Where the storage engine offers no cheap clone, the alternative is a permanent
second copy that lags P at `fin`: every finalized batch is written twice, and a conflict costs
nothing to open.

### 10. The harness for stage 9's test

The done condition needs one node held on a PoW fork more than `MAX_BLOCK_REORG_HEIGHT` blocks
long while the other finalizes alone. `dilated_regtest/run.sh` can stop and restart nodes but
cannot partition them. The node-test format would need a fork of 100 or more blocks plus
decisions on the other branch. The done condition also has no restart in it, although the
regtest restarts both nodes.

To decide: which harness carries the test and what it gains to do so, and whether a restart
during a live conflict is part of the done condition.

### 11. One writer thread, or one logical authority

FINALITY.md §7.1 and the finished stages require one authoritative state, coherent chain views,
and serialized consensus mutations. They meet that by running proposal, validation, decisions,
bootstrap, persistence and `fin` on the one `new_network::sync` thread. That couples BFT
responsiveness and liveness to PoW synchronization, and gives one loop several unrelated jobs.

The requirement is narrower than the implementation: related reads use one versioned snapshot,
and mutations that cross domains pass through one serialized authority. A dedicated BFT actor,
or atomic state-service operations, meets it without the shared thread.

To decide: whether FINALITY.md §7.1 states the requirement in that form. If it does, stage 9's
routing between two states and the remaining moves in CRATE_REMOVAL.md are written against it
rather than against the sync loop.
