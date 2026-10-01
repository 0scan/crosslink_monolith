# The rejected-block backoff in `new_network::sync` is keyed by block hash alone and a forged body does not get its server disconnected, so each forged copy a peer manages to deliver under an honest block's hash stops new requests for that hash to every peer for 1 s doubling to 64 s

**Severity**: Low
**Validation Status**: Partially confirmed
**Location**: `zebra-crosslink/zebra-state/src/new_network.rs:72-75` (`REJECTED_BLOCK_BACKOFF_MIN`, `REJECTED_BLOCK_BACKOFF_MAX`), `:1492-1510` (`Rejection`, `note_rejection`, `is_backing_off`), `:1582` (`rejections: HashMap<Hash, Rejection>`), `:2156-2158` (prune at `retry_at` plus the max), `:2175` (`MAX_REQUEST_DUPLICATES_N` = 2), `:2224-2226` (peers shuffled per pass), `:2270-2291` (by-hash loop, backoff gate at `:2275`), `:2445-2484` (near-tip loop, queue skip at `:2456`, backoff gate at `:2460`), `:2807-2810` (a chunk needs a pending download at that peer), `:2906-2910` (chunk dropped and slot removed while the hash is queued), `:2984-2988` (hash must match), `:3105-3108` (queued block waits for its parent), `:3132-3141` (header, then body), `:3282-3297` (`note_rejection` on any failure, kill only on a header failure); `zebra-crosslink/zebra-consensus/src/sync_verify.rs:89-121` (`block_check_header`), `:131-180` (`block_check_body`, merkle at `:147`); `zebra-crosslink/zebrad/tests/crosslink.rs:1531-1600` (`crosslink_honest_block_accepted_after_forged_body_with_its_hash`)
**Found by agent:** "crosslink spreadsheet triage" session (Claude), 2026-10-01, while reading commit e56e35f8; validated 2026-10-01 at dev e56e35f85a98f
**In scope of audit?** Yes. The lead came from re-checking the test-format review against `e56e35f8`. The backoff (`Rejection`, `note_rejection`, `is_backing_off`, both constants, both gates and the prune) was introduced by `e56e35f8` on dev. The one-peer kill on a bad header came in with the same commit. ClT0 (`s1_dev`) does not carry this commit; it was not inspected further.

## Description

When a block that a peer served fails verification in the commit loop, the commit loop records the hash and, only for a header failure, queues the peer to be killed (`new_network.rs:3288-3297`). `note_rejection` adds a strike to the entry for that hash and sets `retry_at` to now plus 1 s doubled per earlier strike, capped at 64 s (`:1500-1506`). Both request loops then skip the hash for every peer while `is_backing_off` is true: the by-hash loop that serves `bft::missing_pow_blocks` (`:2275`) and the near-tip loop (`:2460`). The entry is pruned 64 s after its backoff ends (`:2158`), so a hash that fails again inside that window keeps its strike count.

The design comment states the assumption (`:1492-1494`): the block hash covers only the header, a forged body can sit under an honest hash, so the hash is put in backoff "rather than being banned", because "a ban would let any peer keep us off the honest block". The kill site adds that a bad body "may be forged under an honest block's hash, and the same peer may still serve the honest one" (`:3285-3287`).

The backoff is a time-limited ban with the same key. The table has no peer in its key, and the peer that served the failed copy stays connected and stays eligible, so:

- a forged copy delivered by one peer silences new requests for that hash to all peers, including the honest ones that advertise it;
- when the backoff ends, the retry goes to up to two peers drawn from a fresh shuffle (`:2224-2226`, `:2175`), with nothing steering it away from the peer that served the failed copy;
- each further forged delivery doubles the wait.

Before `e56e35f8` a failed block was simply removed from the queue and re-requested on the next download pass (`BLOCK_SEND_MS` = 500 ms, `:725`, `:1657`). The peer was not killed then either. So the commit raises the price of one delivered forgery from about one pass to between 1 s and 64 s.

