# `admit_fat_pointer` resolves a PoW block's `fat_pointer_to_bft_block` by BFT block hash alone and never checks its signatures, signer set, quorum or vote template, so any miner (and, through `previous_block_fat_ptr`, any BFT proposer) can put a certificate that does not verify permanently into the PoW chain's headers

**Severity**: Medium
**Validation Status**: Partially confirmed
**Location**: `zebra-crosslink/zebra-state/src/new_network/bft.rs:366-403` (`admit_fat_pointer`, the hash-only resolution at `:399`), `:885-895` (`BftRunner::validate`, hash-only check of `previous_block_fat_ptr`), `:1076-1081` (`decide`, the only `validate_signatures` call), `:293-306` (`fat_pointer_to_block_at_height`), `:544-581` (`fat_pointer_for_template`, fallback at `:577-580`), `:1396` (genesis certificate with no signatures); `zebra-crosslink/zebra-state/src/new_network.rs:2994-3008` (the only caller); `zebra-crosslink/zebra-state/src/new_network/fin.rs:131-135` (`candidate`); `librustzcash/zcash_primitives/src/bft.rs:980-984`, `:1050-1059`, `:1068-1113` (`FatPointerToBftBlock`); `tenderlink/src/lib.rs:197-223` (`round_data_to_fat_pointer`), `:506-508` (decision threshold); consumers `zebra-crosslink/zebra-crosslink/src/viz2.rs:57`, `zebra-gui/src/viz_gui.rs:115`, `zebra-crosslink/zebra-rpc/src/methods.rs:2189-2201`, `:3623-3635`, `:3717-3727`, `zebra-crosslink/zebrad/src/lightwalletd.rs:2097`; related header layout `zebra-crosslink/zebra-chain/src/block/serialize.rs:90-94`, `zebra-crosslink/zebra-chain/src/work/equihash.rs:61`, `:73-90`, `zebra-crosslink/zebra-chain/src/block/hash.rs:88-95`, `zebra-crosslink/zebra-consensus/src/sync_verify.rs:96-112`; design docs `crosslink_book/src/FINALITY.md:159-167`, `:304`, `:317`, `:870-871`, `crosslink_book/src/crosslink-design-overview-20260914.md:77-84`, `:101`, `:169`; on `s1_dev` (`64046aeb`) `zebra-crosslink/zebra-crosslink/src/lib.rs:301-375` and `:724`
**Found by agent:** /code-review high (Claude Fable 5.1), 2026-09-29; validated 2026-09-29 at dev e99404e3de7cc
**In scope of audit?** Yes, as PoW/BFT hybrid consensus correctness, but the defect is **not introduced on dev**. The hash-only resolution already exists on `s1_dev` in `call_from_state_to_crosslink_to_ask_about_fat_pointers` (`zebra-crosslink/zebra-crosslink/src/lib.rs:301-375` at `64046aeb`), and dev moved it into `zebra-state` in `379170fe` ("NewNet: Move the BFT chain and its readers into zebra-state"), dropping the `s1_dev` comment `// TODO: check public keys on the fat pointer against the roster` (`lib.rs:724` at `64046aeb`) on the way. **ClT0 is affected.** The one part that is new on dev is the template fallback that copies the parent block's pointer (`bft.rs:577-580`); `s1_dev` falls back to the null pointer instead.

## Description

A Crosslink PoW header carries `fat_pointer_to_bft_block`, a `FatPointerToBftBlock` (`librustzcash/zcash_primitives/src/bft.rs:980-984`) made of two parts:

1. `vote_for_block_without_finalizer_public_key`, 44 bytes: the 32-byte blake3 hash of the BFT block, then the 8-byte BFT height and 4-byte round (commit flag in the top bit) of the precommit vote (`from_parts`, `:1050-1059`).
2. `signatures`, a list of up to `ACTIVE_ROSTER_MAX_N` (100) `(pub_key, vote_signature)` pairs, the precommit signatures this node collected when the block was decided (`tenderlink/src/lib.rs:197-223`).

