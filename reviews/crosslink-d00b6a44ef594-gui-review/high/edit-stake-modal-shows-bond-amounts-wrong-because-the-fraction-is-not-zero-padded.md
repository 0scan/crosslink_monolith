# `format_stake_amount` prints the fractional zats without zero padding, so every bond row in the Edit Stake modal whose fraction is below 0.1 cTAZ shows an inflated amount (0.01 cTAZ reads `0.100 cTAZ`, 1.05 reads `1.500 cTAZ`), and the rows disagree with the totals beside them

**Severity**: High
**Validation Status**: Confirmed
**Location**: `zebra-gui/src/ui.rs:1224-1230` (`format_stake_amount`); its only two callers, `zebra-gui/src/ui.rs:2507` (Withdrawable Bonds rows) and `zebra-gui/src/ui.rs:2754` (Staked Bonds rows), both inside `Modal::Unstake` (titled "Edit Stake", `:2367-2368`); the correct formatter `wallet/src/lib.rs:1159-1168` (`str_from_ctaz`), used for the totals in the same modal at `zebra-gui/src/ui.rs:2426`, `:2565`, `:2657`; the source of the row value, `zebra-gui/src/ui.rs:1640-1641` and `wallet/src/lib.rs:4754-4763`, `:4887-4891`; reward accrual into the bond amount, `zebra-crosslink/zebra-state/src/service/finalized_state/zebra_db/delegation.rs:376-377`; the smallest Stake amount, `zebra-gui/src/ui.rs:2255` and `:2270`
**Found by agent:** /code-review high, GUI/UX (Claude Fable 5.1), 2026-09-30; validated 2026-10-01 at dev d00b6a44ef594
**In scope of audit?** Yes: a wrong stake figure shown by `zebra-gui`. The function was introduced by `9d1e3f3b` ("UI: Refactored code to reduce duplication", 2026-05-07), which only gathered four inline copies of the same arithmetic. The arithmetic itself dates from the first wallet GUI work (December 2025). The identical bug in `str_from_ctaz` was fixed by `5b20864f` ("fix ui ctaz display", 2026-01-13); the inline copies in the unstake modal were missed.

## Description

The Edit Stake modal lists each of the user's bonds with its current value. The value is formatted by a private helper (`ui.rs:1224-1230`):

```rust
fn format_stake_amount(stake_amount: i64) -> String {
    let full = stake_amount / 100_000_000;
    let part = stake_amount % 100_000_000;
    let part_str = format!("{part}00");
    let trim_part = part_str.trim_end_matches("0");
    format!("{}.{} cTAZ", full, &part_str[..trim_part.len().max(3)])
}
```

`part` is the number of zats below one cTAZ, a value from 0 to 99,999,999. It is printed with `{part}`, which drops leading zeros. The eight-digit fraction `01000000` (0.01 cTAZ) therefore prints as `1000000`, and the digits that follow the decimal point start at the first non-zero digit instead of at the tenths place.

The result is wrong exactly when `0 < part < 10_000_000`, that is, whenever the true fraction is non-zero and below 0.1 cTAZ. Each leading zero that is dropped multiplies the displayed fraction by ten. Whole amounts, and amounts whose fraction is 0.1 or more, come out right.

`wallet::str_from_ctaz` (`wallet/src/lib.rs:1159-1168`) does the same job with `{:08}` and is correct. The same modal uses it for "Staked Bonds" and "Withdrawable Bonds" totals and the per-finalizer subtotal, so a single-bond user sees a row and a total that contradict each other.

The original finding is correct. Validation found the reach is wider than the two examples it gave: the row value is not the staked amount but the bond's latest value including rewards, so bonds drift into the broken range by themselves (see Technical Details).

## Attack Scenario and Steps

No adversary is involved. This is the reproduction.

1. Open the Stake modal, paste a finalizer identity, and click the smallest button, "+0.01 cTAZ" (`ui.rs:2255`, or the `("0.01", ONE_cTAZ / 100)` entry at `:2270`). This stakes 1,000,000 zats.
2. Wait for the transaction to be mined.
3. Click "Edit Stake" (`ui.rs:2912`) and expand "Staked Bonds", then the finalizer.
4. The bond row reads `0.100 cTAZ`. The "Staked Bonds" header and the finalizer subtotal directly above it read `0.010 cTAZ`. The left pane reads `0.010 cTAZ Staked`.

A second route needs no small stake:

1. Stake 1 cTAZ. The row reads `1.000 cTAZ`.
2. Leave it bonded while rewards accrue. Once the bond is worth, say, 1.00012345 cTAZ, the wallet's next bond refresh updates the row to `1.12345 cTAZ`, while the total reads `1.00012 cTAZ`.

**Attack Requirements and Assumptions:**

- All platforms. The defect is pure string arithmetic.
- Flow: Edit Stake modal only, both lists (Staked Bonds rows and Withdrawable Bonds rows).
- The user owns at least one bond whose current value has a non-zero fraction below 0.1 cTAZ. The 0.01 button produces one immediately. Reward accrual produces one from any whole-number stake (the accrual is verified in code; how fast a bond leaves the whole number depends on reward rates that were not measured).

## Impact on Users

- **A stake figure up to ten million times too large.** The smallest stake reads ten times its value. One zat of fraction reads as 0.1 cTAZ.
- **Different bonds look identical.** 0.001, 0.01 and 0.1 cTAZ all read `0.100 cTAZ`. 1.05 and 1.5 both read `1.500 cTAZ`.
- **Rows contradict totals on the same screen.** One 0.01 bond: row `0.100 cTAZ`, total `0.010 cTAZ`. A participant cannot tell which to believe, and the wrong one is the per-bond figure they act on when choosing what to unstake, retarget or withdraw.
- **Rewards appear as a jump.** A 1 cTAZ bond that earns 0.00012345 cTAZ is shown as having earned 0.12345.
- **No funds are at risk.** The unstake, retarget and claim buttons pass `bond_key` only (`ui.rs:2472`, `:2703`, `:2722`), and the claimed amount comes from the node (`wallet/src/lib.rs:2313-2317`). The harm is a wrong displayed figure.

## Technical Details / Code Analysis

**Arithmetic, worked by hand.** `part_str` is `{part}` followed by `00`. `n` is the length of `part_str` after trimming trailing zeros, raised to at least 3. The output is `full`, a dot, and the first `n` characters of `part_str`. For `str_from_ctaz`, `part_str` is `{part:08}` cut to 5 characters, then the same trim and the same minimum of 3.

| zats | true cTAZ | `part` | `format_stake_amount` `part_str` | `format_stake_amount` shows | `str_from_ctaz` shows |
| :- | :- | :- | :- | :- | :- |
| 0 | 0 | 0 | `000` | `0.000 cTAZ` | `0.000` |
| 1 | 0.00000001 | 1 | `100` | `0.100 cTAZ` (wrong) | `0.000` |
| 100,000 | 0.001 | 100000 | `10000000` | `0.100 cTAZ` (wrong, 100x) | `0.001` |
| 1,000,000 | 0.01 | 1000000 | `100000000` | `0.100 cTAZ` (wrong, 10x) | `0.010` |
| 10,000,000 | 0.1 | 10000000 | `1000000000` | `0.100 cTAZ` | `0.100` |
| 105,000,000 | 1.05 | 5000000 | `500000000` | `1.500 cTAZ` (wrong) | `1.050` |
| 150,000,000 | 1.5 | 50000000 | `5000000000` | `1.500 cTAZ` | `1.500` |
| 500,000,000 | 5 | 0 | `000` | `5.000 cTAZ` | `5.000` |
| 100,012,345 | 1.00012345 | 12345 | `1234500` | `1.12345 cTAZ` (wrong) | `1.00012` |
| 112,345,678 | 1.12345678 | 12345678 | `1234567800` | `1.12345678 cTAZ` | `1.12345` |

The 0.01 and 1.05 rows match the review's figures exactly. The slice never panics: `part_str` is ASCII and at least 3 characters long.

The last row shows a second, harmless difference: `format_stake_amount` prints all eight decimals, while `str_from_ctaz` truncates (floors) to five. The first row of the pair, one zat, shows the cost of that truncation: `str_from_ctaz` prints `0.000`. That is a rounding choice, not a wrong digit.

**The correct formatter** (`wallet/src/lib.rs:1159-1168`):

```rust
pub fn str_from_ctaz(val: u64) -> String {
    let full = val / 100_000_000;
    let part = val % 100_000_000;
    let mut part_str = format!("{:08}", part);
    if part_str.len() > 5 {
        part_str = part_str[..5].to_string();
    }
    let trim_part = part_str.trim_end_matches("0");
    format!("{full}.{}", &part_str[..trim_part.len().max(3)])
}
```

**Callers.** `format_stake_amount` has exactly two (grep over `zebra-gui`, `wallet` and the rest of the repo):