**This is a bounded, probabilistic delay of one block on one node, and a regression in how cheaply a forged body can be turned into delay; it is not a way for a single connection to keep a well-connected node off a block indefinitely.** The backoff gates only new requests. A request already in flight at an honest peer is not cancelled, and the node asks two peers at once, so with one dishonest connection and at least one honest peer advertising the block, the honest copy normally arrives and commits. The forger has to be the only peer asked, or win a same-tick race, or hold most of the node's connections. Those cases are set out below.

## Attack Scenario and Steps

An adversary that runs one or more ordinary STP peers. No stake and no hash rate are needed.

1. **Starting state.** The victim node has several honest peers and one or more attacker connections. An honest block X is mined. The attacker holds X, so it has X's header.
2. **Advertise.** The attacker advertises X in STATUS. A served block is accepted only if the node asked that peer for it (`:2807-2810`), so the attacker has to wait to be asked. A peer cannot push a block.
3. **Be asked.** On each download pass the node shuffles its peers and requests X from at most two that advertise it (`:2224-2226`, `:2445-2484`). The attacker is asked alone if it is the only advertiser on that pass (it relays X first), or if honest advertisers have no free download slot (`:2475-2481`). Otherwise it is asked together with one honest peer, with probability about 2 / (number of advertisers).
4. **Serve a forged copy.** The attacker sends X's real header with a body it chose: any body that deserializes and whose coinbase carries X's height. The hash check (`:2984-2988`), the parent check (`:2990-2993`) and the height check (`:2999-3002`) all pass, because all three are fixed by the header and the coinbase height. The copy is queued (`:3054-3055`).
5. **Shadow the honest copy.** While the forged copy is queued, every chunk of X from any other peer is dropped and that peer's download slot is removed (`:2906-2910`). With X's parent committed, the forged copy is verified at the end of the same 100 ms tick, so this only catches honest chunks processed later in the same tick. With X's parent not yet committed, the forged copy waits in the queue (`:3105-3108`) for as long as the parent takes, and the near-tip loop does not request X at all in the meantime (`:2456`).
6. **Fail, back off.** `block_check_header` passes (the header is honest). `block_check_body` fails at the merkle check (`sync_verify.rs:147`), or, for a body whose transaction ids are intact but whose signatures or proofs were altered, `block_verify_expensive` fails (`new_network.rs:3212-3216`). The verdict phase is "body" or "expensive", so the peer is not killed (`:3292-3296`), and `note_rejection` puts X in backoff (`:3289-3291`).
7. **Outcome.** If an honest download of X is still in flight and was not caught in step 5, it completes and X commits: no delay beyond the attacker's own. If not, no peer is asked for X until `retry_at`. On the retry, steps 3 to 6 repeat with the wait doubled.

**Attack Requirements and Assumptions:**

- Any peer the node will sync from. The checkpoint gate (`:2254-2265`) must already be passed by that peer.
- The attacker must be asked for X. It cannot inject unrequested blocks.
- For a re-arm, the attacker must again be asked and again leave no surviving honest download. With one attacker connection among n honest advertisers this happens with probability about 2q / (n + 1) per round, where q is the chance of winning the same-tick race in step 5 (inference; q was not measured).
- With k attacker connections the chance that both requests go to the attacker is k(k - 1) / ((n + k)(n + k - 1)). The connection key is IP, port and 15 key bits (`tenderlink/src/stp.rs:383-390`), and no cap on the number of connections was found by searching `new_network.rs` and `stp.rs` (inference: the inbound accept path was not traced line by line).
- Not required: an invalid header, proof of work, or roster membership.

## Impact on Users

The operator of the targeted node bears the delay; the network as a whole is not affected.

