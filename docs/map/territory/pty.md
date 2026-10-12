# Territory — the PTY loop

## What it is

The family's one I/O crate, `justerm-pty`. It spawns a consumer-chosen command in a pseudo-terminal
and drives a core `Session` from it, using three threads:
- **reader:** feeds the child's output into the Session and hands the engine's replies to the writer;
- **pump:** pumps the Session on a timer, hands its events to the consumer, and watches for the
  child's exit;
- **writer:** the only thread that writes to the child. It writes the engine's replies and the
  consumer's `Pty::write`s, in the order they were handed over.

`Pty::resize` resizes the Session, then the pseudo-terminal to the size the engine adopted.
`Options::decoder` and `Options::encoder` let a consumer convert a child's legacy encoding, while
choosing the encoding stays the consumer's policy:
- the decoder runs on the reader before each chunk is fed;
- the encoder runs on the writer over everything bound for the child.
Everything it does is wiring between the OS and an engine that does no I/O itself.

## Governing decisions

- [**ADR-0034 — the family owns the PTY loop in its own crate, and the Session lives in core**](../../adr/0034-the-family-owns-the-pty-loop-and-the-session-lives-in-core.md)
  — D1 (its own crate), D4 (a default timer, pacing policy still the consumer's), D5 (no async
  runtime, consumer supplies the command), D6 (workspace member, three OSes in CI), and its
  2026-10-12 amendment (replies are not paced; `publish = false` until the loop is complete)
- [ADR-0030](../../adr/0030-which-directory-owns-what.md) — the `justerm-pty/` row and its re-open note
- `docs/architecture.md` §Cadence — the timer paces frames only

## Design model

- **One lock for the Session, one channel for the child.** The reader and the pump share the Session
  behind one mutex, and the consumer reaches the same one through `Pty::session`. Nothing holds it
  across a blocking call. Every byte bound for the child goes through one `mpsc` channel to the
  writer thread, which gives two guarantees:
  - order holds between the engine's replies and the consumer's input;
  - a blocking write never stalls the reader. Without that, a child flooding queries while never
    reading its stdin used to fill the pty input queue and freeze the read loop.
