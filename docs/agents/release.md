# Releasing

Releases are **tag-driven and CI-published**. A `vX.Y.Z` tag push publishes the crate to
crates.io **and** the npm artifact, version-locked. Do **not** run `cargo publish` / `npm publish`
by hand — the tag push does it, and a manual publish would collide (a crates.io version cannot be
re-published).

## The version is one number

`[workspace.package] version` in the root `Cargo.toml` is the single source — it moves both the core
crate (`justerm-core`) and the binding (`justerm-wasm-decode`) together (the lockstep ADR-0005/0008 require). The
**wire `VERSION`** is a *separate* constant (ADR-0008): `feed`/internal changes can ship without a wire
bump. So map the change to the version like this (pre-1.0 / 0.x semver):

- **patch** (`0.3.0 → 0.3.1`) — no public Rust API change and no wire change (`DecodedFrame`
  byte-identical, `WIRE_VERSION` unchanged). Internal perf/refactor (e.g. #41 scroll recycling) and
  test/bench-only work (#42) are patches.
- **minor** (`0.3.x → 0.4.0`) — a breaking public API change (e.g. `Cell` fields → accessors, or `Row`
  alias → struct in #43), or a notable feature. In 0.x a breaking change is a minor bump.

## Cut a release (what the maintainer/agent does)

1. Bump `[workspace.package] version` in `Cargo.toml`; refresh the lock (`cargo check --workspace`).
2. Gate **the whole workspace** (not just the core crate): `cargo test --workspace` green,
   `cargo clippy --workspace --all-targets` clean. The `--workspace` is load-bearing — a bare
   `cargo test` / `cargo build --all-targets` only covers the current package, so a public-API change
   that breaks the `justerm-wasm-decode` binding passes a non-workspace gate silently (it bit v0.4.0; CI's
   `test.yml` already uses `--workspace`, so this just matches the local gate to CI before the tag).
3. Commit: `chore(release): vX.Y.Z — <summary> (#issues)` (Cargo.toml + Cargo.lock).
4. Tag: `git tag -a vX.Y.Z -m "vX.Y.Z — <summary>"`.
5. Push **both** the branch and the tag: `git push origin master && git push origin vX.Y.Z`.

That tag push is the publish trigger. Verify it:

- `gh run list --workflow=publish-crate.yml --limit 1`
- `gh run list --workflow=publish-wasm.yml --limit 1`

## What CI does on a `v*` tag

| Workflow | Publishes | Gate | Secret |
| --- | --- | --- | --- |
| `.github/workflows/publish-crate.yml` | `cargo publish -p justerm-core` → crates.io | tag (minus `v`) must equal the crate version, else fail | `CARGO_REGISTRY_TOKEN` |
| `.github/workflows/publish-wasm.yml` | `wasm-pack build --target bundler` + `npm publish` → npm (`justerm-wasm-decode`) | tag must equal the wasm-pack package version, else fail | `NPM_TOKEN` |

Both secrets are one-time maintainer setup (repo secrets). `justerm-wasm-decode` is `publish = false` for cargo,
so it never goes to crates.io; the core crate never goes to npm.

## GitHub Releases — manual, and the track starts at v0.3.1

CI does **not** create GitHub Releases (only registry publishes). Create them by hand:
`gh release create vX.Y.Z --verify-tag --latest --title "…" --notes-file …`.

The GitHub Release track **starts at v0.3.1**. Tags `v0.1.0`–`v0.3.0` exist as git tags only and are
**intentionally not backfilled** as Releases: GitHub stamps a Release's publish date at creation time
and there is no supported way to backdate it, so a backfill would read as "published today" against
much older tags. Backfilling is a non-goal, not an oversight — leave the pre-v0.3.1 tags as tags.

## Notes

- `v0.1.0` was published to crates.io manually before `publish-crate.yml` existed; re-tagging it would
  fail ("already uploaded"). The automation is for `0.1.1+`.
- **The crates were renamed in v0.6.0** (#100, ADR-0010): `justerm` → `justerm-core` (crates.io) and
  `justerm-wasm` → `justerm-wasm-decode` (npm). `v0.6.0` is the *first* publish of both new names. The
  old names are tombstoned — npm `justerm-wasm` is `deprecate`d, and crates.io `justerm` gets a one-shot
  `0.5.1` facade (`pub use justerm_core::*`); see ADR-0010 for the facade-over-yank rationale. Publish
  order on the tag is automatic (both new names are fresh); the `justerm` 0.5.1 facade is a separate
  manual publish that must come **after** `justerm-core` 0.6.0 is live (it depends on it).
- Bumping the workspace version moves `justerm-wasm-decode` even when only the core crate changed — that is the
  lockstep working as intended, so the wasm decoder and core never drift in version.
