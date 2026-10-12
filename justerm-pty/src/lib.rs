//! Runs a command in a pseudo-terminal and drives a [`justerm_core::Session`] from it: the read
//! loop, the pump timer and exit detection
//! ([ADR-0034](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0034-the-family-owns-the-pty-loop-and-the-session-lives-in-core.md)).
//!
//! `justerm-core` does no I/O; this crate is where the family meets the operating system. It uses
//! no async runtime: the PTY is read on a dedicated thread and the session is pumped on another,
//! so a consumer on any runtime bridges the callbacks into its own.
//!
//! The engine's reply bytes (DA, DSR, …) are written back to the child as soon as the output that
//! asked for them has been fed — not on the pump period, which paces frames only. A Windows
//! ConPTY depends on it: it asks for the cursor position before the child may run at all.
//!
//! In [Raw mode](Pty::spawn_raw) there is no session: the output goes to a consumer's [`RawSink`],
//! and the consumer's own terminal answers the child.
//!
//! The engine reads its input as UTF-8. A child that writes another encoding is the consumer's to
//! convert, through [`Options::decoder`] and [`Options::encoder`]; which encoding is in use stays
//! the consumer's policy.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use justerm_core::{Session, TermEvent};
use portable_pty::{Child, MasterPty, PtySize, native_pty_system};

/// The program to run in the pseudo-terminal, and how. Which shell, which environment and which
/// directory are the consumer's choice; this only carries them.
#[derive(Debug, Clone)]
pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    cwd: Option<PathBuf>,
}

impl Command {
    /// Run `program`, inheriting this process's environment.
    pub fn new(program: impl Into<OsString>) -> Self {
        Command {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
        }
    }

    /// Append an argument.
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Set an environment variable for the child.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Start the child in `dir`.
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    fn build(&self) -> portable_pty::CommandBuilder {
        let mut cmd = portable_pty::CommandBuilder::new(&self.program);
        cmd.args(&self.args);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        if let Some(dir) = &self.cwd {
            cmd.cwd(dir);
        }
        cmd
    }
}

/// Converts the child's output to the UTF-8 the engine reads. It runs on the reader thread, on
/// each chunk as it is read, before the chunk is fed.
///
/// A chunk can end partway through a multi-byte character. Keep the partial bytes and finish the
/// character from the next chunk. [`finish`](Decode::finish) is called once when the output ends,
/// for anything still held.
pub trait Decode: Send {
    /// The UTF-8 for `bytes`, as far as they complete characters.
    fn decode(&mut self, bytes: &[u8]) -> Vec<u8>;

    /// What is still held when the output ends. Nothing, by default.
    fn finish(&mut self) -> Vec<u8> {
        Vec::new()
    }
}

/// Converts bytes bound for the child from UTF-8 to the child's encoding. It runs on the writer
/// thread over everything written to the child, both the consumer's [`Pty::write`]s and the
/// engine's replies, in order.
pub trait Encode: Send {
    /// `bytes` in the child's encoding.
    fn encode(&mut self, bytes: &[u8]) -> Vec<u8>;
}

/// How a [`Pty`] runs.
pub struct Options {
    /// How often the session is pumped while the child runs. The pacing is the consumer's policy;
    /// this is only the default it may override.
    pub pump_every: Duration,
    /// Converts the child's output to UTF-8 before the engine reads it. `None` feeds the output as
    /// read, which is right for a UTF-8 child.
    pub decoder: Option<Box<dyn Decode>>,
    /// Converts what is written to the child out of UTF-8. `None` writes the bytes as they are.
    pub encoder: Option<Box<dyn Encode>>,
    /// Sees the child's output, decoded, on the reader thread: exactly the bytes the engine is fed
    /// (or, in [Raw mode](Pty::spawn_raw), that the sink is handed), in order. It is called under
    /// the reader's lock — the session's, or in Raw mode the sink's — so it must not lock the
    /// session or call [`Pty::resume`] itself. It is never called after `on_exit`.
    pub on_output: Option<OnOutput>,
    /// Runs on the pump thread every [`pump_every`](Options::pump_every), just before the pump and
    /// under the session's lock. It gets the session, or `None` in [Raw mode](Pty::spawn_raw). It
    /// is not called after `on_exit`.
    pub on_tick: Option<OnTick>,
}

