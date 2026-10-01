# The header's `fat_pointer_to_bft_block` is serialized after the Equihash `solution`, so it is outside the Equihash input but inside the `sha256d` block hash that `difficulty_is_valid` compares to the target, and nothing else commits to it: after one Equihash solve a miner can vary the pointer's height and round bytes or its signature list and re-hash with `sha256d` until the block passes the difficulty filter, which makes post-activation PoW a `sha256d` grind instead of an Equihash race

**Severity**: High
**Validation Status**: Confirmed
**Location**: `zebra-crosslink/zebra-chain/src/block/serialize.rs:79-94` (header serialization, pointer after `solution`); `zebra-crosslink/zebra-chain/src/block/header.rs:102-111` (`nonce`, `solution`, `fat_pointer_to_bft_block`); `zebra-crosslink/zebra-chain/src/work/equihash.rs:61` (`INPUT_LENGTH`), `:73-93` (`Solution::check`), `:156-214` (`Solution::solve`, the in-repo miner), `:223-235`; `zebra-crosslink/zebra-chain/src/block/hash.rs:88-95` (`From<&Header> for Hash`, `sha256d` over the whole header); `zebra-crosslink/zebra-chain/src/block/commitment.rs:314-335` (`ChainHistoryBlockTxAuthCommitmentHash::from_commitments`, ZIP 244 terminator); `zebra-crosslink/zebra-consensus/src/sync_verify.rs:96`, `:106-116` (`block_check_header`); `zebra-crosslink/zebra-consensus/src/block/check.rs:112-140` (`difficulty_is_valid`), `:143-150` (`equihash_solution_is_valid`); `zebra-crosslink/zebra-consensus/src/block.rs:215`, `:252-253` and `zebra-crosslink/zebra-consensus/src/checkpoint.rs:608-609` (the other verifier paths, same checks); `zebra-crosslink/zebra-state/src/new_network.rs:2966-2976` (the commit loop that calls `block_check_header`); `zebra-crosslink/zebra-state/src/new_network/bft.rs:191-212` (`header_pow_is_valid`, carried headers), `:1019-1032` (carried headers checked from the header alone), `:366-403` (`admit_fat_pointer`, hash-only resolution at `:399`, pre-activation null rule at `:382`); `zebra-crosslink/zebra-state/src/service/check.rs:185-219` (`block_commitment_is_valid_for_chain_history`); `zebra-crosslink/zebra-rpc/src/methods/types/default_roots.rs:58-108`; `zebra-crosslink/zebra-rpc/src/methods/types/get_block_template.rs:225-226`, `:286`, `:357`, `:386`; `librustzcash/zcash_primitives/src/bft.rs:1050-1059` (`from_parts`), `:1086-1106` (`validate_signatures`), `:1116-1151` (pointer serialization, up to `ACTIVE_ROSTER_MAX_N` signatures); `tenderlink/src/lib.rs:197-223` (`round_data_to_fat_pointer`), `:506-507`, `:725-727` (decision threshold); `zebra-crosslink/zebra-chain/src/parameters/network/testnet.rs:525-534` (testnet PoWLimit `2^251 - 1`); `zebra-crosslink/zebrad/src/config.rs:310-320` (new testnet parameters); upstream reference `zips/protocol/protocol.tex:13580-13584`, `:13764-13777`, `:13879-13885`; design `crosslink_book/src/crosslink-design-overview-20260914.md:94`, `:100`, `:104`, `:123`; on `s1_dev` (`64046aeb`) the same `serialize.rs:79-94`, `equihash.rs:60`, `:87`, `hash.rs:102`, `check.rs:129`
**Found by agent:** Surfaced by the validator of finding 7 (Claude Fable 5.1), 2026-09-29; validated 2026-09-29 at dev e99404e3de7cc
**In scope of audit?** Yes: it is a PoW consensus defect in the hybrid block format. It is **not introduced on dev**. The header layout, the Equihash input and the difficulty hash are identical on `s1_dev` (`64046aeb`), and the layout goes back to the oldest commit of `serialize.rs` in this repository (`3b8b29b2`). **ClT0 is affected**, and there the admission gate is weaker still (hash and ordering only, no sigma or Last Final Snapshot rule).

