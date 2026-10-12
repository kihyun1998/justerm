# ADR-0034: The family owns the PTY loop in its own crate, and the Session it drives lives in core

Status: **accepted** (2026-10-12), #1042. Every decision below is a **judgement** made by the maintainer
during #1042's grilling. Each was picked from enumerated options whose consequences were shown. The
facts each one was decided on are listed under it. A fact those facts could not have shown reopens the
decision. A better argument alone does not.

## Context

Every frame-mode **Consumer** writes the same wiring between the **Engine** and its byte source, and
the traps sit in that wiring. Until now only PenTerm knew them (penterm `src-tauri/src`, read
2026-10-12):

- **Replies have to be written back.** `drain_replies` hands back DA/DSR answers. If the consumer never
  writes them to the byte source, a querying app such as vim waits forever. PenTerm writes them twice,
  once per transport: `pty/manager.rs:1267` for PTY and `ssh/manager/read_loop.rs:345` for SSH.
- **The PTY gets the adopted size, not the requested one.** The engine raises a too-small width
  without saying so (`docs/architecture.md:42-48`).
- **A late subscriber starts from a full frame.** `mark_fully_damaged` exists for exactly this case
  (`justerm-core` `lib.rs:378-382`).
- **PenTerm keeps its own `dirty` flag.** Selection and search-highlight changes leave no **Damage**
  (`term/selection.rs:51-106`, `term/search.rs:269-301`), because ADR-0014 made the overlay per-frame
  state rather than cell content. So nothing in the engine answers *is there a frame worth sending*.
- **SSH drives the same engine with no PTY at all.** Whatever is shared must therefore not carry a
  process dependency.
- **PenTerm transcodes before it feeds.** Legacy encodings (EUC-KR, CP949, Shift_JIS) are turned into
  UTF-8 first (`pty/manager.rs:1349`, `encoding.rs`), and core reads the stream as UTF-8 only.

`docs/architecture.md:113` gave pacing to the consumer ("the engine provides the diff, the consumer
paces it") under an ack-gated model with no timer. PenTerm instead runs a 16 ms timer.

A second consumer (a local bridge serving a visitor's shell to `justerm-web`) would have had to write
all of the above a second time.

## Decision

**D1 — The family owns the PTY loop, in its own crate `justerm-pty`. `justerm-core` stays I/O-free.**
This is the renderer precedent (ADR-0018) applied to I/O: the *family* widens and the core crate does
not. Prior art agrees, from memory and not re-read at a pinned revision:
- alacritty keeps a `tty` module beside its terminal model;
- wezterm ships `portable-pty` as a sibling crate;
- xterm.js's organisation maintains `node-pty` separately.

**D2 — The Session is a module in `justerm-core`: a pure state machine with no clock and no I/O.** It
covers `feed`, **Pump** (send the pending frame to the sink; return replies and events), subscribe
(full frame) and resize (returns the adopted size).

*Decided on:*
- **SSH needs it without a PTY.** Core is the home that adds no dependency for that path.
- **Consumers keep a single dependency.** ADR-0030 D3 has PenTerm consume `justerm-core` alone, and that
  stays true.
- **It fits the existing layout.** A module needs no new top-level area.

The Session returns events unfiltered, because choosing among them is consumer policy.

**D3 — An overlay change raises a frame-pending signal that is separate from Damage.** ADR-0014
stands: overlay is still not damage. What changes is that the engine, which is the only party that
knows which state changed, now says so, and the consumer stops guessing with its own flag. The exact
shape (flag or generation) is a mechanism, left to the implementing slice.

**D4 — The Session has no clock; `justerm-pty` paces with a timer.** The period defaults to 16 ms, the
value PenTerm settled on through dogfooding, and is configurable. Pacing *policy* stays the consumer's.
`justerm-pty` only supplies a default, and `docs/architecture.md:113` is amended to say so. Ack-gated
pacing was offered and not chosen: the one real consumer already runs on a timer.

**D5 — `justerm-pty` depends on no async runtime.** It reads blocking on a dedicated thread.
- *Decided on:* the PTY reader is blocking `std::io::Read`; Windows ConPTY pipes have no useful async
  path; and PenTerm already moves its reader onto `spawn_blocking`.
- A tokio adapter waits for real demand.

The rest of D5:
- **Encoding is a hook.** The consumer injects a stateful decoder and encoder; the default passes UTF-8
  through. Which encoding to use stays the consumer's choice.
- **The consumer supplies the command to spawn.** Shell choice, shell integration and launchers stay
  the consumer's.
- **The loop itself is `justerm-pty`'s:** reply write-back, resize sync and exit detection.

**D6 — `justerm-pty` is a workspace member on the `v*` lockstep track, and its real-process tests run on
Linux, macOS and Windows in CI.**
- *Lockstep, decided on:* the crate sits directly on the Session API.
- *Three OSes, decided on:* PTY behaviour differs per OS (ConPTY exit, EOF vs EIO), and the only shipping
  consumer is Windows.

## Out of scope

Each item below is a judgement as well:
- exposing the Session to wasm (the reserved `justerm-wasm-engine`);
- the web example app (a separate issue);
- PenTerm's migration, which is PenTerm's work. #1042 is done when the crates are published and
  PenTerm's PTY and SSH loops run on them locally with PenTerm's suite green.

## Consequences

- **ADR-0030 D1** gains `justerm-pty/`. Its `:139` requires a new top-level area to reopen the record.
- **CLAUDE.md's "No I/O — reads no PTY"** becomes a statement about `justerm-core`. This follows the
  same split its "No rendering" line already draws for `justerm-renderer`.
- **GLOSSARY's family entry** gains `justerm-pty` when the crate ships. **Session** and **Pump** are
  defined there already.
- **`docs/architecture.md:113`** is amended per D4.