- **Tip blocks.** A node whose attacker peer advertises X first receives X about 1 s later than before `e56e35f8` (one strike), plus up to one 500 ms pass and one 300 ms request interval. A miner on that node works on a stale tip for that long.
- **Catch-up sync.** Commits are serial, so every block the attacker wins stalls everything above it. Each win now costs at least 1 s where it cost about one pass before. This is arithmetic from the constants, not a measurement.
- **Sustained delay needs most of the connections.** Strikes run 1, 2, 4, 8, 16, 32, 64, 64 s: 63 s for six consecutive wins, then 64 s per win. With a per-round success chance r the expected delay per block is the sum of r^k times the k-th wait. For r = 0.1 that is about 0.1 s. For r = 0.75 (for example 40 attacker connections against 8 honest peers) it is about 50 s per block, against about 2.4 s for the same attacker before the commit (inference from the constants; not run).
- **Finalizers.** The same gate covers the by-hash requests for BFT snapshots (`:2275`). A snapshot block in backoff is not requested, `validate` stays `Indeterminate` for that proposal, and the finalizer prevotes nil for rounds that end inside the wait (inference from `bft.rs:107-116` and the consensus report cross-referenced below; not run). The by-hash loop picks two peers at random without checking that they advertise the block, so an honest peer that lacks the block can take the second request.
- **Honest failures are also backed off.** `note_rejection` runs for every non-deferred failure of a peer-served block, including a crosslink `Reject` and a contextual commit error (`:3273-3282`). A transient commit error on an honest block now waits out the same backoff.

**Detectability, stated precisely.** The node prints `Failed to commit <hash>: body: ...` or `expensive: ...` (`:3313`) and a `warn` naming the peer for each dropped duplicate chunk. Nothing logs that a hash is in backoff or which peer delivered the failed copy, so an operator sees repeated failures of one hash but not their source.

## Technical Details / Code Analysis

**The table and its key** (`new_network.rs:1492-1510`):

```rust
// A peer-served block that failed verification. Its hash goes unrequested until `retry_at` rather
// than being banned: the hash covers only the header, and a forged body or forged signatures can
// sit under an honest block's hash, so a ban would let any peer keep us off the honest block.
struct Rejection {
    strikes: u32,
    retry_at: std::time::Instant,
}

fn note_rejection(rejections: &mut HashMap<Hash, Rejection>, hash: Hash) {
    let now = std::time::Instant::now();
    let rejection = rejections.entry(hash).or_insert(Rejection { strikes: 0, retry_at: now });
    rejection.strikes += 1;
    let backoff = REJECTED_BLOCK_BACKOFF_MIN.saturating_mul(1 << (rejection.strikes - 1).min(16));
    rejection.retry_at = now + backoff.min(REJECTED_BLOCK_BACKOFF_MAX);
}

fn is_backing_off(rejections: &HashMap<Hash, Rejection>, hash: Hash) -> bool {
    rejections.get(&hash).is_some_and(|rejection| std::time::Instant::now() < rejection.retry_at)
}
```

The peer that delivered the block is known at the call site (`delivered_by`) but is not stored.

**Who is recorded and who is killed** (`new_network.rs:3285-3297`):

```rust
// Only a bad header gets the peer killed: the hash commits to the header and nothing
// else, so the peer advertised a block that cannot exist. A bad body may be forged
// under an honest block's hash, and the same peer may still serve the honest one.
if let Some(key) = delivered_by {
    if failed {
        note_rejection(&mut rejections, hash);
    }
    if let Err(("header", err, _)) = &verdict {
        if err.misbehavior_score > 0 {
            peers_to_kill.push((key, format!("served block {hash} with an invalid header: {}", err.msg)));
        }
    }
}
```

`failed` is true for every `IngestOutcome::Failed` (`:3282`): the "header", "body", non-deferred "crosslink" and "expensive" phases, and a commit error from `handle_commit`. Blocks that entered through `submit_block_to_new_network` have `delivered_by: None` (`:1805`), so the RPC, the miner and the inbound gossip downloader never add a strike.

**Both gates apply to all peers** (`new_network.rs:2274-2277` and `:2456-2462`):

```rust
let dups = requests_by_hash.entry(hash).or_insert(0);
if *dups >= MAX_REQUEST_DUPLICATES_N || is_backing_off(&rejections, hash) {
    continue;
}
```

```rust
if blocks_to_commit.iter().any(|(block_hash, _)| *block_hash == hash) {
    if TRACE { tracing::info!("Skipped requesting block already in our queue. Peer {connection_address:?}. Hash: {hash}"); }
    continue;
}
if is_backing_off(&rejections, hash) {
    continue;
}
```