## Description

In upstream Zcash every header field a miner can change is inside the Equihash input, except the solution itself. The protocol spec defines the Equihash input as `nVersion || hashPrevBlock || hashMerkleRoot || hashReserved || nTime || nBits || nNonce` (`protocol.tex:13764-13777`), the solution is the last header field (`:13580-13584`), and the difficulty filter is `sha256d` "on the whole block header (including solutionSize and solution)" (`:13879-13885`). So each difficulty-filter trial costs one Equihash solution. That is the whole point of a memory-hard PoW.

Crosslink appends one more field, `fat_pointer_to_bft_block`, after the solution (`serialize.rs:91-94`). That single placement decision has three consequences, each verified by reading:

1. **It is outside the Equihash input.** `Solution::check` serializes the header, keeps only the first `INPUT_LENGTH = 4 + 32 * 3 + 4 * 2` = 108 bytes, and passes the nonce separately (`equihash.rs:61`, `:81-90`). The pointer starts at byte 1487.
2. **It is inside the block hash.** `Hash::from(&Header)` runs `sha256d` over the full serialization (`hash.rs:88-95`), which includes the pointer whenever the logical version is 5 or more. Every template uses version 6 (`get_block_template.rs:386`).
3. **The difficulty filter uses that full hash.** `block_check_header` computes `header.hash()` (`sync_verify.rs:96`) and `difficulty_is_valid` rejects only if `hash > difficulty_threshold` (`check.rs:130`). The legacy verifier and the checkpoint verifier do the same (`block.rs:215`, `:252`; `checkpoint.rs:608`), and so does the carried-header check in BFT proposals (`bft.rs:196`, `:206`).

Nothing else binds the pointer before the difficulty check:

- `commitment_bytes` is the ZIP 244 `hashBlockCommitments`, BLAKE2b over the chain history root, the auth data root and a 32-byte zero terminator (`commitment.rs:314-335`). The pointer is not in it.
- The merkle root covers transactions only, and the template coinbase is built without the pointer (`get_block_template.rs` `new_coinbase(net, height, miner_params, txs_fee)`).
- The next block's `previous_block_hash` and history root commit to this header, pointer included, but only after the fact: the grind happens before the next block exists.

And nothing canonicalizes the pointer bytes. `admit_fat_pointer` reads only the first 32 bytes, the BFT block hash (`bft.rs:399`); the 8 height bytes, the 4 round bytes, the signature count and every `(pub_key, vote_signature)` pair are never checked on the PoW path (finding 7 establishes this in full). So once a miner holds one valid Equihash solution for a template, every distinct pointer that still resolves to an admissible BFT block is a fresh, free difficulty-filter trial.

## Attack Scenario and Steps

**Attack Requirements and Assumptions:**

- An ordinary miner. No stake, no finalizer key, no special network position.
- BFT has decided at least one block, so a non-null pointer can resolve. Pointers are forced null at or below the activation height (`bft.rs:382`), and the null pointer is a single fixed byte string, so the grind exists **only after activation**.
- Modified mining software. The in-repo miner does not grind: `Solution::solve` checks difficulty once per solved header (`equihash.rs:200`, `:223-235`).

Steps:

