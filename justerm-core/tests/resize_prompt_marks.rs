//! A column resize hands an OSC 133-marked prompt to the shell's SIGWINCH redraw (#1083).
//!
//! The shell's `133;A` says how much of the prompt it redraws: no option keeps only the cursor's
//! line (ADR-0035), `redraw=1` leaves the whole prompt from `A` down to the cursor un-reflowed,
//! `redraw=last` and a `k=s` line keep only the cursor's line, `redraw=0` reflows it. The whole
//! prompt is left only to a live prompt: an `A` paired with the cursor's input line, on screen.

use justerm_core::{Engine, MarkerKind};

/// The viewport's non-empty rows, trailing blanks trimmed.
fn rows(t: &Engine) -> Vec<String> {
    (0..t.grid().rows())
        .map(|i| {
            let s: String = t.viewport_line(i).iter().map(|c| c.c()).collect();
            s.trim_end_matches(' ').to_string()
        })
        .filter(|r| !r.is_empty())
        .collect()
}

fn ends_with(t: &Engine, tail: &[&str], case: &str) {
    let rows = rows(t);
    assert!(
        rows.len() >= tail.len() && rows[rows.len() - tail.len()..] == *tail,
        "{case}: {rows:?}"
    );
}

fn count(t: &Engine, row: &str) -> usize {
    rows(t).iter().filter(|r| r.as_str() == row).count()
}

// ---- synthetic -------------------------------------------------------------------------------

#[test]
fn redraw_0_reflows_the_prompt_line() {
    let mut t = Engine::new(30, 6);
    t.feed(b"\x1b]133;A;redraw=0\x07$ \x1b]133;B\x070123456789abcdefghijklmn");
    t.resize(10, 6);
    assert_eq!(rows(&t), ["$ 01234567", "89abcdefgh", "ijklmn"]);
    let c = t.cursor();
    assert_eq!((c.row, c.col), (2, 6));
}

#[test]
fn a_prompt_start_left_in_scrollback_is_not_a_live_prompt() {
    // A one-row screen: the prompt's top line is in history when Ctrl-L blanks the screen, so its
    // `A` survives and pairs with the redraw's `B` — but it is not on screen.
    let mut t = Engine::new(30, 1);
    t.feed(b"\x1b]133;A;redraw=1\x07top line here\r\nuser@host:~$ \x1b]133;B\x07");
    t.feed(b"\x1b[H\x1b[2Juser@host:~$ \x1b]133;B\x07");
    t.resize(10, 1);
    assert!(
        t.accessible_text().contains("top line here"),
        "{:?}",
        t.accessible_text()
    );
}

#[test]
fn under_the_alt_screen_the_primary_prompt_is_kept_the_same_way() {
    let mut t = Engine::new(30, 8);
    t.feed(b"\x1b]133;A;redraw=1\x07top line here\r\nuser@host:~$ \x1b]133;B\x07");
    t.feed(b"\x1b[?1049h");
    t.resize(10, 8);
    t.feed(b"\x1b[?1049l");
    assert_eq!(rows(&t), ["top line h", "user@host:"]);
}

#[test]
fn a_k_s_line_keeps_only_the_cursor_line_and_is_not_a_prompt_mark() {
    // kitty's bash integration: `A` on the first line, `A;k=s` before the last one.
    let mut t = Engine::new(30, 8);
    t.feed(
        b"\x1b]133;A;redraw=1\x07top line here\r\n\x1b]133;A;k=s\x07user@host:~$ \x1b]133;B\x07",
    );
    t.resize(10, 8);
    ends_with(&t, &["top line h", "ere", "user@host:"], "k=s");
    let prompts = t
        .command_marks()
        .iter()
        .filter(|m| m.2 == MarkerKind::PromptStart)
        .count();
    assert_eq!(prompts, 1);
}

#[test]
fn a_command_running_without_c_keeps_its_output_reflowing() {
    // A shell that sends no `C` (sh/dash): the cursor has left the input line.
    let mut t = Engine::new(30, 10);
    t.feed(b"\x1b]133;D;0\x07\x1b]133;A;redraw=1\x07$ \x1b]133;B\x07cat log\r\n");
    t.feed(b"0123456789abcdefghij0123456789\r\nprogress 40%");
    t.resize(10, 10);
    ends_with(
        &t,
        &[
            "$ cat log",
            "0123456789",
            "abcdefghij",
            "0123456789",
            "progress 4",
        ],
        "no C",
    );
}

#[test]
fn each_prompt_start_states_its_own_redraw() {
    // The first prompt says `redraw=1`; the next one says nothing, so only its cursor line is kept.
    let mut t = Engine::new(30, 8);
    t.feed(b"\x1b]133;A;redraw=1\x07$ \x1b]133;B\x07true\r\n\x1b]133;C\x07\x1b]133;D;0\x07");
    t.feed(b"\x1b]133;A\x07top line here\r\nuser@host:~$ \x1b]133;B\x07");
    t.resize(10, 8);
    assert_eq!(rows(&t), ["$ true", "top line h", "ere", "user@host:"]);
}

#[test]
fn without_marks_the_cursor_line_alone_is_kept() {
    let mut t = Engine::new(30, 6);
    t.feed(b"top line here\r\nuser@host:~$ ");
    t.resize(10, 6);
    assert_eq!(rows(&t), ["top line h", "ere", "user@host:"]);
}

