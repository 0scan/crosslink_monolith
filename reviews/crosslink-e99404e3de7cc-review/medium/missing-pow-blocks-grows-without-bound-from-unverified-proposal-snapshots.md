# `validate` inserts a proposer-chosen snapshot hash into the global `MISSING_POW_BLOCKS` set before checking the carried headers' proof of work, entries leave only when the block becomes known, and the sync loop requests every entry ahead of near-tip downloads, so one byzantine proposer can fill the 42-slot download budget with hashes that do not exist and stall PoW sync on every finalizer's node

**Severity**: Medium
**Validation Status**: Partially confirmed
**Location**: `zebra-crosslink/zebra-state/src/new_network/bft.rs:99-116` (`MISSING_POW_BLOCKS`, `needs_pow_block`, `missing_pow_blocks`), `:885-1017` (the checks that run before the insertion), `:1012-1016` (the insertion), `:1026-1032` (header PoW, after it), `:1049-1051` (the Linearity insertion); `zebra-crosslink/zebra-state/src/new_network.rs:70` (`DOWNLOAD_UNMODIFIED_TIMEOUT_DUR`), `:733-737` (`MAX_BANDWIDTH_BLOCKS_PER_RES`), `:998-1026` (`BLOCK_DOWNLOADS_N`, `BlockDownloads::insert`), `:2035` (`MAX_REQUEST_DUPLICATES_N`), `:2045-2057` (timeout and re-request), `:2074`, `:2077`, `:2127-2152` (by-hash requests placed first), `:2305-2306` (near-tip requests share the budget), `:2378` (peer cap), `:2640-2662` (serving side sends nothing for an unknown hash); `tenderlink/src/lib.rs:329-339` (`proposal_is_valid` re-validates while `Indeterminate`), `:729-735`, `:785`, `:2317-2322` (only the scheduled proposer's chunks are accepted), `:1111-1127` (validation on the current round), `:1270-1284` (tenderlink only prints `NeedsBlock`)
**Found by agent:** /code-review high (Claude Fable 5.1), 2026-09-29; validated 2026-09-29 at dev e99404e3de7cc
**In scope of audit?** Yes. `MISSING_POW_BLOCKS` and the by-hash request path were introduced on dev (commit `af76dddd`, "by-hash PoW requests for BFT snapshots"). ClT0 is not affected: on `s1_dev` `validate_bft_block` returns `NeedsBlock` to tenderlink (`zebra-crosslink/zebra-crosslink/src/lib.rs:1028` at `64046aeb`), which only prints it, and there is no missing-hash set or by-hash download.

## Description

A BFT proposal names its `snapshot` as `headers[0].prev_block`. When a validator does not hold that block, `validate` records the hash in a process-global set and answers `Indeterminate`, so the sync loop can fetch the block by hash:

- `needs_pow_block` inserts the hash into `MISSING_POW_BLOCKS` (`bft.rs:107-110`);
- `missing_pow_blocks` drops only entries that have become known, and returns the rest (`bft.rs:112-116`);
- the sync loop calls it on every scheduling pass and requests every entry from peers before it schedules any near-tip download (`new_network.rs:2074`, `:2127-2152`).

The insertion happens after the structural checks but **before** the carried headers' proof of work and Linearity are checked (`bft.rs:1014-1016` versus `:1026-1054`). The snapshot hash is entirely proposer-chosen: nothing ties `headers[0].prev_block` to any real block before this point. Entries have no expiry, no cap, and no link to the proposal or BFT height that created them. A hash that will never exist stays in the set, and is re-requested, until the process restarts.

The review's core claim holds. Its description of the damage needs correcting. Memory grows by one hash per attacker-led round, which is negligible, and bandwidth barely grows, since a peer sends nothing for a hash it does not hold. The real damage is **download-slot starvation**: every bogus request holds a download slot for up to about 16 s, is re-issued as soon as it times out, and is scheduled ahead of near-tip sync against a global budget of only 42 in-flight downloads.

## Attack Scenario and Steps

1. **Build a bogus proposal.** The attacker, a roster member, builds a proposal for the current BFT height with the correct parent fat pointer, height, version, scheduled hardforks and `do_not_include_until_bc_height`. It carries sigma = 4 headers whose `headers[0].prev_block` is a fresh random hash, each linked to the previous by hash. The headers carry no valid proof of work.
2. **Propose it.** When tenderlink schedules the attacker as proposer for a round of the current height, it sends the proposal. Honest roster members accept the chunks, because they carry the scheduled proposer's signature (tenderlink `lib.rs:2317-2322`, `:785`).
3. **Insertion.** In the Propose step of that round, each honest roster member's tenderlink calls `validate` (`lib.rs:1111-1119`). Every check up to `bft.rs:1011` passes, `known_block(random)` is `None`, and `needs_pow_block` inserts the hash (`bft.rs:1014-1016`). The header PoW loop that would reject the headers never runs.
4. **The round fails normally.** The validator prevotes nil, the round times out, and tenderlink moves to the next round. `proposal_is_valid` re-runs `validate` on every tick while the result is `Indeterminate` (`lib.rs:331-336`), but it re-inserts the same hash, so each attacker-led round adds one entry.
5. **Accumulation.** The attacker repeats in every round it leads, at every height. Entries never leave: `missing_pow_blocks` keeps any hash that `known_block` does not find (`bft.rs:114`), and a random hash is never found.
6. **Starvation.** Each scheduling pass requests every entry from up to `MAX_REQUEST_DUPLICATES_N` = 2 peers (`new_network.rs:2135-2150`). No peer holds the block. The serving side sets the height to `u32::MAX` and sends nothing (`new_network.rs:2647-2662`), so the slot sits until `DOWNLOAD_UNMODIFIED_TIMEOUT_DUR` = 8 s plus one 8 s grace strike (`new_network.rs:2045-2057`). It is then cancelled and immediately re-requested in the same pass, ahead of near-tip work. Once there are about 21 entries (with two or more peers), bogus requests fill the whole `MAX_BANDWIDTH_BLOCKS_PER_RES` = 42 budget, and the near-tip loop breaks at its budget check (`new_network.rs:2305-2306`) on every pass.
7. **Consequence.** The node stops downloading new PoW blocks (inference: the push path is commented out, and downloads are how blocks arrive from peers). Its snapshots stop advancing. Legitimate `NeedsBlock` hashes for honest proposals sit in the same set and may be ordered behind the bogus ones in `HashSet` iteration order. Honest proposals then become `Indeterminate` on the affected finalizers, and BFT stops deciding once enough finalizers are affected (inference from the above; not run).

**Attack Requirements and Assumptions:**

- The attacker is a finalizer in the active roster and is scheduled as proposer for some rounds. Proposer selection is stake-weighted per (height, round) (tenderlink `lib.rs:650-675`). An outsider cannot inject, because proposal chunks are verified against the scheduled proposer's key.
- Only nodes on the roster are affected. Proposals are validated in the Propose, Prevote and Precommit conditions only when `on_roster` (`lib.rs:1111-1175`); a non-roster node validates only a value that already has 2f+1 precommits (`lib.rs:1211-1217`).
- Rate: one entry per round the attacker leads. With stake share `p`, the expected number of attacker-led rounds per BFT height is about `p / (1 - p)`, because its rounds fail and hand over to the next proposer. For example, `p = 0.1` reaches 21 entries after about 190 BFT heights (inference from the proposer rule; round timing not measured).
- No proof of work, no hash rate, and no network position are required.
- Restarting the node empties the set, because it is in-memory only. The attacker simply resumes.

## Impact on Users

- **Finalizers and stakers:** a single roster member can stall PoW sync on every other finalizer's node at no cost, which then stalls BFT decisions. The attacker can repeat this after every restart.
- **Node operators:** the only symptoms are a `warn` per validation ("Didn't have hash available for confirmation") and `debug`-level "Requesting PoW block … needed by BFT validation" lines. No operator-visible signal says that downloads are being starved. `missing_pow_blocks` also does one `known_block` lookup per entry, under the mutex, on every scheduling pass (`bft.rs:113-114`), which is linear in the set size.
- **Miners, wallet users, light clients:** only indirectly affected, through stalled finality and stalled finalizer nodes. Non-roster nodes keep syncing.

## Technical Details / Code Analysis

**The set and its only removal path** (`bft.rs:104-116`):

```rust
static MISSING_POW_BLOCKS: LazyLock<Mutex<HashSet<Hash>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn needs_pow_block(hash: Hash) -> (TMStatus, TMStatusReason) {
    MISSING_POW_BLOCKS.lock().unwrap().insert(hash);
    (TMStatus::Indeterminate, TMStatusReason::NeedsBlock { hash: hash.0 })
}

pub(super) fn missing_pow_blocks(read_state: &ReadState) -> Vec<Hash> {
    let mut missing = MISSING_POW_BLOCKS.lock().unwrap();
    missing.retain(|hash| read_state.known_block(*hash).is_none());
    missing.iter().copied().collect()
}
```

**What runs before the insertion.** In `validate` order:

- Before the insertion, all of it structural:
  - parent fat pointer equals our tip (`bft.rs:888-895`);
  - `height` matches the chain position (`:899-903`);
  - version is monotonic (`:911-914`);
  - hardforks match the schedule byte for byte (`:925-950`);
  - `do_not_include_until_bc_height` rules (`:953-981`);
  - exactly sigma headers (`:995-1001`);
  - each header names the previous one (`:1002-1011`).
- The insertion itself: `known_block(snapshot)`, then `needs_pow_block` (`:1014-1016`).
- After it, never reached for this proposal: each header's proof of work (`:1026-1032`) and Linearity (`:1036-1054`).
- In tenderlink, before `validate` is called at all:
  - height equals the current height (`lib.rs:729-735`);
  - the signature is from the scheduled proposer (`:785`, `:2317-2322`);
  - the value id matches the proposal bytes (`:933-940`);
  - the round is current and the step is Propose, or enough votes exist for the value (`:1111-1217`).

```rust
let new_final_hash = Hash(new_block.snapshot_block_hash().0);
if read_state.known_block(new_final_hash).is_none() {
    tracing::warn!("Didn't have hash available for confirmation: {}", new_final_hash);
    return needs_pow_block(new_final_hash);
}
```

**The budget** (`new_network.rs:733-737`, `zebra-chain/src/block/serialize.rs:27`). `MAX_BANDWIDTH_BYTES_PER_RES` = 7,000 × 300 = 2,100,000 bytes, and `MAX_BLOCK_BYTES` = 100,000 in this tree, so the budget is 2 × 21 = 42:

```rust
const MAX_BANDWIDTH_BYTES_PER_MS: usize = 7_000; // 7 MB/s
const MAX_BANDWIDTH_BYTES_PER_RES: usize = MAX_BANDWIDTH_BYTES_PER_MS * PEER_RESPOND_MS as usize;
const MAX_BANDWIDTH_BLOCKS_PER_RES: usize = 2 * (MAX_BANDWIDTH_BYTES_PER_RES / zebra_chain::block::MAX_BLOCK_BYTES as usize);
```

**Missing hashes are scheduled first, per peer, against the shared budget** (`new_network.rs:2130-2151`):

```rust
for &hash in &missing_bft_pow_blocks {
    if active_block_dls >= MAX_BANDWIDTH_BLOCKS_PER_RES {
        break;
    }
    let dups = requests_by_hash.entry(hash).or_insert(0);
    if *dups >= MAX_REQUEST_DUPLICATES_N {
        continue;
    }
    let request = HeightAndHashOr0 {
        height: block::Height(u32::MAX),
        hash_or_0: hash,
    };
    if block_downloads.position(request).is_none()
        && block_downloads.insert(request).is_some()
    {
        tracing::debug!("Requesting PoW block {hash} needed by BFT validation from peer {connection_key:?}");
        active_block_dls += 1;
        *dups += 1;
    }
}
```

The near-tip loop that follows for the same peer stops at the same `active_block_dls >= MAX_BANDWIDTH_BLOCKS_PER_RES` test (`new_network.rs:2305-2306`). Bogus requests also count toward `MAX_PEERS_TO_INIT_DLS_FROM` = 16 (`new_network.rs:2077`, `:2376-2379`), so they use up the peer allowance too. The review's "across up to 16 peers" is that allowance; each individual hash is in flight at no more than 2 peers at a time.

**Serving side for an unknown hash** (`new_network.rs:2647-2662`): `block_from_any_chain` returns `None`, the height stays `u32::MAX`, and the request falls through to "nothing to send". So the requester learns nothing until its slot times out.

**The Linearity insertion** (`bft.rs:1049-1051`) inserts the parent bft-block's snapshot, which comes from the decided chain rather than from the proposer. It is bounded (one hash) but has the same "until known" lifetime; see finding 6 for the case where that hash can never become known again.

## Recommendations

1. **Make entries expire unless a live proposal still needs them.** Change `MISSING_POW_BLOCKS` from `HashSet<Hash>` to `HashMap<Hash, std::time::Instant>` holding the last time `needs_pow_block` asked for each hash. `missing_pow_blocks` then drops entries that are known **or** not refreshed within a short window, for example `MISSING_POW_BLOCK_FRESHNESS = 30 s`.

   This works because tenderlink re-validates an `Indeterminate` proposal on every tick while it is still in play (`lib.rs:331-336`): during the proposer's Propose step, or while 2f+1 votes for that value exist. A needed hash therefore stays fresh exactly as long as some proposal the network is voting on needs it. A bogus hash goes stale one round after the attacker's round ends. This is the "freshness tracking" AGENTS.md prefers, and it satisfies "Bound all loops/allocations over attacker-controlled data".

   Add a hard cap as defence in depth (for example 16 entries, evicting the stalest), so a burst within one window stays bounded too.

   ```rust
   static MISSING_POW_BLOCKS: LazyLock<Mutex<HashMap<Hash, std::time::Instant>>> =
       LazyLock::new(|| Mutex::new(HashMap::new()));

   fn needs_pow_block(hash: Hash) -> (TMStatus, TMStatusReason) {
       let mut missing = MISSING_POW_BLOCKS.lock().unwrap();
       missing.insert(hash, std::time::Instant::now());
       if missing.len() > MAX_MISSING_POW_BLOCKS {
           let stalest = missing
               .iter()
               .min_by_key(|(_, asked)| **asked)
               .map(|(hash, _)| *hash)
               .expect("the map is non-empty because it holds more than the cap");
           missing.remove(&stalest);
       }
       (TMStatus::Indeterminate, TMStatusReason::NeedsBlock { hash: hash.0 })
   }

   pub(super) fn missing_pow_blocks(read_state: &ReadState) -> Vec<Hash> {
       let mut missing = MISSING_POW_BLOCKS.lock().unwrap();
       missing.retain(|hash, asked| {
           asked.elapsed() < MISSING_POW_BLOCK_FRESHNESS && read_state.known_block(*hash).is_none()
       });
       missing.keys().copied().collect()
   }
   ```

2. **Reject cheap forgeries before asking for anything.** In `validate`, run the per-header `header_pow_is_valid` loop (`bft.rs:1026-1032`) before the `known_block` lookup (`bft.rs:1014`); it depends only on the headers. The final verdict for any proposal is unchanged (an invalid tail is `Fail` either way), so this is not a validity-rule change, but a zero-work forgery is no longer recorded at all. On its own this is not a bound: PoWLimit headers are cheap (about 17 Equihash runs each with a `0f0f…0f` limit, inference), and a PoW-disabled network checks no work at all. Hence item 1.

3. **Keep near-tip sync from being starved by BFT requests.** In the scheduling pass (`new_network.rs:2130`), stop issuing by-hash requests once they hold a fixed share of the budget, for example `MAX_BANDWIDTH_BLOCKS_PER_RES / 4`. Track a separate `bft_block_dls` count, measured in the first pass over existing downloads (`height == u32::MAX` identifies them), so near-tip sync always keeps most of the budget. Legitimate BFT needs are a handful of hashes, so the cap never binds for honest traffic.

4. **Considered alternatives.**
   - *Clear the set when the BFT height advances:* bounds growth per height but not within a height, where an attacker leading many failed rounds still adds one entry per round. It also drops a legitimately needed Linearity hash on every decision.
   - *Drop a hash after N timed-out requests:* legitimate snapshots can be temporarily unavailable (the proposer's tail not yet propagated), and a fixed strike count either starves them or still lets many bogus hashes sit in flight.
   - *Remove the by-hash path and wait for the near-tip window:* reopens the deadlock the path was added to fix (`new_network.rs:2127-2129` comment).

5. **Tests** (end to end, crosslink test-format, two nodes):
   - *Bogus snapshots do not stall sync.* Node B mines; node A is a finalizer. Use `LoadPoS` to force-feed node A 30 BFT blocks for its next height, each with distinct random `headers[0].prev_block` values and sigma linked headers (validate runs and inserts, `bft.rs:717-731`). Assert that A reaches B's tip within a timeout comparable to the no-attack baseline, and that `missing_pow_blocks` is back to empty after the freshness window.
   - *A real missing snapshot is still fetched.* Hold back a real side-chain block from A, then submit a valid proposal naming it. Assert that A downloads it by hash and the proposal is decided. This guards against item 1 being too aggressive.
   - Unit test only for the pure expiry and cap logic, if it is factored out of the global.

6. **Rollout.** Items 1 to 3 are node-local scheduling and bookkeeping, and item 2 does not change any proposal's final verdict, so none of them changes consensus. They can ship in any release, node by node, before or after the new testnet's genesis. No `s1_dev` backport is needed: ClT0 has no missing-hash set.

## Validation Information

**Verdict: PARTIALLY CONFIRMED. Severity: Medium.**

| Claim | Verified at |
|-|-|
| `needs_pow_block(snapshot)` runs before the carried headers' PoW check | `bft.rs:1014-1016` versus `:1026-1032` |
| `MISSING_POW_BLOCKS` is global, uncapped, removed from only once known | `bft.rs:104-116`; no other writer (`grep MISSING_POW_BLOCKS`) |
| Returned list is requested on every scheduling pass, before near-tip downloads | `new_network.rs:2074`, `:2127-2152`, `:2305-2306` |
| Global budget is 42 in-flight downloads | `new_network.rs:733-737`; `zebra-chain/src/block/serialize.rs:27` |
| Each hash at no more than 2 peers; bogus requests count toward the 16-peer allowance | `new_network.rs:2035`, `:2077`, `:2376-2379` |
| An unknown hash gets no response, so the slot lives about 16 s and is then re-requested | `new_network.rs:2647-2662`, `:70`, `:2045-2057` |
| Only the scheduled proposer can supply a proposal | tenderlink `lib.rs:2317-2322`, `:785` |
| Validation of an unfinished proposal happens only on roster members' nodes | tenderlink `lib.rs:1111-1217` |
| Re-validation while `Indeterminate` re-inserts the same hash | tenderlink `lib.rs:329-339` |
| ClT0 has no missing-hash path | `64046aeb:zebra-crosslink/zebra-crosslink/src/lib.rs:1028`, `64046aeb:tenderlink/src/lib.rs:1178`; `git log -S MISSING_POW_BLOCKS` shows `af76dddd` on dev |

**Severity justification.**

*Why not High:* the effect is loss of liveness only, with no safety or fund impact. It needs a bonded roster member who gets scheduled as proposer across roughly 21 rounds before sync starves. It affects only roster members' nodes. Restarting clears it: the attacker can repeat the attack, but each cycle takes it many BFT heights to rebuild. The proposals are signed, so the attacker can be identified from the proposer's key.

*Why not Low:* it breaks AGENTS.md's "Bound all loops/allocations over attacker-controlled data" in a way that reaches a scarce shared resource (the 42-slot download budget) rather than just memory. It needs no work or hash rate. It can stall PoW sync, and through it BFT, on every honest finalizer at once, and the only operator-visible signal points at "missing blocks" rather than at an attacker.

**Corrections made during validation.**

1. "Memory and bandwidth grow without bound": the memory growth is real but tiny (one 32-byte hash per attacker-led round), and bandwidth is negligible, because peers send nothing for a hash they do not hold. The material harm is the starvation of the 42-slot download budget, which the review mentioned only as "crowded out". The Description and Impact are rewritten around it.
2. "Spending download slots across up to 16 peers": 16 is the per-pass peer allowance that bogus requests also use up; each hash is in flight at no more than 2 peers at a time.
3. Scope: the attacker must be the scheduled proposer for the round, not merely any roster member sending messages, and only roster members' nodes insert the hash.
4. Reordering the checks alone does not fix it: PoWLimit tails are cheap, and PoW-disabled networks skip the work check. The fix plan therefore bounds the set by freshness and caps the budget share.

**Cross-references.**

- `tail-confirmation-checks-carried-headers-against-their-own-nbits-so-a-proposer-can-fake-confirmations.md` (finding 2): same `validate` path. That attack relies on this by-hash fetch to deliver its side-chain snapshot. Item 1 here keeps that delivery working (the proposal stays live while voted on), so the two fixes do not conflict.
- `dropped-decided-snapshot-leaves-validate-indeterminate-forever-and-decide-can-abort.md` (finding 6): there, the Linearity insertion (`bft.rs:1050`) adds a parent snapshot that can never become known again, so it sits in this set and is re-requested forever. Item 1's freshness rule would keep re-adding it while tenderlink re-validates; finding 6's fix is what removes the cause.
