# Crosslink GUI review reports at dev d00b6a44ef594

One file per finding, in the same template as `crosslink-e99404e3de7cc-review`. Each file's **Recommendations** section is the fix plan. Status of every finding from both reviews is tracked in [`../REVIEW_STATUS.md`](../REVIEW_STATUS.md).

- **Target:** `zebra-gui` on `dev` at `d00b6a44ef594`, with the `wallet`, `clay-rs` and `softer_gui` code it calls.
- **Source:** `/code-review high` of GUI and UX features, 2026-09-30.
- **Method:** every finding was validated by reading code only. Nothing was built or run.
- **Not reproduced:** none. All ten are confirmed; most had details corrected. See each file's "Corrections made during validation".

| # | Severity | Verdict | Fires on | File |
|-|-|-|-|-|
| G1 | High | Confirmed | all platforms | [Edit Stake amounts wrong](high/edit-stake-modal-shows-bond-amounts-wrong-because-the-fraction-is-not-zero-padded.md) |
| G2 | High | Confirmed | Windows; Linux without `xclip` or `xsel` | [no clipboard backend](high/clipboard-has-no-windows-or-wayland-backend-so-paste-only-flows-cannot-complete.md) |
| G3 | Medium | Confirmed | macOS, Linux; Windows once G2 is fixed | [non-ASCII paste aborts the node](medium/pasted-non-ascii-text-panics-the-gui-on-a-byte-index-slice.md) |
| G6 | Medium | Confirmed | all platforms | [Send never validates the address](medium/send-modal-never-validates-the-address-and-a-failed-send-is-silent.md) |
| G7 | Medium | Confirmed | all platforms | [Finalizer Filters zeroes the seconds filter](medium/opening-finalizer-filters-zeroes-the-seconds-filter-and-marks-everyone-online.md) |
| G8 | Medium | Confirmed | all platforms | ["claimed @" uses the wrong bond](medium/claimed-at-label-in-transaction-history-uses-the-wrong-bonds-claim.md) |
| G4 | Low | Confirmed | all platforms | [backdrop passes clicks through](low/modal-backdrop-passes-clicks-through-to-the-controls-underneath.md) |
| G5 | Low | Confirmed | all platforms, Faucet Wallet tab | [faucet tab resets every modal](low/faucet-wallet-tab-resets-every-modal-so-jump-and-convert-rewards-never-open.md) |
| G9 | Low | Confirmed | all platforms | [chain view reacts under panes](low/chain-view-pans-and-recenters-while-the-pointer-is-over-a-pane-or-modal.md) |
| G10 | Low | Confirmed | all platforms | [Esc dead after focus; Jump gives no feedback](low/esc-cannot-close-a-modal-after-textbox-focus-and-jump-gives-no-feedback.md) |

## Landing order for the paste flows

1. G3 items 1 and 2: the crash fix and the single paste function.
2. G2 items 1 and 2: the Windows and Wayland clipboard backends. G3 must be in first, because a working Windows clipboard makes the G3 abort reachable there.
3. One combined change: G2 item 3 with G6 items 2 and 3. They share the notice line under the paste button.
4. G6 item 4 and G2 item 4.

## Cut by the review, not reported here

Unmute resets volume to 100%; Pool Balances rows print raw zatoshi integers; the block inspector resize handle does nothing; the loading dots look frozen when idle; characters typed on a skipped frame are dropped; staking-day labels ignore resized pane widths; the staking-day gate and banner are each off by one block; horizontal and vertical scroll speeds differ when scale is not 1; a textbox click always selects all. None of these was validated.
