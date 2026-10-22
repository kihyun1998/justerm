# Territory — the PTY loop

## What it is

The family's one I/O crate, `justerm-pty`. It spawns a consumer-chosen command in a pseudo-terminal
and either drives a core [Session](session.md) from it (**Frame mode**, `Pty::spawn`) or hands the
child's decoded output to a consumer `RawSink` with no engine at all (**Raw mode**, `Pty::spawn_raw`;
GLOSSARY "Raw mode"). Three threads run for it:
- **reader:** reads the child's output (64 KiB per read), runs the decoder and the observer, and
  feeds the Session (handing the replies that feed produced to the writer at once) or the sink;
- **pump:** every `pump_every` runs the tick and pumps the Session, hands its events to the
  consumer, writes back replies something else queued, and polls for the child's exit;
- **writer:** the only thread that writes to the child — the engine's replies and the consumer's
  `Pty::write`s, in hand-over order, through the encoder.

`Pty::resize` resizes the Session, then the pseudo-terminal to the size the engine adopted. The
consumer keeps the policy: the command, the encoding (`Options::decoder` / `Options::encoder`), the
pace (`Options::pump_every`, default 16 ms), and what to do on each chunk and tick
(`Options::on_output`, `Options::on_tick`). `justerm-core` stays I/O-free (CLAUDE.md "No I/O"); every
OS call in the family lives here.

## Governing decisions

- [**ADR-0034 — the family owns the PTY loop in its own crate, and the Session lives in core**](../../adr/0034-the-family-owns-the-pty-loop-and-the-session-lives-in-core.md)
  — D1 (its own crate), D4 (a default timer; pacing policy stays the consumer's), D5 (no async
  runtime; the consumer supplies the command; encoding is a hook), D6 (workspace member on the `v*`
  lockstep, real-process tests on three OSes), its 2026-10-12 amendment (replies are not paced;
  `publish = false` until the loop was complete), its #1057 amendment R1–R6 (the observer, the tick,
  Raw mode, two constructors, pause/resume as the only backpressure, `Session::send_pending`, and the
  macOS held-slave refinement), and its #1060 amendment P1–P3 (a Raw pty answers its ConPTY's
  start-up query from a consumer-supplied cursor, recognised behind DEC private mode settings).
- [ADR-0030 — which directory owns what](../../adr/0030-which-directory-owns-what.md) — the
  `justerm-pty/` row and its 2026-10-12 re-open note.
- `docs/architecture.md` §Cadence — the timer paces frames only; replies and exit detection are off it.

## Design model

From ADR-0034 and the source (`justerm-pty/src/lib.rs`); measurements are attributed to the issue
that took them.

**Locks and order**
- **One lock for the Session, one channel for the child.** The reader and the pump share the Session
  behind one mutex, and the consumer reaches the same one through `Pty::session`. The pty itself holds
  it across no blocking call; the consumer's `on_output` and `on_tick` run under it. Every byte bound
  for the child goes through one unbounded `mpsc` channel (`Input`) to the writer thread, which gives
  two guarantees:
  - order holds between the engine's replies and the consumer's input — both the reader and the pump
    hand replies over while still holding the Session lock, so drain order is hand-over order;
  - a blocking write never stalls the reader. Without that, a child flooding queries while never
    reading its stdin used to fill the pty input queue and freeze the read loop.
- **What a consumer may do through `Pty::session`** is in its doc-comment: subscribe, read, `edit`,
  `send_pending`. It must not `pump` (the drained replies would never reach the child), nor `feed` or
  `end` (the reader's and the pump's), and it resizes through `Pty::resize`. Why `pump` drains replies
  at all is the [Session](session.md)'s.
- **The observer and the tick run under the lock.** `on_output` runs under the Session's lock (in Raw
  mode, the raw state's), so it sees exactly the engine's input, in order; locking the Session from
  inside it deadlocks. `on_tick` runs under the same lock just before the pump, so it sees
  `frame_pending()` before the pump resets it (ADR-0034 R2). In Raw mode it gets `None`.

