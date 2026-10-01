# The "claimed @" lookup in the transaction history starts at the culled-slice `index`, walks toward older transactions and takes the first `ClaimUnstake` without comparing `bond_key()`, so a bond's rows are labelled with a different bond's claim height and amount, a bond that was never claimed reads as claimed, and the label changes as the list scrolls

**Severity**: Medium
**Validation Status**: Confirmed
**Location**: `zebra-gui/src/ui.rs:3198-3211` (the `'withdrawal` search), `:3213-3220` (the "claimed @" label built from it), `:3166-3234` (the whole `'get_label` block), `:3170` (`sent_stake` excludes `BeginUnstake` and `ConvertReward`); `zebra-gui/src/ui.rs:2918-2932` (the list is built and reversed), `:2974-2984` (culling; `index` is relative to `culled_txs`); `wallet/src/lib.rs:1281-1285` and `:4406` (wallet list is ascending by height), `:4744-4748` (a claimed bond leaves `stake_positions_unbonded`); `librustzcash/zcash_primitives/src/transaction/mod.rs:1652-1660` (`bond_key()` is `unique_pubkey` in every variant)
**Found by agent:** /code-review high, GUI/UX (Claude Fable 5.1), 2026-09-30; validated 2026-10-01 at dev d00b6a44ef594
**In scope of audit?** Yes: a wrong stake figure and a wrong status shown in the `zebra-gui` transaction history. The search was introduced by `102cd522` ("Wallet GUI: first-pass of keeping stake position info updated", 2026-01-28) and reshaped by `3fb295e3` (2026-04-09). The culling it collides with landed one to two days earlier (`9c849fc0`, `a0eb6ad6`, `e4117561`, 2026-01-26 and 27).

## Description

Each staking row in the transaction history gets a descriptive label. While a bond is alive, the label comes from the bond rosters ("Staked @ 400 to F, now 10.000 cTAZ"). Once nothing in the rosters matches, the code looks for the bond's withdrawal so it can say "claimed @ H for X cTAZ". That lookup is (`ui.rs:3198-3211`):

```rust
let withdrawal = 'withdrawal: {
    let mut tx_i = index+1;
    while tx_i < txs.len() {
        if txs[tx_i].is_on_bc() && txs[tx_i].h.is_in_block() {
            if let (Some(staking_action), WalletTxKind::ClaimUnstake) = (txs[tx_i].staking_action, txs[tx_i].kind()) {
                // NOTE: ignoring fee
                let b = WalletTxPart::from_staking_action(txs[tx_i].staking_action);
                break 'withdrawal Some((b, txs[tx_i].h));
            }
        }
        tx_i += 1;
    }
    None
};
```

Three things are wrong with it, and they compound.

1. **No bond match.** The inner `staking_action` binding is never used. The first `ClaimUnstake` found is accepted, whichever bond it belongs to.
2. **Wrong direction.** `txs` is newest first. A bond's claim is always newer than its Stake and BeginUnstake, so it sits at a smaller index than they do. The loop goes from `index+1` upward, toward older transactions. With the list scrolled to the top it can never reach the row's own claim; it can only reach claims of earlier bonds.
3. **Wrong coordinate.** `index` counts from the start of the visible slice `culled_txs`, but it is used to index the full `txs`. Once the list is scrolled, the search starts somewhere unrelated to the row, and the start moves by one for every row scrolled.

There is also a gating problem: `BeginUnstake` and `ConvertReward` rows skip the roster check entirely (`sent_stake` is false for them, `:3170`), so they run this search even while their bond is still alive.

The original finding is correct on every point. Validation adds two consequences it did not state: with a single bond the "claimed @" label never appears at the top of the list at all, and a bond that has not been claimed (or a Convert bond that is still active) is labelled as claimed.

## Attack Scenario and Steps

No adversary is involved. This is the reproduction, with a concrete two-bond history. F is the target finalizer.

1. Stake 1 cTAZ to F; mined at height 100. Call this bond A.
2. On a Staking Day, unstake bond A; mined at 200.
3. Withdraw bond A; mined at 300, for 1.02 cTAZ.
4. Stake 10 cTAZ to F; mined at 400. Call this bond B.
5. Unstake bond B; mined at 520. Do not withdraw it yet.
6. Look at the transaction history, scrolled to the top (so `culled_pre_n == 0` and `index` is the true position):