Neither loop cancels a download that is already in a peer's `block_downloads`, which is why an in-flight honest request survives a strike.

**A queued copy shadows every other copy of its hash** (`new_network.rs:2906-2910`):

```rust
if blocks_to_commit.iter().any(|(queued_hash, _)| *queued_hash == alleged_hash) {
    drop_block!(alleged_hash, "Block was already queued to commit!: {alleged_hash}");
    peer.block_downloads.remove(dl_i);
    continue 'process_packets;
}
```

This runs on every chunk, not only the last one, and predates `e56e35f8`. The queued copy has not been checked against the merkle root at this point: `block_check_body` runs only in the commit loop, after the parent is known (`:3105-3108`, `:3132-3134`). The receipt path still carries `// @Todo(Phil): Semantic verification.` (`:3038`).

**What a forged body hits** (`sync_verify.rs:89-121`, `:131-180`). `block_check_header` reads only the header (difficulty, Equihash, time), so it passes for an honest header. `block_check_body` recomputes the transaction hashes and calls `check::merkle_root_validity` (`:145-148`), which rejects any change to the transaction ids. A change that leaves the ids intact (the existing test flips one byte of a transparent unlock script, `crosslink.rs:1510-1529`) passes the body phase and fails in `block_verify_expensive`. Whether an altered body that also keeps every signature valid would fail at the authorizing-data commitment inside `handle_commit` was not traced.

**Other sources of block X.** `is_crosslink_testnet` is a constant `true` (`zebra-chain/src/parameters/network.rs:39`), and with it the legacy syncer task is replaced by a pending future (`zebrad/src/commands/start.rs:860-866`), so `sync/downloads.rs` is not live. `inbound/downloads.rs:398` and `zebra-rpc/src/methods.rs:3848` submit through the doorway at `new_network.rs:1773-1806`, which checks neither backoff nor adds strikes. Those paths fire when someone else offers the block; none of them is a way for the node to ask for X while X backs off.

## Recommendations

1. **Key the backoff by hash and peer.** Change `rejections` to `HashMap<(Hash, ConnectionKey), Rejection>`, pass `key` at `:3290`, and pass `connection_key` at `:2275` and `:2460`. The peer that served the failed copy is not asked for that hash again until its own wait ends; every other peer is asked on the next pass, as before `e56e35f8`. The point of the backoff is kept (no tight re-fetch loop against one peer) and a forged delivery no longer buys any wait at other peers. A reconnect under a new key gets a fresh entry, which is no worse than the behaviour before the commit. Cost: a block that is invalid for everyone is fetched once per advertising peer per wait, not once overall.

   ```rust
   fn note_rejection(rejections: &mut HashMap<(Hash, ConnectionKey), Rejection>, hash: Hash, served_by: ConnectionKey) {
       let now = std::time::Instant::now();
       let rejection = rejections.entry((hash, served_by)).or_insert(Rejection { strikes: 0, retry_at: now });
       rejection.strikes += 1;
       let backoff = REJECTED_BLOCK_BACKOFF_MIN.saturating_mul(1 << (rejection.strikes - 1).min(16));
       rejection.retry_at = now + backoff.min(REJECTED_BLOCK_BACKOFF_MAX);
   }

   fn is_backing_off(rejections: &HashMap<(Hash, ConnectionKey), Rejection>, hash: Hash, peer: ConnectionKey) -> bool {
       match rejections.get(&(hash, peer)) {
           Some(rejection) => std::time::Instant::now() < rejection.retry_at,
           None => false,
       }
   }
   ```

2. **Bind the body to the header before the block takes the queue slot for its hash.** Run the merkle check at receipt, after the hash check at `:2984-2988` and before `blocks_to_commit.push` at `:3054`, and treat a mismatch as a rejection of that peer's copy (strike for that hash and peer, block not queued). A merkle-mismatched body then never shadows the honest copy at `:2906-2910` or at `:2456`, including while the parent is missing. This is the `@Todo` at `:3038`. It does not cover a body with intact transaction ids and altered signatures or proofs, which still needs the commit-time checks; item 1 bounds that case. Decide separately whether `block_check_header` moves to receipt as well: its time check uses the current time, so a header that is too far in the future at receipt would be judged earlier than it is today.

