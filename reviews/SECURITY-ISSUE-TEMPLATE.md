# Review finding template

The structure every report under `reviews/` follows. It is taken from the Taylor Hornby review of zeronym (`zeronym-22aa9851caf68-high-medium/`), whose files end by naming a `docs/SECURITY-ISSUE-TEMPLATE.md`. That document was never available here, so this file was reconstructed from the reports themselves and from how the two crosslink reviews applied it.

Use it for any validated finding: a security issue, a correctness bug, or a UX defect. The section names stay the same in all three cases.

## Folder layout

```text
reviews/
  REVIEW_STATUS.md                      one row per finding, across all reviews
  SECURITY-ISSUE-TEMPLATE.md            this file
  <project>-<commit13>-<topic>review/   one folder per review, named for the commit it was validated at
    README.md                           index: target, source, method, table of findings, landing order
    high/    medium/    low/            one file per finding, in the folder matching its severity
```

- **Filename:** a lowercase, hyphenated sentence that states the defect and its consequence, for example `restart-exits-when-killed-between-activation-and-the-bft-genesis-decision.md`. Reports cross-reference each other by filename, so do not rename one after others cite it.
- **One finding per file.** A second defect found while validating gets its own file and its own row.
- **The severity folder is the severity.** If validation changes the severity, move the file.

## How a report is produced

1. **Start from an unverified lead.** A reviewer, a tool or an agent claims a defect at a file and line.
2. **Validate adversarially.** Try to disprove it. Trace every step of the failure through the code, with a `path:line` citation for each. Look for a guard elsewhere that would prevent it.
3. **Settle a verdict:** `Confirmed`, `Partially confirmed` (say exactly which part is wrong), or `Not reproduced`. A false positive still gets a file, so the lead is not raised again.
4. **Settle a severity** and justify it in both directions: why not one level higher, why not one level lower.
5. **Write the fix plan** in Recommendations, as numbered items that can each land on their own.
6. **Add the row** to `REVIEW_STATUS.md`, and one row per numbered recommendation to its recommendation table.

## Rules for the text

- **Do not overclaim.** Separate what was verified by reading or running from what is inferred, and label the inferences. Say plainly when nothing was built or run.
- **Record corrections.** Where the original lead overstated or misdescribed something, fix it in the body and list it under "Corrections made during validation".
- **Check whether the behaviour is deliberate.** Look for a comment, a TODO, a design document or a commit message, and say what was found.
- **Use the codebase's own terms.** Grep for the code's word before naming a concept.
- **Quote code, with its location.** Every fenced block holds code or output only, never prose.
- **No em dashes, en dashes or double hyphens.** Use commas, colons, semicolons or a new sentence. Write table separator rows as `|-|-|`. This is a house rule for this repo's reviews; the original zeronym files do use dashes.

## What each part is for

| Part | Content |
|-|-|
| Title | One long sentence: the defect, then its consequence. Code identifiers in backticks |
| `**Severity**:` | `High`, `Medium` or `Low`. Matches the folder |
| `**Validation Status**:` | `Confirmed`, `Partially confirmed` or `Not reproduced` |
| `**Location**:` | Every relevant `path:line`, repo-relative, each with a few words saying what is there |
| `**Found by agent:**` | Who or what raised it and when; then who validated it, when, and at which commit |
| `**In scope of audit?**` | Yes or No, the scope it was judged against, and where the code came from (which branch or commit introduced it; whether a deployed network is affected) |
| Description | What the code does, what it assumes, and why the assumption is false. End with the calibration: what this is not |
| Attack Scenario and Steps | Numbered steps from a normal starting state to the failure. For a non-adversarial bug the trigger is an ordinary event such as a crash, a restart or a reorg. For a UX defect it is the user's actions |
| Attack Requirements and Assumptions | Who can trigger it and what they need: stake, hash rate, a platform, a wallet state, a configuration |
| Impact on Users | Who is harmed and how: node operators, finalizers, miners, wallet users, light clients. Say how the affected party could tell, if at all |
| Technical Details / Code Analysis | The code, quoted with locations, under a short bold lead-in per point |
| Recommendations | The fix plan. See below |
| Validation Information | The evidence. See below |

**Recommendations**, numbered so the tracking table can list each one:

1. The change itself: which functions and files, the new logic, the invariant it restores. A short snippet is welcome.
2. Alternatives considered, and why each was not chosen.
3. Tests: the harness that exists, what the test drives and what it asserts. Prefer an end-to-end or integration test; keep unit tests for pure leaf functions. If nothing mechanical is possible, give manual steps and the expected result.
4. Rollout: whether the fix changes consensus or validity rules, what it must land before, and whether a deployed branch needs a backport. For a UX fix, say exactly what the user will see.

