# Crosslink finality implementation plan

[`FINALITY.md`](./crosslink_book/src/FINALITY.md) defines the behavior. This file orders the work that brings the
code to it. Each stage names the FINALITY.md sections it implements, the code it touches, and
the condition under which it is done. Where this file and FINALITY.md disagree, FINALITY.md is
right and this file is corrected.

Stages 1 to 8 are done, so `zebra-crosslink/zebra-crosslink` holds no finality state and
answers no finality question. Their text is in this file's history
(`git show 30ac270a:IMPLEMENTATION.md`).
No stage is open.
Removing what is left of the crate is separate work, in
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

## Needs design pass

A design session reads the cited FINALITY.md sections, re-checks the code facts listed, asks the
user, and records each answer in FINALITY.md as a Zebra Crosslink requirement. It then updates
the stage the question blocks and deletes the question. Question numbers are stable: an answered
question is removed and its number is not reused. No question blocks a stage; 11 changes how
CRATE_REMOVAL.md is written.

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
(FINALITY.md §8.1), and the depth past which a node drops BFT rather than follow a decision
(FINALITY.md §4.3); a larger value makes that rarer.

No implementation stage changes the constant. Increasing it is a separate policy change.

To decide: those bootstrap heights, and which stage carries the change once they are chosen.

### 11. One writer thread, or one logical authority

FINALITY.md §7.1 and the finished stages require one authoritative state, coherent chain views,
and serialized consensus mutations. They meet that by running proposal, validation, decisions,
bootstrap, persistence and `fin` on the one `new_network::sync` thread. That couples BFT
responsiveness and liveness to PoW synchronization, and gives one loop several unrelated jobs.

The requirement is narrower than the implementation: related reads use one versioned snapshot,
and mutations that cross domains pass through one serialized authority. A dedicated BFT actor,
or atomic state-service operations, meets it without the shared thread.

To decide: whether FINALITY.md §7.1 states the requirement in that form. If it does, the
remaining moves in CRATE_REMOVAL.md are written against it rather than against the sync loop.
