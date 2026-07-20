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

## The renderer publishes on its OWN track (`renderer-v*`, #394)

`justerm-renderer` is **deliberately outside** the `[workspace.package]` version-lockstep (its
web-sys/glow deps are wasm32-only and it ships on its own cadence, root `Cargo.toml`). So it carries
its own version (in `justerm-renderer/Cargo.toml`, a `0.1.0`-series, **not** the workspace `0.7.x`) and
its own tag prefix — a `v*` tag does **not** publish it, and a `renderer-v*` tag publishes **only** it.

Cut a renderer release:

1. Bump `version` in `justerm-renderer/Cargo.toml`.
2. Gate the renderer (out of every cargo umbrella — see `docs/agents/theflow.md` §gate matrix):
   `cargo fmt/test/clippy/build --manifest-path justerm-renderer/Cargo.toml` + `pnpm test:proofs`.
3. Commit, then tag: `git tag -a renderer-vX.Y.Z -m "renderer-vX.Y.Z — …"`.
4. Push the tag: `git push origin renderer-vX.Y.Z`.

| Workflow | Trigger | Publishes | Gate | Secret |
| --- | --- | --- | --- | --- |
| `.github/workflows/publish-renderer.yml` | `renderer-v*` | `wasm-pack build --target bundler` + `npm publish` → npm (`justerm-renderer`) | tag (minus `renderer-v`) must equal the wasm-pack package version, else fail | `NPM_TOKEN` |

Uses the same `NPM_TOKEN`, which must have publish rights for the **`justerm-renderer`** package too (a
distinct npm package from `justerm-wasm-decode`; the first publish may need the token widened or a
one-time manual `npm publish ./justerm-renderer/pkg --access public`). The bundler build is the
published artifact (justerm-web's Vite consumes it); the renderer's GL proofs use `--target web`
separately, so the job builds its own bundler output. No `finish-pkg` step — the renderer ships pure
wasm-bindgen output, no hand-written JS helpers.

## `justerm-web` publishes on its OWN track too (`web-v*`, #466)

Same reasoning as the renderer: `justerm-web` consumes `justerm-wasm-decode` and `justerm-renderer`
through **caret ranges**, not the `[workspace.package]` lockstep, and moves on its own cadence. So it
carries its own version (in `justerm-web/package.json`) and its own tag prefix — a `v*` tag does
**not** publish it, and a `web-v*` tag publishes **only** it.

The workflow exists and is **inert until a `web-v*` tag is pushed** — having it is not the same
decision as starting to publish. As of this writing nothing has been published: `justerm-web` is not
on npm. Whether the first publish waits for penterm's migration is open on #466.

Cut a web release:

1. Bump `version` in `justerm-web/package.json`.
2. Gate it: `pnpm typecheck && pnpm test && pnpm build` (+ `pnpm test:e2e` for UI-observable changes).
3. Commit, then tag: `git tag -a web-vX.Y.Z -m "web-vX.Y.Z — …"`.
4. Push the tag: `git push origin web-vX.Y.Z`.

| Workflow | Trigger | Publishes | Gate | Secret |
| --- | --- | --- | --- | --- |
| `.github/workflows/publish-web.yml` | `web-v*` | `pnpm build` (tsup) + `npm publish ./justerm-web` → npm (`justerm-web`) | tag (minus `web-v`) must equal `package.json` version, **and** the tarball must contain `dist/index.js` + `dist/index.d.ts`, else fail | `NPM_TOKEN` |

`files: ["dist"]` keeps the tarball to the tsup output (+ `README.md`/`package.json`, which npm always
includes) — the demo, e2e and tests are not shipped. `NPM_TOKEN` must have publish rights for the
**`justerm-web`** package, a distinct npm package from `justerm-wasm-decode` and `justerm-renderer`;
a per-package-scoped token needs widening, and the first publish may need a one-time manual
`npm publish ./justerm-web --access public` (the renderer hit exactly this).

**Which version bump?** `justerm-web`'s public surface is `src/index.ts` (what `dist` re-exports) —
*not* its dependencies' Rust symbols. Note the renderer's own versioning basis is still open (#465):
two `renderer-v*` minors were cut for Rust symbols no consumer imports, and each forced a caret bump
here. Don't inherit that pattern on this track.

## GitHub Releases — manual, and the track starts at v0.3.1

CI does **not** create GitHub Releases (only registry publishes). Create them by hand:
`gh release create vX.Y.Z --verify-tag --latest --title "…" --notes-file …`.

The GitHub Release track **starts at v0.3.1**. Tags `v0.1.0`–`v0.3.0` exist as git tags only and are
**intentionally not backfilled** as Releases: GitHub stamps a Release's publish date at creation time
and there is no supported way to backdate it, so a backfill would read as "published today" against
much older tags. Backfilling is a non-goal, not an oversight — leave the pre-v0.3.1 tags as tags.

**`v0.6.0`'s Release page was created late** (2026-07-10, for a 2026-06-26 tag; #351), so its
`publishedAt` is wrong and the list is not in chronological order. That was a judged exception, not a
new policy: v0.6.0 is the rename release (`justerm` → `justerm-core`, ADR-0010), and it is the page a
reader lands on when asking why the bare crate name stopped moving. The wrong date is neutralised by
saying so in the first line of the body — the objection to backfilling is that a silent wrong date
misleads, and a stated one does not. **The tag is the record of truth**; publishing is tag-driven, so
a missing Release page never affected a consumer.

When backfilling, pass **`--latest=false`** — otherwise the older release steals the `Latest` badge
from the newest one, and `gh release create` will not warn you. Check with
`gh release list --json tagName,isLatest`; a green exit proves nothing.

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