1. Take a block template (or build one) for height `H` whose pointer names an admissible decided BFT block `B`.
2. Solve Equihash once over the unchanged 108-byte input plus a nonce. Keep any valid solution, whether or not its hash meets the target.
3. Loop: write a new value into the pointer's 12 height and round bytes (`vote_for_block_without_finalizer_public_key[32..44]`), recompute `sha256d` over the header, and compare with the target. The Equihash solution stays valid because its input did not change.
4. On a hit, broadcast. `block_check_header` passes (difficulty, then Equihash), `block_check_body` passes (the pointer is not in the merkle root), `admit_fat_pointer` resolves `B` by hash and returns `Accept`, and `block_commitment_is_valid_for_chain_history` passes because the commitment does not cover the pointer.

**Cost of one trial, by reading (inference, not measured).** With no signatures the header is 1533 bytes and the varying bytes sit at offsets 1519 to 1530. Everything before byte 1472 is constant, so a grinder precomputes the SHA-256 midstate and pays three compression-function calls per trial: the block holding the varying bytes, the constant padding block, and the outer hash. Bitcoin pays two. So this is Bitcoin-class `sha256d` work that runs well on CPUs and GPUs. Whether Bitcoin ASICs can be repurposed for it is not established; their pipelines assume an 80-byte header.

## Impact on Users

**How big the gain is.** Let `p` be the probability that one header hash meets the target, `E` the attacker's Equihash solution rate and `S` their `sha256d` trial rate, with `r = S / E`. An honest miner needs about `1/p` solutions per block. A grinder needs one solution plus `1/p` hashes. The grinder's speed-up over an honest miner with the same Equihash hardware is therefore about `1 / (p + 1/r)`, which is roughly `min(1/p, r)`:

- **At the new testnet's minimum difficulty the gain is small.** The testnet PoWLimit is `2^251 - 1` (`testnet.rs:525-534`, and `zebrad/src/config.rs` builds the new testnet on the default limit), so `p` is at most about 1/32. At minimum difficulty the grind is worth at most about 32 times.
- **The gain grows with difficulty and does not limit itself.** Grinders raise the block rate, the difficulty adjustment lowers `p`, and the cap `1/p` rises with it. The ceiling is `r`. Commonly cited rates put `sha256d` around 10^9 per second on a GPU and Equihash 200,9 around 10^2 to 10^3 solutions per second, and a CPU core is in a similar ratio, so `r` is on the order of 10^6 (an estimate from public figures, not measured here).
- **In equilibrium, the difficulty tracks `sha256d` throughput, not Equihash throughput.** Honest miners running the stock solver fall to a negligible share of blocks.

**Who is affected.**

- **Miners:** fair mining is gone. Whoever grinds takes nearly all block subsidy and fees; honest Equihash hardware earns close to nothing.
- **Node operators and wallet users:** the PoW half of the hybrid protocol stops measuring what it is meant to measure. A single grinder can hold the most-work chain, censor transactions from it, and reorganize anything above the last final snapshot at will. BFT finality bounds how deep that goes (the sticky-finality rule), so finalized history and funds are not directly at risk from this defect alone.
- **Finalizers and stakers:** the proposer's snapshot is `tip - sigma` of its best chain, so whoever controls the best chain controls what BFT is asked to finalize. The same grind applies to headers carried in BFT proposals, because `header_pow_is_valid` hashes the same full header (`bft.rs:191-212`); once finding 2's fix checks carried headers against chain difficulty, each of the sigma headers would cost about one Equihash solve instead of `1/p`.
- **Everyone who uses the new testnet to evaluate Crosslink:** measurements of PoW behaviour (fork choice, sigma confirmations, finality lag under PoW contention) stop reflecting a memory-hard PoW.

## Technical Details / Code Analysis

**The layout** (`zebra-crosslink/zebra-chain/src/block/serialize.rs:79-94`):