/// The type of [`Options::on_output`].
pub type OnOutput = Box<dyn FnMut(&[u8]) + Send>;

/// The type of [`Options::on_tick`].
pub type OnTick = Box<dyn FnMut(Option<&Session>) + Send>;

impl Default for Options {
    fn default() -> Self {
        Options {
            pump_every: Duration::from_millis(16),
            decoder: None,
            encoder: None,
            on_output: None,
            on_tick: None,
        }
    }
}

impl std::fmt::Debug for Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("pump_every", &self.pump_every)
            .field("decoder", &self.decoder.is_some())
            .field("encoder", &self.encoder.is_some())
            .field("on_output", &self.on_output.is_some())
            .field("on_tick", &self.on_tick.is_some())
            .finish()
    }
}

/// Where a [Raw mode](Pty::spawn_raw) pty hands the child's output, decoded, on the reader thread.
/// The consumer's own terminal reads it and answers the child's queries through [`Pty::write`].
pub trait RawSink: Send {
    /// Take one chunk of output, and say whether reading may go on. It runs under the reader's
    /// lock, which [`Pty::resume`] takes too: resume from another thread, never from here.
    fn output(&mut self, bytes: &[u8]) -> Flow;
}

/// What a [`RawSink`] answers after each chunk: the only backpressure a Raw mode pty has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Keep reading.
    Continue,
    /// Stop reading until [`Pty::resume`]. The child blocks once the pseudo-terminal's buffer
    /// fills. When the child exits, reading resumes regardless, so the rest of the output arrives
    /// before `on_exit`.
    Pause,
}

/// How the child ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    /// The exit code. A child ended by a signal reports `1` here and names the signal below.
    pub code: u32,
    /// The signal that ended the child, on Unix, as the platform names it.
    pub signal: Option<String>,
}

impl Exit {
    /// Whether the child exited with code `0` and was not killed by a signal.
    pub fn success(&self) -> bool {
        self.code == 0 && self.signal.is_none()
    }
}

impl From<portable_pty::ExitStatus> for Exit {
    fn from(status: portable_pty::ExitStatus) -> Self {
        Exit {
            code: status.exit_code(),
            signal: status.signal().map(str::to_owned),
        }
    }
}

/// A command running in a pseudo-terminal, feeding a [`Session`] (or, in
/// [Raw mode](Pty::spawn_raw), a [`RawSink`]).
///
/// Three threads run for it:
/// - one reads the child's output into the session, or the [`RawSink`];
/// - one pumps the session every [`Options::pump_every`], hands its events to the consumer, and
///   watches for the child's exit (in Raw mode it only ticks and watches);
/// - one writes to the child: the engine's replies and the consumer's [`write`](Pty::write)s, in the
///   order they were handed over. A write therefore never blocks the reader or the pump.
///
/// When the child exits, the pseudo-terminal is closed and the rest of the output is read. The
/// session is then [ended](Session::end), which sends what is pending (in Raw mode, the sink is
/// closed to further output), and the exit is reported.
///
/// `on_exit` is called **exactly once**, including when the `Pty` is dropped first: dropping it
/// terminates the child, and that exit is reported the same way as any other. Nothing reaches the
/// session or the sink after that, so no frame or output follows `on_exit`.
pub struct Pty {
    target: Target,
    child: Arc<Mutex<ChildState>>,
    input: Input,
    master: Master,
}

/// Where the child's output goes: into a [`Session`]'s engine, or to a [`RawSink`].
#[derive(Clone)]
enum Target {
    Frame(Arc<Mutex<Session>>),
    Raw(Arc<Raw>),
}

