# ADR-0006: CI action supply-chain gate (just-shield)

Status: accepted (2026-06-18)

## Context

justerm's CI (`.github/workflows/ci.yml`) consumes third-party GitHub Actions —
`dtolnay/rust-toolchain`, `Swatinem/rust-cache`, `actions/checkout` — each of
which executes with the workflow's token. A compromised or repointed action is a
supply-chain path straight into CI: the TeamPCP / UNC6780 campaign hijacked
exactly this surface, moving mutable version tags onto malicious commits to
harvest CI credentials. Until now justerm referenced these actions by mutable
tag (`@stable`, `@v2`, `@v4`) and declared no `permissions`, so a moved tag or an
over-scoped token would have passed unnoticed.

## Decision

Add a `supply-chain` CI job that runs
**[just-shield](https://github.com/kihyun1998/just-shield)** in strict mode on
every push and PR, and bring the existing workflow into compliance with it:

    supply-chain:
      runs-on: ubuntu-latest
      steps:
        - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4
        - uses: kihyun1998/just-shield@bfe605c359607bddb3fcbc04ee568e0ff4f60bf3 # v0.3.0
          with:
            strict: true

just-shield inspects every workflow and fails the build when an action is
referenced by a mutable tag instead of a commit SHA, when a workflow grants more
token scope than it declares a need for, or when a referenced action version is
on a known-compromised list. As part of adopting it:

- every action is now pinned to a commit SHA with a trailing `# version` comment
  (`actions/checkout`, `dtolnay/rust-toolchain`, `Swatinem/rust-cache`);
- the workflow declares least-privilege `permissions: contents: read`;
- just-shield is itself SHA-pinned — the rule dogfoods itself.

## Consequences

- Any new action added to any workflow must be SHA-pinned and pass just-shield,
  or CI fails — supply-chain hygiene is enforced, not merely documented.
- Tags are mutable; SHAs are not. Pinning to a SHA is the one reference that
  cannot be changed out from under us after review — the direct mitigation for
  the tag-repointing attack class. Updating an action becomes a deliberate SHA
  bump (visibility is the feature), which Dependabot can automate.
- `dtolnay/rust-toolchain` is pinned from its `stable` branch, so the Rust
  toolchain no longer floats automatically; bumping it is now an explicit,
  reviewable change. If float is ever wanted back for a specific action,
  just-shield's reason-carrying `# just-shield: ignore R1 -- <why>` escape hatch
  is the sanctioned route rather than silently un-pinning.
- just-shield is self-authored and young (v0.3.0). Gating CI on it is a
  dependency in its own right, accepted because (a) it runs read-only — it
  inspects YAML and reports, it does not mutate the repo; (b) it is SHA-pinned
  like everything else; and (c) it is replaceable — the checks it performs are
  also offered by drop-in tools such as `zizmor` and StepSecurity's
  Harden-Runner, so a stall in just-shield does not strand the project.
