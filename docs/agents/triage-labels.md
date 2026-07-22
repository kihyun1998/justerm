# Triage Labels

The label vocabulary this repo's tracker actually uses. All of these exist — `gh label list` is the
source of truth if this file and the tracker ever disagree.

## Triage roles (what the skills speak in)

The skills name five canonical triage roles; here they are spelled the same on both sides, so a skill
that says "apply the AFK-ready triage label" means literally `ready-for-agent`.

| Role in the skills | Label here       | Meaning                                  |
| ------------------ | ---------------- | ---------------------------------------- |
| `needs-triage`     | `needs-triage`   | Maintainer needs to evaluate this issue  |
| `needs-info`       | `needs-info`     | Waiting on reporter for more information |
| `ready-for-agent`  | `ready-for-agent`| Fully specified, ready for an AFK agent  |
| `ready-for-human`  | `ready-for-human`| Requires human implementation            |
| `wontfix`          | `wontfix`        | Will not be actioned                     |

## Workflow labels (what the backlog is actually wearing)

The roles above are the skills' vocabulary; these are the labels this backlog carries day to day, and
a skill reading an issue will meet them first.

| Label           | Meaning                                                    |
| --------------- | ---------------------------------------------------------- |
| `epic`          | Tracking issue / PRD-equivalent. Its body is a **live checklist** — tick slices as they close (theflow Step 6) |
| `blocked`       | Has unresolved Blocked-by deps; **not grabbable yet**. The one label that changes whether an agent may pick the work up, so it must come off the moment the blocker closes |
| `enhancement`   | New feature or request — the default for a scoped work item here |
| `bug`           | Something isn't working                                     |
| `documentation` | Docs-only change                                            |
| `duplicate`     | Already tracked elsewhere                                   |

GitHub's remaining stock labels (`good first issue`, `help wanted`, `invalid`, `question`) and
Dependabot's (`dependencies`, `github_actions`, `rust`) exist but carry no local convention.

## Known gap, so you don't infer a workflow that isn't running

As of 2026-07-22 the five triage labels are on **zero** open issues; the backlog is labelled with
`enhancement` / `epic` / `blocked` only. So the triage roles are available but not an established
practice here — do not read an unlabelled issue as "already triaged", and do not assume a
`ready-for-human` sweep exists to hand a product decision to.
