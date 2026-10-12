//! `justerm-pty` against a real child process ([#1045](https://github.com/kihyun1998/justerm/issues/1045)).
//!
//! The child is this test binary itself, started again with `JUSTERM_PTY_HELPER` set, so every OS
//! runs the same child with the same output and no shell is involved. `harness = false` keeps
//! libtest's own output out of the child's terminal.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use justerm_core::{Engine, FrameSink, Session, decode};
use justerm_pty::{Command, Decode, Encode, Exit, Options, Pty};

const HELPER: &str = "JUSTERM_PTY_HELPER";
const MARKER: &str = "hello-from-the-child";
const UTF8_SAMPLE: &str = "\u{d55c}\u{ae00} e\u{301}";

/// What the child does, chosen by the value of `JUSTERM_PTY_HELPER`.
fn helper(mode: &str) {
    use std::io::Write;
    let say = |s: &str| {
        let mut out = std::io::stdout();
        out.write_all(s.as_bytes()).unwrap();
        out.flush().unwrap();
    };
    match mode {
        // Print, give the terminal time to read it, exit with a chosen code.
        "print-exit-7" => {
            say(MARKER);
            std::thread::sleep(Duration::from_millis(300));
            std::process::exit(7);
        }
        // Print and exit at once, so the output is still in flight when the exit is seen.
        "print-exit-now" => {
            say(MARKER);
            std::process::exit(0);
        }
        // Print, then stay quiet until killed.
        "print-then-sleep" => {
            say(MARKER);
            std::thread::sleep(Duration::from_secs(60));
        }
        // Like `print-then-sleep`, but a Unix child ignores SIGHUP, so only SIGKILL ends it.
        "ignore-hup-then-sleep" => {
            #[cfg(unix)]
            unsafe {
                libc::signal(libc::SIGHUP, libc::SIG_IGN);
            }
            say(MARKER);
            std::thread::sleep(Duration::from_secs(60));
        }
        // Read one line and print it back, control bytes made visible (`ESC` as `^[`).
        // One two-byte legacy character (`B0 A1`, see `ToyLegacy`) written in two halves, so it
        // reaches the reader split across two reads.
        "split-legacy" => {
            let mut out = std::io::stdout();
            out.write_all(&[0xB0]).unwrap();
            out.flush().unwrap();
            std::thread::sleep(Duration::from_millis(300));
            out.write_all(&[0xA1]).unwrap();
            out.flush().unwrap();
            std::thread::sleep(Duration::from_millis(300));
        }
        // A legacy lead byte with no second half, then exit: the decoder still holds it at EOF.
        "lone-lead-then-exit" => {
            let mut out = std::io::stdout();
            out.write_all(&[0xB0]).unwrap();
            out.flush().unwrap();
            std::thread::sleep(Duration::from_millis(300));
        }
        // UTF-8 with a wide character and a combining sequence.
        "print-utf8" => {
            say(UTF8_SAMPLE);
            std::thread::sleep(Duration::from_millis(300));
        }
        "echo-line" => {
            say(&format!("GOT:{}", visible(&read_line())));
            std::thread::sleep(Duration::from_millis(300));
        }
        // Ask the terminal for its attributes (DA1), then print the line that comes back.
        "query-then-echo" => {
            say("\x1b[c");
            say(&format!("GOT:{}", visible(&read_line())));
            std::thread::sleep(Duration::from_millis(300));
        }
        // Print this terminal's size now, and again after every line read.
        "size-on-line" => {
            let size = || {
                let (terminal_size::Width(w), terminal_size::Height(h)) =
                    terminal_size::terminal_size().expect("stdout is a terminal");
                format!("SIZE:{w}x{h};")
            };
            say(&size());
            loop {
                read_line();
                say(&size());
            }
        }
        other => panic!("unknown helper mode {other}"),
    }
}

/// One line from stdin, without its line ending.
fn read_line() -> String {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    line.trim_end_matches(['\r', '\n']).to_owned()
}

/// `s` with `ESC` shown as `^[`, so a reply prints as text instead of being interpreted.
fn visible(s: &str) -> String {
    s.replace('\x1b', "^[")
}

/// Records the frames a Session sends.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<Vec<u8>>>>);

impl FrameSink for Recorder {
    fn send_frame(&mut self, frame: Vec<u8>) {
        self.0.lock().unwrap().push(frame);
    }
}

