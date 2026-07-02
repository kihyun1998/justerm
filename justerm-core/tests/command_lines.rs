//! #166 — OSC 133 command-*line* extraction for screen-reader command navigation.
//! Pairs CommandStart(B)→OutputStart(C) into the typed command text (the shell
//! prompt excluded via the columns captured at emit time, VSCode
//! `extractCommandLine` parity), with the trailing CommandFinished(D) exit and
//! the B-line jump anchor (VSCode nav lands on `command.marker` =
//! commandStartMarker).

use justerm_core::Engine;

/// The canonical `A user$ B ls C out D;0` cycle: the command text is exactly the
/// bytes between B and C — the prompt "user$ " (before B) and the output (after
/// C) are excluded via the captured columns. `line` is the CommandStart document
/// line (0 here — no wraps); `exit` is the D code.
#[test]
fn extracts_command_text_excluding_prompt_and_output() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;A\x07user$ \x1b]133;B\x07ls\x1b]133;C\x07out\r\n\x1b]133;D;0\x07");

    let cmds = t.command_lines();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].command, "ls");
    assert_eq!(cmds[0].exit, Some(0));
    assert_eq!(cmds[0].line, 0, "jump anchor = CommandStart (B) line");
}

/// A non-zero exit rides through to the entry.
#[test]
fn carries_nonzero_exit() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07false\x1b]133;C\x07\r\n\x1b]133;D;1\x07");

    let cmds = t.command_lines();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].command, "false");
    assert_eq!(cmds[0].exit, Some(1));
}

/// A command submitted but not yet finished (B and C, no D) is a real navigable
/// command — its text is bounded by C — with no exit yet.
#[test]
fn command_without_finish_has_no_exit() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07sleep 10\x1b]133;C\x07");

    let cmds = t.command_lines();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].command, "sleep 10");
    assert_eq!(cmds[0].exit, None);
}

/// A command still being typed (B, no C) has no bounded text, so it is NOT yet a
/// navigable history entry — excluded until output starts.
#[test]
fn command_still_typing_is_excluded() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls -l");

    assert!(t.command_lines().is_empty());
}

/// Several commands in a session are returned in buffer order, each with its own
/// text/exit/line.
#[test]
fn multiple_commands_in_order() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07echo hi\x1b]133;C\x07hi\r\n\x1b]133;D;0\x07");
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07nope\x1b]133;C\x07\r\n\x1b]133;D;127\x07");

    let cmds = t.command_lines();
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].command, "echo hi");
    assert_eq!(cmds[0].exit, Some(0));
    assert_eq!(cmds[1].command, "nope");
    assert_eq!(cmds[1].exit, Some(127));
    assert!(cmds[1].line > cmds[0].line, "second command sits lower");
}

/// A command long enough to soft-wrap across rows joins into one logical command
/// string (the wrap boundary is not a break — VSCode concatenates wrapped rows).
#[test]
fn soft_wrapped_command_joins() {
    // 10 cols forces the 14-char command to wrap once.
    let mut t = Engine::new(10, 24);
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07echo abcdefgh\x1b]133;C\x07\r\n\x1b]133;D;0\x07");

    let cmds = t.command_lines();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].command, "echo abcdefgh");
}

/// The jump `line` is a *document* line (accessible-view coordinates), not the
/// absolute buffer line: soft-wrapped output between commands collapses to one
/// logical line, so a later command's document line is *below* its absolute line.
#[test]
fn jump_line_is_document_line_not_absolute() {
    let mut t = Engine::new(10, 24);
    // cmd1 "one" on abs row 0, then a hard newline.
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07one\x1b]133;C\x07\r\n");
    // 13 chars of output soft-wrap abs row 1 -> row 2 (row 1 gets WRAPLINE), \r\n.
    t.feed(b"0123456789XYZ\r\n");
    // cmd2 "two" now sits on abs row 3, but document row 2 (the wrap shrank one).
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07two\x1b]133;C\x07\r\n\x1b]133;D;0\x07");

    let cmds = t.command_lines();
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].command, "one");
    assert_eq!(cmds[0].line, 0);
    assert_eq!(cmds[1].command, "two");
    assert_eq!(
        cmds[1].line, 2,
        "abs row 3 -> document row 2 (one soft wrap)"
    );
}