```rust
        writer.write_u32::<LittleEndian>(self.version)?;
        self.previous_block_hash.zcash_serialize(&mut writer)?;
        writer.write_all(&self.merkle_root.0[..])?;
        writer.write_all(&self.commitment_bytes[..])?;
        writer.write_u32::<LittleEndian>(
            self.time
                .timestamp()
                .try_into()
                .expect("deserialized and generated timestamps are u32 values"),
        )?;
        writer.write_u32::<LittleEndian>(self.difficulty_threshold.0)?;
        writer.write_all(&self.nonce[..])?;
        self.solution.zcash_serialize(&mut writer)?;
        if logical_version >= 5 {
            self.fat_pointer_to_bft_block.zcash_serialize(&mut writer)?;
        }
```

**The Equihash input stops at `nBits`** (`zebra-crosslink/zebra-chain/src/work/equihash.rs:61`, `:81-90`):

```rust
    pub const INPUT_LENGTH: usize = 4 + 32 * 3 + 4 * 2;

        let mut input = Vec::new();
        header
            .zcash_serialize(&mut input)
            .expect("serialization into a vec can't fail");

        // The part of the header before the nonce and solution.
        // This data is kept constant during solver runs, so the verifier API takes it separately.
        let input = &input[0..Solution::INPUT_LENGTH];

        equihash::is_valid_solution(n, k, input, nonce.as_ref(), self.value())?;
```

**The block hash covers everything** (`zebra-crosslink/zebra-chain/src/block/hash.rs:88-95`):

```rust
impl<'a> From<&'a Header> for Hash {
    fn from(block_header: &'a Header) -> Self {
        let mut hash_writer = sha256d::Writer::default();
        block_header
            .zcash_serialize(&mut hash_writer)
            .expect("Sha256dWriter is infallible");
        Self(hash_writer.finish())
    }
}
```

**And that is the hash the difficulty filter compares** (`zebra-crosslink/zebra-consensus/src/sync_verify.rs:96`, `:109-113`, then `check.rs:130`):

```rust
    let hash = header.hash();

    if check_pow {
        check::difficulty_is_valid(header, network, &alleged_height, &hash)
            .map_err(VerifyBlockError::from)?;
        check::equihash_solution_is_valid(header).map_err(VerifyBlockError::from)?;
    }
```

```rust
    if hash > &difficulty_threshold {
        Err(BlockError::DifficultyFilter(
```

`check_pow` is `!network.disable_pow()` (`new_network.rs:2966`), and `disable_pow` is `false` on testnets (`testnet.rs:534`), so both checks run on the new testnet.

**The one commitment in the Equihash input does not cover the pointer** (`zebra-crosslink/zebra-chain/src/block/commitment.rs:323-330`):

```rust
        let hash_block_commitments: [u8; 32] = blake2b_simd::Params::new()
            .hash_length(32)
            .personal(b"ZcashBlockCommit")
            .to_state()
            .update(&<[u8; 32]>::from(*history_tree_root)[..])
            .update(&<[u8; 32]>::from(*auth_data_root))
            .update(&[0u8; 32])
            .finalize()
```

The third input is ZIP 244's zero terminator, a slot reserved for exactly this kind of extension. It is checked on every committed block by `block_commitment_is_valid_for_chain_history` (`zebra-state/src/service/check.rs:185-219`), reached from the commit loop through `handle_commit` (`write.rs:392`) and `NonFinalizedState::validate_and_commit` (`non_finalized_state.rs:608`). That makes it the natural place to bind the pointer (Recommendations, step 1).

**Grinding freedom, in bits.**

| Source of variation | Current rules (dev and `s1_dev`) | With finding 7's `certificate_is_valid` |
|-|-|-|
| Height bytes (8) and round bytes (4) of the vote template | 96 bits, never read on the PoW path | Height pinned to the BFT height; round fixed by the signed vote message, so 0 bits |
| Signature count and `(pub_key, vote_signature)` pairs | Up to 100 entries of 96 arbitrary bytes each (`bft.rs:1134` in `librustzcash` caps the count), about 76,800 bits | Only subsets of genuine precommits that reach the `2f + 1` stake threshold, in roster order |
| Number of valid quorum subsets, equal stake, all members signed (computed) | n/a | 4 members: 5 subsets, 2.3 bits; 7: 29, 4.9 bits; 10: 176, 7.5 bits; 20: about 1.4e5, 17.1 bits; 100: about 5.5e26, 88.8 bits |
| Re-signing by any roster key holder | n/a | Unbounded. Ed25519 verification cannot tell a fresh nonce from the deterministic one, so a finalizer who mines, or colludes with a miner, can issue as many valid signatures over the same vote as wanted (property of Ed25519, not of this code) |
| Choice of BFT block hash among admissible ones | `log2(k)`, `k` usually small | Same |