- **Replies are handed over by whoever drained them, at once.** The reader hands over the replies to
  what it just fed. The pump hands over replies that something else produced, such as a consumer's
  `report_*`, or bytes fed through `Session::edit`. Pacing them with frames would hold a Windows
  child for a whole period, because a ConPTY asks `ESC[6n` before the child may run. Measured
  (#1045): answered, the child exited in 352 ms; unanswered, nothing happened in 8 s.
- **On Windows, conhost is the console's terminal.** Measured (#1046):
  - conhost answered the child's DA1 itself, with `?61;6;7;21;22;23;24;28;32;42c`, never forwarding
    it;
  - it consumed a VT reply that arrived as input (`ESC[0n`), so the child read an empty line.

  The engine's replies therefore matter on Windows only for conhost's own queries, such as the
  start-up `ESC[6n`. The "a child's query is answered" and "a pump-drained reply reaches the child"
  tests run on Unix only, where the tty passes the bytes through.
- **Resize is engine first, then the pty at the adopted size.** The engine widens a too-narrow
  width to its minimum. Telling the child the requested width would make the two disagree, so the
  child is told the adopted one.
- **Exit is polled, not awaited.** A ConPTY reader can stay blocked after the child has exited, so EOF
  cannot be the exit signal. The pump asks `try_wait` on its own tick, at most `EXIT_POLL`, so a long
  pump period does not delay it.
- **Closing comes before draining.** On exit, the pump first drops the writer and the master. On a
  ConPTY that makes conhost flush and end its output pipe, so the reader reaches EOF. Then the pump
  waits up to `DRAIN_AFTER_EXIT` for that EOF, ends the Session (which sends what is pending), and
  reports the exit. `output_written_just_before_exit_still_arrives` guards the tail: a child that
  prints and exits at once. The order changes latency, not correctness. Measured on Windows (#1045,
  5 runs each, from spawn to the exit being reported):
  - close first: median 49 ms;
  - drain first: median 545 ms, since the 500 ms bound ran out every time.

  No test asserts this timing, so swapping the order back stays green.
- **Ending suppresses late output, but no test reaches it.** The reader checks `is_ended` only when
  output arrives after the end, which needs a grandchild holding a Unix pty open. Removing the check
  stays green.
- **`on_exit` exactly once, even after a drop.** `Drop` is `terminate`, and the pump always runs to
  the exit, so the child is reaped (no Unix zombie) and the consumer is told. Once the Session has
  ended, the reader feeds nothing: the check sits under the same lock as `end`, so no frame follows
  `on_exit`.
- **Never signal a reaped child.** `ChildState::exited` is set under the lock `terminate` takes.
  After a Unix reap the pid can belong to an unrelated process, and `portable-pty`'s killer sends a
  bare `kill(pid, SIGHUP)`.
- **Terminate escalates on Unix.** It calls the child's own `kill`, which is `SIGHUP` then `SIGKILL`
  after about 250 ms, not the `clone_killer` handle, which sends `SIGHUP` only. On Windows it is
  `TerminateProcess`, exit code 127. Either way the resulting exit is reported the same as a natural
  one.
- **The slave is dropped right after spawn.** Otherwise this process keeps it open and a Unix reader
  never sees EOF.
- **Own public types only.** `Command`, `Options`, `Exit` wrap `portable_pty`'s, so a `portable-pty`
  major bump is not a breaking change here.

## Code

- `justerm-pty/src/lib.rs` — `Pty::spawn`, `Pty::write`, `Pty::resize`, `Pty::terminate`,
  `Pump::run`, `spawn_reader`, `spawn_writer`, `write_back`, `EXIT_POLL`, `DRAIN_AFTER_EXIT`
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
- `.github/workflows/test.yml`'s `pty` job runs this crate on macOS and Windows; Linux runs in
  `test`. To see another OS before CI does, follow
  [`docs/agents/local-os-checks.md`](../../agents/local-os-checks.md).

## Known holes

- **A legacy decoder is proven on Unix only.** On Windows a console child cannot put legacy bytes on
  the pseudo-terminal. Measured (#1047): Rust's std refuses to write non-UTF-8 to a console with
  `InvalidData`. By ConPTY's design, not measured here, conhost re-emits console output as UTF-8.
  Windows proves where the decoder and encoder sit, with an ASCII rewrite, but not that a split
  character is reassembled.
- **The channel to the writer is unbounded.** A child that floods queries while never reading its
  stdin no longer stalls the reader. Its replies queue in memory instead, until the child exits.
- **The reply tests are proven on Unix only**, for the conhost reason above. On Windows only the
  start-up `ESC[6n` proves the reader's write-back, and no test there proves the pump's. The Unix
  proof was mutation-run on Linux under WSL (#1046), and each mutation reddened exactly its test:
  - dropping the pump's hand-over → `a_reply_the_pump_drains_reaches_the_child`;
  - dropping the reader's hand-over → `a_query_from_the_child_is_answered`;
  - `terminate` sending `SIGHUP` only → `terminate_ends_a_child_that_ignores_sighup`.
- **A grandchild holding a Unix pty open keeps the reader thread blocked** after the exit is reported.
  The reader has its own fd, so dropping the master does not release it. It feeds nothing once the
  Session has ended, so this is a parked thread rather than wrong output. A ConPTY reader is released
  by closing the pseudo-console.
- **A spawn error keeps its `ErrorKind` only where `portable-pty` passed an `io::Error` through.**
  On Windows it formats a failed `CreateProcessW` into a string, so a missing program is `Other`.
- **No test reaches the pid-reuse guard** (`ChildState::exited`). A reused pid cannot be arranged
  deterministically, so removing the guard stays green on every OS. It rests on reading
  `portable-pty`'s Unix killer, which sends a bare `kill(pid, SIGHUP)`.
