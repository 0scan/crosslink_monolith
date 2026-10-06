# Tenderlink and STP: open work

Everything outstanding on the BFT engine (`tenderlink/src/lib.rs`) and its transport
(`tenderlink/src/stp.rs`). Code is named by symbol, because line numbers drift.

| # | topic | state |
|---|---|---|
| 1 | Out-of-range proposal chunk index crashes the receiver | not fixed |
| 2 | Gossip scheduler replacing the resend storm | written, uncommitted, unreviewed |
| 3 | Receive-side overload and backpressure | hard cap and duplicate skip shipped; the rest waits on a measurement |
| 4 | Line 28 checks the cited round | fixed and deployed (`b150bec0`); expert review not recorded |

## 1. An out-of-range proposal chunk index crashes the receiver

Found 2026-09-23 as a test panic and confirmed by reading the code. Not reproduced over the network.

A proposal chunk packet carries its own `chunk_i` and `proposal_size`. Nothing checks that `chunk_i`
lies inside the proposal.

1. **Receive loop, the `PACKET_TYPE_PROPOSAL_CHUNK` branch:**
   `chunk_size = min(PROPOSAL_CHUNK_DATA_SIZE, proposal_size - chunk_i * PROPOSAL_CHUNK_DATA_SIZE)`.
   A `chunk_i` past the end of the proposal underflows the subtraction.
   - Without overflow checks (release, which the nodes run) the value wraps, `min` turns it into a
     full chunk size, the length check on the next line passes, and the packet reaches
     `check_and_incorporate_msg`.
   - With overflow checks (the `dev` profile) the subtraction panics before any signature check, so
     **any connected peer** can crash such a build.
2. **`check_and_incorporate_msg`:** `round_data.proposal_sigs[chunk_i]` indexes past the end and
   panics. The chunk's signature is checked before this point, so in release builds only that
   round's proposer gets here, but **that proposer can crash every node it reaches.**

The build uses `panic = "abort"`, so a panic kills the node. The duplicate-chunk shortcut is not
affected: it tests `chunk_i < round_data.proposal_sigs.len()` before indexing.

**Fix.** Drop the packet on receive unless `chunk_i * PROPOSAL_CHUNK_DATA_SIZE < proposal_size`.
That removes both panics and the underflow. A size check in `check_and_incorporate_msg` as well
protects every other caller.

**Test.** Build a validly signed chunk for a 2-chunk proposal with `chunk_i = 5` and pass it to
`check_and_incorporate_msg`. It should return `Fail` and leave the round data unchanged.

## 2. Gossip scheduler

### The problem it replaces

`send_round_data_to_peer` re-sent every round at the current height to every peer on every tick,
with no memory of what it had sent and no rate limit. Rounds at a height are pruned only on commit,
and the round timeout backs off linearly, so a height stalled for time `t` holds about `√t` rounds:
roughly 44 after an hour, 235 after a day, 630 after a week. In the September 2026 featurenet stall
this measured about 12.3 MB/s of inbound BFT traffic per node (8,864 packets/s) against 0.1 MB/s of
block sync, and with an unbounded receive queue it killed nodes repeatedly. That this loop produced
the traffic was never confirmed by decoding a capture.

### What is in the working tree

An uncommitted change to `tenderlink/src/lib.rs` (+374 −208) removes `send_round_data_to_peer` and
sends through per-peer cursors:

- Three focus cursors per peer: our round, the peer's round from its last status, and the
  `valid_round` our proposal cites. They share `GOSSIP_BYTES_PER_PEER_TICK` (20 KB) less the repair
  share.
- One repair cursor per peer that walks every other round at the height in turn, with
  `GOSSIP_REPAIR_BYTES_PER_TICK` (3 KB) plus whatever the focus rounds left unused.
- A round whose content has not changed is re-sent only every `GOSSIP_REFRESH_TICKS` (4) ticks. A
  `GossipFingerprint` of its proposal and vote counts detects change.
- A peer asking for an older height is served that height's committed round, without prevotes.
- Peers that are neither validators nor catching up get nothing.
- `bft_update` runs again after a receive pass that incorporated new evidence, and the loop yields
  instead of sleeping when messages arrived.