Under the current rules the 96 height and round bits alone exceed any target a testnet will reach, so the grind is complete. Under finding 7's fix it shrinks to `log2(k)` plus the quorum-subset bits, and stays unbounded for anyone holding a roster key. Even a handful of variants is a direct multiplier on a miner's effective Equihash rate, so validating certificates narrows the grind and does not close it. The quorum-subset counts assume equal stake and every member's precommit being visible to the miner; they are computed from tenderlink's threshold (`f = (n - 1) / 3`, threshold `2f + 1`, `tenderlink/src/lib.rs:506-507`, `:725-727`) and are an illustration, not a measurement of the live roster.

**Upstream comparison.** Zcash's solution is the last header field, and every earlier field is in the Equihash input (`protocol.tex:13580-13584`, `:13764-13777`). The only bytes outside the Equihash input are the solution itself, and a solution is not free to vary: each one costs an Equihash run. Crosslink's pointer is the first header content upstream never had to protect.

## Recommendations

1. **Bind the pointer into the Equihash input through the ZIP 244 terminator of `hashBlockCommitments`** (option (a); restores the upstream invariant that one difficulty-filter trial costs one Equihash solution).
   - Add a pure digest in `zebra-chain/src/block/commitment.rs`. The null pointer maps to the upstream zero terminator, so every pre-activation block, including the configured new-testnet genesis (`zebrad/src/config.rs`, `05a60a92…`), keeps its ZIP 244 commitment and hash unchanged.

   ```rust
   pub fn fat_pointer_digest(fat_pointer: &FatPointerToBftBlock) -> [u8; 32] {
       if *fat_pointer == FatPointerToBftBlock::null() {
           return [0u8; 32];
       }
       let mut bytes = Vec::new();
       fat_pointer
           .zcash_serialize(&mut bytes)
           .expect("serialization into a vec can't fail");
       let digest = blake2b_simd::Params::new()
           .hash_length(32)
           .personal(b"ZcashCLFatPtrHsh")
           .hash(&bytes);
       <[u8; 32]>::try_from(digest.as_bytes()).expect("hash_length is 32")
   }
   ```

   - Give `ChainHistoryBlockTxAuthCommitmentHash::from_commitments` a third argument, `fat_pointer_digest: &[u8; 32]`, and feed it where `&[0u8; 32]` is today (`commitment.rs:329`).
   - Validator: in `block_commitment_is_valid_for_chain_history` (`zebra-state/src/service/check.rs:206`) pass `&fat_pointer_digest(&block.header.fat_pointer_to_bft_block)`. In every other arm (`PreSaplingReserved`, `FinalSaplingRoot`, `ChainHistoryActivationReserved`, `ChainHistoryRoot`), which bind nothing, require the pointer to be null and return a new `CommitmentError` variant otherwise. On Crosslink networks those arms cover only genesis and the Heartwood activation block, and `admit_fat_pointer` already forces null there when an activation height exists (`bft.rs:382`); the extra rule closes the `BftBootstrap::Supplied` case where it does not.
   - Template: add `fat_pointer: &FatPointerToBftBlock` to `DefaultRoots::from_coinbase` (`default_roots.rs:58`) and pass the pointer `BlockTemplateResponse::new_internal` already receives (`get_block_template.rs:286`, call at `:357`). Add a `fatpointerdigest` field to `defaultroots` so a pool that re-selects transactions and recomputes `hashBlockCommitments` can still build a valid block.
   - Other callers of `from_commitments`: the crosslink harness (`zebrad/tests/crosslink.rs:1371`) passes the digest of the pointer it puts in the header; `arbitrary.rs:522`, `acceptance.rs:3132` and `e2e/trusted_chain.rs:166`, `:171` pass `[0u8; 32]` because their pointers are null.
   - Why this and not another field: `commitment_bytes` is already in the 108-byte Equihash input, already checked on every committed block, and already computed by the node for `getblocktemplate` consumers, so the header layout, the Equihash input and the solver interface all stay exactly as upstream. It is the "commit only" storage option of the design overview (`crosslink-design-overview-20260914.md:100`) layered on the current in-header pointer, and it keeps the property the overview's `:94` warns about: miners' header interpretation does not change.