/// A Raw mode pty's sink, and the reader's pause.
struct Raw {
    state: Mutex<RawState>,
    /// Wakes a paused reader: on [`Pty::resume`], at exit, and when the sink is ended.
    wake: Condvar,
}

struct RawState {
    sink: Box<dyn RawSink>,
    /// The sink asked for a pause, and no resume has come.
    paused: bool,
    /// The child has exited and the rest of the output is being read: the reader does not wait.
    draining: bool,
    /// `on_exit` is about to run: nothing more reaches the sink.
    ended: bool,
}

/// The child, and whether it has been reaped. After a Unix reap its pid can be reused by an
/// unrelated process, so nothing may signal it any more. The flag is set under the same lock that
/// the kill takes.
struct ChildState {
    child: Box<dyn Child + Send + Sync>,
    exited: bool,
}

/// Where everything bound for the child is handed over: the writer thread's channel. `None` once
/// the child has exited, which ends the writer thread and closes the pseudo-terminal's input.
type Input = Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>;

/// The pseudo-terminal's control side. It is held so the terminal stays open and so a resize can
/// reach it. `None` once the child has exited.
type Master = Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>;

/// How long the pump waits, once the child has exited and the pseudo-terminal is closed, for the
/// reader to drain the rest of the output.
const DRAIN_AFTER_EXIT: Duration = Duration::from_millis(500);

/// The longest the pump thread goes without asking whether the child has exited, whatever the pump
/// period. Exit detection is not paced with the frames.
const EXIT_POLL: Duration = Duration::from_millis(50);

impl Pty {
    /// Start `command` in a pseudo-terminal sized to `session`'s grid, and drive `session` from it.
    ///
    /// `on_event` receives the engine's events, unfiltered, on the pump thread. `on_exit` is
    /// called once, on the pump thread, after the session has been ended.
    ///
    /// An error keeps its [`ErrorKind`](std::io::ErrorKind) only where `portable-pty` passed an
    /// `std::io::Error` through. On Windows it formats a failed `CreateProcessW` into the message,
    /// so a missing program there is `Other`. If the child was started and a later step then
    /// failed, the child is killed before the error is returned.
    pub fn spawn(
        command: Command,
        session: Session,
        options: Options,
        on_event: impl FnMut(TermEvent) + Send + 'static,
        on_exit: impl FnOnce(Exit) + Send + 'static,
    ) -> std::io::Result<Pty> {
        let grid = session.engine().grid();
        let size = pty_size(grid.cols(), grid.rows());
        let target = Target::Frame(Arc::new(Mutex::new(session)));
        start(command, size, target, options, on_event, on_exit)
    }

    /// Start `command` in a pseudo-terminal of `cols` × `rows` in **Raw mode**: no engine, and the
    /// child's output, decoded, goes to `sink`. Nothing answers the child's queries but the
    /// consumer, through [`write`](Pty::write); on Windows that includes the cursor-position query
    /// a ConPTY sends before the child may run.
    ///
    /// `on_exit` is called once, on the pump thread, after the rest of the output has reached
    /// `sink`. Errors are as for [`spawn`](Pty::spawn).
    pub fn spawn_raw(
        command: Command,
        cols: u16,
        rows: u16,
        options: Options,
        sink: impl RawSink + 'static,
        on_exit: impl FnOnce(Exit) + Send + 'static,
    ) -> std::io::Result<Pty> {
        let size = pty_size(usize::from(cols), usize::from(rows));
        let target = Target::Raw(Arc::new(Raw {
            state: Mutex::new(RawState {
                sink: Box::new(sink),
                paused: false,
                draining: false,
                ended: false,
            }),
            wake: Condvar::new(),
        }));
        start(command, size, target, options, |_| {}, on_exit)
    }