/// A resize reflows the buffer, moving the command's B/C marks to new
/// (line, column) slots. The marker *column* must reflow with the line — else the
/// text is extracted from stale columns on the re-wrapped grid. (#166 completeness
/// pass, lens 1: markers previously reflowed by line only, col hardcoded 0.)
#[test]
fn command_text_survives_a_resize_reflow() {
    let mut t = Engine::new(20, 6);
    // "prompt> mycommand" fits one 20-col row: B at col 8, C at col 17.
    t.feed(
        b"\x1b]133;A\x07prompt> \x1b]133;B\x07mycommand\x1b]133;C\x07\r\nout\r\n\x1b]133;D;0\x07",
    );
    assert_eq!(t.command_lines()[0].command, "mycommand");

    // Resize to 8 cols: the row reflows across 3 rows, so B moves to (row1,col0)
    // and C to (row2,col1). With a stale col the slice would read "d"; with the
    // column reflowed it stays "mycommand".
    t.resize(8, 6);
    assert_eq!(
        t.command_lines()[0].command,
        "mycommand",
        "the marker column reflows with the line"
    );
}

/// Re-emitting CommandStart(B) before any OutputStart(C) — e.g. a shell prompt
/// redraw — takes the latest B as the command's start (the earlier, output-less B
/// was an aborted start with no bounded text). The command pairs B2->C.
#[test]
fn re_emitted_command_start_takes_the_latest() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07old\x1b]133;B\x07new\x1b]133;C\x07\r\n\x1b]133;D;0\x07");

    let cmds = t.command_lines();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].command, "new");
}

/// OutputStart(C) with no preceding CommandStart(B) is a malformed stream — no
/// command is emitted (there is no start column to bound the text).
#[test]
fn output_start_without_command_start_is_ignored() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;C\x07out\r\n\x1b]133;D;0\x07");

    assert!(t.command_lines().is_empty());
}

/// #192: a command's text comes from the PRIMARY buffer even when `command_lines`
/// is queried while on the alt screen. Extraction used to read the active (alt)
/// grid, so a primary command's text came back empty inside a full-screen TUI;
/// the marks/exit were always primary-scoped, only the text was wrong.
#[test]
fn command_text_is_primary_scoped_on_the_alt_screen() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls -la\x1b]133;C\x07\r\nout\r\n\x1b]133;D;0\x07");
    t.feed(b"\x1b[?1049h"); // enter alt — primary content swaps out of the active grid

    let cmds = t.command_lines();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].command, "ls -la"); // was "" (read the alt grid)
    assert_eq!(cmds[0].exit, Some(0));
}

/// #192 (twin of the text bug, caught by the completeness pass): the command's
/// document `line` is primary-scoped too. `doc_line_of` counts soft-wraps via the
/// grid; on the alt screen it must count the PRIMARY buffer's wraps, not the blank
/// alt grid, or the jump line drifts.
#[test]
fn command_line_number_is_primary_scoped_on_alt() {
    let mut t = Engine::new(6, 24);
    t.feed(b"0123456789\r\n"); // soft-wraps abs row0 (WRAP) -> row1, then newline to row2
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07cmd\x1b]133;C\x07\r\n\x1b]133;D;0\x07"); // command on abs row2
    let on_primary = t.command_lines()[0].line;
    assert_eq!(
        on_primary, 1,
        "abs row2 -> document row1 (one soft wrap above)"
    );

    t.feed(b"\x1b[?1049h"); // enter alt — the primary wraps must still be counted
    assert_eq!(
        t.command_lines()[0].line,
        on_primary,
        "document line tracks the primary buffer, not the blank alt grid"
    );
}

/// Alt-screen apps emit no shell-integration marks, so no command lines surface
/// from an alt-screen session.
#[test]
fn alt_screen_yields_no_commands() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b[?1049h");
    t.feed(b"\x1b]133;A\x07\x1b]133;B\x07x\x1b]133;C\x07\x1b]133;D;0\x07");

    assert!(t.command_lines().is_empty());
}