2. **Decide the carried-header path together with finding 2, before implementing step 1.** Step 1 binds the pointer for every block that enters the PoW chain, because the commitment check runs on every commit. It does not reach headers carried in BFT proposals: `header_pow_is_valid` checks them from the header alone (`bft.rs:1019-1032`), and the commitment cannot be recomputed without the block's history and auth roots. Under finding 2's fix those headers would be checked against chain difficulty and the grind would make each one cost about one Equihash solve. Two ways out:
   - (i) Carry the upstream ZIP 244 value in the header after the pointer, and define the commitment slot as BLAKE2b over that value and the pointer digest. A lone header then proves its own binding. Cost: 32 more header bytes and a second change to the header format, so it must replace step 1's definition, not be layered on it.
   - (ii) Require carried headers to be blocks the validator has fully validated, which is what the design overview asks for (`:123`). The code rejects that on purpose, because a node catching up could never download an orphaned tail (`bft.rs:1019-1026`).
   - Recommended: (i) if finding 2's fix keeps checking carried headers without their blocks, otherwise step 1 alone.

3. **Considered alternatives.**
   - **(a), other placements.** Moving the pointer before the nonce makes the Equihash input variable-length and breaks every Equihash solver and ZIP 301 stratum job, which assume the 140-byte input; it also changes `INPUT_LENGTH`, `Header::SERIALIZED_SIZE`, `librustzcash`'s `BlockHeader::read_data` and the RPC raw-header output. A new 32-byte field inside the Equihash input breaks the same solvers. Binding a pointer digest into the leading bytes of `nNonce` is context-free and keeps the layout, but it collides with ZIP 301's pool-assigned `NONCE_1` prefix and shrinks the nonce space. A commitment in the coinbase works through the merkle root but duplicates the header data and forces every coinbase builder to add it. All of these break more mining software than step 1.
   - **(b) Exclude the pointer from the difficulty hash.** Either the block hash still covers the pointer, in which case anyone relaying a block can swap its pointer at zero cost and produce sibling blocks with the same work and different hashes, or the block hash also excludes it, in which case two nodes can hold "the same" block with different pointers, and admission and `fin` (both keyed on the pointer) diverge: a consensus split. It also contradicts the spec's difficulty rule (`protocol.tex:13879-13885`), which every external miner implements when it decides whether a solution is a block. Not viable.
   - **(c) Make the pointer canonical.** It cannot be made fully canonical. Honest nodes hold different signature sets for the same decision (`round_data_to_fat_pointer` lists whatever precommits this node received, `tenderlink/src/lib.rs:207-221`), several decided BFT blocks can be admissible at once, "the newest admissible block" depends on each node's view, and roster key holders can re-sign. Pinning height and round and requiring roster-ordered signatures (finding 7) is good hygiene, but it leaves the subset, block-choice and re-signing freedom in the table above. Even the smallest canonical form, a hash-only pointer with no signatures, keeps `log2(k)` bits and gives up the design overview's settled choice of a self-verifying link.

