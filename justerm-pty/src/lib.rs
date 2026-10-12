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
//! **Not yet complete.** There is no writer for the consumer's own input yet, and a resize does
//! not reach the pseudo-terminal.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
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

/// How a [`Pty`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// How often the session is pumped while the child runs. The pacing is the consumer's policy;
    /// this is only the default it may override.
    pub pump_every: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            pump_every: Duration::from_millis(16),
        }
    }
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

/// A command running in a pseudo-terminal, feeding a [`Session`].
///
/// Two threads run for it:
/// - one reads the child's output into the session;
/// - the other pumps the session every [`Options::pump_every`], hands its events to the consumer,
///   and watches for the child's exit.
///
/// When the child exits, the pseudo-terminal is closed and the rest of the output is read. The
/// session is then [ended](Session::end), which sends what is pending, and the exit is reported.
///
/// `on_exit` is called **exactly once**, including when the `Pty` is dropped first: dropping it
/// terminates the child, and that exit is reported the same way as any other. Nothing is fed into
/// the session after it has ended, so no frame follows `on_exit`.
pub struct Pty {
    session: Arc<Mutex<Session>>,
    child: Arc<Mutex<ChildState>>,
}

/// The child, and whether it has been reaped. After a Unix reap its pid can be reused by an
/// unrelated process, so nothing may signal it any more. The flag is set under the same lock that
/// the kill takes.
struct ChildState {
    child: Box<dyn Child + Send + Sync>,
    exited: bool,
}

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
        mut on_event: impl FnMut(TermEvent) + Send + 'static,
        on_exit: impl FnOnce(Exit) + Send + 'static,
    ) -> std::io::Result<Pty> {
        let grid = session.engine().grid();
        let size = PtySize {
            rows: u16::try_from(grid.rows()).unwrap_or(u16::MAX),
            cols: u16::try_from(grid.cols()).unwrap_or(u16::MAX),
            pixel_width: 0,
            pixel_height: 0,
        };
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
        let writer: Writer = Arc::new(Mutex::new(Some(writer)));
        let session = Arc::new(Mutex::new(session));
        let child = Arc::new(Mutex::new(ChildState {
            child,
            exited: false,
        }));
        let eof = Arc::new(AtomicBool::new(false));
        let started = (|| {
            spawn_reader(
                reader,
                Arc::clone(&session),
                Arc::clone(&writer),
                Arc::clone(&eof),
            )?;
            let pump = Pump {
                session: Arc::clone(&session),
                master: Some(pair.master),
                writer,
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
        Ok(Pty { session, child })
    }

    /// The session this pty drives: subscribe to it, read it, scroll or select. Lock it briefly,
    /// because the reader and the pump take the same lock.
    pub fn session(&self) -> Arc<Mutex<Session>> {
        Arc::clone(&self.session)
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

/// The pseudo-terminal's input side. Two threads share it: the reader writes replies to what it
/// just fed, and the pump writes any other replies. `None` once the child has exited.
type Writer = Arc<Mutex<Option<Box<dyn Write + Send>>>>;

fn write_back(writer: &Writer, replies: &[u8]) {
    if replies.is_empty() {
        return;
    }
    if let Some(w) = lock(writer).as_mut() {
        let _ = w.write_all(replies).and_then(|()| w.flush());
    }
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    session: Arc<Mutex<Session>>,
    writer: Writer,
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
                let replies = {
                    let mut session = lock(&session);
                    // Output that arrives after the session has ended is not fed, because nothing
                    // may follow `on_exit`. This happens when a grandchild still holds a Unix pty open.
                    if session.is_ended() {
                        break;
                    }
                    session.feed(&buf[..n]);
                    session.edit(|engine| engine.drain_replies())
                };
                write_back(&writer, &replies);
            }
            eof.store(true, Ordering::Release);
        })
        .map(drop)
}

/// The pump thread's state.
struct Pump {
    session: Arc<Mutex<Session>>,
    /// Held so the pseudo-terminal stays open; dropped once the child has exited.
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Writer,
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
        lock(&self.writer).take();
        drop(self.master.take());
        let deadline = Instant::now() + DRAIN_AFTER_EXIT;
        while !self.eof.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let pumped = {
            let mut session = lock(&self.session);
            let pumped = session.pump();
            session.end();
            pumped
        };
        pumped.events.into_iter().for_each(&mut *on_event);
        on_exit(exit);
    }

    fn pump(&self, on_event: &mut impl FnMut(TermEvent)) {
        let pumped = lock(&self.session).pump();
        write_back(&self.writer, &pumped.replies);
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
