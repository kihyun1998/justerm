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

## Amended 2026-10-12, while building #1045

Each item below records whose call it was.

- **Reply write-back is not paced with frames** (a derivation). D4's timer paces frames. Replies are
  written back as soon as the output that asked for them is fed. Measured on Windows: a ConPTY sends
  `ESC[6n` at start and lets the child run only once it is answered. Unanswered, there was no output
  and no exit in 8 s. Answered, the child printed and exited in 352 ms. This fact is new since the
  grilling. It does not reopen D4, because pacing frames is still the consumer's policy. It adds the
  rule that the pace never applies to replies.
- **Write-back ships with the first slice** (the maintainer's call). It moved from #1046 into #1045,
  because without it nothing runs on Windows at all, and D6 puts Windows in CI from the first slice.
  #1046 keeps the consumer's writer and resize sync.
- **`justerm-pty` stays `publish = false` until the loop is complete** (the maintainer's call, made at
  #1045's confirm). Publishing it before reply write-back and resize sync existed would ship a PTY
  loop that hangs a querying application. D6's lockstep holds from the release that first publishes
  it: the publish step and the release table landed with #1047. crates.io takes a new crate's first
  version only with an API token, never through trusted publishing. So that one publish is manual
  (`docs/agents/release.md` § A new crate's first publish). For `v0.27.0` the workflow skipped the
  crate until it existed. The maintainer chose that at #1047, over keeping `publish = false` until
  release time. Once `justerm-pty` 0.27.0 was published by hand, the skip was removed, because it
  would otherwise let a failed lookup silently break the lockstep.

## Amended 2026-10-12, #1057: an output observer, a tick, and Raw mode

Each decision below is **a judgement made by the maintainer** during #1057's grilling. Every one was
chosen from options presented with their consequences. They were decided on these facts, read from
PenTerm (`src-tauri/src/pty/manager.rs` and the 2026-09-30 SPEC of
`the-terminal-runs-on-our-own-engine`):

- PenTerm's local read loop hands every decoded chunk to more than the engine: connect steps (a
  login procedure that waits for text and expires a 10 s deadline on the 16 ms tick), an OSC 7
  scanner, and a debug-only kitty-keyboard scan.
- Its xterm.js path has no `Engine`. It sends bytes to the webview and stops issuing reads above
  2 MB unacknowledged, resuming at 512 KB on the frontend's "drawn" ack. xterm.js answers the child's
  queries, and its replies come back through the PTY writer.
- PenTerm keeps xterm as its default "until [Native] has earned the flip", with no date set. So an
  xterm path will exist for an open-ended time.

**R1 — An output observer, `Options::on_output`.** It is called on the reader thread with the
decoded bytes, the same bytes the engine or the raw sink receives, in order. It is called after the
has-the-session-ended check, so never after `on_exit`. A pass-through `Decode` could already see the
bytes, but it runs before that check, so it could observe one chunk after `on_exit`. A slow observer
slows reading, and the doc says so.

**R2 — A periodic tick, `Options::on_tick`.** It is called on the pump thread every `pump_every`,
in both modes. What to do on it, such as expiring a deadline, stays the consumer's policy. This is
D4's timer offered to the consumer; the pacing policy is unchanged.

The tick runs **under the session lock, immediately before the pump**, and receives
`Option<&Session>` (`None` in Raw mode). That gives a consumer two things:
- It can ask `Engine::frame_pending()` whether anything changed since the last pump.
- It can act on the answer before the pump resets it, atomically.

This is how PenTerm gates its prompt-input scan. Running the tick before the pump but outside the
lock would leave a window: output fed between the check and the pump is sent and reset, and its
change would never be seen. Running it after the pump would make the consumer re-lock the session.

The constraint is the same as `on_output`'s: the tick must not lock the session itself. This
refinement was settled at the maintainer's call on #1057, after PenTerm's migration planning raised
it.

**R6 — `Session::send_pending()`: send the pending frame now, and drain nothing.** It is today's
private flush, made public.

Under a `Pty`, only `justerm-pty` writes replies back. A consumer that called `pump()` itself would
drain replies no one then writes, and a querying application would wait forever. PenTerm's Output
Rules need the opposite case: a paint sent at once, not on the next tick.

So `Pty::session()` documents which `Session` calls are safe while the pty runs. `pump` and the
`drain_*` calls are not.

#1043 had left this call out, on the derivation that the next pump sends a paint within
`pump_every`. A real consumer need, plus the pump trap, is the better derivation that reopened it,
and the maintainer chose it on #1057.

**R3 — Raw mode: `Pty::spawn_raw`.** A pty with no `Engine` hands the decoded output to a byte sink
the consumer supplies. Raw mode widens D1's crate from "drives a `Session`" to "runs a PTY loop,
with or without the engine". It was chosen over declaring raw mode out of scope. In that alternative
PenTerm would keep a second local PTY loop for xterm, for an open-ended time, and that loop holds
exactly the logic #1045's completeness pass found six defects in: a reaped-pid kill, no SIGKILL
escalation, a zombie on drop, ConPTY EOF, and others. In Raw mode:
- There is no pump and no engine replies; the consumer's own terminal answers through `Pty::write`.
- `decoder`, `encoder`, `on_output` and `on_tick` all apply.
- `session()` returns `None`.

**R4 — Two constructors, not a generic `Pty<S>` or an enum argument.** `Pty::spawn` keeps its
signature, `Pty::spawn_raw` is added, and both share one loop. The consumer's code barely moves. The
alternatives were a generic, which is type-safe but has a heavier surface, and an enum argument,
which breaks `spawn`.

**R5 — Backpressure is pause and resume only.**
- The raw sink answers each chunk with continue or pause. On pause the reader issues no further
  read until `Pty::resume()`.
- Thresholds and credit accounting stay the consumer's (ADR-0017, D4). PenTerm itself runs two
  policies: 2 MB/512 KB local, 4 MB/1 MB SSH. Credit can be built on pause/resume in a few lines,
  but the reverse is not possible.
- At exit, a paused reader is released to drain within `DRAIN_AFTER_EXIT`, and no sink call
  follows `on_exit`. Exit polling continues while paused.
- **macOS refinement (#1057, maintainer's call after CI measurement).** XNU discards output nobody
  has read when the last slave fd closes. A paused child that exited would lose its tail.
  - A macOS Raw pty therefore keeps the slave open until the exit drain.
  - The kernel then holds the child's exit (a killed one too) until its output is read. On macOS a
    pause holds the child's exit as well, and the exit is reported after `resume`, with nothing
    lost.
  - `terminate` (and so `Drop`) releases a pause, so ending the child always completes.
  - The alternative shown, dropping the slave and recording the tail loss as a known hole (as
    node-pty and portable-pty do), was declined.

**Vocabulary.** **Raw mode**, the counterpart of *Frame mode*, is defined in GLOSSARY.

## Amended 2026-10-13, #1060: a Raw pty answers its ConPTY's start-up query

**P1 — On Windows, a Raw mode pty answers the ConPTY's start-up `ESC[6n` itself and withholds it
from the sink.** This narrows R3's "the consumer's own terminal answers" to the *child's* queries.
**A judgement made by the maintainer** at #1060's triage, over two alternatives shown with their
consequences: an `Options` switch to turn it off (no known consumer wants it), and leaving it to the
consumer with a stronger doc. It was decided on these facts:
- `portable-pty` 0.9 creates every ConPTY with `PSEUDOCONSOLE_INHERIT_CURSOR` (0.8.1 did not), so
  conhost asks for the cursor position before it runs the child, and runs nothing unanswered
  (measured in Raw mode: no start in 8 s). The asker is the pseudo-terminal, not the child.
- PenTerm's xterm.js answers through the same input path as typing, which a login procedure keeps
  closed from spawn: the answer was dropped and the shell never started.
- Measured on Windows 11 26200: conhost answers a *child's* CPR itself and never forwards it, as
  #1046 measured for DA1. So on Windows the consumer was only ever answering this one query, and
  #1060's own criterion that "a child's `ESC[6n` still reaches the sink" holds on Unix only.

**P2 — What it answers is the consumer's: `Options::start_cursor`, the terminal's cursor at spawn,
defaulting to the top left.** Its type, a 0-based `CursorPosition { row, col }`, is a derivation made
at implementation (the triage brief had said 1-based): 0-based like core's `Cursor`, and named
because the crate's sizes run `(cols, rows)`. **A judgement made by the maintainer**, over a fixed `1;1`. Decided on:
- conhost adopts the answer as fact (answered `10;5`, the child's later CPR read `10;10`), so a wrong
  answer puts conhost's cursor out of step with the consumer's terminal;
- PenTerm's default local reconnect reuses the same xterm with its content and cursor.
This is mechanism in the pty and the fact with the party that holds it. Frame mode never reads the
field: the engine answers from its own cursor.

**P3 — How the query is recognised** (a derivation): at the start of the raw output, before the
decoder, behind nothing but DEC private mode settings (`ESC[?…h`/`l`), which are handed on; a read
ending inside the query or a setting holds it for the next read; any other byte ends watching. The
settings are allowed because conhost's order differs by version (microsoft/terminal
`src/host/VtIo.cpp`: v1.12 and v1.17 send `ESC[?9001h ESC[?1004h` before `ESC[6n`; v1.21 and
Windows 11 26200 send the query first). The #1060 lens found this; a first-bytes-only match would
have hung on Windows 10's inbox conhost. Linux and macOS send no such query and are unchanged, which a Unix
test proves with a child whose first output is its own `ESC[6n`.
