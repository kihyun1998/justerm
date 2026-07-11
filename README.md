# justerm

[![crates.io](https://img.shields.io/crates/v/justerm-core.svg?label=justerm-core)](https://crates.io/crates/justerm-core)
[![docs.rs](https://img.shields.io/docsrs/justerm-core.svg)](https://docs.rs/justerm-core)
[![npm](https://img.shields.io/npm/v/justerm-wasm-decode.svg?label=justerm-wasm-decode)](https://www.npmjs.com/package/justerm-wasm-decode)
[![license](https://img.shields.io/crates/l/justerm-core.svg)](#license)

The **`justerm` family** — a pure terminal **engine**, its bindings, a browser widget, and a
first-party renderer. Feed a VT byte stream in; get terminal *state* and *damage* out. The **engine**
(`justerm-core`) does no I/O, no IPC, no rendering, and is theme-agnostic. The family has grown from an
engine-only library into a **first-party full-stack** terminal (ADR-0018): the renderer — once the
third-party [`beamterm`](https://github.com/junkdog/beamterm) (WebGL2) — is being reimplemented
in-family as [`justerm-renderer`](./justerm-renderer). beamterm still renders until the switch lands
([#273](https://github.com/kihyun1998/justerm/issues/273)). First consumer: PenTerm.

> `justerm` is the family umbrella, not a single crate. The core engine is **`justerm-core`**. (Before
> v0.6.0 the bare name `justerm` *was* the engine crate; it was renamed for this disambiguation —
> see [ADR-0010](./docs/adr/0010-all-prefixed-crate-naming.md).)

## Crates

| Crate / package | Registry | Role |
| --- | --- | --- |
| [`justerm-core`](./justerm-core) | [crates.io](https://crates.io/crates/justerm-core) · [docs.rs](https://docs.rs/justerm-core) | The engine: VT stream → grid + scrollback + cursor + selection → viewport snapshot, damage, scroll ops, text. |
| [`justerm-wasm-decode`](./justerm-wasm-decode) | [npm](https://www.npmjs.com/package/justerm-wasm-decode) | The canonical web decoder — `justerm-core`'s wire-format `decode` compiled to WASM, so a web consumer shares one decoder with the native backend (no TS mirror). Version-locked to the core. |
| [`justerm-web`](./justerm-web) | npm (separate) | Browser terminal **widget**: consumes a decoded frame and drives the renderer. Frame mode now (decode wire frames in the consumer), in-wasm later — both behind a `FrameSource` seam. Ships to npm on its own version track. |
| [`justerm-renderer`](./justerm-renderer) | wasm (`wasm-pack`) | First-party **WebGL2 renderer** (Rust → wasm, `glow`): consumes a decoded frame + an injected palette and paints one instanced draw call. Reimplements `beamterm` in-family (ADR-0018, Epic #258). Under construction; outside the cargo workspace. |
| [`justerm-facade`](https://crates.io/crates/justerm) | [crates.io](https://crates.io/crates/justerm) | One-shot `justerm` 0.5.1 tombstone re-exporting `justerm-core` for the old crate name (ADR-0010). Not updated. |

Reserved for future work: `justerm-wasm-engine` (the in-wasm `feed` binding, ADR-0008) — the browser
running the engine itself rather than decoding frames from a native backend.

## Docs (start here)

- [`CLAUDE.md`](./CLAUDE.md) — identity, boundary invariants, conventions, working method.
- [`CONTEXT.md`](./CONTEXT.md) — glossary (the family's ubiquitous language).
- [`docs/architecture.md`](./docs/architecture.md) — the authoritative contract: cell, damage,
  viewport/scroll, cadence, selection, serialization, engine API, plus a **Hidden VT state** checklist.
- [`docs/adr/`](./docs/adr/) — key decisions (build on `vte`; adopt then replace `beamterm` with the
  first-party `justerm-renderer`, ADR-0002 → ADR-0012/0018; the WASM decode binding; all-prefixed
  crate naming).
- **Build plan**: GitHub issues — Epic #1 (engine, closed) established the core; the family now builds
  under Epic #103 (`justerm-web`) and Epic #258 (`justerm-renderer`).

## Develop

```bash
cargo test --workspace        # gates justerm-core + justerm-wasm-decode (--workspace is required)
cargo bench                   # throughput micro-bench
```

The root is a virtual manifest; `fuzz`, `justerm-facade`, and `justerm-renderer` sit *outside* the
workspace (see `CLAUDE.md`). Verify them separately — e.g. `cargo check --manifest-path fuzz/Cargo.toml`
and `cargo build --manifest-path justerm-renderer/Cargo.toml --target wasm32-unknown-unknown`. The
`justerm-web` widget has its own `pnpm` gates (`pnpm test` / `typecheck` / `build`).

## License

Dual-licensed under [Apache-2.0](./LICENSE-APACHE) or [MIT](./LICENSE-MIT), at your option. Any
contribution you submit is dual-licensed as above without additional terms.