- Tests: `gossip_chunks_continue_while_votes_change`,
  `gossip_unchanged_round_refreshes_after_four_ticks`, `gossip_chunk_header_keeps_status_bytes`,
  `gossip_repair_cursor_reaches_every_nonfocus_round`.

It compiles (`pheck.bat zebra-crosslink`). Its tests have not been run, and nobody has reviewed it.

### What the review has to check it against

These were settled on 2026-09-23 and are the requirements, whatever the code does today.

1. **A hard byte budget per peer per tick**, so no consensus bug can flood the network again.
2. **Every old round stays reachable.** A node may hold round `r`'s proposal while others hold its
   precommits, and without the proposal a peer cannot decide on `r`. The repair cursor covers all
   rounds at the height on a fixed budget, and may visit rounds holding value precommits more often.
   *Today it is a plain rotation.*
3. **Decisive votes and status go before proposal chunks and repair traffic.** STP's send queue
   shuffles and drops oldest, treating everything alike, so the ordering has to be made here.
   *Today a cursor sends a round's chunks first, then precommits, then prevotes. Every packet does
   carry the status.*
4. **Round data is signed items, not per-round streams.** `PacketVotes` repacks up to 18 signatures,
   so packet boundaries are not item boundaries. Back off per item. *Today the backoff is per
   round.*
5. **Status need-ranges are hints, never proof of what a peer holds.** As sent today they mislead:
   `need_vote_rngs` defaults to `[0, roster_n]` and is overwritten only when something is missing,
   so a peer holding every value vote says "need all"; it looks only at value-vote slots, not nil
   slots; and `gen_mostly_empty_rngs` lets single filled gaps through. An exact `[0, roster_n]` from
   an old peer means "unknown". *The scheduler ignores the ranges, which is safe. The misleading
   default in the statuses new nodes send is not fixed.*
6. **A transport ACK means "received by STP"**, never that the application has the item. It may
   bring the next resend decision forward. It never suppresses repair for good.

Line 28 reads round `vr` (section 4), so that round's prevotes must be deliverable. That is why
the cited `valid_round` is a focus round.

### Still to do

- Review the diff against the six points, then run the `gossip_` tests:
  `phest.bat tenderlink Debug Win64 gossip_`. Never run the suite unfiltered; `single_rt` is
  long-running.
- Measure on a stalled height: the `receive loop processed … messages/s` line the node prints every
  10 s, before and after. `PRINT_SENDS` and `PRINT_SEND_CS` report per-tick send counts.
- Commit it apart from everything else in `lib.rs`.

## 3. Receive-side overload and backpressure

### What is in the tree

- **A hard cap.** `MAX_RECV_QUEUE_BYTES` (64 MiB) bounds the application-facing receive queue. Past
  it messages are dropped and counted, and the node prints the queue's peak and drop counts
  (`45ac515f`). The cap drops *after* STP has ACKed, so the sender never learns it is overrunning
  the receiver.
- **Exact duplicates skip the signature check** (`25b12d36`). A vote or chunk byte-identical to one
  already verified and stored costs almost nothing. During a stall almost everything inbound is a
  duplicate, so this may be most of the fix by itself; that is unmeasured.

### Decided

1. Transport receipt is not application acceptance. Do not delay an ACK until the application has
   consumed the message: the ACK bit is set before fragments are parsed, one datagram carries
   several messages, the ACK bitmap is also the replay check, there is no durable per-message
   handle, and an ACK later than the loss timeout is ignored, so under load the sender would never
   see "acked" and would keep resending. The mechanism fails exactly when it is needed.
2. **Withholding ACKs does not stop the sender.** STP has no retransmission, and its loss loop
   retires in-flight packets without an ACK, so the send window reopens by itself. A peer that sees
   no ACK for 15 seconds drops the connection.
3. The order of work is: duplicate skip (done), the scheduler of section 2, and only then, **if a
   mixed-version measurement still shows overload**, admission drops before the ACK. Old senders
   never get the new scheduler, so pre-ACK drops are the only step that feeds congestion back to
   them, as loss their CUBIC already understands, with no peer cooperation and no wire change.