3. **Alternatives considered.**
   - *Kill the peer on a body failure:* the body phase also holds the subsidy rules (`sync_verify.rs:160-170`), where an honest peer on a different rules version would be killed. It could be narrowed to the merkle mismatch alone, which no honest node can produce for a block it committed, but item 1 already removes the benefit of serving one.
   - *Keep the hash key and steer the retry away from the last server:* needs the last server stored per hash, which is item 1 with a smaller table and the same call sites, and it forgets the earlier server once a second one fails.
   - *Shorten the constants:* reduces the leverage but keeps one peer's forgery delaying every peer.
   - *Remove the backoff:* restores the previous behaviour, including the tight re-fetch loop the commit set out to stop.

4. **Tests.** Extend `crosslink_honest_block_accepted_after_forged_body_with_its_hash` (`zebrad/tests/crosslink.rs:1531`) or add a sibling scenario with two synthetic peers. Drive: `RECV_POW` of the merkle-forged copy from peer 0 six times, each `SHOULD_FAIL`; then `RECV_POW` of the honest block from peer 1; then `expect_pow_chain_length`. Assert: the honest block is committed. At `e56e35f85a98f` the sixth strike sets a 32 s wait, longer than the harness's `NODE_ANSWER_WAIT` of 30 s (`zebra-crosslink/zebra-crosslink/src/test_format.rs:1200`), so the honest `RECV_POW` should report that the node sent no request (inference; not run). With item 1 the node asks peer 1 on the next pass. The six forged deliveries take about 31 s of wall time either way, because peer 0's own wait still doubles. For item 2, add a forged copy from peer 0 whose parent has not been delivered, then the honest copy from peer 1, then the parent, and assert the chain grows by two. A thin unit test of `note_rejection` and `is_backing_off` is reasonable, since they are pure.

5. **Rollout.** Items 1 and 2 change request scheduling and the point at which an existing check runs. No block's final verdict changes, so there is no consensus or validity-rule change, and nodes can upgrade one at a time. No `s1_dev` backport is needed unless `e56e35f8` is merged there.

## Validation Information

**Verdict: PARTIALLY CONFIRMED. Severity: Low.**

Read only, at dev `e56e35f85a98f`, with `git show e56e35f8` and `git show e56e35f8^:zebra-crosslink/zebra-state/src/new_network.rs` for the earlier behaviour. Nothing was built or run.

| Claim | Verified at |
|-|-|
| The backoff table is keyed by hash alone | `new_network.rs:1500`, `:1508`, `:1582` |
| Wait is 1 s doubling to a 64 s cap; the entry lives 64 s past `retry_at` | `:74-75`, `:1503-1505`, `:2158` |
| Every non-deferred failure of a peer-served block adds a strike | `:3232-3236`, `:3282`, `:3288-3291` |
| Only a "header" failure with a positive score kills the peer | `:3292-3296`, `:3350-3355` |
| Both request loops skip a backing-off hash for every peer | `:2275`, `:2460` |
| A served chunk needs a pending download at that peer; hash, parent and height must match | `:2807-2810`, `:2984-3002` |
| At most two peers hold a request for one hash; peers are shuffled per pass | `:2175`, `:2224-2226`, `:2473-2477` |
| A strike does not cancel downloads already in flight | no removal in `:3288-3297` or `:2156-2158` |
| A queued copy causes other copies' chunks to be dropped and their slots removed | `:2906-2910` |
| A queued copy is not requested by the near-tip loop and waits for its parent | `:2456-2459`, `:3105-3108` |
| A forged body with changed transaction ids fails in the body phase | `sync_verify.rs:145-148` |
| Submissions never add a strike and are not gated by the backoff | `new_network.rs:1773-1806` |
| The legacy syncer is not spawned on crosslink networks | `zebrad/src/commands/start.rs:860-866`; `zebra-chain/src/parameters/network.rs:39` |
| Before the commit a failed block was dropped with no wait and no kill | `e56e35f8^` `new_network.rs:3168-3206` (no `peers_to_kill`, no `rejections`) |
| The behaviour was weighed by the author | comments at `:1492-1494`, `:3285-3287`; test doc at `crosslink.rs:1531-1533` naming upstream zebra PR 11052 |
| The same-tick race can be won by a timed reply | inferred from `:2906-2910` and `IDLE_MS` = 100 (`:736`); not run |
| No cap on connection count | inferred: nothing found by search in `new_network.rs` or `tenderlink/src/stp.rs`; accept path not traced |
| A snapshot block in backoff leaves `validate` `Indeterminate` | inferred from `:2270-2291` and `bft.rs:104-116`; not run |