| `txs` index | Row | Label shown | Correct? |
| :- | :- | :- | :- |
| 0 | BeginUnstake B @ 520 | `Unstaked @ 520, claimed @ 300 for 1.020 cTAZ` | **No.** B has not been claimed. 300 and 1.020 belong to bond A. |
| 1 | Stake B @ 400 | `Staked @ 400 to F (unstaked), now 10.000 cTAZ` | Yes (from the unbonded roster) |
| 2 | ClaimUnstake A @ 300 | `Withdrawn @ 300` | Yes |
| 3 | BeginUnstake A @ 200 | `Unstaked @ 200` | Incomplete: its own claim at index 2 is never examined |
| 4 | Stake A @ 100 | `Staked @ 100 to F` | Incomplete, same reason |

7. Withdraw bond B; mined at 600, for 10.3 cTAZ. Bond B leaves both rosters. Every row moves down one index:

| `txs` index | Row | Label shown | Should be |
| :- | :- | :- | :- |
| 0 | ClaimUnstake B @ 600 | `Withdrawn @ 600` | same |
| 1 | BeginUnstake B @ 520 | `Unstaked @ 520, claimed @ 300 for 1.020 cTAZ` | `Unstaked @ 520, claimed @ 600 for 10.300 cTAZ` |
| 2 | Stake B @ 400 | `Staked @ 400 to F, claimed @ 300 for 1.020 cTAZ` | `Staked @ 400 to F, claimed @ 600 for 10.300 cTAZ` |
| 3 | ClaimUnstake A @ 300 | `Withdrawn @ 300` | same |
| 4 | BeginUnstake A @ 200 | `Unstaked @ 200` | `Unstaked @ 200, claimed @ 300 for 1.020 cTAZ` |
| 5 | Stake A @ 100 | `Staked @ 100 to F` | `Staked @ 100 to F, claimed @ 300 for 1.020 cTAZ` |

   The 10 cTAZ bond staked at height 400 reads as claimed at height 300, before it existed, for a tenth of its value.

8. With enough newer transactions above these rows to scroll, scroll down. Take the BeginUnstake A row at true position 4 in the step 7 list. `culled_pre_n` is `floor(scroll / row_height) - 1`, floored at 0; the row's `index` is `4 - culled_pre_n`; the search starts at `index + 1`.

| `culled_pre_n` | `index` of the row | search starts at | first claim found | label |
| :- | :- | :- | :- | :- |
| 0 | 4 | 5 | none | `Unstaked @ 200` |
| 1 | 3 | 4 | none | `Unstaked @ 200` |
| 2 | 2 | 3 | Claim A @ 300 | `Unstaked @ 200, claimed @ 300 for 1.020 cTAZ` (right, by accident) |

   The label on a row that is still on screen changes as the list scrolls. In a longer history, a start point above a newer bond's claim picks that one instead.

**Attack Requirements and Assumptions:**

- All platforms. The defect is index logic in `ui.rs`.
- Flow: the transaction history in the left pane, for the user wallet tab and the faucet (miner) wallet tab alike (`ui.rs:2921-2925`).
- For a wrong claim to be shown at the top of the list: at least one mined `ClaimUnstake` in the history, older than a row that reaches the search. Rows that reach it are: `BeginUnstake` (always), `ConvertReward` (always), and `Stake` or `Retarget` whose bond is in neither roster (claimed).
- The GUI enables unstake and withdraw only on Staking Day (`ui.rs:2370`, `:2471`, `:2702`), so the first wrong label appears after a participant has completed one full bond lifecycle and begun another. How long that takes on the new testnet was not determined; any unbonding delay enforced by consensus was not traced.
- The scroll behaviour needs a history taller than the viewport by at least two rows.

## Impact on Users

- **A wrong height and a wrong cTAZ amount on a bond's history rows.** The figures are real, but they belong to a different bond.
- **A false status.** An unstaked but unclaimed bond reads "claimed". A participant who trusts it may believe a withdrawal already happened and look for funds that are still in the bond. A still-active Convert bond can read "claimed" in the same way.
- **The correct label is effectively unreachable.** With a single bond, or for the oldest bond, the row's own claim is never found at the top of the list, so the feature shows nothing where it should show something.
- **Labels change while scrolling**, which reads as the history being unreliable.
- **No funds are affected and nothing acts on the label.** The right-hand amount column, the `Withdrawn @ H` row itself, and the Staked and Withdrawable balances are computed separately and are correct.

