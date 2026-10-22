# Territory — release

## What it is

How a version leaves this repo: which tag publishes what, to which registry, and which artifacts
move together. Releases are **tag-driven and automatic** — a tag push publishes, with no
confirmation step, and only a yank comes back. It also holds the one artifact with no track at all:
the frozen `justerm` 0.5.1 tombstone (`justerm-facade/`).

What a stranger reads once a version has shipped is [published surface](published-surface.md). How
to cut a release step by step is `docs/agents/release.md`; this note holds why the machinery is
shaped as it is, and what a change obliges.

## Governing decisions

- [ADR-0008 — wasm-decode as a separate crate](../../adr/0008-wasm-decode-binding-separate-crate.md)
  and [ADR-0005 — binary reference-based serialization](../../adr/0005-binary-reference-based-serialization.md)
  — together they require the **version lockstep**: the wire format is shared, so one bump moves the
  engine and its decoder
- [ADR-0034 — the family owns the PTY loop](../../adr/0034-the-family-owns-the-pty-loop-and-the-session-lives-in-core.md)
  D6 — puts `justerm-pty` on the `v*` lockstep track
- [ADR-0010 — all-prefixed crate naming](../../adr/0010-all-prefixed-crate-naming.md) — the rename
  that produced the tombstone, and its facade-over-yank rationale
- [ADR-0006 — pin actions; least privilege](../../adr/0006-supply-chain-action-pinning.md) — why the
  publish jobs carry no third-party action and keep `id-token: write` apart from `contents: write`;
  a convention since its 2026-10-12 amendment