// ---- the recording ---------------------------------------------------------------------------

/// `capture-resize-prompt-marks.sh`: records of `<case> <op> <cols> <len>\n<len bytes>`.
const RECORDING: &[u8] = include_bytes!("fixtures/resize_prompt_marks.raw");

fn replay(case: &str) -> Engine {
    let mut raw = RECORDING;
    let mut t: Option<Engine> = None;
    while !raw.is_empty() {
        let nl = raw.iter().position(|&b| b == b'\n').expect("header line");
        let header = std::str::from_utf8(&raw[..nl]).expect("ascii header");
        let parts: Vec<&str> = header.split(' ').collect();
        let (name, op) = (parts[0], parts[1]);
        let cols: usize = parts[2].parse().unwrap();
        let len: usize = parts[3].parse().unwrap();
        let bytes = &raw[nl + 1..nl + 1 + len];
        raw = &raw[nl + 1 + len..];
        if name != case {
            continue;
        }
        match op {
            "start" => {
                let mut e = Engine::new(cols, 12);
                e.feed(bytes);
                t = Some(e);
            }
            "resize" => {
                let e = t.as_mut().expect("start first");
                e.resize(cols, 12);
                e.feed(bytes);
            }
            _ => t.as_mut().expect("start first").feed(bytes),
        }
    }
    t.unwrap_or_else(|| panic!("no case {case}"))
}

#[test]
fn bash_without_a_hint_redraws_one_prompt() {
    for case in ["bash-13-narrow", "bash-steps"] {
        let t = replay(case);
        ends_with(&t, &["user@host:", "~$"], case);
        assert_eq!(count(&t, "user@host:"), 1, "{case}");
    }
    let t = replay("bash-typed-narrow");
    ends_with(&t, &["user@host:", "~$ echo he", "llo world"], "typed");
    assert_eq!(count(&t, "user@host:"), 1);
    let t = replay("bash-32-widen");
    ends_with(&t, &["user@host:~/a/very/long/path/x$"], "widen");
}

#[test]
fn bash_two_line_prompt_without_a_hint_keeps_its_top_line_whole() {
    let t = replay("bash-two-narrow");
    ends_with(&t, &["top line h", "ere", "user@host:", "~$"], "bash two");
}

#[test]
fn zsh_two_line_prompt_without_a_hint_is_as_before() {
    // No hint keeps only the cursor's line: zsh's climb over the top line still leaves a piece.
    let t = replay("zsh-two-narrow");
    ends_with(
        &t,
        &["top line h", "top line h", "ere", "user@host:", "~$"],
        "zsh two",
    );
}

#[test]
fn zsh_with_redraw_1_redraws_one_whole_prompt() {
    let t = replay("zsh1-13-narrow");
    ends_with(&t, &["user@host:", "~$"], "zsh1 13");
    assert_eq!(count(&t, "user@host:"), 1);
    let t = replay("zsh1-two-narrow");
    ends_with(&t, &["top line h", "ere", "user@host:", "~$"], "zsh1 two");
    assert_eq!(count(&t, "top line h"), 1);
    let t = replay("zsh1-32-widen");
    ends_with(&t, &["user@host:~/a/very/long/path/x$"], "zsh1 widen");
}

#[test]
fn a_readline_repl_after_c_keeps_its_line() {
    let t = replay("zsh1-python-narrow");
    assert_eq!(count(&t, ">>> print(\"h"), 1, "{:?}", rows(&t));
    let t = replay("zsh1-python-widen");
    assert!(
        rows(&t).iter().any(|r| r == "user@host:~$ python3 -q"),
        "{:?}",
        rows(&t)
    );
}

#[test]
fn read_p_after_c_is_cut_as_before() {
    let t = replay("bash-readp");
    ends_with(&t, &["Name of td", "ef"], "read -p");
}

#[test]
fn a_redraw_after_ctrl_l_leaves_one_prompt() {
    let t = replay("bash-ctrll");
    ends_with(&t, &["user@host:", "~$"], "ctrl-l");
    assert_eq!(count(&t, "user@host:"), 1);
}

#[test]
fn a_stale_prompt_start_after_ctrl_l_does_not_cut_history() {
    // bash prints `A` from PROMPT_COMMAND, so readline's Ctrl-L (`ESC[H ESC[2J` + PS1) re-sends
    // only `B`; the screen is short enough that the previous prompt's `A` is in scrollback.
    let mut t = Engine::new(30, 4);
    t.feed(b"\x1b]133;D;0\x07\x1b]133;A;redraw=1\x07user@host:~$ \x1b]133;B\x07");
    t.feed(b"echo WWWWWWWWWWWWWWWWWWWWWWWWWWWW; seq 3\r\n\x1b]133;C\x07");
    t.feed(b"WWWWWWWWWWWWWWWWWWWWWWWWWWWW\r\n1\r\n2\r\n3\r\n");
    t.feed(b"\x1b]133;D;0\x07\x1b]133;A;redraw=1\x07user@host:~$ \x1b]133;B\x07");
    t.feed(b"\x1b[H\x1b[2Juser@host:~$ \x1b]133;B\x07");
    t.resize(10, 4);
    assert!(
        t.accessible_text()
            .contains("echo WWWWWWWWWWWWWWWWWWWWWWWWWWWW; seq 3"),
        "{:?}",
        t.accessible_text()
    );
}