**Severity justification.**

*Why not Medium:* the effect is delay on one node, with no safety or fund impact, and it ends on its own. A single attacker connection among honest peers cannot re-arm reliably, because the second request goes to an honest peer and is not cancelled by the strike. Sustained delay needs the attacker to hold most of the node's connections, at which point it could also delay the node before `e56e35f8`, by a smaller constant. If the connection count is confirmed to be uncapped and cheap to fill, the roughly twentyfold amplification for such an attacker is the argument for revisiting this as Medium.

*Why it is still worth a report:* the commit's own comment rejects a ban by hash because any peer could then keep the node off the honest block, and the backoff is that ban with a timer. It raises what one delivered forgery buys from about 0.5 s to as much as 64 s, it leaves the forging peer connected and eligible, it applies to the by-hash requests that BFT validation depends on, and the existing regression test passes because it never delivers a second forgery.

**Corrections made during validation.**

1. "While it backs off the node requests that hash from no peer, including the honest ones": true for new requests only. A request already in flight at an honest peer continues, and the node asks two peers per hash, so one dishonest connection normally loses to the honest copy.
2. "One peer could delay an honest block for every peer, possibly indefinitely": not with one connection among several honest peers. Each round needs the attacker to be asked and to leave no honest download alive, and the expected run of wins is short. Indefinite delay needs close to all of the node's connections.
3. "The peer is disconnected only when the header is invalid": correct, and before `e56e35f8` no verification failure disconnected a peer at all. The kill is new in this commit, as is the backoff.
4. The lead did not mention the shadowing at `:2906-2910` and `:2456`, which is how a forged copy removes the honest download, and which predates the commit.
5. `note_rejection` is not limited to forged blocks: any failed commit of a peer-served block, including an honest one failing for a transient reason, is backed off.

**What this issue does not claim.** It does not claim a consensus fault, a way to make the node accept a forged body, or a permanent block of an honest hash. It does not claim the pre-commit code was safe against a peer serving forged bodies: that peer was never penalised then either. The probabilities and expected delays are arithmetic from the constants and were not measured. The size of the same-tick race window, the cost of holding many connections, and the path by which an altered body with valid signatures would fail were not verified.

**Cross-references.**

- `missing-pow-blocks-grows-without-bound-from-unverified-proposal-snapshots.md` (in `crosslink-e99404e3de7cc-review/medium/`): the by-hash loop described there now also carries the backoff gate (`:2275`). The two do not conflict. Bogus hashes in that report are never served, so they never reach `note_rejection`; item 1 here does not change its fix.
- `dropped-decided-snapshot-leaves-validate-indeterminate-forever-and-decide-can-abort.md` (in `crosslink-e99404e3de7cc-review/high/`): that report covers a snapshot that can never be fetched. This one can only lengthen the time a fetchable snapshot is missing, by the length of the backoff. No ordering between the fixes.

DO NOT DEVIATE FROM THIS TEMPLATE (`reviews/SECURITY-ISSUE-TEMPLATE.md`).
LEAVE THIS MESSAGE IN PLACE SO THAT ALL AGENTS KNOW NOT TO DEVIATE FROM THIS TEMPLATE (`reviews/SECURITY-ISSUE-TEMPLATE.md`).