## Technical Details / Code Analysis

**Order of the list.** The wallet keeps `txs` ascending by `h`, ties in discovery order (`wallet/src/lib.rs:1281-1285`, re-sorted at `:4406`):

```rust
fn update_insert_i(txs: &[WalletTx], insert_i: &mut usize, block_h: BlockHeight) {
    // put at the *end* of txs at the same height
    // i.e. primarily sorted by mined height, secondarily by discovered_time
    *insert_i += txs[*insert_i..].partition_point(|tx| tx.h <= block_h);
}
```

The lifecycle stages (`MEMPOOL`, `SENT`, `BUILT`, `PROPOSED`) are the largest `u32` values (`wallet/src/lib.rs:145-150`), so unmined transactions sort last. The GUI appends the in-flight local transactions and reverses (`ui.rs:2927-2931`):

```rust
for i in 0..local_n {
    txs.push(locals[i]);
}
txs.reverse();
txs
```

So `txs[0]` is the newest entry and indices grow toward older transactions.

**What `index` means.** Only the visible rows are laid out (`ui.rs:2974-2984`):

```rust
let culled_pre_n    = ((ui.scale(scroll) / transaction_element_height).floor() as usize).saturating_sub(1);
let culled_in_bgn_o = culled_pre_n;
let culled_in_n     = 1 + (viewport_h / transaction_element_height).ceil() as usize + 1;
let culled_in_end_o = (culled_pre_n + culled_in_n).min(txs.len());
```

```rust
let culled_txs = &txs[culled_in_bgn_o .. culled_in_end_o];
for (index, tx) in culled_txs.iter().enumerate() {
```

The row's true position in `txs` is `culled_pre_n + index`. The code two lines below knows this (`if index + culled_pre_n > 0`, `:2985`), but the search uses bare `index` against `txs`. The search therefore covers true positions `index + 1` to the end, which begins `culled_pre_n` rows before the row's own successor.

**How a bond is identified.** All three lifecycle rows carry the same key (`zcash_primitives/src/transaction/mod.rs:1651-1660`):

```rust
/// The bond this action operates on: `unique_pubkey` in every variant.
pub fn bond_key(&self) -> [u8; 32] {
    match self {
        StakingAction::CreateNewDelegationBond { unique_pubkey, .. }
        | StakingAction::BeginDelegationUnbonding { unique_pubkey, .. }
        | StakingAction::WithdrawDelegationBond { unique_pubkey, .. }
        | StakingAction::RetargetDelegationBond { unique_pubkey, .. }
        | StakingAction::ConvertFinalizerRewardToDelegationBond { unique_pubkey, .. } => *unique_pubkey,
    }
}
```

The roster lookups a few lines above the search already match on it (`ui.rs:3174`, `:3185`: `bond.0 == staking_action.bond_key()`), and the wallet matches claims to bonds the same way (`wallet/src/lib.rs:4744-4746`). The search is the one place that does not. A comment in the primitives says a bond key is single-use ("a duplicate bond is rejected", `mod.rs:1625-1626`), so a key has at most one claim on the best chain; that uniqueness was read from the comment, not traced through consensus.

**Which rows reach the search** (`ui.rs:3169-3196`). `sent_stake` is true only for `Stake` and `Retarget`. Those rows return early if the bond is in `staked_roster_bonded` or `staked_roster_unbonded`. `ClaimUnstake` rows return early with `Withdrawn @ H`. Everything else with a staking action falls through to the search:

- `Stake` or `Retarget` of a bond in neither roster. The wallet removes a bond from `stake_positions_unbonded` when it sees its `WithdrawDelegationBond` (`wallet/src/lib.rs:4744-4748`), so this means claimed. The search is appropriate here, and only wrong in how it searches.
- `BeginUnstake`, always, including while the bond is unbonding and unclaimed (step 6, index 0).
- `ConvertReward`, always, including while the converted bond is active. Such a row never gets the "now X cTAZ" label that a `Stake` row gets, and with an older claim in the history it reads `Converted To Bond @ H, claimed @ H2 for X cTAZ`.

**The amount.** `WalletTxPart::from_staking_action` puts a withdrawal's `amount_zats` in `spent_zats` (`wallet/src/lib.rs:601-603`), and the label prints `str_from_ctaz(b.spent_zats.into_u64())` (`ui.rs:3216`, `:3218`). The formatting is correct; the transaction it is taken from is not.

