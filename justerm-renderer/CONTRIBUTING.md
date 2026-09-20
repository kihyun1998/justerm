# Working on justerm-renderer

This file is for people with the repository checked out. It is deliberately not part of the npm
package — wasm-pack copies only `README.md` and the licences into `pkg/`, so nothing here reaches a
consumer.

## Gates

This crate is **excluded from the root cargo workspace** (its `web-sys`/`glow` deps are wasm32-only),
so `cargo test --workspace` at the repo root does **not** reach it. Always gate it by manifest path —
not with `-p justerm-renderer`, which selects from a workspace this crate is in none of and fails
with "did not match any packages".

```bash
# pure logic (host) — the GL/wasm layer is 0-compile here
cargo test --manifest-path justerm-renderer/Cargo.toml
cargo fmt --manifest-path justerm-renderer/Cargo.toml --check
# full crate incl. the WebGL glue (wasm32 gate)
cargo build --manifest-path justerm-renderer/Cargo.toml --target wasm32-unknown-unknown
```

The GL layer is proved in a real browser rather than by unit test — `demo/*.html` pages that draw
and then read pixels back, swept across device pixel ratios:

```bash
pnpm run test:unit    # the pixel helpers the proofs read their evidence through (browserless)
pnpm run test:proofs  # builds the wasm, then drives the demo pages in headless Chromium
```

## Release track

Published to npm as `justerm-renderer` on its own `renderer-v*` tag track, deliberately separate
from the workspace `v*` tags (which publish `justerm-core` + `justerm-wasm-decode`): this crate's
`web-sys`/`glow` deps are wasm32-only, so it carries its own version line and ships on its own
cadence. Semver here is measured against the wasm/JS class, not against Rust symbols — see
`docs/agents/release.md`.