    /// The session this pty drives, or `None` in [Raw mode](Pty::spawn_raw). Lock it briefly,
    /// because the reader and the pump take the same lock.
    ///
    /// Through it, a consumer may [`subscribe`](Session::subscribe), read the
    /// [`engine`](Session::engine), [`edit`](Session::edit) the view (scroll, select, search) and
    /// [`send_pending`](Session::send_pending) to show an edit at once. It must not
    /// [`pump`](Session::pump), which would take replies only this pty writes back to the child,
    /// nor [`feed`](Session::feed) or [`end`](Session::end), which are the reader's and the pump's;
    /// and it resizes through [`Pty::resize`], so the child learns the size too.
    pub fn session(&self) -> Option<Arc<Mutex<Session>>> {
        match &self.target {
            Target::Frame(session) => Some(Arc::clone(session)),
            Target::Raw(_) => None,
        }
    }

    /// Let a [Raw mode](Pty::spawn_raw) reader go on after its sink answered [`Flow::Pause`].
    /// Does nothing when reading is not paused, or in frame mode.
    pub fn resume(&self) {
        if let Target::Raw(raw) = &self.target {
            lock(&raw.state).paused = false;
            raw.wake.notify_all();
        }
    }
}

/// Open a pseudo-terminal of `size`, start `command` in it, and run the reader, writer and pump
/// threads that carry its output to `target`.
fn start(
    command: Command,
    size: PtySize,
    target: Target,
    options: Options,
    mut on_event: impl FnMut(TermEvent) + Send + 'static,
    on_exit: impl FnOnce(Exit) + Send + 'static,
) -> std::io::Result<Pty> {
    let pair = native_pty_system().openpty(size).map_err(io_error)?;
    let mut child = pair
        .slave
        .spawn_command(command.build())
        .map_err(io_error)?;
    // The slave stays open in this process otherwise, and a Unix reader would never see EOF.
    drop(pair.slave);
    let ends = pair
        .master
        .try_clone_reader()
        .and_then(|r| Ok((r, pair.master.take_writer()?)));
    let (reader, writer) = match ends {
        Ok(ends) => ends,
        Err(e) => {
            let _ = child.kill();
            return Err(io_error(e));
        }
    };
    let child = Arc::new(Mutex::new(ChildState {
        child,
        exited: false,
    }));
    let master: Master = Arc::new(Mutex::new(Some(pair.master)));
    let (tx, rx) = mpsc::channel();
    let input: Input = Arc::new(Mutex::new(Some(tx)));
    let eof = Arc::new(AtomicBool::new(false));
    let started = (|| {
        spawn_writer(writer, rx, options.encoder)?;
        spawn_reader(
            options.decoder,
            options.on_output,
            reader,
            target.clone(),
            Arc::clone(&input),
            Arc::clone(&eof),
        )?;
        let pump = Pump {
            target: target.clone(),
            on_tick: options.on_tick,
            master: Arc::clone(&master),
            input: Arc::clone(&input),
            child: Arc::clone(&child),
            eof,
            period: options.pump_every,
        };
        thread::Builder::new()
            .name("justerm-pty pump".into())
            .spawn(move || pump.run(&mut on_event, on_exit))
            .map(drop)
    })();
    if let Err(e) = started {
        let _ = lock(&child).child.kill();
        return Err(e);
    }
    Ok(Pty {
        target,
        child,
        input,
        master,
    })
}