**Not deliberate.** The only comments on the block are `// NOTE: this assumes that objects maintain bond keys after retargeting` (`:3168`), `// TODO: compress` (`:3191`) and `// fallback if not found anywhere, not ideal` (`:3222`). The first shows the author intended bond keys to be the identity. `git blame` shows the search was written two days after culling landed, by a different author; nothing in either commit message mentions the interaction.

## Recommendations

1. **Find the claim by bond key over the whole list.** Replace `ui.rs:3198-3211` with a search that does not depend on position:

   ```rust
   let bond_key = staking_action.bond_key();
   let mut withdrawal = None;
   for other in &txs {
       if !(other.is_on_bc() && other.h.is_in_block()) {
           continue;
       }
       let Some(other_action) = other.staking_action else { continue; };
       if other.kind() == WalletTxKind::ClaimUnstake && other_action.bond_key() == bond_key {
           // NOTE: ignoring fee
           withdrawal = Some((WalletTxPart::from_staking_action(other.staking_action), other.h));
           break;
       }
   }
   ```

   - Put it in a small free function, `claim_for_bond(txs: &[WalletTx], bond_key: [u8; 32]) -> Option<(WalletTxPart, BlockHeight)>`, next to `bank_balance` (`ui.rs:65`), so item 4 can test it.
   - Scanning the whole list removes all three defects at once: no dependence on `index`, on direction, or on same-height ordering.
   - Cost: one pass over `txs` per visible staking row per frame. At most about a dozen rows are visible, and the frame already clones and reverses the whole list (`ui.rs:2918-2932`).

2. **Give `ConvertReward` rows the roster lookup.** At `ui.rs:3170`, include `WalletTxKind::ConvertReward` in `sent_stake`, so an active converted bond reads `Converted To Bond @ H to F, now X cTAZ` like a staked one, and reaches the claim search only once the bond has left both rosters. `target_finalizer_pk()` already returns `this_finalizer` for that variant (`mod.rs:1741`).

3. **Give `BeginUnstake` rows an honest label while the bond is unclaimed.** Before the claim search, for a `BeginUnstake` row, look the bond up in `staked_roster_unbonded`; if it is there and its value is not the `u64::MAX` placeholder, label it `Unstaked @ H, now X cTAZ`. With item 1 in place this is an improvement, not a correctness fix: an unclaimed bond no longer finds a claim, and falls back to `Unstaked @ H`.

4. **Tests.**
   - Harness: `zebra-gui` has no UI-driving harness; its tests are `#[test]` units over pure functions (`ui.rs:5326-5353`, `lib.rs:2400` onward). The cheapest real check is a unit test of `claim_for_bond` beside `roster_identity_tests`.
   - Build the step 7 list with `WalletTx::with_fake_data(...)` (`wallet/src/lib.rs:853`), then set `staking_action` on each (`wallet::StakingAction` is re-exported, `wallet/src/lib.rs:64`; all `WalletTx` fields are public). Use two distinct `unique_pubkey` values.
   - Assert: `claim_for_bond(&txs, key_b)` returns height 600 and 10.3 cTAZ; `claim_for_bond(&txs, key_a)` returns height 300 and 1.02 cTAZ; with bond B's claim removed, `claim_for_bond(&txs, key_b)` is `None`; a claim whose `status` is `SoftFail` is ignored.
   - Manual check: run the reproduction steps 1 to 7; every row must match the "Should be" column, and no label may change while scrolling.
   - Not run during validation (no builds allowed on the box).

**Considered alternatives, not chosen.**

- Keep the positional search and fix the coordinate (`culled_pre_n + index`) and direction (walk toward index 0). Still needs the key match, and relies on a claim never sharing a height with, and sorting after, its own BeginUnstake. Matching by key needs neither assumption.
- Have the wallet publish claimed bonds (key, height, amount) in `WalletState`, so the GUI does not scan. Cleaner in the long run, but it adds wallet state for a label; the scan is cheap and local.
- Drop the "claimed @" label and leave `Withdrawn @ H` as the only record. Simplest, but it throws away the one place the history ties a stake to its outcome.

**What the user should see after the fix.**