The design overview says why part 2 exists: the fat pointer is "a hash identifying a BFT certificate, and a set of Ed25519 signatures attesting to that certificate" (`crosslink-design-overview-20260914.md:77-80`), and the rejected hash-only placement was rejected "at the cost that a new PoW block's link cannot be verified as carrying the required votes without consulting an up-to-date PoS service" (`:101`).

The PoW admission gate uses only part 1's first 32 bytes. `admit_fat_pointer` (`bft.rs:366`) looks the hash up in the node's own decided chain (`chain.hash_to_height`, `:399`) and then checks `do_not_include_until_bc_height`, the ordering against the parent's pointer, the sigma depth, and Last Final Snapshot. Nothing checks:

- that any signature verifies;
- that the signers are members of the roster that voted at that BFT height, are distinct, or carry enough stake;
- that the vote template's height and round are the ones the BFT block was decided at.

It is the only fat-pointer gate on the PoW path: every block, from peers and from `submit_block`, goes through the `blocks_to_commit` retain loop (`new_network.rs:2943-3044`), and that loop's only crosslink check is this call (`:3008`). `zebra-consensus` does not look at the pointer at all (its only mention is `block.rs:371-373`, setting `pos_payout: false`). The lite checkpoint pins one hash and does not bypass the gate.

`FatPointerToBftBlock::validate_signatures` exists (`bft.rs:1086-1106` in `librustzcash`) and has exactly one caller, `BftRunner::decide` (`bft.rs:1079`), which runs on this node's own decisions. Even there it verifies only that each listed signature is valid for its listed key; it checks neither roster membership nor quorum (tenderlink already counted the quorum before calling `decide`), and a list with no signatures passes, which is what BFT genesis relies on (`bft.rs:1396`, `:1420`).

A second, related route puts unverified certificates into headers without any miner misbehaving. `BftRunner::validate` checks a proposal's `previous_block_fat_ptr` by hash only (`bft.rs:888-895`), and tenderlink never parses the proposal. `fat_pointer_to_block_at_height` (`bft.rs:293-306`) hands out `blocks[h].previous_block_fat_ptr` as the certificate for every non-tip height, and `fat_pointer_for_template` uses it whenever the newest qualifying decided block is not the tip. So a byzantine proposer who gets one block decided can choose the signature list that every honest template later carries for the parent height.

**What is not affected.** Node-local finality does not read the signatures: `fin::candidate` resolves the tip's pointer by hash in the node's own decided chain (`fin.rs:131-135`), and a pointer only resolves once this node itself decided that block through tenderlink. FINALITY.md states this design explicitly: "a node resolves it only against the node's own store of decided bft-blocks ... Every context a node accepts is therefore final" (`FINALITY.md:161-165`). So a forged signature list cannot make a node treat an undecided BFT block as final, cannot change `fin`, and cannot change which PoW blocks are admitted beyond what the hash already determines.

## Attack Scenario and Steps

**Attack Requirements and Assumptions:**

- For route A: the ability to mine one PoW block. On the public testnet PoWLimit is low (`disable_pow` defaults to `false` on testnets, `zebra-chain/src/parameters/network/testnet.rs:534`, so Equihash and the difficulty filter apply), so this is cheap.
- For route B: one byzantine finalizer that is proposer for one round and whose proposal is decided.
- BFT must have decided at least one block (post-activation), so that there is a hash to point at.

Route A, a miner:

