# justerm

The **`justerm` family** — a pure terminal **engine** and its bindings. Feed a VT byte stream in; get
terminal *state* and *damage* out. No I/O, no IPC, no rendering, theme-agnostic. Pairs with the
renderer [`beamterm`](https://github.com/junkdog/beamterm) (WebGL2) to form the `-term` family. First
consumer: PenTerm.

> `justerm` is the family umbrella, not a single crate. The core engine is **`justerm-core`**. (Before
> v0.6.0 the bare name `justerm` *was* the engine crate; it was renamed for this disambiguation —
> see [ADR-0010](./docs/adr/0010-all-prefixed-crate-naming.md).)

## Crates

| Crate / package | Registry | Role |
| --- | --- | --- |
| [`justerm-core`](./justerm-core) | crates.io | The engine: VT stream → grid + scrollback + cursor + selection → viewport snapshot, damage, scroll ops, text. |
| [`justerm-wasm-decode`](./justerm-wasm-decode) | npm | The canonical web decoder — `justerm-core`'s wire-format `decode` compiled to WASM, so a web consumer shares one decoder with the native backend (no TS mirror). Version-locked to the core. |

Reserved for future work: `justerm-wasm-engine` (in-wasm `feed` binding, ADR-0008) and `justerm-web`
(the renderer-side TS package). The repo also carries `justerm-facade` — a one-shot `justerm` 0.5.1
tombstone that re-exports `justerm-core` for anyone still on the old crate name (see ADR-0010).

## Docs (start here)

- [`CLAUDE.md`](./CLAUDE.md) — identity, boundary invariants, conventions, working method.
- [`CONTEXT.md`](./CONTEXT.md) — glossary (the family's ubiquitous language).
- [`docs/architecture.md`](./docs/architecture.md) — the authoritative contract: cell, damage,
  viewport/scroll, cadence, selection, serialization, engine API, plus a **Hidden VT state** checklist.
- [`docs/adr/`](./docs/adr/) — key decisions (build on `vte`; adopt `beamterm`; the WASM decode
  binding; all-prefixed crate naming).
- **Build plan**: GitHub issues — Epic #1 + slices #2–#12.

## Develop

```bash
cargo test --workspace        # gates justerm-core + justerm-wasm-decode (--workspace is required)
cargo bench                   # throughput micro-bench
```

The root is a virtual manifest; `fuzz` and `justerm-facade` sit *outside* the workspace (see
`CLAUDE.md`). Verify them with `cargo check --manifest-path fuzz/Cargo.toml`.

## License

Dual-licensed under [Apache-2.0](./LICENSE-APACHE) or [MIT](./LICENSE-MIT), at your option. Any
contribution you submit is dual-licensed as above without additional terms.