4. **Tests** (prefer the test-format scenarios in `zebrad/tests/crosslink.rs`, which drive the real commit loop).
   - A post-activation scenario: build an honest block citing a decided BFT block and expect it accepted. Then load the same block with one pointer height byte changed and `commitment_bytes` left as it was, `SHOULD_FAIL`, with `push_instr_expect_pow_chain_length` unchanged. Repeat with a different signature list and with a pointer to a different admissible BFT block. These assert the binding and do not depend on `disable_pow`.
   - If the harness can run with PoW enabled: take a block with a valid Equihash solution, change the pointer and recompute `commitment_bytes` to match; it must now fail `equihash_solution_is_valid`. This is the grind itself, closed.
   - A pre-activation block and the genesis block keep their current hashes (null pointer, zero digest), and a non-null pointer at the Heartwood activation height is rejected under `BftBootstrap::Supplied`.
   - `getblocktemplate` then `submitblock` at a post-activation height: the template's `blockcommitmentshash` equals `from_commitments(history, auth, fat_pointer_digest(template pointer))` and the submitted block commits.
   - Unit test on `fat_pointer_digest` alone (a pure leaf): null gives `[0; 32]`, and `from_commitments` with a zero digest reproduces the upstream ZIP 244 value.
   - Regenerate the binary vectors in `crosslink-test-data` (`test_pow_block_*.bin`, `blocks.zeccltf`) if any post-activation block in them carries a non-null pointer; they will fail the new commitment check. Not enumerated here.

5. **Rollout.**
   - This changes block validity (a new meaning for `hashBlockCommitments` on blocks with a non-null pointer), so it is a consensus change. **Land it before the new testnet's genesis.** Strictly it only has to precede the first non-null pointer (BFT activation), since earlier blocks are unchanged, but nothing is gained by cutting it that fine.
   - Breakage summary. Header hash stability: only post-activation hashes change, and none exist yet on the new testnet. RPCs: `getblocktemplate` commitments change value and gain `fatpointerdigest`; `getblock` is unchanged in form. Test vectors: post-activation binaries need regeneration. Mining software: Equihash input and header layout are unchanged, so solvers, stratum and `getblocktemplate` miners that copy `blockcommitmentshash` keep working; only pools that recompute the commitment themselves must add the digest.
   - ClT0 (`s1_dev`) has the same layout and is exploitable today. A backport is a hard fork with an activation height and a coordinated upgrade of every miner; given ClT0 is being replaced, it is optional.

## Validation Information

**Verdict: CONFIRMED. Severity: High.**

| Claim | Verified at |
|-|-|
| The pointer is serialized after the solution | `serialize.rs:91-94`, read directly; templates use version 6 (`get_block_template.rs:386`) so it is always serialized |
| The Equihash input excludes it | `equihash.rs:61`, `:81-90`: 108 bytes plus the nonce; the pointer starts at byte 1487 |
| The difficulty hash includes it | `hash.rs:88-95` (full header); `sync_verify.rs:96`, `:110`; `check.rs:130` compares the full hash, not a prefix hash |
| Every block-hash path is the same | `Block::hash` delegates to the header (`block.rs:97-99`, `:287-290` in `zebra-chain`); legacy verifier `zebra-consensus/src/block.rs:215`, `:252`; checkpoint `checkpoint.rs:608`; carried headers `bft.rs:196`, `:206`; in-repo miner `equihash.rs:231` |
| No other commitment binds it | `commitment.rs:323-330` (zero terminator); coinbase built without the pointer; history root and next block commit only after the fact |
| Nothing canonicalizes the bytes | `admit_fat_pointer` reads only the hash (`bft.rs:399`); finding 7's validation covers the rest of the PoW path |
| Grind exists only post-activation | non-null rejected at or below activation (`bft.rs:382`); an unresolvable hash defers (`bft.rs:399-401`) |
| Upstream solution is the last field and the only bytes outside the Equihash input | `protocol.tex:13580-13584`, `:13764-13777`, `:13879-13885` |
| New testnet PoW is enabled with PoWLimit `2^251 - 1` | `testnet.rs:525-534`; `zebrad/src/config.rs:310-320` uses the default limit |
| Same layout on `s1_dev` | `git show 64046aeb:` of `serialize.rs`, `equihash.rs` (`:60`, `:87`), `hash.rs` (`:102`), `check.rs` (`:129`) |
| `hashBlockCommitments` is checked on every committed block | `check.rs:185-219` via `write.rs:392` and `non_finalized_state.rs:608` |
| Midstate cost, `r` of order 10^6, ASIC reuse | inference and public figures; not measured |