impl Pty {
    /// Send `bytes` to the child: what the user typed, or the consumer's own answer to a query the
    /// engine leaves to policy (a colour or clipboard query). This queues the bytes and returns
    /// without waiting, and they reach the child in the order written, interleaved with the
    /// engine's replies.
    ///
    /// Fails with [`BrokenPipe`](std::io::ErrorKind::BrokenPipe) once the child has exited.
    pub fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        if write_back(&self.input, bytes) {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "the child has exited",
            ))
        }
    }

    /// Resize the session, then the pseudo-terminal to the size the engine **adopted**, and return
    /// that size as `(cols, rows)`.
    ///
    /// The engine widens a too-narrow width to its minimum (see
    /// [`Engine::resize`](justerm_core::Engine::resize)), and the child is told that width, never the
    /// one asked for, so the two never disagree. After the child has exited only the session is
    /// resized. In [Raw mode](Pty::spawn_raw) there is no engine: the child is told `cols` × `rows`
    /// as asked, and that is returned.
    pub fn resize(&self, cols: usize, rows: usize) -> (usize, usize) {
        let session = match &self.target {
            Target::Frame(session) => session,
            Target::Raw(_) => {
                if let Some(master) = lock(&self.master).as_ref() {
                    let _ = master.resize(pty_size(cols, rows));
                }
                return (cols, rows);
            }
        };
        // The session stays locked until the pty has its size, so two resizes racing cannot leave
        // the engine at one size and the child told the other.
        let mut session = lock(session);
        let adopted = session.resize(cols, rows);
        if let Some(master) = lock(&self.master).as_ref() {
            let _ = master.resize(pty_size(adopted.0, adopted.1));
        }
        adopted
    }

    /// End the child.
    ///
    /// - On Unix it is sent `SIGHUP`, then `SIGKILL` if it is still alive a moment later.
    /// - On Windows it is terminated.
    ///
    /// Its exit is reported through `on_exit` like any other. Once the child has exited, this does
    /// nothing.
    pub fn terminate(&self) {
        let mut state = lock(&self.child);
        if !state.exited {
            let _ = state.child.kill();
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn pty_size(cols: usize, rows: usize) -> PtySize {
    PtySize {
        rows: u16::try_from(rows).unwrap_or(u16::MAX),
        cols: u16::try_from(cols).unwrap_or(u16::MAX),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Hand `bytes` to the writer thread. Returns `false` once the child has exited.
fn write_back(input: &Input, bytes: &[u8]) -> bool {
    match lock(input).as_ref() {
        Some(tx) => bytes.is_empty() || tx.send(bytes.to_vec()).is_ok(),
        None => false,
    }
}

fn spawn_writer(
    mut writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Vec<u8>>,
    mut encoder: Option<Box<dyn Encode>>,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name("justerm-pty writer".into())
        .spawn(move || {
            // Ends when every sender is gone, which happens at exit; dropping the writer then closes
            // the child's input.
            for bytes in rx {
                let bytes = match encoder.as_mut() {
                    Some(encoder) => encoder.encode(&bytes),
                    None => bytes,
                };
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        })
        .map(drop)
}

fn spawn_reader(
    mut decoder: Option<Box<dyn Decode>>,
    mut on_output: Option<OnOutput>,
    mut reader: Box<dyn Read + Send>,
    target: Target,
    input: Input,
    eof: Arc<AtomicBool>,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name("justerm-pty reader".into())
        .spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };
                let decoded;
                let chunk = match decoder.as_mut() {
                    Some(decoder) => {
                        decoded = decoder.decode(&buf[..n]);
                        &decoded[..]
                    }
                    None => &buf[..n],
                };
                if !feed(&target, &input, &mut on_output, chunk) {
                    break;
                }
            }
            if let Some(tail) = decoder.as_mut().map(|d| d.finish()) {
                feed(&target, &input, &mut on_output, &tail);
            }
            eof.store(true, Ordering::Release);
        })
        .map(drop)
}

/// Hand `bytes` to the target, showing them to `on_output` first. Returns `false` once the target
/// has ended: output arriving after that goes nowhere, because nothing may follow `on_exit`. That
/// happens when a grandchild still holds a Unix pty open.
fn feed(target: &Target, input: &Input, on_output: &mut Option<OnOutput>, bytes: &[u8]) -> bool {
    match target {
        Target::Frame(session) => feed_session(session, input, on_output, bytes),
        Target::Raw(raw) => feed_raw(raw, on_output, bytes),
    }
}

/// Hand `bytes` to the sink, then wait while it has paused reading — until a resume, or until
/// the child's exit releases the pause so the rest of the output can be drained.
fn feed_raw(raw: &Raw, on_output: &mut Option<OnOutput>, bytes: &[u8]) -> bool {
    let mut state = lock(&raw.state);
    if state.ended {
        return false;
    }
    if bytes.is_empty() {
        return true;
    }
    if let Some(on_output) = on_output.as_mut() {
        on_output(bytes);
    }
    if state.sink.output(bytes) == Flow::Pause {
        state.paused = true;
    }
    while state.paused && !state.draining && !state.ended {
        state = raw
            .wake
            .wait(state)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    true
}

/// Feed `bytes` and hand the replies they produced to the writer.
fn feed_session(
    session: &Mutex<Session>,
    input: &Input,
    on_output: &mut Option<OnOutput>,
    bytes: &[u8],
) -> bool {
    let mut session = lock(session);
    if session.is_ended() {
        return false;
    }
    if bytes.is_empty() {
        return true;
    }
    if let Some(on_output) = on_output.as_mut() {
        on_output(bytes);
    }
    session.feed(bytes);
    let replies = session.edit(|engine| engine.drain_replies());
    // Handed over while the session is still locked, so replies reach the writer in the order they
    // were drained. The send never blocks: the channel is unbounded.
    write_back(input, &replies);
    true
}

/// The pump thread's state.
struct Pump {
    target: Target,
    on_tick: Option<OnTick>,
    master: Master,
    input: Input,
    child: Arc<Mutex<ChildState>>,
    eof: Arc<AtomicBool>,
    period: Duration,
}

impl Pump {
    fn run(mut self, on_event: &mut impl FnMut(TermEvent), on_exit: impl FnOnce(Exit)) {
        let tick = self.period.min(EXIT_POLL);
        let mut next_pump = Instant::now() + self.period;
        let exit = loop {
            thread::sleep(tick);
            if Instant::now() >= next_pump {
                self.pump(on_event);
                next_pump = Instant::now() + self.period;
            }
            let mut state = lock(&self.child);
            if let Ok(Some(status)) = state.child.try_wait() {
                state.exited = true;
                break Exit::from(status);
            }
        };
        // Closing the pseudo-terminal lets a ConPTY flush and end its output pipe. The reader then
        // reaches EOF, instead of the drain waiting out its full bound.
        // A paused Raw reader is released before the pseudo-terminal closes.
        if let Target::Raw(raw) = &self.target {
            lock(&raw.state).draining = true;
            raw.wake.notify_all();
        }
        lock(&self.input).take();
        drop(lock(&self.master).take());
        let deadline = Instant::now() + DRAIN_AFTER_EXIT;
        while !self.eof.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        match &self.target {
            Target::Frame(session) => {
                let pumped = {
                    let mut session = lock(session);
                    let pumped = session.pump();
                    session.end();
                    pumped
                };
                pumped.events.into_iter().for_each(&mut *on_event);
            }
            Target::Raw(raw) => {
                lock(&raw.state).ended = true;
                raw.wake.notify_all();
            }
        }
        on_exit(exit);
    }

    fn pump(&mut self, on_event: &mut impl FnMut(TermEvent)) {
        let session = match &self.target {
            Target::Frame(session) => session,
            Target::Raw(_) => {
                if let Some(on_tick) = self.on_tick.as_mut() {
                    on_tick(None);
                }
                return;
            }
        };
        let pumped = {
            let mut session = lock(session);
            if let Some(on_tick) = self.on_tick.as_mut() {
                on_tick(Some(&session));
            }
            let pumped = session.pump();
            // Under the same lock as the reader's hand-over, so drain order is hand-over order.
            write_back(&self.input, &pumped.replies);
            pumped
        };
        pumped.events.into_iter().for_each(on_event);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `portable-pty` reports errors through `anyhow`. Where the error wraps a `std::io::Error`, return
/// that one, so its kind survives.
fn io_error(e: anyhow::Error) -> std::io::Error {
    match e.downcast::<std::io::Error>() {
        Ok(io) => io,
        Err(other) => std::io::Error::other(other.to_string()),
    }
}
