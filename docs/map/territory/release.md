# Territory — release

## What it is

How a version leaves this repo: which tag ships what, to which registry, and which artifacts move
together. **Tag-driven and automatic** — pushing a tag publishes, with no confirmation step and
nothing but a yank coming back.

What a stranger *reads* once it has shipped is a different concept — see
[published surface](published-surface.md).

## Governing decisions

- [**ADR-0008 — wasm-decode as a separate crate**](../../adr/0008-wasm-decode-binding-separate-crate.md)
  and [**ADR-0005**](../../adr/0005-binary-reference-based-serialization.md) — together they require
  the **version lockstep**: the wire format is shared, so one bump has to move both sides
- [ADR-0010 — all-prefixed crate naming](../../adr/0010-all-prefixed-crate-naming.md) — the rename
  whose tombstone sits outside every track below
- [ADR-0006 — supply-chain action pinning](../../adr/0006-supply-chain-action-pinning.md) — the
  publish workflows are the paths it protects
- `docs/agents/release.md` is the operational procedure and **not** a decision record

## Design model

- **Three tag tracks, and the first one is the non-obvious one:**

  | tag | publishes | to |
  |---|---|---|
  | `v*` | `justerm-core` **and** `justerm-wasm-decode` | crates.io **and** npm |
  | `renderer-v*` | `justerm-renderer` | npm |
  | `web-v*` | `justerm-web` | npm |

  **One tag, two registries.** `v*` fires `publish-crate.yml` *and* `publish-wasm.yml` — the lockstep
  made mechanical. Reading only the crates.io version gives half the picture.
- **`justerm-renderer` does not ship on `v*`.** It has its own version track because it is
  workspace-excluded and outside the lockstep, so a `v*` release leaves it untouched. A change
  spanning both needs two tags.
- **A change propagates by the kind of *edge*, not by the track — and the tracks above answer the
  wrong half of the question.** They say what a tag *publishes*. What a maintainer holding a core
  change actually needs to know is what must now be published, and where the chain stops. Three edge
  kinds decide it, and every dependency in the family is exactly one of them:

  | edge | reaches the dependent | what catches a break |
  |---|---|---|
  | **path** (`{ path = "../justerm-core" }`) | immediately, in the working tree | `cargo test --workspace` — a core API break is a compile error in the same PR |
  | **version range** (`"0.6.0"`, `^0.14.0`) | only after a **publish** *and* a **pin bump**, both manual | nothing in between; the consumer is compiling against the old artifact and is correct to |
  | **no edge** | never | n/a — and this is the one people assume wrong |

  So, for a change to `justerm-core`: the `v*` tag ships core **and** `justerm-wasm-decode`, because
  that binding is a path dependant and the lockstep makes it one release. **`justerm-renderer` needs
  nothing — it does not depend on `justerm-core` at all**, and neither does `justerm-web` *directly*:
  core reaches a browser only as compiled bytes **inside** the wasm-decode artifact, so `justerm-web`
  is reached by raising its `justerm-wasm-decode` pin, and penterm by raising its own. Neither happens
  because a tag was pushed.

  **The npm version of the two wasm packages is not written down anywhere you would look.** wasm-pack
  derives it from the crate's `Cargo.toml`, so `justerm-wasm-decode` and `justerm-renderer` have no
  `package.json` to bump — the one in `justerm-renderer/` is `@justerm/renderer-proofs`, the headless
  proof runner, and is not the published package.

  **Do not write the consumer list here.** It is derivable and it rots the day a pin moves — the rule
  `docs/agents/theflow.md` states for the downstream loop, and the same reason
  [the alt-screen floor note](../invariant/alt-screen-buffer-floor.md) carries a grep instead of a
  roster. Derive the edges instead:

  ```
  rg -n '^justerm-core\s*=' --glob '*/Cargo.toml' . ../penterm     # path vs version, in one read
  rg -n 'justerm-' justerm-web/package.json                        # the npm pins
  ```

  **A live illustration of the middle row, measured 2026-08-06 rather than imagined:**
  `renderer-v0.11.0` is published and `justerm-web` pins `^0.10.0`, which under npm's 0.x caret is
  `>=0.10.0 <0.11.0` — so the installed renderer is **0.10.0** and everything in 0.11.0 is unreachable
  from `justerm-web` until someone raises that pin. Nothing is broken and nothing reports it; that is
  the normal, quiet state of a version-range edge, and it is why *"the family is on the latest"* is a
  claim to check rather than assume.
- **The tombstone has no track at all.** `justerm-facade` was published **manually, once**, after
  `justerm-core` 0.6.0 was live — it depends on it, so it could not be built inside the rename PR.
  Frozen at `0.5.1` forever; its own manifest says *"Do not re-publish; do not update."*
- **A version lockstep is expressed in the manifest, not in a script.** `[workspace.package] version`
  is the single source for core and the wasm binding, which is why the facade had to be excluded from
  the workspace rather than merely skipped.