4. Pre-ACK admission drops are deliberate loss, not flow control. They report application overload
   as path congestion and can slow fresh votes along with duplicates. If built:
   - a fair share per connection, with a minimum accepted rate so ACKs and RTT samples keep flowing;
   - deliberate drops reported to the application and counted apart from path loss, on both sides;
   - the 64 MiB cap stays as the backstop for a peer that does not cooperate.
5. Later and optional, for upgraded pairs only: packet type 10 carrying session-scoped
   `HAVE`/`WANT` and receive credit, enabled only after receiving it from the peer. Old nodes ignore
   unknown types. `HAVE` means validated and incorporated. Do not change `MAGIC2_APP_CROSSLINK` or
   renumber the vote types. The magic2 negotiation is a list, so offering `[V2, V1]` is the
   backwards-compatible route for a future STP-level change.
6. Later and optional: STP reports per-message ACK and loss to the sender as a faster "maybe
   delivered" signal.
7. ECN echo on queue pressure is at most a hint, counted apart from real congestion. NewNet needs
   no new transport mechanism; it uses its existing request path.

### Constraints on anything built here

- **Pressure is global and the remedy must be per peer.** The receive queue is one queue for all
  connections. Throttling every peer because one floods punishes the honest ones, so per-peer byte
  accounting comes first.
- **The measure underestimates.** A batch leaves STP's byte count when the application takes it, so
  a per-connection measure sees only what arrives while the application is busy. Size thresholds
  knowing it can be off by about 2×.
- **Measure standing queue, not instantaneous length.** A long queue that drains is fine and a short
  one that never empties is not. The send side already reasons this way:
  `UnreliableSendBuffer::current_byte_capacity` is in microseconds of data at the current rate.
- **Throttling consensus traffic can entrench the stall that caused it.** The flood happens because
  BFT is stuck. Dropping the vote that would resolve the height turns a crash into a hang. Keep a
  floor below which a peer is never throttled, and let votes outrank proposal chunks.
- **Layering.** The network thread owns the queue and the consumer owns the drain rate. Whatever is
  measured has to be readable from the network thread without a lock on the consumer's path; the
  exchange is a `mem::swap` under an atomic state word and it is hot.
- **Judge any design against the send side.** `UnreliableSendBuffer` is already a bounded queue with
  explicit byte accounting, capacity sized to a latency target, and `cut_to_size` dropping from the
  front. STP also already has CUBIC rate control, a BDP-derived in-flight window, RTT estimation, ACK
  pacing, ECN receive and echo, app-limited detection, and path-MTU probing. The receive side never
  got the same treatment.

### A byte budget is not a CPU budget

Not fixed, and not covered by anything above. The receive loop accepts vote packets from any peer
past the Noise handshake, and checks each vote against the roster key the vote names, not against
who sent it. A vote with a bad signature is never stored, so it is verified in full every time it
arrives (one Ed25519 check, roughly 50 to 200 µs). Nothing counts the failures or disconnects the
sender. A peer sending current-height votes with junk signatures, 18 to a 1,240-byte packet, costs
about a millisecond of receive-loop time per packet; about 1,000 packets/s, roughly 1.2 MB/s,
saturates the loop. Honest traffic backs up behind it and is dropped at the cap. Nothing has to
stall for this to work, and byte accounting will not flag it.

The remedy charges failed signature checks to the sending peer and disconnects or throttles it past
a limit, or accepts votes only from identified validator peers.

### Open

- The mixed-version stall test that decides point 3. It has to cover useful evidence, multi-fragment
  messages, idle and active peers, and throttling at both ends at once.
- Whether tenderlink's STP instance and `new_network`'s want different thresholds: BFT traffic is
  small and latency-critical, block download is bulk and loss-tolerant.
- Whether the reliable and unreliable channels share one pressure signal.
- Who sets `target_queue_size_us`, `min_queue_size_bytes` and `max_queue_size_bytes`, and to what;
  the receive side should probably borrow the same tuning.

## 4. Line 28