impl Recorder {
    fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }
    /// Every character any recorded frame carried, in order.
    fn text(&self) -> String {
        self.0
            .lock()
            .unwrap()
            .iter()
            .flat_map(|f| {
                decode(f)
                    .unwrap()
                    .spans
                    .iter()
                    .flat_map(|s| s.cells.iter().map(|c| c.c()).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

/// A running child in helper `mode`, its recorder, and the channel its exit arrives on.
fn spawn(mode: &str, options: Options) -> (Pty, Recorder, mpsc::Receiver<Exit>) {
    let mut session = Session::new(Engine::new(80, 24));
    let rec = Recorder::default();
    session.subscribe(Box::new(rec.clone()));
    rec.0.lock().unwrap().clear();
    let (tx, rx) = mpsc::channel();
    let command = Command::new(std::env::current_exe().unwrap()).env(HELPER, mode);
    let pty = Pty::spawn(
        command,
        session,
        options,
        |_event| {},
        move |exit| {
            let _ = tx.send(exit);
        },
    )
    .expect("spawn the helper in a pty");
    (pty, rec, rx)
}

/// Poll `cond` until it holds or `limit` passes.
fn eventually(limit: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    cond()
}

fn the_childs_output_reaches_the_sink_as_a_frame() {
    let (_pty, rec, rx) = spawn("print-exit-7", Options::default());
    rx.recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
    assert!(
        rec.text().contains(MARKER),
        "frames carried: {:?}",
        rec.text()
    );
}

fn the_exit_code_is_reported() {
    let (_pty, _rec, rx) = spawn("print-exit-7", Options::default());
    let exit = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
    assert_eq!(exit.code, 7);
}

fn frames_arrive_on_the_default_timer_while_the_child_runs() {
    assert_eq!(Options::default().pump_every, Duration::from_millis(16));
    let (pty, rec, rx) = spawn("print-then-sleep", Options::default());
    assert!(
        eventually(Duration::from_secs(10), || rec.text().contains(MARKER)),
        "the timer pumps the output while the child is still alive"
    );
    pty.terminate();
    rx.recv_timeout(Duration::from_secs(20))
        .expect("terminated");
}

fn a_quiet_child_sends_no_frames() {
    let (pty, rec, rx) = spawn("print-then-sleep", Options::default());
    assert!(eventually(Duration::from_secs(10), || rec
        .text()
        .contains(MARKER)));
    // Let the terminal settle (a ConPTY repaints once after start), then count.
    std::thread::sleep(Duration::from_millis(500));
    let settled = rec.count();
    std::thread::sleep(Duration::from_millis(400)); // 25 pump periods
    assert_eq!(rec.count(), settled, "nothing changed, so nothing was sent");
    pty.terminate();
    rx.recv_timeout(Duration::from_secs(20))
        .expect("terminated");
}

fn the_pump_period_is_honoured() {
    let options = Options {
        pump_every: Duration::from_secs(3600),
        ..Options::default()
    };
    let (pty, rec, rx) = spawn("print-then-sleep", options);
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(rec.count(), 0, "no pump has run within an hour-long period");
    pty.terminate();
    rx.recv_timeout(Duration::from_secs(20))
        .expect("terminated");
    assert!(
        rec.text().contains(MARKER),
        "ending the session sends what was pending"
    );
}

fn terminate_ends_the_child_and_reports_its_exit() {
    let (pty, _rec, rx) = spawn("print-then-sleep", Options::default());
    std::thread::sleep(Duration::from_millis(300));
    pty.terminate();
    let exit = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("a terminated child is reported");
    assert!(!exit.success(), "{exit:?}");
}

fn the_session_stays_readable_after_exit() {
    let (pty, _rec, rx) = spawn("print-exit-7", Options::default());
    rx.recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
    let session = pty.session();
    let session = session.lock().unwrap();
    assert!(session.is_ended());
    assert!(session.engine().accessible_text().contains(MARKER));
}

fn output_written_just_before_exit_still_arrives() {
    let (_pty, rec, rx) = spawn("print-exit-now", Options::default());
    let exit = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
    assert!(exit.success(), "{exit:?}");
    assert!(
        rec.text().contains(MARKER),
        "frames carried: {:?}",
        rec.text()
    );
}

fn no_frame_follows_on_exit() {
    let (_pty, rec, rx) = spawn("print-exit-now", Options::default());
    rx.recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
    let at_exit = rec.count();
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(rec.count(), at_exit);
}

fn dropping_the_pty_terminates_the_child_and_still_reports_its_exit() {
    let (pty, _rec, rx) = spawn("print-then-sleep", Options::default());
    std::thread::sleep(Duration::from_millis(300));
    drop(pty);
    let exit = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("on_exit fires after a drop too");
    assert!(!exit.success(), "{exit:?}");
}

fn terminate_ends_a_child_that_ignores_sighup() {
    let (pty, rec, rx) = spawn("ignore-hup-then-sleep", Options::default());
    assert!(eventually(Duration::from_secs(10), || rec
        .text()
        .contains(MARKER)));
    pty.terminate();
    let exit = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("SIGKILL follows the ignored SIGHUP");
    assert!(!exit.success(), "{exit:?}");
}

fn a_missing_program_is_an_error_not_a_running_pty() {
    let session = Session::new(Engine::new(80, 24));
    let command = Command::new("justerm-pty-no-such-program-anywhere");
    let result = Pty::spawn(command, session, Options::default(), |_| {}, |_| {});
    assert!(result.is_err(), "spawning a missing program fails");
}

/// The session's whole buffer as text, soft wraps joined — what the child has printed so far.
fn screen(pty: &Pty) -> String {
    pty.session().lock().unwrap().engine().accessible_text()
}

fn bytes_the_consumer_writes_reach_the_child() {
    let (pty, _rec, rx) = spawn("echo-line", Options::default());
    pty.write(b"typed\r").expect("the child is running");
    assert!(
        eventually(Duration::from_secs(10), || screen(&pty)
            .contains("GOT:typed")),
        "screen: {:?}",
        screen(&pty)
    );
    rx.recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
    assert!(
        pty.write(b"late").is_err(),
        "writing after exit is an error"
    );
}

/// Unix only. A Windows ConPTY is itself the console's terminal: measured (#1046), conhost answered
/// the child's DA1 with its own `?61;6;7;21;22;23;24;28;32;42c` and consumed a VT reply arriving
/// as input, so neither the query nor the reply crosses it. Windows proves write-back through the
/// `ESC[6n` conhost sends at start instead: without the reader's write-back no child runs.
#[cfg(unix)]
fn a_query_from_the_child_is_answered() {
    let (pty, _rec, rx) = spawn("query-then-echo", Options::default());
    // The reply has no line ending; end the line so a line-mode child hands it to read_line.
    std::thread::sleep(Duration::from_millis(500));
    pty.write(b"\r").unwrap();
    assert!(
        eventually(Duration::from_secs(10), || screen(&pty)
            .contains("GOT:^[[?62;22c")),
        "screen: {:?}",
        screen(&pty)
    );
    rx.recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
}

/// Unix only. A Windows ConPTY is itself the console's terminal: measured (#1046), conhost answered
/// the child's DA1 with its own `?61;6;7;21;22;23;24;28;32;42c` and consumed a VT reply arriving
/// as input, so neither the query nor the reply crosses it. Windows proves write-back through the
/// `ESC[6n` conhost sends at start instead: without the reader's write-back no child runs.
#[cfg(unix)]
fn a_reply_the_pump_drains_reaches_the_child() {
    let (pty, _rec, rx) = spawn("echo-line", Options::default());
    std::thread::sleep(Duration::from_millis(300));
    // Fed through the session rather than read from the child, so the reader never sees it: only
    // the pump drains this reply (a DSR status report, `ESC[0n`).
    pty.session()
        .lock()
        .unwrap()
        .edit(|engine| engine.feed(b"\x1b[5n"));
    std::thread::sleep(Duration::from_millis(300));
    pty.write(b"\r").unwrap();
    assert!(
        eventually(Duration::from_secs(10), || screen(&pty)
            .contains("GOT:^[[0n")),
        "screen: {:?}",
        screen(&pty)
    );
    rx.recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
}

fn a_resize_reaches_the_child_at_the_new_size() {
    let (pty, rec, rx) = spawn("size-on-line", Options::default());
    assert!(eventually(Duration::from_secs(10), || screen(&pty)
        .contains("SIZE:80x24;")));
    assert_eq!(pty.resize(100, 30), (100, 30));
    pty.write(b"\r").unwrap();
    assert!(
        eventually(Duration::from_secs(10), || screen(&pty)
            .contains("SIZE:100x30;")),
        "screen: {:?}",
        screen(&pty)
    );
    // The screen above is the engine's state, fed as the child writes; frames leave on the next
    // pump. A fast child answers inside one pump period, so wait for the frame rather than read the
    // last one sent so far.
    let last_size = || {
        rec.0
            .lock()
            .unwrap()
            .last()
            .map(|f| decode(f).unwrap())
            .map(|f| (f.cols, f.rows))
    };
    assert!(
        eventually(Duration::from_secs(10), || last_size() == Some((100, 30))),
        "last frame size: {:?}",
        last_size()
    );
    pty.terminate();
    rx.recv_timeout(Duration::from_secs(20))
        .expect("terminated");
}

fn a_too_narrow_resize_reaches_the_child_at_the_adopted_size() {
    let (pty, _rec, rx) = spawn("size-on-line", Options::default());
    assert!(eventually(Duration::from_secs(10), || screen(&pty)
        .contains("SIZE:80x24;")));
    let adopted = pty.resize(1, 12);
    assert_eq!(adopted, (justerm_core::MIN_COLUMNS, 12));
    pty.write(b"\r").unwrap();
    let want = format!("SIZE:{}x12;", justerm_core::MIN_COLUMNS);
    assert!(
        eventually(Duration::from_secs(10), || screen(&pty).contains(&want)),
        "screen: {:?}",
        screen(&pty)
    );
    pty.terminate();
    rx.recv_timeout(Duration::from_secs(20))
        .expect("terminated");
}

/// A stand-in legacy encoding with one two-byte character, enough to test a decoder that carries
/// state between reads: `B0 A1` is U+AC00. A lone byte of `0x80` and above waits for its pair;
/// anything else passes through.
#[cfg(unix)]
#[derive(Default)]
struct ToyLegacy {
    lead: Option<u8>,
}

#[cfg(unix)]
impl Decode for ToyLegacy {
    fn decode(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for &b in bytes {
            match self.lead.take() {
                Some(0xB0) if b == 0xA1 => out.extend_from_slice("\u{ac00}".as_bytes()),
                Some(_) => {
                    out.push(b'?');
                    self.push(b, &mut out);
                }
                None => self.push(b, &mut out),
            }
        }
        out
    }

    /// A lead byte still waiting for its pair when the output ends becomes `?`.
    fn finish(&mut self) -> Vec<u8> {
        self.lead.take().map(|_| b"?".to_vec()).unwrap_or_default()
    }
}

#[cfg(unix)]
impl ToyLegacy {
    fn push(&mut self, b: u8, out: &mut Vec<u8>) {
        if b >= 0x80 {
            self.lead = Some(b);
        } else {
            out.push(b);
        }
    }
}

/// Rewrites `-` as `_`: a visible mark that output went through the decoder.
struct DashToUnderscore;

impl Decode for DashToUnderscore {
    fn decode(&mut self, bytes: &[u8]) -> Vec<u8> {
        bytes
            .iter()
            .map(|&b| if b == b'-' { b'_' } else { b })
            .collect()
    }
}

/// Rewrites `a` as `b`: a visible mark that input went through the encoder.
struct AToB;

impl Encode for AToB {
    fn encode(&mut self, bytes: &[u8]) -> Vec<u8> {
        bytes
            .iter()
            .map(|&b| if b == b'a' { b'b' } else { b })
            .collect()
    }
}

fn the_decoder_sees_the_output_before_the_engine() {
    let options = Options {
        decoder: Some(Box::new(DashToUnderscore)),
        ..Options::default()
    };
    let (pty, _rec, rx) = spawn("print-exit-7", options);
    rx.recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
    let text = screen(&pty);
    assert!(text.contains("hello_from_the_child"), "screen: {text:?}");
    assert!(!text.contains(MARKER), "screen: {text:?}");
}

fn the_encoder_sees_what_the_consumer_writes() {
    let options = Options {
        encoder: Some(Box::new(AToB)),
        ..Options::default()
    };
    let (pty, _rec, rx) = spawn("echo-line", options);
    pty.write(b"aaa\r").unwrap();
    assert!(
        eventually(Duration::from_secs(10), || screen(&pty).contains("GOT:bbb")),
        "screen: {:?}",
        screen(&pty)
    );
    rx.recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
}

fn utf8_passes_through_without_a_hook() {
    let (pty, _rec, rx) = spawn("print-utf8", Options::default());
    rx.recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
    let text = screen(&pty);
    assert!(text.contains(UTF8_SAMPLE), "screen: {text:?}");
}

/// Unix only. On Windows a console child cannot put these bytes on the pseudo-terminal:
/// - measured (#1047): Rust's std refuses to write non-UTF-8 to a console (`InvalidData`);
/// - by ConPTY's design, not measured here: conhost re-emits console output as UTF-8.
///
/// So on Windows there is nothing for a decoder to reassemble. The decoder's placement is proven on
/// every OS by `the_decoder_sees_the_output_before_the_engine`.
#[cfg(unix)]
fn a_legacy_character_split_across_reads_is_decoded_whole() {
    let options = Options {
        decoder: Some(Box::new(ToyLegacy::default())),
        ..Options::default()
    };
    let (pty, _rec, rx) = spawn("split-legacy", options);
    rx.recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
    let text = screen(&pty);
    assert!(text.contains('\u{ac00}'), "screen: {text:?}");
    assert!(!text.contains('?'), "screen: {text:?}");
}

/// Unix only, for the same reason as the test above.
#[cfg(unix)]
fn a_byte_the_decoder_still_holds_at_the_end_is_flushed() {
    let options = Options {
        decoder: Some(Box::new(ToyLegacy::default())),
        ..Options::default()
    };
    let (pty, _rec, rx) = spawn("lone-lead-then-exit", options);
    rx.recv_timeout(Duration::from_secs(20))
        .expect("the child exits");
    let text = screen(&pty);
    assert!(text.contains('?'), "screen: {text:?}");
}

fn main() {
    if let Ok(mode) = std::env::var(HELPER) {
        helper(&mode);
        return;
    }
    // Extended below on Unix only.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut tests: Vec<(&str, fn())> = vec![
        (
            "the_childs_output_reaches_the_sink_as_a_frame",
            the_childs_output_reaches_the_sink_as_a_frame,
        ),
        ("the_exit_code_is_reported", the_exit_code_is_reported),
        (
            "frames_arrive_on_the_default_timer_while_the_child_runs",
            frames_arrive_on_the_default_timer_while_the_child_runs,
        ),
        (
            "a_quiet_child_sends_no_frames",
            a_quiet_child_sends_no_frames,
        ),
        ("the_pump_period_is_honoured", the_pump_period_is_honoured),
        (
            "terminate_ends_the_child_and_reports_its_exit",
            terminate_ends_the_child_and_reports_its_exit,
        ),
        (
            "the_session_stays_readable_after_exit",
            the_session_stays_readable_after_exit,
        ),
        (
            "output_written_just_before_exit_still_arrives",
            output_written_just_before_exit_still_arrives,
        ),
        ("no_frame_follows_on_exit", no_frame_follows_on_exit),
        (
            "dropping_the_pty_terminates_the_child_and_still_reports_its_exit",
            dropping_the_pty_terminates_the_child_and_still_reports_its_exit,
        ),
        (
            "terminate_ends_a_child_that_ignores_sighup",
            terminate_ends_a_child_that_ignores_sighup,
        ),
        (
            "a_missing_program_is_an_error_not_a_running_pty",
            a_missing_program_is_an_error_not_a_running_pty,
        ),
        (
            "bytes_the_consumer_writes_reach_the_child",
            bytes_the_consumer_writes_reach_the_child,
        ),
        (
            "a_resize_reaches_the_child_at_the_new_size",
            a_resize_reaches_the_child_at_the_new_size,
        ),
        (
            "a_too_narrow_resize_reaches_the_child_at_the_adopted_size",
            a_too_narrow_resize_reaches_the_child_at_the_adopted_size,
        ),
        (
            "the_decoder_sees_the_output_before_the_engine",
            the_decoder_sees_the_output_before_the_engine,
        ),
        (
            "the_encoder_sees_what_the_consumer_writes",
            the_encoder_sees_what_the_consumer_writes,
        ),
        (
            "utf8_passes_through_without_a_hook",
            utf8_passes_through_without_a_hook,
        ),
    ];
    #[cfg(unix)]
    tests.extend([
        (
            "a_byte_the_decoder_still_holds_at_the_end_is_flushed",
            a_byte_the_decoder_still_holds_at_the_end_is_flushed as fn(),
        ),
        (
            "a_legacy_character_split_across_reads_is_decoded_whole",
            a_legacy_character_split_across_reads_is_decoded_whole as fn(),
        ),
        (
            "a_query_from_the_child_is_answered",
            a_query_from_the_child_is_answered as fn(),
        ),
        (
            "a_reply_the_pump_drains_reaches_the_child",
            a_reply_the_pump_drains_reaches_the_child as fn(),
        ),
    ]);
    let filter: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let mut failed = Vec::new();
    for (name, test) in tests {
        if !filter.is_empty() && !filter.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        let ok = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        if !ok {
            failed.push(name);
        }
    }
    if failed.is_empty() {
        println!("test result: ok.");
    } else {
        println!("test result: FAILED. failures: {failed:?}");
        std::process::exit(101);
    }
}