- **Release notes are GitHub Releases; there is no `CHANGELOG.md`.** A published entry is never
  rewritten — a correction opens a new note.
- **Why the four publish workflows are shaped the way they are** — the facts their comments point
  here:
  - *No build cache in a publish job*: just-shield R6 flags a third-party action sharing a job that
    holds the publish token, so dropping `rust-cache` (and using `npm install -g pnpm@10` rather than
    `pnpm/action-setup`) keeps `CARGO_REGISTRY_TOKEN` / `NPM_TOKEN` away from any third-party action
    (ADR-0006). A publish runs on a rare tag, so the cache bought little.
  - *The `wasm-pack` pin is for determinism, not speed* (#616): unpinned, two tags a week apart could
    ship differently-generated wasm from one source tree — live when written, with crates.io serving
    0.15.0 while a maintainer built proofs on 0.14.0. It pins wasm-pack's own codegen and the
    `wasm-opt` release it downloads, **not** the `wasm-bindgen` CLI, which wasm-pack derives from the
    crate's lockfile (#613 pinned that half). `--version X.Y.Z` is exact, not a caret (`cargo help
    install`); `~0.15` was rejected because it absorbs a patch bump silently. While the pin equals the
    runner image's version the install short-circuits (~0.2 s); any other value builds wasm-pack from
    source, uncached in the publish jobs by the rule above — the accepted price of a deterministic
    artifact.
  - *The renderer's lockfile gate is its own step* (#613): `-- --locked` through wasm-pack inspects
    nothing, because wasm-pack resolves and writes `Cargo.lock` before cargo runs (measured both ways:
    no lock, it creates one; a stale lock, it rewrites it) while bare cargo refuses in both.
    `cargo metadata` is the cheapest bare command that resolves, and on refusal leaves the lock
    untouched.
  - *Licence texts must be pushed into `files`* (#472): wasm-pack copies `LICENSE-MIT` /
    `LICENSE-APACHE` into `pkg/`, but `files` is an allowlist and npm's automatic inclusion covers
    `LICENSE`/`LICENSE.md`, not the hyphenated form — renderer 0.5.0/0.6.0 and web 0.7.0 shipped a
    licence declaration with no licence text. The decoder's `finish-pkg.mjs` and the renderer's inline
    step both add them; the web job asserts them, with the entry and its types, inside the tarball.
  - *An already-published version is a green no-op*: npm refuses a re-publish (403), so each npm job
    checks `npm view pkg@ver version` first — re-tagging is always safe.
  - *A GitHub Release per npm track, in its own job* (#474): a registry publish carries no notes, and
    the release job needs `contents: write`, which must not share a job with `NPM_TOKEN`. Created at
    tag time, so `publishedAt` is right by construction; the body is the tag's annotation (a
    lightweight tag falls through to its commit message, verified); `--latest=false` keeps the Latest
    badge on the `v*` engine track.

## Code

- `.github/workflows/publish-crate.yml` · `publish-wasm.yml` · `publish-renderer.yml` ·
  `publish-web.yml`
- Root `Cargo.toml` — `[workspace.package] version`, and the `exclude` list that keeps the facade and
  the renderer off the lockstep. Since #608 the renderer's standalone status has a **second**
  anchor, `justerm-renderer/Cargo.toml`'s own `[workspace]`; the `exclude` entry alone did not
  survive being reached from a git worktree
  ([workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md))
- `justerm-facade/Cargo.toml` — the frozen version and the do-not-republish note

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md` — how comparable multi-artifact projects
stage a lockstep release, or carry a renamed-package tombstone, has never been checked against a
pinned tree.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the exclusion that keeps the facade off the lockstep is the same exclusion that keeps every gate
  away from it

## Blast radius

- [wire format](wire-format.md) — a `WIRE_VERSION` bump ships on `v*` and therefore to two registries
  at once; a consumer decoding a wrong layout gets garbage cells, not an error
- [published surface](published-surface.md) — publishing is what snapshots the README, so every
  release freezes prose as well as code
- the renderer crate — its own track, so it can silently fall behind a `v*` release. Every
  renderer territory ships on that tag and none on `v*`; start at
  [cell compositing](cell-compositing.md) (`rg -l 'justerm-renderer/src' docs/map/territory/`
  for the set — a count written here was wrong within a day)
- [widget lifecycle](widget-lifecycle.md) — `justerm-web` consumes the *published* wasm decoder, so a
  new binding is `undefined` at runtime until that package is republished

## Known holes / open

- **No reference comparison** for the release model (§Reference behaviour).
- **Nothing verifies the two-registry coupling.** That `v*` must reach both crates.io and npm is
  encoded only as two workflows watching the same tag pattern; if one workflow were renamed or
  disabled, the lockstep would silently become a half-release.
- **The renderer's separate track has no reminder mechanism.** Nothing prompts a second tag when a
  change spans both, and #429's issue comments missed exactly that.
