# justerm-core

A pure terminal **engine** in Rust — the core crate of the [`justerm`](https://github.com/kihyun1998/justerm)
family. Feed it a VT byte stream; it owns the terminal state (grid + scrollback + cursor + selection)
and emits a viewport snapshot, damage, scroll ops, and extractable text. It is **not** a renderer and
**not** a full emulator.

- **No I/O** — the caller feeds bytes (`feed(&[u8])`); justerm-core never touches a PTY/SSH/socket.
- **No IPC** — it provides a binary *format*, not transport.
- **No rendering** — a renderer draws (the family's first-party [`justerm-renderer`](https://github.com/kihyun1998/justerm/tree/master/justerm-renderer),
  WebGL2, replacing the third-party [`beamterm`](https://github.com/junkdog/beamterm)).
- **Theme-agnostic** — colours are *references* (Default / Indexed / RGB), never resolved hex; the
  consumer resolves them.

## Install

```sh
cargo add justerm-core
```

## Usage

Feed VT bytes in, read terminal state out — no PTY, no rendering, no theme:

```rust
use justerm_core::{Color, Engine};

// An 80×24 viewport. The caller owns I/O; justerm-core only parses bytes.
let mut term = Engine::new(80, 24);

// A VT stream: "hi" in SGR red (ESC[31m … ESC[0m).
term.feed(b"\x1b[31mhi\x1b[0m");

// Read the grid back: the character, and the colour *reference*
// (Indexed(1) = ANSI red — resolving it to a hex value is the consumer's job).
assert_eq!(term.grid().cell(0, 0).c(), 'h');
assert_eq!(term.grid().cell(0, 0).fg(), Color::Indexed(1));
```

`Engine` also exposes `damage()` (the line + column ranges changed since the last read),
`viewport_line()` / `viewport_logical_lines()`, `resize()`, and a binary wire format
(`serialize`) for shipping frames to a consumer with no TypeScript mirror to drift.

> **Status:** in active use. The core engine is implemented and consumed across the family
> (`justerm-wasm-decode`, `justerm-web`, `justerm-renderer`). VT compliance is **cumulative** — the
> common cases are covered and the long tail grows as dogfooding surfaces it. See the issue tracker for
> the current frontier. First consumer: PenTerm (a Tauri terminal app).

## Docs

- [`CLAUDE.md`](https://github.com/kihyun1998/justerm/blob/master/CLAUDE.md) — identity, boundary invariants, conventions, working method.
- [`CONTEXT.md`](https://github.com/kihyun1998/justerm/blob/master/CONTEXT.md) — glossary.
- [`docs/architecture.md`](https://github.com/kihyun1998/justerm/blob/master/docs/architecture.md) — the contract: cell, damage, viewport/scroll,
  cadence, selection, serialization, engine API — plus a **Hidden VT state** checklist (with where to
  look in reference impls) for implementers.
- [`docs/adr/`](https://github.com/kihyun1998/justerm/tree/master/docs/adr) — key decisions (build on `vte`, not `alacritty_terminal`; adopt then
  replace `beamterm` with the first-party `justerm-renderer`, ADR-0002 → ADR-0018).

## Web consumers

The wire format's decoder is shipped to the web as [`justerm-wasm-decode`](https://github.com/kihyun1998/justerm/tree/master/justerm-wasm-decode) — the
native `decode` compiled to WASM and published to npm, version-locked to this crate, so the backend
encoder and the webview decoder share one implementation (no TypeScript mirror to drift). It decodes
into structure-of-arrays cell columns and ships the **format-owned** helpers (`resolveRgb` /
`buildPalette` / `flags`); the *theme values* (your palette) and *render policy* (atlas, cursor) stay
the consumer's adapter. See [`justerm-wasm-decode/README.md`](https://github.com/kihyun1998/justerm/blob/master/justerm-wasm-decode/README.md) and
[ADR-0008](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0008-wasm-decode-binding-separate-crate.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](https://github.com/kihyun1998/justerm/blob/master/justerm-core/LICENSE-APACHE))
- MIT license ([`LICENSE-MIT`](https://github.com/kihyun1998/justerm/blob/master/justerm-core/LICENSE-MIT))

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