```rust
let str = if initial == u64::MAX {
    frame_strf!(data, "...")
} else {
    frame_strf!(data, "{}", format_stake_amount(stake_amount))
};
```

This block appears at `ui.rs:2504-2508` (Withdrawable Bonds) and `:2751-2755` (Staked Bonds), with `let stake_amount = initial as i64;` at `:2439` and `:2670`.

**The value is the bond's latest value, not the staked amount.** The loop variable is named `initial`, but the tuple it comes from is declared as "current estimated" (`ui.rs:1632-1633`) and filled from `latest_zats` (`:1640-1641`). The wallet overwrites `latest_zats` with the node's figure (`wallet/src/lib.rs:4754-4757`):

```rust
for (bond, _finalizer, latest_zats) in &mut stake_positions_bonded {
    if let Some(zats) = user_wallet.seen_bond_values.get(&bond.pk.0) {
        *latest_zats = *zats;
    }
    user_staked_funds += *latest_zats;
}
```

`seen_bond_values` is refreshed from `get_bond_info` (`wallet/src/lib.rs:4887-4891`), which returns `bond.amount` (`zebra-crosslink/zebra-state/src/service.rs:2016-2019`), and the node adds rewards to that amount (`zebra-crosslink/zebra-state/src/service/finalized_state/zebra_db/delegation.rs:376-377`):

```rust
// Add reward to bond amount
bond.amount = (bond.amount + Amount::try_from(*reward_amount as i64)?)?;
```

So a bond staked with a round button amount does not stay round. Any reward below 0.1 cTAZ on a whole-number bond puts it in the broken range.

**Other amount formatters in the GUI.** Checked by grepping `zebra-gui/src` for `100_000_000`, `ONE_cTAZ`, `cTAZ` and `zats`:

- Every other amount shown in `ui.rs` goes through `str_from_ctaz` (balances `:2849-2855`, history amounts and fees `:3303`, `:3314`, history labels `:3177-3218`, roster and tooltips `:1514-1554`, `:3627`, `:3833-3843`, right pane `:3920-3928`, Convert Commission `:2122`). None shares the flaw.
- `viz_gui.rs`, `lib.rs` and `main.rs` format no amounts.
- `parse_ctaz` (`ui.rs:70-81`) is the inverse direction. It pads the fraction on the right with `{frac:0<8}`, which is correct: `"1.05"` parses to 105,000,000.
- The stake and send buttons use fixed labels with matching constants (`ui.rs:1994-2003`, `:2255-2261`, `:2270-2276`).

**Not deliberate.** No comment or TODO defends the unpadded form. `git show 5b20864f` ("fix ui ctaz display") changes `format!("{part}00")` to `format!("{:08}", part)` in `str_from_ctaz`; the unstake modal's inline copies were left, and `9d1e3f3b` later moved them verbatim into `format_stake_amount`.

## Recommendations

1. **Delete `format_stake_amount` and format bond rows with `str_from_ctaz`.**
   - At `ui.rs:2504-2508` and `:2751-2755`, replace the call:

   ```rust
   let str = if initial == u64::MAX {
       frame_strf!(data, "...")
   } else {
       frame_strf!(data, "{} cTAZ", str_from_ctaz(initial))
   };
   ```

   - Remove `let stake_amount = initial as i64;` at `:2439` and `:2670`, and the function at `:1224-1230`. `str_from_ctaz` is already imported (`ui.rs:21`).
   - This also removes a needless `u64` to `i64` cast.
   - While there, rename the loop binding `initial` to `latest` in both loops (`:2430`, `:2575`): the tuple field is the current value, and the name invites the wrong mental model that caused this to go unnoticed.

2. **Pin `str_from_ctaz` with a unit test.** It is now the only amount formatter and has no test. Add to the existing `#[cfg(test)] mod tests` in `wallet/src/lib.rs` (`:5279`):

   ```rust
   #[test]
   fn str_from_ctaz_pads_the_fraction() {
       assert_eq!(str_from_ctaz(0), "0.000");
       assert_eq!(str_from_ctaz(1_000_000), "0.010");
       assert_eq!(str_from_ctaz(10_000_000), "0.100");
       assert_eq!(str_from_ctaz(105_000_000), "1.050");
       assert_eq!(str_from_ctaz(150_000_000), "1.500");
       assert_eq!(str_from_ctaz(500_000_000), "5.000");
       assert_eq!(str_from_ctaz(100_012_345), "1.00012");
   }
   ```

   - Harness: `zebra-gui` has no UI-driving test harness. Its tests are plain `#[test]` units over pure functions (`ui.rs:5326-5353`, `lib.rs:2400` onward); `wallet` has the same (`wallet/src/lib.rs:5279`). A pure-function test is the cheapest real check, and it tests the real function the rows will call.
   - Manual check after item 1: stake 0.01 cTAZ, open Edit Stake, expand Staked Bonds; the row, the finalizer subtotal and the header must all read `0.010 cTAZ`.
   - These tests were not run during validation (no builds allowed on the box).