**Validation Information**, in this order:

1. A verdict line: `**Verdict: CONFIRMED. Severity: High.**`
2. A table of every mechanical claim and where it was verified.
3. Severity justification: why not higher, why not lower.
4. Corrections made during validation.
5. What this issue does not claim, when there is a risk of over-reading it.
6. Cross-references to other findings, by filename, saying whether they must land together or in an order.

## The template

Copy everything inside the fence into a new file and replace each `<...>` placeholder.

````markdown
# <The defect, stated as what the code does>, so <the consequence for a user or the network>

**Severity**: <High | Medium | Low>
**Validation Status**: <Confirmed | Partially confirmed | Not reproduced>
**Location**: `<path/to/file.rs:10-20>` (<what is there>), `<:45>` (<what is there>); `<other/file.rs:7>` (<what is there>)
**Found by agent:** <reviewer, tool or agent, and date>; validated <date> at <branch> <commit13>
**In scope of audit?** <Yes | No>. <The scope it was judged against. Which commit or branch introduced the code. Whether a deployed network or branch is affected.>

## Description

<What the code does, in the order it happens, with locations.>

<What it assumes, quoted from a comment or a design document where one exists.>

<Why the assumption does not hold.>

**<The calibration, in one bold sentence: what this finding is and is not.>** <What bounds it, stated before the reader has to ask.>

## Attack Scenario and Steps

<One sentence on who triggers this: an adversary, an ordinary event, or a user action.>

1. <Starting state.>
2. <Step, with the location it exercises.>
3. <Step.>
4. <The failure, and what is observable when it happens.>

**Attack Requirements and Assumptions:**

- <Who can do this and what they need.>
- <What must already be true of the network, node, platform or wallet.>
- <What does not need to be true, where that is surprising.>

## Impact on Users

<Who bears the loss, in one sentence.>

- **<First consequence.>** <Detail.>
- **<Second consequence.>** <Detail.>

**Detectability, stated precisely.** <Who can tell, by what signal, and who cannot.>

## Technical Details / Code Analysis

**<First point>** (`<path:line>`):

```rust
<quoted code>
```

<What the quoted code shows.>

**<Second point>** (`<path:line>`):

```rust
<quoted code>
```

<What the quoted code shows.>

## Recommendations

1. **<The change, as an imperative.>** <Which functions and files, the new logic, the invariant it restores.>
2. **<A second change that can land on its own.>** <Detail.>
3. **Alternatives considered.** <Each alternative and why it was not chosen.>
4. **Tests.** <The harness, what the test drives, what it asserts. Manual steps if nothing mechanical is possible.>
5. **Rollout.** <Whether consensus or validity rules change. What it must land before. Whether a deployed branch needs a backport.>

## Validation Information

**Verdict: <CONFIRMED | PARTIALLY CONFIRMED | NOT REPRODUCED>. Severity: <High | Medium | Low>.**

<One line on method: read only, or built and run, and at which commit.>

| Claim | Verified at |
|-|-|
| <A mechanical claim from the body> | `<path:line>` |
| <Another claim> | `<path:line>` |
| <A claim that is inferred, not verified> | <inferred from `<path:line>`; not run> |

**Severity justification.**

*Why not <one level higher>:* <reasons.>

*Why not <one level lower>:* <reasons.>

**Corrections made during validation.**

1. <What the original lead said, and what is true.>
2. <Another correction.>

**What this issue does not claim.** <Limits, so the report is not over-read.>

**Cross-references.** <Other findings by filename. Which must land together, and which have an ordering.>

DO NOT DEVIATE FROM THIS TEMPLATE (`reviews/SECURITY-ISSUE-TEMPLATE.md`).
LEAVE THIS MESSAGE IN PLACE SO THAT ALL AGENTS KNOW NOT TO DEVIATE FROM THIS TEMPLATE (`reviews/SECURITY-ISSUE-TEMPLATE.md`).
````

## The status row that goes with it

Each report gets one row in `REVIEW_STATUS.md`, in the table for its review:

```markdown
| <id> | [<short name>](<review-folder>/<severity>/<filename>.md) | <high, medium or low> | <what triggers it> | <fork, launch or any> | <FIXED `commit`, PARTIAL, CLEAR, DECIDE, MEASURE or EXTERNAL> | <test route> | <owner> | <one or two sentences: the fact that decides what happens next> |
```

Each numbered recommendation gets one row in the recommendation table:

```markdown
| <id>.<n> | <the recommendation, as an imperative> | <DONE `commit`, PARTIAL, OPEN, SUPERSEDED or DECLINED> | <evidence, or what it waits on> |
```

Column meanings are defined at the top of `REVIEW_STATUS.md`. Anything not verified against the tree is `OPEN`, never assumed done.