1. Take the 32-byte hash of any decided BFT block `B` at 0-based height `h` that satisfies the other admission rules for the height being mined (in practice the one an honest template would pick).
2. Build `fat_pointer_to_bft_block` with that hash, any height and round bytes, and either no signatures or up to 100 arbitrary `(pub_key, vote_signature)` pairs.
3. Mine and broadcast the block. `admit_fat_pointer` resolves the hash (`:399`), every remaining check reads only `chain.blocks[h]` and the PoW chain, and the gate returns `Accept`.
4. The block is committed with the forged certificate in its header, covered by the block hash and therefore permanent once buried.
5. If honest templates later hit the fallback (no decided block qualifies for the template, for example while the newest decided snapshots are off this node's best chain), `fat_pointer_for_template` copies the parent block's pointer (`bft.rs:577-580`), so honest miners repeat the forged certificate.

Route B, a proposer:

1. When proposing height `h + 1`, set `previous_block_fat_ptr` to `B`'s hash with garbage signatures.
2. `validate` compares only `points_at_block_hash()` (`:888`), so honest finalizers vote and the block is decided.
3. From then on `fat_pointer_to_block_at_height(.., h + 1)` returns the garbage pointer, and every honest template citing `B` while it is not the tip carries it.

**Corrected claim.** The original finding says honest templates reuse a miner's forged pointer "via the parent's own context fallback". That is true only when the fallback fires; in steady state an honest template takes the pointer from its own decided chain (route B's source), not from the parent header.

## Impact on Users

- **Node operators, finalizers, stakers:** no consensus or finality impact was found. Admission, `fin`, and (disabled) `pos_payout` depend only on the hash, which resolves only against decisions this node validated. Nothing on a consensus path reads `signatures` or the height and round bytes.
- **Anyone who reads a header's certificate as evidence:** the PoW chain's headers are not self-verifying. A tool, explorer or future light client that checks a header's certificate against the roster will find blocks on the canonical chain whose certificates fail, and cannot tell from the header alone whether a given BFT decision really had a quorum. That is the property the design overview says the signatures are there to provide (`:101`).
- **In-repo consumers of header fat pointers, checked one by one:**
  - GUI: `viz2.rs:57` renders the full pointer (`to_string`, including every signature) into the PoW block inspection, and `zebra-gui/src/viz_gui.rs:115` prints it as "BFT pointer". A forged list is displayed as if it were a certificate. `viz2.rs:258`, `:820` and `:863` read only the hash; `viz2.rs:778` reads the node's own `fat_pointer_to_tip`, not a header.
  - RPC: `get_tfl_fat_pointer_to_bft_chain_tip` (`methods.rs:2189-2201`) and both `getblocktemplate` paths (`:3623-3635`, `:3717-3727`) return `fat_pointer_for_template`, which returns a header's pointer in the fallback case and a proposer-chosen `previous_block_fat_ptr` in the non-tip case. `getblock` exposes the pointer only inside the raw serialized header.
  - Wallet: `C:\crosslink\wallet\src` does not read fat pointers at all (no match for `fat_pointer`, `FatPointer`, `validate_signatures` or `vote_signature`).
  - Light clients: the lightwalletd compact block sends an empty header (`zebrad/src/lightwalletd.rs:2097`, `header: Vec::new()`), so no in-repo light client sees the pointer.
- **Chain history:** once forged certificates are buried on the new testnet, a later rule that requires valid certificates cannot be applied from genesis without invalidating that history; it would need an activation height.
- **Test-format export:** `viz2.rs:652-688` exports each BFT block with `blocks[i + 1].previous_block_fat_ptr` as its certificate. A route B certificate would then reach `ForceFeed`, whose `decide` panics on `validate_signatures` failure (`bft.rs:1079-1080`), so a replay of that export aborts. This is a test-tooling consequence, inferred from reading, not run.

## Technical Details / Code Analysis

**The type and its only verifier** (`librustzcash/zcash_primitives/src/bft.rs:980-984`, `:1086-1106`, abridged):

```rust
pub struct FatPointerToBftBlock {
    #[serde(with = "serde_big_array::BigArray")]
    pub vote_for_block_without_finalizer_public_key: [u8; 76 - 32],
    pub signatures: Vec<FatPointerSignature>,
}

pub fn validate_signatures(&self, vote_namespace: &[u8; 32]) -> bool {
    let mut batch = ed25519_zebra::batch::Verifier::new();
    for (vote, signature) in self.inflate() {
        let vk_bytes = ed25519_zebra::VerificationKeyBytes::from(vote.validator_address.0);
        let sig = ed25519_zebra::Signature::from_bytes(&signature);
        let msg = vote.to_bytes();
        // namespace handling elided
        batch.queue((vk_bytes, sig, &msg[..]));
    }
    batch.verify(rand::thread_rng()).is_ok()
}
```

It verifies each signature under the key written next to it. It has no roster, so on its own it would accept a certificate signed by any key the miner generates.

**The admission gate resolves by hash** (`zebra-crosslink/zebra-state/src/new_network/bft.rs:396-403`; the two-line source comment above it at `:394-395` is omitted):

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

Nothing after this point reads `child_fat_pointer` again except through `child_index`.

**The only signature check is on this node's own decisions** (`bft.rs:1076-1081`):

```rust
        let vote_namespace = namespace_for_bft_height(hardforks, new_block.height as u64);
        if !fat_pointer.validate_signatures(&vote_namespace) {
            panic!("Signatures are not valid. Rejecting block.");
        }
```

**Proposal validation checks the parent certificate by hash** (`bft.rs:888-895`):

```rust
        if new_block.previous_block_fat_ptr.points_at_block_hash() != chain.fat_pointer_to_tip.points_at_block_hash() {
            tracing::warn!(
                "Block has invalid previous block fat pointer hash: was {} but should be {}",
                new_block.previous_block_fat_ptr.points_at_block_hash(),
                chain.fat_pointer_to_tip.points_at_block_hash(),
            );
            return fail;
        }
```

**And templates hand that field out** (`bft.rs:298-305`):

```rust
    if at_height == 0 || at_height as usize - 1 >= bft_blocks.len() {
        return None;
    }
    if at_height as usize == bft_blocks.len() {
        Some(fat_pointer_to_tip.clone())
    } else {
        Some(bft_blocks[at_height as usize].previous_block_fat_ptr.clone())
    }
```

**The fallback copies the parent header's pointer** (`bft.rs:577-580`):

```rust
    parent_hash
        .and_then(|parent_hash| read_state.any_chain_block_header(parent_hash.into()))
        .map(|hdr| hdr.fat_pointer_to_bft_block.clone())
        .unwrap_or_else(FatPointerToBftBlock::null)
```

**Honest certificates are built in roster order and legitimately differ between nodes** (`tenderlink/src/lib.rs:207-221`, abridged):

```rust
    FatPointerToBftBlock {
        vote_for_block_without_finalizer_public_key,
        signatures: round_data.msg_val_sigs
            .iter()
            .map(|x| &x[1])
            .enumerate()
            .filter_map(|(roster_i, (value_id, commit_signature))| {
                if *value_id == round_data.proposal_id && *commit_signature != TMSig::NIL {
                    Some(FatPointerSignature {
                        pub_key: roster[roster_i].pub_key,
                        vote_signature: commit_signature.0,
                    })
                } else { None }
            })
            .collect(),
    }
```

Two consequences matter for the fix. Signatures come out in roster index order, so that order can be required. And each node lists the precommits it happened to receive, so a check cannot demand equality with this node's own list (`crosslink-design-overview-20260914.md:169`; `FINALITY.md:870-871`).

**Is this deliberate?** Partly.

- Resolving by hash is deliberate and documented (`FINALITY.md:159-167`), as is comparing certificates "by BFT block hash, not by the whole fat pointer: two honest nodes can carry different signature sets for the same decision" (`:870-871`).
- Leaving the signatures unchecked is not documented as a decision anywhere found. `FINALITY.md:304` lists "Valid context: `H.context_bft` is bft-block-valid" and `:317` says Zebra Crosslink "enforces all five rules"; the doc treats resolution in the local store as satisfying Valid Context and never says the header's signatures are or are not consensus-checked. `IMPLEMENTATION.md` says nothing either way. The design overview says the opposite of a hash-only design is the settled choice (`:84`, `:101`).
- `s1_dev` carried `// TODO: check public keys on the fat pointer against the roster` in its decide path (`lib.rs:724` at `64046aeb`), which shows the missing roster check was known; dev removed the TODO without implementing it.
- The test harness depends on the current behaviour: `create_pos_and_ptr_to_finalize_pow` builds certificates from a `sigs` argument that every caller passes as `&[]` (`zebrad/tests/crosslink.rs:1989-2026`; one test is named `crosslink_gen_pow_and_no_signature_no_roster_pos`), and `HARNESS_PARAMETERS` uses `BftBootstrap::Supplied` (`zebra-crosslink/src/test_format.rs:631-644`).

**Related defect found during validation: the fat pointer is outside the Equihash input but inside the block hash.** This is a separate issue and should be filed and severity-rated on its own; it is recorded here because the unchecked certificate bytes are what make it free to exploit.

- The header serializes the fat pointer after the Equihash solution (`zebra-chain/src/block/serialize.rs:90-94`).
- The Equihash input is the first `INPUT_LENGTH = 4 + 32 * 3 + 4 * 2` bytes plus the nonce (`zebra-chain/src/work/equihash.rs:61`, `:81-90`), so the fat pointer is not in it.
- The block hash is `sha256d` over the whole serialized header (`zebra-chain/src/block/hash.rs:88-95`), and the difficulty filter compares that hash to the target (`zebra-consensus/src/sync_verify.rs:96-112`, `block/check.rs:130`).

So after one Equihash solve, a miner can vary the 12 height and round bytes, or any signature bytes, and re-hash with `sha256d` until the block meets the target, without solving Equihash again. With the certificate unchecked, 96 bits are free even with no signatures. By reading, this turns the PoW into `sha256d` grinding for any post-activation block, which a GPU does many orders of magnitude faster than honest Equihash miners solve. Checking certificates as proposed below removes the height and round freedom and the garbage-signature freedom, but not the choice of which valid quorum subset to include, so it narrows the grind and does not close it. The header layout is the same on `s1_dev`. Not tested; inferred from reading the four files above.

## Recommendations

1. **Make the header certificate a checked certificate (restores Valid Context for the header's full contents).**
   - Record, per decided BFT height, the tenderlink roster that voted there and its vote namespace. Add `voting_rosters: Vec<(Arc<[SortedRosterMember]>, [u8; 32])>` to `BftChain`, indexed like `blocks`; consecutive heights usually share a roster, so share the `Arc`. Fill it in `bootstrap` (index 0: the nil roster, `terminated_finalizers_at(hardforks, 0, 0)` applied to `[]`), in `finish_decision` (index `next_bft_height`: the `roster` it already computes at `bft.rs:1184`, plus the namespace it sends at `:1208`), and in `restore` (the per-height `roster` at `:1273` and `namespace_for_bft_height(hardforks, this_bft_height)`). This reuses `SortedRosterMember` and the existing roster derivation, which is already deterministic across nodes (`FINALITY.md` §8.1).
   - Expose tenderlink's decision threshold (`tenderlink/src/lib.rs:506-507`, `:1074-1081`) as one public function so the gate cannot drift from the engine.
   - Add a pure leaf function and call it from `admit_fat_pointer` immediately after `child_index` resolves; a failure is `Some(CrosslinkVerdict::Reject)`, which is permanent because the roster and namespace at a decided height are fixed by the decided chain and finalized PoW state.

   ```rust
   pub fn certificate_is_valid(
       fat_pointer: &FatPointerToBftBlock,
       bft_height: u64,
       voting_roster: &[SortedRosterMember],
       vote_namespace: &[u8; 32],
   ) -> bool {
       let vote = fat_pointer.get_vote_template();
       if vote.height != bft_height || !vote.typ {
           return false;
       }
       let active = &voting_roster[..voting_roster.len().min(ACTIVE_ROSTER_MAX_N)];
       let total_stake: u64 = active.iter().map(|m| m.stake).sum();
       if total_stake == 0 {
           // BFT genesis is decided by the nil roster and its certificate carries no signatures.
           return fat_pointer.signatures.is_empty() && vote.round == 0;
       }
       let mut signed_stake: u64 = 0;
       let mut next = 0;
       for sig in &fat_pointer.signatures {
           let Some(offset) = active[next..].iter().position(|m| m.pub_key == sig.pub_key) else {
               return false;
           };
           signed_stake = signed_stake.saturating_add(active[next + offset].stake);
           next += offset + 1;
       }
       if signed_stake < tenderlink::decision_threshold(total_stake) {
           return false;
       }
       fat_pointer.validate_signatures(vote_namespace)
   }
   ```

   Requiring strictly increasing roster positions rejects duplicates and non-members in one pass and matches the order `round_data_to_fat_pointer` already produces.
   - Apply the same function in `BftRunner::validate` to `new_block.previous_block_fat_ptr` at `bft_height - 1` when `bft_height > 0`, returning `fail`. This closes route B, so every pointer `fat_pointer_to_block_at_height` hands out is valid by construction.
   - Memoize the verdict for the last few pointers seen (a small bounded map keyed by the blake3 of the pointer bytes), since consecutive PoW blocks usually carry the same pointer; keep it bounded per the AGENTS.md rule "Bound all loops/allocations over attacker-controlled data". Per-block cost without the cache is one batch Ed25519 verification of at most 100 signatures.
   - Update `FINALITY.md` §3.4 and §6.2 to say what Valid Context checks, and remove the implication that hash resolution alone satisfies it.

2. **Considered alternatives.**
   - *Declare the header pointer hash-only and require `signatures` to be empty.* Smaller headers and no garbage surface, but it contradicts the design overview's settled choice (`:84`, `:101`) and gives up the self-verifying link. This is a design decision for the protocol owners; if chosen, the height and round bytes must still be pinned to canonical values.
   - *Require the header list to equal this node's own list.* Impossible: honest nodes legitimately hold different subsets.
   - *Verify signatures without the roster and quorum.* Worthless: anyone can sign with a fresh key.
   - *Verify only in consumers (GUI, RPC).* Leaves the chain data untrustworthy for everyone else.
   - *Fix only the Equihash grind by binding the fat pointer into the Equihash input.* Needed anyway (see below) but does not make the certificate valid.

3. **Tests** (test-format scenarios in `zebrad/tests/crosslink.rs`, driving the real commit loop; sign with keys from `finalizer_key_from_seed`).
   - A staking scene that yields a non-empty roster, a decided BFT block, then one PoW block per malformed certificate, each loaded `SHOULD_FAIL` with `push_instr_expect_pow_chain_length` unchanged: no signatures, one garbage signature, a signer not in the roster, valid signatures below the threshold, a duplicated signer, signatures out of roster order, template height off by one, commit bit clear. A control block with the honest certificate is accepted.
   - A BFT proposal whose `previous_block_fat_ptr` has the right hash and a garbage signature is loaded `SHOULD_FAIL` and `push_instr_expect_pos_chain_length` stays put.
   - A restart scenario: decide several BFT heights, restart the node, then submit a PoW block citing a pre-restart height with a valid certificate; it must be accepted, which proves `restore` rebuilt `voting_rosters`.
   - Migrate the existing scenes that pass `sigs = &[]` at heights with a non-nil roster to real signatures; scenes with the nil roster keep passing once `create_pos_and_ptr_to_finalize_pow` builds their certificate at round 0 (it passes round 1 today, `zebrad/tests/crosslink.rs:2025`, which the nil-roster branch above rejects so that a signature-free certificate has no free bytes). A unit test on `certificate_is_valid` alone is fine, since it is a pure leaf.

4. **Rollout.**
   - This changes PoW block validity and BFT block validity, so it is a consensus change. Land it before the new testnet's genesis so the chain never contains an invalid certificate.
   - ClT0 (`s1_dev`) has the same hash-only gate. A backport is a hard fork of an existing chain and would need an activation height; before choosing one, replay ClT0's history against the new rule to confirm honest history passes (expected, since honest certificates come from `round_data_to_fat_pointer` with a counted quorum, but not verified here). Given ClT0 is being replaced, a backport is optional.
   - **File the Equihash-input issue separately and fix it before genesis as well.** The fix there is to bind the fat pointer into the Equihash input (for example, a digest of the serialized pointer included in the input the solver and verifier hash), which is a header-format and consensus change.

## Validation Information

**Verdict: PARTIALLY CONFIRMED. Severity: Medium.**

| Claim | Verified at |
|-|-|
| `admit_fat_pointer` resolves the child pointer by hash only | `bft.rs:396-403`, read directly; nothing after reads `child_fat_pointer` |
| No signature, quorum, roster or height/round check on the PoW path | `bft.rs:366-523`; only caller `new_network.rs:3008`; `zebra-consensus` never reads the pointer (`block.rs:371-373`) |
| `validate_signatures` has one caller, on this node's own decisions | grep over `C:\crosslink` for `validate_signatures`: `bft.rs:1079` only |
| An empty signature list passes `decide` | inferred: bootstrap decides genesis with `&[]` (`bft.rs:1396`, `:1420`) and would panic at `:1080` otherwise |
| Proposal validation checks `previous_block_fat_ptr` by hash only | `bft.rs:888-895`; tenderlink does not parse `BftBlock` (`tenderlink/src/lib.rs`, only type imports) |
| Templates hand out `previous_block_fat_ptr` for non-tip heights | `bft.rs:298-305`, `:573-575` |
| Template fallback copies the parent header's pointer (dev only) | `bft.rs:577-580`; `s1_dev` `lib.rs:1895-1907` falls back to null |
| `fin` does not depend on the signatures | `fin.rs:131-135` |
| Wallets and lightwalletd light clients do not read header pointers | no matches in `wallet/src`; `lightwalletd.rs:2097` sends an empty header |
| GUI displays the full pointer including signatures | `viz2.rs:57`, `zebra-gui/src/viz_gui.rs:115` |
| Same defect on `s1_dev`; roster TODO dropped on dev | `64046aeb:zebra-crosslink/zebra-crosslink/src/lib.rs:301-375`, `:724`; removed in `379170fe` |
| Fat pointer is outside the Equihash input, inside the block hash | `serialize.rs:90-94`, `equihash.rs:61`, `:81-90`, `hash.rs:88-95`, `sync_verify.rs:96-112` |

**Severity justification: Medium.**

*Why not High:* by reading, no node-level consensus, finality or funds effect follows from the missing check itself. Admission and `fin` resolve the hash only against decisions this node validated through tenderlink, so a forged list cannot finalize anything, split nodes, or change rewards (the variable reward that once compared certificates is disabled, `bft.rs:449-451`). No in-repo consumer treats the signatures as evidence: the wallet ignores them and light clients never receive them. The PoW-grinding consequence is real and serious but has its own root cause (header layout) and does not go away with signature checking, so it is rated in its own finding rather than inflating this one.

*Why not Low:* the design documents say the header signatures exist so a PoW block's link can be verified without a PoS service, and on the current code that property is simply false for the whole chain. On an adversarial public testnet, one cheap block (route A) or one decided proposal (route B, which makes honest miners do it) permanently writes invalid certificates into canonical history, and a later fix then needs an activation height instead of applying from genesis. The unchecked bytes are also the free grinding surface for the Equihash issue.

**Corrections made during validation.**

1. The original finding presents the gap as a dev change. It already exists on `s1_dev` (`call_from_state_to_crosslink_to_ask_about_fat_pointers`), so ClT0 is affected; dev moved the code and dropped the roster TODO.
2. "Later honest templates reuse it via the fallback" was narrowed: that happens only when no decided block qualifies for the template. The more reliable route to honest templates carrying an invalid certificate was missed: a proposer's `previous_block_fat_ptr`, checked by hash only in `validate`, is what `fat_pointer_to_block_at_height` returns for every non-tip height.
3. "Light clients, wallets, finalizer displays" was narrowed: the in-repo wallet does not read fat pointers and lightwalletd sends light clients an empty header. The affected in-repo consumers are the GUI's PoW inspection and the template and tip RPCs; external header readers are the real audience.
4. The finding does not mention that `validate_signatures` on its own is not a certificate check (no roster, no quorum, empty list passes), so the fix has to add roster membership and quorum, not just call the existing function.
5. Added the Equihash-input observation as a separate, related defect.

**Cross-references.**

- `template-fat-pointer-is-chosen-against-the-live-tip-not-the-template-parent.md` and `template-fat-pointer-walk-scans-the-whole-bft-chain-under-the-read-lock.md` change `fat_pointer_for_template`; recommendation 1 does not touch that function, but after it the fallback's parent pointer is valid by admission, so neither fix needs to re-check signatures.
- `tail-confirmation-checks-carried-headers-against-their-own-nbits-so-a-proposer-can-fake-confirmations.md` is the other place where PoW evidence inside a certificate is taken at less than face value; the Equihash grind noted here would also cheapen the carried headers' PoW if they were checked against chain difficulty.
- `new-bft-code-breaks-agents-md-cast-comment-and-misleading-default-rules.md`: `bft.rs:411` sits inside `admit_fat_pointer`; any edit here should add the cast comments that finding asks for.
