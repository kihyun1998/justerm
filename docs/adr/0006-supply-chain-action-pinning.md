# ADR-0006: Pin third-party GitHub Actions to a commit SHA; least privilege

Status: accepted (2026-06-19)

## Context

This repo gains CI for the first time: a `test` gate (fmt/clippy/test) and a `supply-chain` check. `rust-toolchain.toml` already anticipated it ("keep in sync with ci.yml"). Any workflow runs third-party code with access to the repo token, so the moment CI exists, the supply chain of the *actions* it uses becomes part of this crate's trust surface.

A `uses:` reference to a **mutable** ref — a tag (`@v4`) or branch — is one the action's author, or anyone who compromises the action's repo, can silently re-point after review. This is not hypothetical: in the **tj-actions/changed-files** compromise (CVE-2025-30066, March 2025) an attacker retroactively moved version tags to a commit that exfiltrated CI secrets; repos pinning by tag were hit, those pinning by commit SHA were not.

We already own the right tool: **just-shield** (`kihyun1998/just-shield`), a dependency-free scanner of GitHub Actions supply-chain posture. It shares this repo's owner, so it is first-party here — adopting it adds no new third-party trust.

## Decision

Every workflow in this repo is bound by the following, mechanically enforced by just-shield (`supply-chain.yml`, `scan --strict`):

- **Third-party actions** (owner neither this repo's nor GitHub's) **must** be pinned to a full 40-character commit SHA, with the version in a trailing comment (`# v1.2.3`). Pin the SHA a tag *resolves to* (`^{commit}`), never an annotated-tag object (just-shield R5 rejects the latter as unreachable).
- **GitHub-owned actions** (`actions/*`, `github/*`) **may** use a version tag — GitHub is a higher-trust publisher and the tj-actions class was a third-party action. We pin them too in practice, because `just-shield fix` makes it free.
- Every workflow declares an explicit `permissions:` block; the default is `contents: read` (just-shield R7).

The cost objection to SHA-pinning ("you freeze the action and stop getting updates, and updating the hash by hand is tedious") does not apply here: `just-shield fix` does the initial pin, and the `github-actions` Dependabot ecosystem (`.github/dependabot.yml`) opens weekly PRs that advance the pinned SHA and its version comment together. Pins stay immutable and current at once.

## Consequences

- CI cannot be turned against this crate through a moved tag; a new mutable reference cannot land without failing the supply-chain job.
- Policy, tool, and CI share one definition — just-shield's R1/R5/R7 rules *are* this policy, so there is no second prose spec to drift from.
- Adding the guard adds no third-party trust: just-shield is first-party here, offline, and zero-dependency, and is itself SHA-pinned.

## Alternatives considered

- **Trust tags (do nothing).** Rejected — it is exactly the posture the tj-actions/CVE-2025-30066 victims had.
- **Require SHA pins for `actions/*` too, as a hard rule.** Rejected as the floor — it adds verbosity and Dependabot noise for the lowest-risk publisher (GitHub) with little marginal safety. We pin them in practice (free via `fix`) but do not gate on it.
- **A third-party scanner (StepSecurity Harden-Runner, zizmor).** Rejected in favor of first-party just-shield — adopting an external scanner to defend the supply chain would itself add the kind of third-party CI trust being defended against.