**Replies**
- **Replies are handed over by whoever drained them, at once.** The reader drains the replies its own
  feed produced (`feed_session`). The pump hands over replies something else produced, such as a
  consumer's `report_*` or bytes fed through `Session::edit`. Pacing them with frames would hold a
  Windows child for a whole period, because a ConPTY asks `ESC[6n` before the child may run. Measured
  (#1045): answered, the child exited in 352 ms; unanswered, nothing happened in 8 s.
- **On Windows, conhost is the console's terminal.** Measured (#1046):
  - conhost answered the child's DA1 itself, with `?61;6;7;21;22;23;24;28;32;42c`, never forwarding
    it;
  - it consumed a VT reply that arrived as input (`ESC[0n`), so the child read an empty line.

  The engine's replies therefore matter on Windows only for conhost's own queries, such as the
  start-up `ESC[6n`. `a_query_from_the_child_is_answered` and
  `a_reply_the_pump_drains_reaches_the_child` run on Unix only, where the tty passes the bytes through.

**Size**
- **Resize is engine first, then the pty at the adopted size.** The engine widens a too-narrow width to
  its minimum; telling the child the requested width would make the two disagree, so the child is
  told the adopted one. The Session stays locked until the pty has its size, so two racing resizes
  cannot split. In Raw mode there is no engine: the child is told the size as asked. `pty_size`
  saturates both axes at `u16::MAX`.

**Exit**
- **Exit is polled, not awaited.** A ConPTY reader can stay blocked after the child has exited, so EOF
  cannot be the exit signal. The pump asks `try_wait` on its own tick, at most `EXIT_POLL` (50 ms),
  so a long pump period does not delay it.
- **Closing comes before draining.** On exit the pump first releases a paused Raw reader, then drops
  the writer's channel and the master. On a ConPTY that makes conhost flush and end its output pipe,
  so the reader reaches EOF. Then it waits up to `DRAIN_AFTER_EXIT` (500 ms) for that EOF, does a last
  pump and ends the Session (which sends what is pending), and reports the exit.
  `output_written_just_before_exit_still_arrives` guards the tail. The order changes latency, not
  correctness. Measured on Windows (#1045, 5 runs each, spawn to exit reported): close first, median
  49 ms; drain first, median 545 ms (the 500 ms bound ran out every time). No test asserts this
  timing, so swapping the order back stays green.
- **Ending suppresses late output, and the observer with it.** The reader checks `is_ended` (in Raw
  mode, `RawState::ended`) under the lock, before `on_output` runs. Late output needs a grandchild
  holding a Unix pty open and ignoring `SIGHUP`; otherwise the leader's exit hangs it up before it
  prints and the test's positive control finds nothing. On macOS even that is not enough: measured in
  CI (#1057), the grandchild's write after the leader's exit fails with `EIO`, so the window exists on
  Linux only. Measured under WSL (#1057): removing the check, or calling the observer before it,
  reddens `nothing_is_observed_after_on_exit` (Frame) and `nothing_reaches_the_raw_sink_after_on_exit`
  (Raw).
- **`on_exit` exactly once, even after a drop.** `Drop` is `terminate`, and the pump always runs to the
  exit, so the child is reaped (no Unix zombie) and the consumer is told. Nothing reaches the Session
  or the sink after it, so no frame or output follows `on_exit`.
- **Never signal a reaped child.** `ChildState::exited` is set under the lock `terminate` takes. After a
  Unix reap the pid can belong to an unrelated process, and `portable-pty`'s killer sends a bare
  `kill(pid, SIGHUP)` (read in `portable-pty`, #1045).
- **Terminate escalates on Unix.** It calls the child's own `kill`, which is `SIGHUP` then `SIGKILL`
  after about 250 ms, not the `clone_killer` handle, which sends `SIGHUP` only. On Windows it is
  `TerminateProcess`, exit code 127 (both read in `portable-pty`, #1045). Either way the exit is
  reported as a natural one. In Raw mode `terminate` first releases a pause.
- **A failed read is reported on `Exit::read_error`, with no timing test** (#1062). The dependencies
  already turn the ordinary end into `Ok(0)`: `filedescriptor` 0.8.3's Windows `read` maps
  `ReadFile`'s `BrokenPipe` to `Ok(0)`, and `portable-pty` 0.9.0's Unix `PtyFd::read` maps `EIO`
  (slave closed) to `Ok(0)`. So any other `Err` but `Interrupted` is a real failure, and the reader
  keeps its kind in `Reading::error` before setting `eof`; the pump copies it into the `Exit` after the
  drain wait. Riding on `on_exit` rather than a callback of its own means "exactly once, and last"
  needed no second ordering rule. `a_child_that_exits_reports_no_read_error` and the terminate test
  notice a dependency bump that stops mapping the end to `Ok(0)`. `Exit::success()` stays the child's
  status alone.

**Encoding**
- **On Windows the ConPTY speaks UTF-8 at both ends** (#1066), and conhost converts to and from the
  console code page itself: the decoder only ever receives UTF-8 and the encoder must hand back UTF-8.
  A legacy hook there garbles the text, so a legacy child's encoding is set by its console code page,
  not by a hook. The crate header, `Decode`, `Encode`, both `Options` fields and the README say so
  because PenTerm, reading them without it, passed its CP949 decoder on Windows too and rendered
  Korean as `�쓿�` (#1048).
- The decoder runs on the reader before each chunk is fed; `Decode::finish` flushes what it holds at
  EOF. The encoder runs on the writer over everything bound for the child.

**Raw mode**
- **Same three threads, no engine** (ADR-0034 R3). The sink's only backpressure is `Flow::Pause`,
  which parks the reader on a condvar until `Pty::resume`, the exit drain, or the end (R5).
- **A paused Raw reader is released at exit, before the pseudo-terminal closes.** The pump sets
  `draining` and wakes the reader, so a child that exits while the sink has paused is still drained
  before `on_exit`; then `ended` is set under the same lock the reader checks. The release comes first
  because `ClosePseudoConsole` may write a last frame to the output pipe, and Microsoft documents that
  it can then wait indefinitely if nobody drains the pipe — on the pump thread, with the master lock
  held.
- **On Windows the pty answers ConPTY's start-up `ESC[6n`** (#1060, P1). The asker is conhost, not the
  child, and the child does not run until it is answered. Left to the consumer, the answer has to
  cross the consumer's UI: PenTerm's xterm.js answers through the same input path as typing, which a
  login procedure keeps closed from spawn, so the shell never started. Measured on Windows 11 26200
  (#1060 triage): unanswered, the child did not start in 8 s; conhost answers a *child's* CPR itself
  and never forwards it, so the start-up query is the only `ESC[6n` a Windows Raw sink ever saw, and it
  was the first bytes of output; conhost adopts the position it is told (answered `10;5`, a child that
  printed `START` then asked read back `10;10`).
- **What the reader answers is the consumer's fact, `Options::start_cursor`** (P2). conhost draws from
  the answer, so `1;1` is right only for a fresh terminal; PenTerm's default local reconnect keeps the
  same xterm with its content and cursor. It is a `CursorPosition`, 0-based like core's `Cursor`, with
  named fields, because the crate's sizes are `(cols, rows)` and a bare `(row, col)` tuple of the same
  type would swap silently; `cursor_report` adds the 1 and clamps to the size. Frame mode never reads
  it: the engine answers from its own cursor.
- **The start-up query is matched on the raw bytes, before the decoder** (P3). It is conhost's output,
  not the child's encoding, and withheld bytes must not enter a stateful decoder. Only DEC private
  mode settings (`ESC[?…h`/`l`) may come before it, because conhost's order differs by version
  (microsoft/terminal `src/host/VtIo.cpp`: v1.12 and v1.17 send `ESC[?9001h` with `ESC[?1004h`
  *before* `ESC[6n`, v1.21 after it, `main` sends `ESC[6n` first then DA1, `?1004h`, `?9001h`);
  Windows 10's inbox conhost is of the older line, so a "first bytes only" match would hang there.
  Those settings are handed on as they come; any other byte ends watching, so a later `ESC[6n` always
  reaches the sink. A read that ends inside the query or a setting holds those bytes (a setting at most
  `MAX_MODE_SETTING`) until the next read decides, and EOF hands them on. The reply goes through the
  writer, so the encoder applies to it as to any engine reply. Unix sends no such query and is
  unchanged.
- **macOS discards output nobody has read when the last slave fd closes**, so a macOS Raw pty keeps the
  slave until the exit drain (the R5 macOS refinement). XNU's `ptsclose` → `ttyclose` runs
  `ttyflush(tp, FREAD | FWRITE)`, and the master then reads 0 bytes and EOF; Linux's `pty_close` only
  marks the other side closed and delivers what is buffered.
  - Measured in CI (#1057, macos-latest): a child writes 455 bytes and exits, the parent reads
    afterwards. Slave dropped: 0 bytes and EOF; slave held: 455/455, and EOF once it was closed. Linux
    read 455/455 both ways.
  - A reader that keeps reading is not hit: 5 runs of a 202 KB burst followed by an immediate exit all
    arrived whole on macOS. So every other pty still drops the slave right after spawn (otherwise this
    process keeps it open and a Unix reader never sees EOF).
  - At exit the pump releases the reader, waits for it to sit in one `read` for `READER_IDLE`
    (`Pump::wait_until_the_reader_is_idle`), then drops the slave so EOF arrives, all within
    `DRAIN_AFTER_EXIT`.
  - **With the slave held, the kernel holds the child's exit until its output is read**, a killed child
    included. Measured in CI (#1057), sink paused: a `terminate` went unreported for 10 s, then was
    reported at once after `resume`; a natural exit was sometimes reported at once and sometimes held
    past 20 s. On Windows both took about 25 ms. So on macOS a pause holds the exit too, and
    `terminate` releases the pause before it kills.
  - `a_child_that_exits_while_paused_loses_nothing_once_resumed` is the macOS proof that the tail
    survives (without the held slave it read 0 of 455 bytes); it cannot redden on Linux or Windows,
    where nothing is discarded. `terminate_ends_a_paused_raw_pty` is the macOS proof of the release; it
    is green on Windows with or without it.

**Inputs and their bounds**
- The child's output arrives in reads of at most 64 KiB; the Session's queues between pumps are
  bounded only by that rate times `pump_every`. A Raw sink bounds its own intake with `Flow::Pause`.
- The channel to the writer is unbounded (see Known holes). `Pty::write` takes any slice.

**Own public types only.** `Command`, `Options`, `Exit`, `CursorPosition` are this crate's, wrapping
`portable_pty`'s, so a `portable-pty` major bump is not a breaking change here. `Exit` is not
`#[non_exhaustive]` (#844): a consumer only receives one.

## Code

- `justerm-pty/src/lib.rs` — `Command`, `Decode`, `Encode`, `Options`, `CursorPosition`, `OnOutput`,
  `OnTick`, `RawSink`, `Flow`, `Exit::read_error`, `Exit::success`, `Pty::spawn`, `Pty::spawn_raw`,
  `Pty::session`, `Pty::resume`, `Pty::write`, `Pty::resize`, `Pty::terminate`, `Target`, `Raw`,
  `RawState`, `ChildState`, `Input`, `Master`, `start`, `start_with`, `pty_size`, `write_back`,
  `spawn_writer`, `Reading`, `spawn_reader`, `decode_and_feed`, `feed`, `feed_raw`, `feed_session`,
  `Pump::run`, `Pump::pump`, `Pump::wait_until_the_reader_is_idle`, `StartupQuery`, `Lead`, `lead`,
  `STARTUP_QUERY`, `MODE_SETTING_LEAD`, `MAX_MODE_SETTING`, `cursor_report`, `EXIT_POLL`,
  `DRAIN_AFTER_EXIT`, `READER_IDLE`; its unit tests drive `StartupQuery` at every split point and
  inject a failing reader through `start_with` (`a_failed_read_is_reported_on_the_exit`)
- `justerm-pty/tests/pty.rs` — `HELPER`, `helper`, `main`: a harness-free runner. With that variable
  set the binary is the child; without it, it runs the tests against itself, gating some by OS with
  `cfg` inside `main`
- `.github/workflows/test.yml` — the `pty` job (macOS, Windows); Linux runs in `test`

## Reference behaviour

No row in `docs/agents/reference-facts.md`. The comparisons this territory made are attributed to
their own sources:
- conhost's start-up order per version — microsoft/terminal `src/host/VtIo.cpp`, read at #1060
  (unpinned);
- dropping the slave after spawn — node-pty, portable-pty, alacritty and creack/pty drop it;
  pexpect#662 was closed unfixed; Go's `cmd/go` tests delay the child's exit instead (#1057,
  unpinned);
- the crate layout — wezterm's `pty/` crate and alacritty's `tty` module, in ADR-0030's re-open note.

PenTerm's former local PTY loop is the consumer this replaced, not a reference.

## Cross-cutting invariants

- [whoever drains a reply owns writing it back](../invariant/a-drained-reply-is-the-drainers-to-write.md)
  — `feed_session` and `Pump::pump` drain and `write_back` under one lock, and `Pty::session` forbids a consumer to pump underneath them

"The engine does no I/O" is CLAUDE.md's boundary statement, not an invariant note; this
crate is where it is discharged.

## Blast radius

- [session](session.md) — the reader and pump assume `pump` drains both queues and that a frame leaves
  only when `frame_pending` is raised; the rule that a consumer must not pump exists because `pump`
  takes the replies only this crate writes back.
- [events & replies](events-and-replies.md) — every reply the engine queues is written to the child
  unexamined, and every event goes to `on_event` unfiltered. A reply path whose bytes a child must not
  receive, or an event volume a consumer cannot absorb, meets no filter here.
- [CI & supply chain](ci-and-supply-chain.md) — the real-process tests are the only proof of
  OS-specific behaviour, and several run on one or two OSes only (`cfg` in `main`); a test added
  without a matching `cfg` claims an OS it never ran on. To see another OS before CI does, follow
  [`docs/agents/local-os-checks.md`](../../agents/local-os-checks.md).
- [release](release.md) — the crate is on the `v*` lockstep with `justerm-core` and sits directly on
  the Session API, so a Session change is a `justerm-pty` release too.

## Known holes / open

- **A legacy decoder is proven on Unix only.** On Windows a console child cannot put legacy bytes on
  the pseudo-terminal. Measured (#1047): Rust's std refuses to write non-UTF-8 to a console with
  `InvalidData`. And conhost converts both directions, measured (#1066, Windows 11, OEMCP=ACP=949):
  - output: a child writing `C7 D1 B1 DB` (`한글` in CP949) reached a Raw sink with no decoder as UTF-8
    `한글` under `chcp 949` and as `EF BF BD D1 B1 EF BF BD` under `chcp 65001`. So conhost decodes by
    the console code page and sends UTF-8 either way. PenTerm's `type` of a CP949 file came out as
    `한글` under both code pages; that `type` converts the file itself and writes UTF-16 is the likely
    reason, not measured;
  - input: `한글` written as UTF-8 reached a `chcp 949` child as `C7 D1 B1 DB`, and as `???` through a
    CP949 encoder. Under `chcp 65001` it arrived as UTF-8, and as `EF BF BD D1 B1 EF BF BD` through the
    encoder.

  Both children were Python with `PYTHONLEGACYWINDOWSSTDIO=1` (raw `WriteFile`/`ReadFile`), and the
  input child printed what it read as hex. Neither is a test: it is conhost's behaviour. Windows proves
  where the decoder and encoder sit, with an ASCII rewrite, but not that a split character is
  reassembled.
- **The channel to the writer is unbounded.** A child that floods queries while never reading its
  stdin no longer stalls the reader; its replies queue in memory instead, until the child exits.
- **The reply tests are proven on Unix only**, for the conhost reason above. On Windows only the
  start-up `ESC[6n` proves the reader's write-back (Frame mode; in Raw mode the reader's own start-up
  answer), and no test there proves the pump's. The Unix proof was mutation-run on Linux under WSL
  (#1046), each mutation reddening exactly its test: dropping the pump's hand-over →
  `a_reply_the_pump_drains_reaches_the_child`; dropping the reader's →
  `a_query_from_the_child_is_answered`; `terminate` sending `SIGHUP` only →
  `terminate_ends_a_child_that_ignores_sighup`.
- **A grandchild holding a Unix pty open keeps the reader thread blocked** after the exit is reported.
  The reader has its own fd, so dropping the master does not release it. It feeds nothing once the
  Session (or the Raw sink) has ended, so this is a parked thread rather than wrong output. A ConPTY
  reader is released by closing the pseudo-console.
- **A spawn error keeps its `ErrorKind` only where `portable-pty` passed an `io::Error` through.** On
  Windows it formats a failed `CreateProcessW` into a string, so a missing program is `Other`.
- **A start-up query split across reads, and mode settings ahead of it, are proven by unit test only.**
  conhost writes the query in one piece, and the measured build (Windows 11 26200) sends it first, so
  no real-process test reaches either. An older conhost is the unmeasured case.
- **conhost's `main` also sends DA1 (`ESC[c`) at start, after the cursor query** (microsoft/terminal
  `src/host/VtIo.cpp`), and does not wait for its answer. In Raw mode it reaches the sink, and the
  consumer's terminal answers it into the child's input. No shipped build measured here sends it
  (Windows 11 26200 does not), so what conhost does with that answer is unmeasured.
- **The "nothing after `on_exit`" tests run on Linux only, and the Raw DA1 and CPR tests on Unix only.**
  The first need a grandchild writing after the child's exit (a ConPTY ends with its pseudo-console;
  macOS fails the write with `EIO`); the second is the conhost reason above.
- **No test reaches the paused-close order.** Terminating a Raw pty while its sink is paused reported
  the exit in 11 ms on Windows 11 build 26200 even with the release after the close (#1057 lens
  probe), so the documented `ClosePseudoConsole` wait did not occur on this build. Swapping the order
  back stays green here; older Windows builds are the unmeasured risk.
- **After a failed read, a child that keeps writing can block, and `on_exit` waits for `terminate`**
  (#1062). Nothing reads its output any more, so once the pipe fills the child stalls in its own write
  and never exits. Measured through `start_with` with a reader failing after a mark and a child
  printing 300 000 lines: Windows 11 26200 (`cmd` loop) intact 15.2 s, failed no exit after 60 s, then
  `terminate` reported `read_error: Some(ConnectionReset)`; Linux under WSL Ubuntu-24.04 (`sh` loop)
  intact 2.6 s, failed no exit after 60 s, then `signal: Hangup` with the same `read_error`. macOS is
  unmeasured. **Leaving it documented is the maintainer's judgement (2026-10-13, #1062)**, made on
  those measurements against two alternatives (terminate the child on a failed read, or close only the
  pseudo-terminal); theirs to reverse. It did not cover what a consumer shows meanwhile: it learns of
  the failure only from `on_exit`, so only after a `terminate`.
- **A failed write ends the writer silently** (#1064). The writer stops on the first failed
  `write_all`/`flush`, or when the consumer's `Encode` panics, and the cause is dropped. The chunk that
  failed (possibly part-written) and the bytes still queued are lost, and every later non-empty
  `Pty::write` fails with the same `BrokenPipe` ("the pty's input is closed") as after an exit. An empty
  write still returns `Ok`, because `write_back` short-circuits it while `Input` is `Some`. Left
  documented rather than reported because a failed write was not found while a child runs:
  - Linux, measured under WSL Ubuntu-24.04 at #1064's triage: with the slave left without an opener
    (the child redirected its tty fds to `/dev/null` and stayed alive), three master writes each
    succeeded, as did the control;
  - Windows, from source: `filedescriptor` 0.8.3's `WriteFile` passes `BrokenPipe` through, but
    conhost's input pipe closes only when the pump closes the pseudo-console at exit, after `Input` is
    already `None`. conhost dying on its own is the remaining, unarrangeable case;
  - macOS: unmeasured. #1057's `EIO` was a grandchild's *slave*-side write, a different case.

  `a_write_after_the_exit_fails_with_broken_pipe` pins the kind and the message after an exit.
- **A real failed read is proven through an injected reader only.** No real pty read failure can be
  arranged, so `a_failed_read_is_reported_on_the_exit` wraps the real reader (`start_with`).
- **No test reaches the pid-reuse guard** (`ChildState::exited`). A reused pid cannot be arranged
  deterministically, so removing the guard stays green on every OS. It rests on reading
  `portable-pty`'s Unix killer.
