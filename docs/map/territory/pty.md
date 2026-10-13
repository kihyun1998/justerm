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
Two more hooks serve a consumer that keeps state of its own beside the engine (#1057):
- `Options::on_output` sees each decoded chunk on the reader, exactly what the engine is fed;
- `Options::on_tick` runs on the pump thread just before each pump.

**Raw mode** (`Pty::spawn_raw`) runs the same three threads with no engine. The decoded output goes
to a consumer `RawSink`, and only the consumer answers the child's queries. On Windows the reader
answers the ConPTY's own start-up `ESC[6n` itself, from `Options::start_cursor`, and withholds it
from the sink (#1060). The sink's only backpressure is `Flow::Pause`, which stops the reader until
`Pty::resume`.
Everything it does is wiring between the OS and an engine that does no I/O itself.

## Governing decisions

- [**ADR-0034 — the family owns the PTY loop in its own crate, and the Session lives in core**](../../adr/0034-the-family-owns-the-pty-loop-and-the-session-lives-in-core.md)
  — D1 (its own crate), D4 (a default timer, pacing policy still the consumer's), D5 (no async
  runtime, consumer supplies the command), D6 (workspace member, three OSes in CI), and its
  2026-10-12 amendment (replies are not paced; `publish = false` until the loop is complete), and
  its #1057 amendment R1–R6 (the observer, the tick, Raw mode, two constructors, pause/resume as
  the only backpressure, `Session::send_pending`), and its #1060 amendment (a Raw pty answers its
  ConPTY's start-up query from a consumer-supplied cursor)
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
- **Ending suppresses late output, and the observer with it.** The reader checks `is_ended` (in Raw
  mode, `RawState::ended`) under the lock, before `on_output` runs. Late output needs a grandchild
  holding a Unix pty open, and that grandchild must ignore `SIGHUP`. Otherwise the session leader's
  exit hangs it up before it prints, and the test's positive control finds nothing. On macOS even
  that is not enough: measured in CI (#1057), the grandchild's write after the leader's exit fails
  with `EIO`, so the window exists on Linux only. Measured under WSL (#1057): removing the check, or
  calling the observer before it, reddens
  `nothing_is_observed_after_on_exit` (Frame) and `nothing_reaches_the_raw_sink_after_on_exit`
  (Raw).
- **The observer and the tick run under the lock.** `on_output` runs under the Session's lock (in
  Raw mode, the raw state's), so it sees exactly the engine's input, in order. Locking the Session
  from inside it deadlocks. `on_tick` runs under the same lock just before the pump, so it sees
  `frame_pending()` before the pump resets it. In Raw mode it gets `None`.
- **A paused Raw reader is released at exit, before the pseudo-terminal closes.** The pump sets
  `draining` and wakes the reader, so a child that exits while the sink has paused is still drained
  before `on_exit`. Then `ended` is set, under the same lock the reader checks. The release comes
  before the master is dropped because `ClosePseudoConsole` may write a last frame to the output
  pipe. Microsoft documents that it can then wait indefinitely if nobody drains the pipe, and it
  would wait on the pump thread with the master lock held.
- **Raw mode on Windows: the pty answers ConPTY's start-up `ESC[6n`** (#1060, which replaced "the
  consumer answers it"). The asker is conhost, not the child, and the child does not run until it is
  answered. Left to the consumer, the answer has to cross the consumer's UI: PenTerm's xterm.js
  answers through the same input path as typing, which a login procedure keeps closed from spawn, so
  the shell never started. Measured on Windows 11 26200 (#1060 triage):
  - unanswered, the child did not start in 8 s;
  - conhost answers a *child's* CPR itself and never forwards it, as #1046 measured for DA1. So the
    start-up query is the only `ESC[6n` a Windows Raw sink ever saw, and it was the first bytes of
    output, ahead of `?9001h` and the title;
  - conhost adopts the position it is told: answered `10;5`, a child that printed `START` then asked
    read back `10;10`.
- **What the reader answers is the consumer's fact, `Options::start_cursor`.** conhost draws from the
  answer, so `1;1` is right only for a fresh terminal. PenTerm's default local reconnect keeps the
  same xterm with its content and cursor (`clearOnReconnect` defaults to false, and xterm's `clear()`
  keeps the cursor's column). The default, the top left, is the fresh-terminal case. Frame mode never
  reads it: there the engine answers from its own cursor, which is the truth even for a Session fed
  before spawn.
- **The start-up query is matched on the raw bytes, before the decoder.** It is conhost's output, not
  the child's encoding, and withheld bytes must not enter a stateful decoder. Only DEC private mode
  settings (`ESC[?…h`/`l`) may come before it: conhost's order differs by version, read in
  microsoft/terminal `src/host/VtIo.cpp` — v1.12 and v1.17 send `ESC[?9001h` (with `ESC[?1004h`)
  *before* `ESC[6n`, v1.21 after it, and `main` sends `ESC[6n` first then DA1, `?1004h`, `?9001h`.
  Windows 10's inbox conhost is of the older line, so a "first bytes only" match would hang there.
  Those settings are handed on as they come; any other byte ends watching, so a later `ESC[6n` always
  reaches the sink. A read that ends inside the query or a setting holds those bytes (a setting at
  most `MAX_MODE_SETTING`) until the next read decides, and EOF hands them on. The reply goes through
  the writer, so the encoder applies to it as to any engine reply.
- **`start_cursor` is a `CursorPosition`, 0-based** like core's `Cursor`, with named fields: the
  crate's sizes are `(cols, rows)`, so a bare `(row, col)` tuple of the same type swaps silently.
  #1060's agent brief said 1-based; the 0-based field was chosen at implementation, and
  `cursor_report` adds the 1.
- **What a consumer may do through `Pty::session`** is in its doc-comment: subscribe, read, edit,
  `send_pending`. It must not `pump`, because the drained replies would never reach the child. A
  consumer that wants an edit on screen at once calls `Session::send_pending`, which drains nothing.
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
- **The slave is dropped right after spawn, except for a macOS Raw mode pty.** Otherwise this
  process keeps it open and a Unix reader never sees EOF.
- **macOS discards output nobody has read when the last slave fd closes.** XNU's `ptsclose` →
  `ttyclose` runs `ttyflush(tp, FREAD | FWRITE)`, and the master then reads 0 bytes and EOF. Linux's
  `pty_close` only marks the other side closed and delivers what is buffered.
  - Measured in CI (#1057, macos-latest): a child writes 455 bytes and exits, and the parent reads
    afterwards. With the slave dropped, it got 0 bytes and EOF; with the slave held, 455/455, and EOF
    once the slave was closed. Linux read 455/455 both ways.
  - A reader that keeps reading is not hit: 5 runs of a 202 KB burst followed by an immediate exit
    all arrived whole on macOS. Frame mode keeps its old path.
  - A paused Raw reader is hit. A Raw pty on macOS therefore keeps the slave until the exit drain:
    the pump releases the reader, waits for it to sit in one `read` for `READER_IDLE`, then drops
    the slave so EOF arrives. All of it stays within `DRAIN_AFTER_EXIT`.
  - **With the slave held, the kernel holds the child's exit until its output is read.** That
    includes a killed child. Measured in CI (#1057), with the sink paused: a `terminate` went
    unreported for 10 s, then was reported at once after `resume`. A natural exit was sometimes
    reported at once and sometimes held past 20 s. On Windows both took about 25 ms. So on macOS a
    pause holds the exit too (the ADR-0034 macOS refinement), and `terminate` releases the pause
    before it kills.
  - `a_child_that_exits_while_paused_loses_nothing_once_resumed` is the macOS proof that the tail
    survives. Without the held slave the same scenario read 0 of 455 bytes. On Linux and Windows
    nothing is discarded, so it cannot redden there. `terminate_ends_a_paused_raw_pty` is the macOS
    proof of the release; it is green on Windows with or without the release.
  - Prior art does not fix this: node-pty, portable-pty, alacritty and creack/pty drop the slave;
    pexpect#662 was closed unfixed; Go's `cmd/go` tests delay the child's exit instead.
- **Own public types only.** `Command`, `Options`, `Exit` wrap `portable_pty`'s, so a `portable-pty`
  major bump is not a breaking change here.

## Code

- `justerm-pty/src/lib.rs` — `Pty::spawn`, `Pty::spawn_raw`, `Pty::session`, `Pty::resume`,
  `Pty::write`, `Pty::resize`, `Pty::terminate`, `start`, `Target`, `RawState`, `Reading`, `Pump::run`,
  `spawn_reader`, `decode_and_feed`, `feed_session`, `feed_raw`, `spawn_writer`, `write_back`,
  `StartupQuery`, `lead`, `STARTUP_QUERY`, `MAX_MODE_SETTING`, `cursor_report`, `CursorPosition`,
  `Options::start_cursor`, `EXIT_POLL`,
  `DRAIN_AFTER_EXIT`, `READER_IDLE`, `Pump::wait_until_the_reader_is_idle`
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
  start-up `ESC[6n` proves the reader's write-back (in Frame mode; in Raw mode the reader's own
  start-up answer), and no test there proves the pump's. The Unix
  proof was mutation-run on Linux under WSL (#1046), and each mutation reddened exactly its test:
  - dropping the pump's hand-over → `a_reply_the_pump_drains_reaches_the_child`;
  - dropping the reader's hand-over → `a_query_from_the_child_is_answered`;
  - `terminate` sending `SIGHUP` only → `terminate_ends_a_child_that_ignores_sighup`.
- **A grandchild holding a Unix pty open keeps the reader thread blocked** after the exit is reported.
  The reader has its own fd, so dropping the master does not release it. It feeds nothing once the
  Session (or the Raw sink) has ended, so this is a parked thread rather than wrong output. A ConPTY reader is released
  by closing the pseudo-console.
- **A spawn error keeps its `ErrorKind` only where `portable-pty` passed an `io::Error` through.**
  On Windows it formats a failed `CreateProcessW` into a string, so a missing program is `Other`.
- **A start-up query split across reads, and mode settings ahead of it, are proven by unit test
  only.** conhost writes the query in one piece, and the measured build (Windows 11 26200) sends it
  first, so no real-process test here reaches either: the unit tests drive `StartupQuery` with every
  split point and with v1.17's order. An older conhost is the unmeasured case.
- **conhost's `main` also sends DA1 (`ESC[c`) at start, after the cursor query** (microsoft/terminal
  `src/host/VtIo.cpp`), and does not wait for its answer. In Raw mode it reaches the sink, and the
  consumer's terminal answers it into the child's input. No shipped build measured here sends it
  (Windows 11 26200 does not), so what conhost does with that answer is unmeasured.
- **The "nothing after `on_exit`" tests run on Linux only, and the Raw DA1 and CPR tests on Unix only.** The
  first need a grandchild writing to a pty after the child's exit: a ConPTY ends with its
  pseudo-console, and macOS fails the write with `EIO`. The second is the conhost reason above.
- **No test reaches the paused-close order.** Terminating a Raw pty while its sink is paused
  reported the exit in 11 ms on Windows 11 build 26200 even with the release after the close
  (#1057 lens probe), so the documented `ClosePseudoConsole` wait did not occur on this build.
  Swapping the order back stays green here; older Windows builds are the unmeasured risk.
- **No test reaches the pid-reuse guard** (`ChildState::exited`). A reused pid cannot be arranged
  deterministically, so removing the guard stays green on every OS. It rests on reading
  `portable-pty`'s Unix killer, which sends a bare `kill(pid, SIGHUP)`.
