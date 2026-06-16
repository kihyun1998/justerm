# justerm

A pure terminal **engine** in Rust. Feed it a VT byte stream; it owns the terminal state (grid +
scrollback + cursor + selection) and emits a viewport snapshot, damage, scroll ops, and extractable
text. It is **not** a renderer and **not** a full emulator.

- **No I/O** — the caller feeds bytes (`feed(&[u8])`); justerm never touches a PTY/SSH/socket.
- **No IPC** — it provides a binary *format*, not transport.
- **No rendering** — a renderer draws (e.g. [`beamterm`](https://github.com/junkdog/beamterm), WebGL2).
- **Theme-agnostic** — colors are *references* (Default / Indexed / RGB), never resolved hex; the
  consumer resolves them.

Pairs as the engine half of a `-term` family with the renderer `beamterm`. First consumer: PenTerm.

> **Status:** early. The architecture is design-complete and prior-art-validated; implementation is in
> progress per the build plan. Start at issue #2 (the only currently-unblocked slice).

## Docs (start here)

- [`CLAUDE.md`](./CLAUDE.md) — identity, boundary invariants, conventions, working method.
- [`CONTEXT.md`](./CONTEXT.md) — glossary.
- [`docs/architecture.md`](./docs/architecture.md) — the contract: cell, damage, viewport/scroll,
  cadence, selection, serialization, engine API — plus a **Hidden VT state** checklist (with where to
  look in reference impls) for implementers.
- [`docs/adr/`](./docs/adr/) — key decisions (build on `vte`, not `alacritty_terminal`; adopt
  `beamterm`).
- **Build plan**: GitHub issues — Epic #1 (the PRD-equivalent) + slices #2–#12.

## License

MIT — see [`LICENSE`](./LICENSE).