- A Stake, Retarget or BeginUnstake row of a claimed bond: `… claimed @ H for X cTAZ`, where H and X are that bond's own withdrawal, identical on every row of the bond and at every scroll position.
- A BeginUnstake row of an unclaimed bond: `Unstaked @ H` (item 1 alone) or `Unstaked @ H, now X cTAZ` (with item 3). Never "claimed".
- A ConvertReward row of an active bond: `Converted To Bond @ H to F, now X cTAZ` (with item 2).

## Validation Information

**Verdict: CONFIRMED. Severity: Medium.**

| Claim | Verified at |
| :- | :- |
| `txs` is newest first | `wallet/src/lib.rs:1281-1285`, `:4406`, `:145-150`; `zebra-gui/src/ui.rs:2927-2931` |
| `index` is relative to `culled_txs`, not `txs` | `ui.rs:2983-2984`; contrast `:2985` |
| The search starts at `index+1` and walks older | `ui.rs:3199-3208` |
| The first `ClaimUnstake` is taken with no key comparison | `ui.rs:3202-3205` (the bound `staking_action` is unused) |
| A bond's rows share one key | `zcash_primitives/src/transaction/mod.rs:1651-1660`; used at `ui.rs:3174`, `:3185` |
| A claimed bond is in neither roster, so its Stake row reaches the search | `wallet/src/lib.rs:4744-4748`; `ui.rs:3172-3189` |
| `BeginUnstake` and `ConvertReward` rows always reach the search | `ui.rs:3170`, `:3192-3198` |
| The two-bond example labels | Walked by hand through `ui.rs:3166-3227` for each row |
| The search start moves with scroll | `ui.rs:2974`, `:3199`; table in step 8 |
| The amount shown is the found claim's `amount_zats` | `wallet/src/lib.rs:601-603`; `ui.rs:3216`, `:3218` |
| Not deliberate | Comments at `ui.rs:3168`, `:3191`, `:3222`; `git blame -L3198,3211`, `-L2974,2984` |

Inferred, not verified: that a bond key can have at most one claim on the best chain (from the comment at `mod.rs:1625-1626`); how soon a testnet participant reaches a second bond lifecycle.

**Severity justification.**

- *Why not High:* the wrong figures are in a secondary descriptive label. The amount column on the same row, the `Withdrawn @ H` row and the balances are right, and nothing acts on the label. It also needs a completed claim plus a second bond, which a participant reaches only after a full stake, unstake and withdraw cycle; the companion finding G1 fires on the first stake.
- *Why not Low:* it states a specific height and cTAZ amount that belong to another bond, and it says "claimed" about a bond that still holds the user's funds. That is a wrong money statement, not merely confusing wording. Repeat staking is the main activity on a staking testnet, so regular participants will meet it, and the scroll-dependent label makes the whole history look untrustworthy.

**Corrections made during validation.**

1. The review said the second bond's rows read `Unstaked @ H, claimed @ H2 for X cTAZ`. More precisely: the BeginUnstake row reads that as soon as it is mined, before any claim of that bond exists; the Stake row reads its "claimed @" variant only after the bond itself is claimed and leaves the rosters.
2. Added: with the list at the top, a row can never find its own claim, so the correct label is unreachable for a single bond and for the oldest bond.
3. Added: `ConvertReward` rows take the same path and can read "claimed" while the bond is active; they also never get a "now X cTAZ" label.
4. The review said the search start "moves as the list scrolls". Made exact: it starts `culled_pre_n` rows earlier than intended, and `culled_pre_n` stays 0 until the list has scrolled two full rows (`saturating_sub(1)`, `ui.rs:2974`).

**Cross-references.**

- `edit-stake-modal-shows-bond-amounts-wrong-because-the-fraction-is-not-zero-padded.md` (G1): the other wrong-figure finding. The labels here format with `str_from_ctaz` and are unaffected by it. Independent fixes, any order.
- `chain-view-pans-and-recenters-while-the-pointer-is-over-a-pane-or-modal.md` (G9) and `modal-backdrop-passes-clicks-through-to-the-controls-underneath.md` (G4): both mention transaction rows as click targets. They use the row's `tx_chain_h`, not `index`, and do not interact with this fix.
- Observation, not validated as a finding: the same slice-relative `index` also forms the element ids of each row (`id_index("Transaction", index as u32)`, `ui.rs:3005`), so a row's id changes as the list scrolls. Whether that has a visible effect was not traced.