`docs/agents/release.md` is the operational procedure and records two maintainer decisions with no
ADR: the shared version number line (2026-09-04) and per-track semver surfaces (#465).

## Design model

- **Four tag tracks, and `v*` publishes to two registries** (the workflows' `on.push.tags`):

  | tag | publishes | to | workflow |
  |---|---|---|---|
  | `v*` | `justerm-core`, then `justerm-pty` | crates.io | `publish-crate.yml` |
  | `v*` | `justerm-wasm-decode` | npm | `publish-wasm.yml` |
  | `renderer-v*` | `justerm-renderer` | npm | `publish-renderer.yml` |
  | `web-v*` | `justerm-web` | npm | `publish-web.yml` |

  One `v*` tag fires two workflows — the lockstep made mechanical. Reading only crates.io gives half
  the picture. `justerm-pty` depends on the exact `justerm-core` version, so `publish-crate.yml`
  sends core first and cargo waits for it to be live.
- **The lockstep lives in the manifest, not in a script.** `[workspace.package] version` in the root
  `Cargo.toml` is the single source for `justerm-core`, `justerm-wasm-decode` and `justerm-pty`
  (each takes `version.workspace = true`), which is why the facade had to be excluded from the
  workspace rather than skipped. `justerm-pty`'s `justerm-core` dependency also carries an explicit
  `version`, so a release bumps that line too or `cargo check` fails with "failed to select a
  version" (`docs/agents/release.md`). Each publish job fails before publishing when the tag differs
  from the built version.
- **`justerm-renderer` and `justerm-web` are off the lockstep**, each with its own version (renderer
  in its own `Cargo.toml`, web in `package.json`) and its own tag. A change spanning core and the
  renderer needs two tags. Since 2026-09-04 the tracks share **one number line**: a release takes the
  next number above every track's current version and tags only the track that changed, so equal
  numbers mean "cut together" and a skipped number is normal (`docs/agents/release.md`). A higher
  number never means "compatible with".
- **Semver is measured per track, against what that track ships** (#465): `v*` against the public
  Rust API, `renderer-v*` against the wasm/JS class *and observable rendering*, `web-v*` against
  `src/index.ts`. Two renderer minors cut for Rust symbols no consumer imports each forced a web
  caret bump; that is the reason the rule exists.
- **A change propagates by the kind of edge, not by the track.** The tracks say what a tag
  publishes; what a maintainer holding a change needs is what must now be published, and where the
  chain stops. Every dependency in the family is one of three edges:

  | edge | reaches the dependent | what catches a break |
  |---|---|---|
  | **path** (`{ path = "../justerm-core" }`) | immediately, in the working tree | `cargo test --workspace` — a core API break is a compile error in the same PR (except `fuzz`, see [robustness testing](robustness-testing.md)) |
  | **version range** (`"0.30"`, `^0.25.0`) | only after a **publish** and a **pin bump**, both manual | nothing in between; the consumer compiles against the old artifact, correctly |
  | **no edge** | never | n/a — and the one people assume wrong |

  So a `justerm-core` change ships on `v*` with `justerm-wasm-decode` and `justerm-pty`, both path
  dependants. **`justerm-renderer` does not depend on `justerm-core` at all** (its `Cargo.toml` has no
  such line), and neither does `justerm-web` directly: core reaches a browser only as bytes compiled
  into the wasm-decode artifact. `justerm-web` is reached by raising its `justerm-wasm-decode` pin,
  and penterm by raising its own. A tag pushes neither.
- **Derive the edges; do not list the consumers here.** A consumer list rots the day a pin moves.

  ```
  rg -n '^justerm-(core|pty)\s*=' --glob 'Cargo.toml' . ../penterm   # path vs version, nested manifests included
  rg -n '"justerm-' justerm-web/package.json ../penterm/package.json   # the npm pins
  ```

  The previous form globbed `*/Cargo.toml`, which misses penterm's `src-tauri/Cargo.toml` — the one
  that holds penterm's crate pins.
- **A version range decides when a drift is reachable, and the quiet state is the normal one.**
  Measured 2026-08-06: `renderer-v0.11.0` was published while `justerm-web` pinned `^0.10.0`, so
  0.11.0 was unreachable from the widget until the pin moved. At HEAD the same shape stands on the
  decoder: `justerm-web` pins `justerm-wasm-decode` `^0.22.0` (0.x caret: `<0.23.0`) while `v0.30.0`
  is tagged, and penterm overrides the widget's decoder to `0.30.0` (penterm 7dadc3c95
  `package.json` — `pnpm.overrides`), outside the range the widget's own types were gated against.
- **The npm version of the two wasm packages is written in `Cargo.toml`.** wasm-pack derives
  `pkg/package.json` from the crate, so neither has a `package.json` to bump; the one in
  `justerm-renderer/` is `@justerm/renderer-proofs`, the private proof runner.
- **The tombstone has no track.** `justerm` 0.5.1 re-exports `justerm-core = "0.6"` and was published
  by hand, once, after `justerm-core` 0.6.0 was live — it depends on it, so it could not be built
  inside the rename PR. Its manifest says *"Do not re-publish; do not update."* It stays outside the
  workspace on the root `exclude` list alone, unlike `fuzz` and `justerm-renderer`, which each
  declare their own `[workspace]` since #608 (root `Cargo.toml` comment).
- **Release notes are GitHub Releases; there is no `CHANGELOG.md`.** The `renderer-v*` and `web-v*`
  publish workflows create the Release in a separate `release` job, body taken from the tag's
  annotation (a lightweight tag falls through to its commit message), `--latest=false` so the Latest
  badge stays on `v*` (#474). The `v*` Release page is created **by hand**, and its note must name
  every change a consumer can observe (`docs/agents/release.md`). A published note is never rewritten
  — a correction opens a new one.
- **Why the publish workflows are shaped the way they are** — the facts their comments point here:
  - *No build cache in a publish job*: a third-party action in a job holding `id-token: write` could
    use it, so `rust-cache` is dropped and pnpm comes from `npm install -g pnpm@10` rather than
    `pnpm/action-setup` (ADR-0006). A publish runs on a rare tag, so the cache bought little.
  - *The wasm-pack pin is for determinism, not speed* (#616): unpinned, two tags a week apart could
    ship differently generated wasm from one source tree. It pins wasm-pack's codegen and the
    `wasm-opt` it downloads, not the `wasm-bindgen` CLI, which follows the crate's lockfile (#613).
    `--version X.Y.Z` is exact; `~0.15` was rejected because it absorbs a patch silently. While the
    pin equals the runner image's version the install short-circuits; any other value compiles
    wasm-pack from source, uncached by the rule above.
  - *The renderer's lockfile gate is its own step* (#613): `cargo metadata --locked` on the bare
    manifest, because wasm-pack resolves and rewrites `Cargo.lock` before cargo runs, so
    `-- --locked` through it inspects nothing.
  - *Licence texts must be in `files`* (#472): npm auto-includes `LICENSE`/`LICENSE.md`, not
    `LICENSE-MIT`. Renderer 0.5.0/0.6.0 and web 0.7.0 shipped a licence declaration with no text.
    `finish-pkg.mjs` (decoder) and an inline step (renderer) push them into `files`; `publish-web.yml`
    asserts them, with `dist/index.js` and `dist/index.d.ts`, inside the `npm pack --dry-run` listing.
  - *Trusted publishing, no secrets* (2026-10-12): each publish job holds `id-token: write` and no
    stored token. crates.io's exchange is three `curl` calls in one step — mint a GitHub OIDC JWT for
    audience `crates.io`, POST it to `/api/v1/trusted_publishing/tokens`, DELETE the token in an
    `EXIT` trap — rather than `rust-lang/crates-io-auth-action`, which would put third-party code in
    the credential-minting job. npm needs CLI ≥ 11.5.1 and the runner ships npm 10 (image 20261004: Node 22.23.3, npm
    10.9.9, whose Node already meets npm's ≥ 22.14.0 — recorded 2026-10-12), hence
    `sudo npm install -g npm@11` in each npm job (`sudo` because the image's global prefix is not
    writable by the runner user).
  - *A manual run verifies and publishes nothing*: each publish workflow has a `verify-auth` job on
    `workflow_dispatch` that runs `verify-trusted-publish.sh`, exchanging the OIDC token and
    discarding it, so a registry entry can be checked before its first tag.
  - *Re-tagging is a green no-op on two tracks only*: `publish-renderer.yml` and `publish-web.yml`
    check `npm view pkg@ver version` and skip an existing version. `publish-wasm.yml` has no such
    check, and crates.io refuses a re-upload ("already uploaded"), so a re-pushed `v*` tag fails.
  - *The Release job is separate* (#474): it needs `contents: write`, which must not share a job
    with `id-token: write`. Created at tag time, so `publishedAt` is right by construction.
- **Outside input** is a pushed tag. Its name is checked only against the built version; nothing
  bounds which commit it points at beyond the convention that it is one master already gated
  (`publish-web.yml`'s comment).

## Code

- `.github/workflows/publish-crate.yml` — jobs `publish` and `verify-auth`
- `.github/workflows/publish-wasm.yml` — jobs `publish` and `verify-auth`
- `.github/workflows/publish-renderer.yml` · `publish-web.yml` — jobs `publish`, `release`,
  `verify-auth`
- `.github/scripts/verify-trusted-publish.sh` — the publish-nothing OIDC check
- `justerm-wasm-decode/scripts/finish-pkg.mjs` — colour helpers and licence texts into the decoder's
  `pkg/`
- Root `Cargo.toml` — `[workspace.package]` `version`, `members`, and the `exclude` list
- `justerm-pty/Cargo.toml` — the `justerm-core` dependency with its explicit `version`
- `justerm-renderer/Cargo.toml` — its own `version` and its own `[workspace]` (#608)
- `justerm-web/package.json` — its `version` and the two npm pins
- `justerm-facade/Cargo.toml` · `justerm-facade/src/lib.rs` — the tombstone: frozen version, the
  do-not-republish note, and a `//!` page over one `pub use justerm_core::*`

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md`; how comparable multi-artifact projects stage
a lockstep release or carry a renamed-package tombstone has never been checked against a pinned
tree. `docs/agents/release.md` cites Ruffle and Automerge only for the observation that neither
consumes its own wasm package through a version range.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the exclusion that keeps the facade off the lockstep also keeps every `--workspace` gate away
  from it

## Blast radius

- [wire format](wire-format.md) — a `WIRE_VERSION` bump ships on `v*`, so on two registries at
  once; a consumer still on the old decoder gets wrong cells, not an error, until its pin moves
- [published surface](published-surface.md) — publishing snapshots every README, rustdoc page and
  `.d.ts`; a release freezes prose as well as code
- [pty](pty.md) — `justerm-pty` is published by the same job right after core, at the same version;
  a manifest that forgets its explicit core `version` fails the release, not a PR
- [cell compositing](cell-compositing.md) and every renderer territory
  (`rg -l 'justerm-renderer/src' docs/map/territory/`) — they ship on `renderer-v*` and none on
  `v*`, so a cross-cutting change can leave the renderer a track behind with nothing reporting it
- [widget lifecycle](widget-lifecycle.md) — `justerm-web` consumes the published decoder and
  renderer, so a new binding is `undefined` at runtime until it is republished and the pin moves
- [CI gates](ci-and-supply-chain.md) — the publish jobs share the wasm-pack pin; a pin moved in
  `test.yml` alone ships an artifact built by a different tool than CI tested

## Known holes / open

- **Nothing verifies the two-registry coupling.** That `v*` reaches crates.io and npm is encoded
  only as two workflows watching the same pattern; renaming or disabling one silently makes a
  half-release.
- **A re-pushed `v*` tag fails** on both halves (crates.io refuses; the wasm job has no
  already-published check), unlike the two npm-only tracks.
- **The renderer and web tracks have no reminder mechanism.** Nothing prompts a second tag when a
  change spans tracks; #429's comments missed exactly that.
- **The consumer can step outside the widget's declared range.** penterm's `pnpm.overrides` runs
  `justerm-web` against a decoder its published types were never gated against; nothing on this side
  can see that combination.
- **No reference comparison** for the release model (§ Reference behaviour).
