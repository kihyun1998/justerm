# Territory — the PTY loop

## What it is

The family's one I/O crate, `justerm-pty`. It spawns a consumer-chosen command in a pseudo-terminal
and drives a core `Session` from it. A reader thread feeds the child's output in and writes the
engine's replies straight back. A pump thread pumps the Session on a timer, hands its events to the
consumer, and watches for the child's exit. Everything it does is wiring between the OS and an engine
that does no I/O itself.

## Governing decisions

- [**ADR-0034 — the family owns the PTY loop in its own crate, and the Session lives in core**](../../adr/0034-the-family-owns-the-pty-loop-and-the-session-lives-in-core.md)
  — D1 (its own crate), D4 (a default timer, pacing policy still the consumer's), D5 (no async
  runtime, consumer supplies the command), D6 (workspace member, three OSes in CI), and its
  2026-10-12 amendment (replies are not paced; `publish = false` until the loop is complete)
- [ADR-0030](../../adr/0030-which-directory-owns-what.md) — the `justerm-pty/` row and its re-open note
- `docs/architecture.md` §Cadence — the timer paces frames only

## Design model

- **Two threads, one lock.** The reader and the pump share the Session behind one mutex, and the
  consumer reaches the same one through `Pty::session`. Nothing holds it across a blocking call.
- **Replies are written by whoever drained them, at once.** The reader writes the replies to what it
  just fed. The pump writes replies that something else produced, such as a consumer's `report_*`.
  Pacing them with frames would hold a Windows child for a whole period, because a ConPTY asks
  `ESC[6n` before the child may run (measured #1045: answered, the child exited in 352 ms;
  unanswered, nothing happened in 8 s).
- **Exit is polled, not awaited.** A ConPTY reader can stay blocked after the child has exited, so EOF
  cannot be the exit signal. The pump asks `try_wait` on its own tick, at most `EXIT_POLL`, so a long
  pump period does not delay it. On exit it waits up to `DRAIN_AFTER_EXIT` for the reader to finish,
  then drops the writer and the master, which releases the reader. Then it ends the Session (which
  sends what is pending) and reports the exit once.
- **The slave is dropped right after spawn.** Otherwise this process keeps it open and a Unix reader
  never sees EOF.
- **Terminate is a kill.** The resulting exit travels the ordinary path, so it is reported exactly
  like a natural one.
- **Own public types only.** `Command`, `Options`, `Exit` wrap `portable_pty`'s, so a `portable-pty`
  major bump is not a breaking change here.

## Code

- `justerm-pty/src/lib.rs` — `Pty::spawn`, `Pty::terminate`, `Pump::run`, `spawn_reader`,
  `write_back`, `EXIT_POLL`, `DRAIN_AFTER_EXIT`
- `justerm-pty/tests/pty.rs` — `HELPER`, `helper`: the harness-free runner. With that variable set
  it is the child; without it, it runs the tests against itself.

## Reference behaviour

Not compared against a pinned reference yet. The layout peers (wezterm's `pty/` crate, alacritty's
`tty` module) are recorded in ADR-0030's re-open note. PenTerm's local PTY loop
(`src-tauri/src/pty/manager.rs`) is the consumer this replaces, not a reference.

## Cross-cutting invariants

- The engine does no I/O (CLAUDE.md). Every OS call in the family lives here.

## Blast radius

- A change to `Session`'s pump or flush contract changes what the pump thread sends.
- `.github/workflows/test.yml`'s `pty` job runs this crate on macOS and Windows. Linux runs in `test`.

## Known holes

- No writer for the consumer's own input, and a resize does not reach the pseudo-terminal: #1046.
- No encoding hook. Output is fed as received: #1047.
- `publish = false`: the publish step and crates.io token land with #1047.
- The explicit cross-OS "query → reply arrives" test needs raw mode on Unix and comes with #1046. On
  Windows, the start-up `ESC[6n` already proves the **reader's** write-back: without it, every test
  times out.
- **The pump's write-back is unproven.** Removing it leaves every test green (mutation run, #1045).
  The replies it carries are the consumer's own, e.g. from `report_*`. A line-mode child receives
  bytes without a trailing newline only once something else ends the line, which needs #1046's
  consumer writer. Prove it there.
