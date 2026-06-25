# Supply-chain check (just-shield)

CI runs a **supply-chain** gate (`.github/workflows/supply-chain.yml`) that scans every workflow
in this repo with **just-shield** in `scan --strict` mode. A violation fails the job. The *decision*
(pin third-party actions to a commit SHA; least privilege) is **ADR-0006**; this file is the
*operational* side — what just-shield is and how to service a failure.

## What just-shield is

A dependency-free, offline CLI that checks whether the GitHub Actions a pipeline consumes are
*authentic* and *blast-radius-limited* **before** they run. Its north star is preventing CI
credential theft (the TeamPCP / UNC6780 campaign is its reference scenario). It is **first-party
here** — same owner (`kihyun1998`) as this repo — so adopting it adds no new third-party trust, and
it is itself SHA-pinned.

- **Published:** `kihyun1998/just-shield` (the GitHub Action) + crates.io / Homebrew / Scoop / ghcr.io.
- **Pinned version in our workflows:** currently `v0.3.0` (a full 40-char SHA + `# v0.3.0` comment).
  Dependabot advances the pin; do not hand-edit the SHA.
- **Source lives next to this repo:** the sibling working copy is at `../just-shield`
  (`D:\github\just-shield`). Its `README.md`, `CONTEXT.md`, and `docs/adr/` are the authority for rule
  semantics — read them there, don't re-derive.

## Reproduce a CI failure locally

When the `supply-chain` / `just-shield` check fails on a PR (it bit the Dependabot `actions/checkout`
v6→v7 bump, #29), run the same scan locally instead of guessing from CI logs:

```bash
just-shield scan . --strict          # if installed (brew/scoop/cargo install just-shield)
```

No local install? Run it from the sibling source, or pin-by-digest via the container image:

```bash
cargo run --manifest-path ../just-shield/Cargo.toml -- scan . --strict
# or (digest is in just-shield's release notes):
docker run --rm -v "$PWD:/work" ghcr.io/kihyun1998/just-shield@sha256:<digest> scan /work --strict
```

Exit codes: `0` pass · `1` violation (🔴; with `--strict` also 🟡) · `2` usage/IO error.

## Rule legend (so a CI code like "R5" is decodable here)

| Rule | Sev | Meaning |
|------|-----|---------|
| R1 | 🔴 / 🔵 | Third-party action on a **mutable ref** (tag/branch). `actions/*`,`github/*` softened to 🔵. |
| R2 | 🔵→🔴 | Typosquat (one char / transposition off a popular action); 🔴 only under `--online`. |
| R3 | 🔵 | Unverified `curl \| sh`-style pipe install (heuristic; silent when checksum-verified). |
| R4 | 🟡 | Container image ref without a digest (`image:`/`container:`/`docker://`). |
| R5 | 🔴 | (`--online`) Pinned SHA **unreachable** in the repo's real history — impostor commit. |
| R6 | 🟡 | Third-party action run in a job that has secrets. |
| R7 | 🟡 | No `permissions:` block, or `write-all`. |
| R8 | 🔴 | `pull_request_target`/`workflow_run` + external-PR checkout. |
| R9 | 🔴 | Version/commit listed malicious in the bundled advisory DB (offline). |
| R10 | 🟡 | (`--online`) Ref published < 7 days ago — cooldown (tune with `--cooldown-days`). |
| LOCK | 🔴 / 🔵 | Tag moved vs the `shield.lock` snapshot (exact-version move = 🔴). |

Trust classes: local / same-owner = first-party (skipped); `actions/*`,`github/*` = official
(softened); everything else = third-party (strict, no reputation exemption). Unknown → third-party
(fail-closed).

## Fixing a violation

- **Auto-pin** a mutable ref: `just-shield fix .` rewrites tag/branch refs to a commit SHA with a
  `# vX.Y.Z` comment (network needed). Preview with `--dry-run`.
- **Action version bumps** ride Dependabot (`.github/dependabot.yml`, `github-actions` ecosystem):
  it advances the pinned SHA *and* its version comment together, so pins stay immutable **and**
  current. A **major** bump (e.g. checkout v6→v7) is deliberately surfaced for human review rather
  than auto-merged — that surfacing *is* the gate working, not a bug.
- **Intentionally accept** a finding: a reason-mandatory ignore comment on (or above) the line —
  `# just-shield: ignore R1 -- <reason>`. No `--` reason → the ignore is void and reported 🔵.
  Ignored findings are not hidden; they stay in the report as ⚪ with their reason.

## When a just-shield CI check is red

1. Reproduce locally (above) to get the exact rule + line.
2. If it's a stale Dependabot branch (the common case), rebase it onto `master`
   (`@dependabot rebase`) and re-check — the scan re-runs against current policy.
3. If it's a real policy gap in a workflow we own, `just-shield fix` (for pin issues) or add the
   missing `permissions:`/digest, then re-run the scan.