**Severity justification: High.**

*Why High.* Any miner, with no stake and no special position, can use it; it needs only modified mining software. It removes the defining property of the PoW half of the protocol, that each difficulty trial costs an Equihash solution, for every post-activation block, and the gain grows with difficulty up to roughly the `sha256d` to Equihash rate ratio. It also cheapens the PoW evidence inside BFT proposals. And the fix is a header-format and validity change that is cheap before genesis and a coordinated hard fork after it.

*Why not higher.* High is the top of this review's scale, and two things bound it. BFT finality caps how far a PoW-dominant grinder can reorganize, so finalized history and funds are not at risk from this defect alone. And on a low-difficulty testnet the immediate gain is small: at the PoWLimit it is at most about 32 times, and honest hashrate is low enough that a determined attacker could already buy a PoW majority with ordinary Equihash hardware. The grind makes that majority far cheaper and permanent in the format, rather than creating a capability that did not exist.

*Why not Medium.* Medium would fit a defect whose exploitation needs a privileged role or whose effect stays local. This one needs nothing, applies to every block after activation, and degrades a consensus assumption the rest of the design leans on (most-work fork choice, sigma confirmations, Tail Confirmation evidence).

**Corrections made during validation.**

1. The grind exists only after activation: pointers at or below the activation height must be null (`bft.rs:382`), and a non-null pointer must resolve to a decided BFT block or the block is deferred.
2. The freedom is larger than "12 bytes or signature bytes": the signature count, every public key, and the choice among admissible BFT blocks are also free under the current rules.
3. "A GPU does orders of magnitude faster" needed a bound: the speed-up is about `min(1/p, r)`, so it is at most about 32 times at the new testnet's PoWLimit and grows with difficulty toward `r`. A CPU gets a similar ratio; a GPU is not required.
4. The claim that finding 7's fix narrows but does not close the grind is correct and is now quantified. It also missed that any roster key holder can re-sign votes with fresh Ed25519 nonces, which leaves the grind unbounded for a mining finalizer or a miner colluding with one.
5. Added: the same grind applies to headers carried in BFT proposals (`header_pow_is_valid`), which interacts with finding 2 and constrains which fix to choose.

**Cross-references.**

- `admit-fat-pointer-accepts-a-certificate-by-hash-without-checking-signatures.md` (finding 7, `medium/`): where this issue was first recorded. Its unchecked certificate bytes are what make the grind free under the current rules, and its `certificate_is_valid` narrows the grind as tabulated above. The two fixes are independent and both belong before genesis; this one closes the grind whether or not finding 7's fix lands.
- `tail-confirmation-checks-carried-headers-against-their-own-nbits-so-a-proposer-can-fake-confirmations.md` (finding 2): carried headers are hashed and checked the same way, so its fix and Recommendations step 2 here should be designed together.
- `template-fat-pointer-is-chosen-against-the-live-tip-not-the-template-parent.md` (finding 8): step 1 makes the template's commitments depend on the chosen pointer, so the pointer must be fixed before `DefaultRoots::from_coinbase` runs; both changes touch the same `getblocktemplate` path and are best made together.