**Considered alternatives, not chosen.**

- Fix the padding inside `format_stake_amount` (`{part:08}`). That keeps two formatters with different precision (eight decimals in rows, five in totals), which is how this bug survived the January fix.
- Show eight decimals for bonds by adding a precision argument to `str_from_ctaz`. More precise, but rows would then not match the totals' digits, and no other figure in the GUI shows more than five. Worth revisiting only if sub-0.00001 rewards need to be visible.

**What the user should see after the fix.** Each bond row shows the same digits as every other amount in the GUI: at least three decimals, at most five, truncated. 0.01 cTAZ reads `0.010 cTAZ`; 1.05 reads `1.050 cTAZ`; 1.00012345 reads `1.00012 cTAZ`. With one bond, the row equals the total above it.

## Validation Information

**Verdict: CONFIRMED. Severity: High.**

| Claim | Verified at |
| :- | :- |
| The fraction is formatted without zero padding | `zebra-gui/src/ui.rs:1227` |
| 0.01 cTAZ renders `0.100 cTAZ`; 1.05 renders `1.500 cTAZ` | Worked by hand from `ui.rs:1225-1229` (table above) |
| 1.5 cTAZ and whole amounts render correctly | Same |
| `str_from_ctaz` pads with `{:08}` and is correct for the same inputs | `wallet/src/lib.rs:1162`; worked by hand |
| Totals in the same modal use `str_from_ctaz` | `ui.rs:2426`, `:2565`, `:2657` |
| Exactly two callers, both in the Edit Stake modal | `ui.rs:2507`, `:2754`; repo-wide grep |
| 0.01 cTAZ is a stake amount the GUI offers | `ui.rs:2255`, `:2270` |
| The row value includes accrued rewards | `ui.rs:1640-1641`; `wallet/src/lib.rs:4754-4763`, `:4887-4891`; `zebra-state/src/service.rs:2019`; `delegation.rs:376-377` |
| No other GUI amount formatter has the flaw | Grep of `zebra-gui/src`; `parse_ctaz` checked at `ui.rs:70-81` |
| Not deliberate; the same bug was fixed in `str_from_ctaz` earlier | `git show 5b20864f`; `git show 9d1e3f3b`; `git blame -L1224,1230` |

**Severity justification.**

- *Why not higher:* High is the top of the scale used here. For calibration, no funds move wrongly: every action in the modal is keyed by `bond_key`, and the withdrawn amount is the node's.
- *Why not Medium:* it is a wrong stake figure, the category the brief ranks first. It fires on the first use of the smallest Stake button with no unusual state, it is off by a factor of ten or more, and it then recurs on ordinary bonds as rewards accrue. The correct figure on the same screen does not rescue it, because the user has no way to know which of two contradicting numbers is right.

**Corrections made during validation.**

1. The review framed the trigger as "bond amounts whose fraction has a leading zero" and gave stake-button examples. Added: the row shows the bond's latest value including rewards, so whole-number bonds enter the broken range without any user action. This widens the reach; it does not change the verdict.
2. The review said rows and totals "disagree". Made precise: they disagree only for bonds in the broken range, and additionally differ in precision (eight decimals against five) for all other fractional bonds.
3. Added: one zat of fraction is displayed as 0.1 cTAZ, so the maximum error is far above the tenfold figure quoted.
4. Added the history of the bug (`5b20864f` fixed one copy; `9d1e3f3b` consolidated the unfixed copies).

**Cross-references.**

- `claimed-at-label-in-transaction-history-uses-the-wrong-bonds-claim.md` (G8): the other wrong-figure finding. Its labels use `str_from_ctaz` (`ui.rs:3177-3218`), so after this fix the "now X cTAZ" history label and the Edit Stake row for the same bond show the same digits. The two fixes touch different lines and can land in either order.
- No other finding touches `format_stake_amount` or the two call sites.