Tendermint Algorithm 1 line 28 lets a node prevote a re-proposed value `v`, whose proposal cites an
earlier round `vr`, when **round `vr`** holds prevotes for `v` from at least `big_threshold` stake.
The code tested the current round's `yes_prevotes` instead. `b150bec0`, which is deployed, counts round `vr`'s prevotes
for the exact value, on round `vr`'s roster, whether or not `vr`'s proposal was received
(`has_prevote_certificate`), and adds eleven `line28_` tests: nine of the counting and two through
`bft_update`.

### Still owed

- Expert review of the diff. It is a change to when a validator prevotes, and a bug in it is shared
  by every upgraded node. None is recorded.
- The deployed nodes run it without the scheduler of section 2, so round `vr`'s prevotes reach a
  peer only by the old resend loop.

### What the change is and is not

- **Wire format:** unchanged.
- **Finality certificate:** unchanged. The fat pointer holds precommit signatures over
  `hash || height || round|0x8000_0000`, and `FatPointerToBftBlock::validate_signatures` accepts any
  round. The fix can change *which round* a height commits in, not what a valid certificate is.
- **Nothing checks why a peer prevoted.** Votes do not carry `vr`, and the `valid_round` check in
  `check_and_incorporate_msg` is a TODO. Old and new nodes accept the same messages.
- **What changes:** from the same evidence, a fixed node can prevote yes where an old node prevotes
  nil.
- **No double votes.** Lines 22 and 28 are mutually exclusive (`vr == -1` against `vr >= 0`), and
  `step == Propose` allows one prevote per round.
- **Not a hard fork, as far as was found.** The Crosslink verifier did not get a full upgrade audit.

Mixed networks:

- **Safety.** Under the paper's assumptions, an old node's nil vote and a new node's yes vote do not
  by themselves give a path to conflicting decisions. Restart and persistence, roster changes and
  proposal equivocation were not audited.
- **Liveness while partly upgraded.** No path was found where a mixed network does worse, but timing
  differs: a round may wait for the prevote timeout instead of precommitting nil at once.
- **When the benefit starts.** In the plain bootstrap case, once at least 2/3 of stake runs the fix.
  Byzantine or other current-round yes votes can move that.

### What the bug cost

A liveness risk, not a proof of a stuck height. The wrong-round check keeps a valid-round
re-proposal from unlocking honest validators. Where locks are incompatible and no old-round commit
certificate is already signed, later proposals can keep timing out. A fresh proposal (`vr = -1`)
helps only validators whose locks permit its value, so locks split across different values can
leave every fresh value short. Restart is not the only way out.

It was probably not the cause of the September 2026 featurenet stall, which looks like finalizers
not taking part. The evidence neither confirms nor excludes it. Logging `proposal_valid_round`,
whether the cited certificate is present, line-28 entries, timeouts and lock state would settle it.

### Why the bug looks like liveness and not safety

A bounded argument, not a proof.

- **Different value.** Once a round's proposal is known, votes for any other value are rejected, and
  votes stored before it was known are purged when the first chunk arrives. The branch also needs
  the full proposal. So when it ran, `yes_prevotes` counted only votes for that proposal's value.
- **Same value.** The wrong condition needed 2f+1 current-round prevotes for `v`. With `vr ≥ 0` no
  honest node can cast the first one: line 22 needs `vr == -1`, and line 28 needed the quorum to
  exist already. That leaves at most f Byzantine votes.
- **Equivocating proposer.** The proposer sends `vr = -1` to some nodes and `vr ≥ 0` to others.
  Honest nodes prevote `v` by line 22 only if unlocked or locked on `v`. If some `w ≠ v` was decided
  earlier, at least f+1 honest nodes are locked on `w`. That leaves at most f unlocked honest nodes
  plus f Byzantine ones: 2f, short of 2f+1.

Lines 36 and 49 are value-bound once a full valid proposal is present: the first chunk purges
other-value votes, later ones are rejected, and a second proposal value is a fault. A restart that
forgets a node's lock and signed votes breaks the locked-honest-node assumption for every Tendermint
rule, not only this one; the amnesiac-proposer flush is that known limitation.
