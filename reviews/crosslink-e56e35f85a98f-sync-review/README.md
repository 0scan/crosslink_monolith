# Crosslink sync follow-up at dev e56e35f85a98f

One report, in the template at [`../SECURITY-ISSUE-TEMPLATE.md`](../SECURITY-ISSUE-TEMPLATE.md). Its status is tracked as row S1 in [`../REVIEW_STATUS.md`](../REVIEW_STATUS.md).

- **Target:** `zebra-crosslink/zebra-state/src/new_network.rs` on `dev` at `e56e35f85a98f`, the commit "NewNet: Answer every queued block through a fate table; back off re-fetching rejected blocks".
- **Source:** a lead raised by the "crosslink spreadsheet triage" session on 2026-10-01 while reading that commit.
- **Method:** validated by reading code only. Nothing was built or run, and every delay figure in the report is arithmetic from the constants.

| # | Severity | Verdict | Fires | File |
|-|-|-|-|-|
| S1 | Low | Partially confirmed | needs a dishonest peer | [rejected-block backoff is keyed by hash alone](low/rejected-block-backoff-is-keyed-by-hash-alone-so-one-peer-can-delay-an-honest-block-for-every-peer.md) |

## Not verified

- The size of the same-tick race window between a forged copy and an honest one.
- Whether inbound connections are capped. If they are cheap to fill, an attacker holding most of a node's connections gets a roughly twentyfold longer delay than before the commit, which is the argument for Medium.
- Whether crosslink nodes still gossip blocks over the legacy network, a path the backoff does not gate.
