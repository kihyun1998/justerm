//! #750 — a command mark is repaired when the buffer *moves* and by nothing when its
//! row's **content** dies in place, so `Engine::command_lines` answers with commands
//! that are not there and with text that belongs to something else.
//!
//! Two defects, and they do not fold together — which is the whole reason this file
//! pins both halves separately.
//!
//! **The text half — captured, not re-read.** `command_lines` used to re-extract each
//! command from *current* cells, clipped to `[b_col, c_col)`. Measured, four different
//! verbs make that clip name somebody else's content: a plain overwrite, ICH, DCH and
//! an erase. Only the last is a verb a disposal rule could ever reach, so no lifetime
//! rule closes this — the text is frozen at OSC-133 `C`, which is the instant it is
//! complete and on screen, and read back verbatim afterwards. That is where the fact is
//! first true, and it is what the one system that solves this at all does (VSCode's
//! shell integration extracts at its command-executed handler and stores the string).
//!
//! **The lifetime half — ED only, deliberately.** A mark whose row was blanked still
//! reports a *document line*, and revealing to it lands on an empty row. ED's whole-row
//! arms therefore dispose the marks on those lines, firing `MarkerDisposed` so the
//! consumer's cleanup stays one path. **EL and ECH deliberately do not**, and that is
//! not an omission: both references retire a mark on a whole-row reset and on nothing
//! else (xterm.js `Buffer.clearMarkers` is called only from `_resetBufferLine`, reached
//! from `eraseInDisplay`; ghostty's whole-struct `row.* = .{ .cells = … }` in
//! `Screen.clearRows` resets `semantic_prompt` while `Screen.clearCells`, which
//! `eraseLine`/`eraseChars` use, never touches it). There is also a reference-free
//! reason, and it is the decisive one: `\r ESC[K` is how a line editor redraws the input
//! line on **every keystroke**, and `B` was emitted before the user began typing. An EL
//! that retired marks would delete the `CommandStart` of the command being typed, so no
//! command would ever be reported at all.
//!
//! **Why the exit code is resolved in the stream.** Disposing a row's marks used to
//! re-parent the *next* exit code onto the *previous* command, because the pairing was
//! positional over survivors (`out.last_mut()`), and only the from-the-oldest ordering
//! of eviction had been hiding it. Measured before the fix:
//! `[(0,"a0",Some(1)),(1,"a1",Some(2)),(2,"a2",Some(3))]` → dispose everything on
//! absolute line 1 → `[(0,"a0",Some(2)),(2,"a2",Some(3))]`. The exit is now written
//! onto the `OutputStart` mark when `D` arrives, so the pairing is decided where it is
//! unambiguous and a disposal can only ever *drop* an answer, never move one.

use justerm_core::{Engine, MarkerKind, TermEvent};

/// Four complete OSC-133 groups on their own rows: `$ cN` typed, `o` printed.
fn four_commands() -> Engine {
    let mut e = Engine::with_scrollback(16, 6, 40);
    for i in 0..4 {
        e.feed(
            format!("\x1b]133;A\x07$ \x1b]133;B\x07c{i}\x1b]133;C\x07o\r\n\x1b]133;D;0\x07")
                .as_bytes(),
        );
    }
    e.drain_events();
    e
}

fn lines(e: &Engine) -> Vec<(usize, String, Option<i32>)> {
    e.command_lines()
        .into_iter()
        .map(|c| (c.line, c.command, c.exit))
        .collect()
}

fn disposed(e: &mut Engine) -> usize {
    e.drain_events()
        .into_iter()
        .filter(|ev| matches!(ev, TermEvent::MarkerDisposed(_)))
        .count()
}

/// The control every other test is read against.
#[test]
fn the_commands_are_reported_before_anything_is_erased() {
    let e = four_commands();
    assert_eq!(
        lines(&e),
        vec![
            (0, "c0".into(), Some(0)),
            (1, "c1".into(), Some(0)),
            (2, "c2".into(), Some(0)),
            (3, "c3".into(), Some(0)),
        ]
    );
}

// ---- the lifetime half -----------------------------------------------------------

/// `clear` — the dominant path. Every mark on the screen goes, through the channel the
/// consumer already handles.
#[test]
fn ed_2_disposes_the_marks_on_every_row_it_blanks() {
    let mut e = four_commands();
    e.feed(b"\x1b[H\x1b[2J");

    assert_eq!(lines(&e), vec![], "no command survives a cleared screen");
    assert!(e.command_marks().is_empty(), "and no mark does either");
    assert_eq!(disposed(&mut e), 16, "each of the four groups' four marks");
}

/// The failure the phantom actually produced: after a `clear` the shell redraws its
/// prompt onto the columns the dead marks bound, so every later command was reported
/// twice — once as itself and once through a corpse.
#[test]
fn a_command_run_after_a_clear_is_reported_exactly_once() {
    let mut e = four_commands();
    e.feed(b"\x1b[H\x1b[2J");
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07new\x1b]133;C\x07n\r\n\x1b]133;D;0\x07");

    assert_eq!(lines(&e), vec![(0, "new".into(), Some(0))]);
}

/// ED 0 and ED 1 blank whole rows too, and only those.
///
/// **The cursor's own row is the known edge, pinned here rather than fixed.** Both
/// references route it through the *partial* helper and so keep its marks even when the
/// erase covers the full width — xterm.js's ED 1 arm is the starkest case, erasing
/// `[0, x+1)` with `x+1 == cols` through `_eraseInBufferLine` and disposing nothing. So
/// `ESC[H ESC[0J` leaves one phantom on row 0. Followed rather than widened because two
/// independent references converge *including* the edge, and the only argument for
/// widening is symmetry — which is the tell ADR-0019's retracted first amendment was
/// caught by (a rule with no user-facing benefit anyone could name).
#[test]
fn ed_0_disposes_the_rows_below_and_keeps_the_cursor_row_marks() {
    let mut e = four_commands();
    e.feed(b"\x1b[2;1H\x1b[0J"); // cursor on row 1, erase below

    assert_eq!(
        lines(&e),
        vec![(0, "c0".into(), Some(0)), (1, "c1".into(), Some(0))],
        "rows 2..5 are blanked whole and retire; row 0 is untouched and row 1 is the \
         cursor row"
    );
}

#[test]
fn ed_1_disposes_the_rows_above_and_keeps_the_cursor_row_marks() {
    let mut e = four_commands();
    e.feed(b"\x1b[3;1H\x1b[1J"); // cursor on row 2, erase above

    assert_eq!(
        lines(&e),
        vec![(2, "c2".into(), Some(0)), (3, "c3".into(), Some(0))],
        "rows 0 and 1 retire; row 2 is the cursor row and keeps c2. The document lines \
         do NOT renumber — a blanked row is still a hard-ended row, so `doc_line_of` \
         still counts it. `line` stays derived rather than frozen precisely because it \
         is the half the anchor fixups do maintain"
    );
}

/// The deliberate divergence, pinned so it cannot be "fixed" by accident. Both
/// references retire a mark on a whole-row *reset* and on no other erase, and EL is
/// what a prompt redraw uses.
#[test]
fn el_2_does_not_dispose_the_mark_on_the_row_it_blanks() {
    let mut e = four_commands();
    e.feed(b"\x1b[1;1H\x1b[2K");

    assert_eq!(e.command_marks().len(), 16, "EL retires nothing");
    assert_eq!(disposed(&mut e), 0);
    assert_eq!(
        lines(&e)[0],
        (0, "c0".into(), Some(0)),
        "and because the text is captured, the surviving mark still answers the \
         command that ran rather than the blanks now under it"
    );
}

/// Scrollback is not the screen: ED blanks grid rows, so a command that has already
/// scrolled off keeps both its marks and its place.
#[test]
fn ed_2_leaves_the_marks_that_have_scrolled_into_scrollback() {
    let mut e = Engine::with_scrollback(16, 3, 40);
    for i in 0..4 {
        e.feed(
            format!("\x1b]133;A\x07$ \x1b]133;B\x07c{i}\x1b]133;C\x07o\r\n\x1b]133;D;0\x07")
                .as_bytes(),
        );
    }
    e.drain_events();
    let before = lines(&e);
    assert!(before.len() >= 2, "fixture must push some rows off-screen");

    e.feed(b"\x1b[H\x1b[2J");

    let after = lines(&e);
    assert!(
        !after.is_empty() && after.len() < before.len(),
        "the scrolled-off commands survive and the on-screen ones do not: \
         before {before:?}, after {after:?}"
    );
    assert_eq!(after, before[..after.len()].to_vec());
}

/// The alt screen has its own marker population, and OSC-133 marks are primary-only.
/// A `vim` starting up must not delete the shell's command history.
#[test]
fn an_erase_on_the_alt_screen_does_not_dispose_primary_command_marks() {
    let mut e = four_commands();
    let before = lines(&e);

    e.feed(b"\x1b[?1049h"); // enter alt
    e.feed(b"\x1b[H\x1b[2J");
    assert_eq!(lines(&e), before, "while on alt");

    e.feed(b"\x1b[?1049l"); // leave
    assert_eq!(lines(&e), before, "and after leaving");
}

// ---- the exit-code pairing -------------------------------------------------------

/// Three commands with distinct codes, the fixture both exit-code tests read against.
/// A group's `A`/`B`/`C` land on its own row and its `D` on the next, so absolute line
/// `n` holds command `n-1`'s `D` together with the whole of command `n`.
fn three_coded_commands() -> Engine {
    let mut e = Engine::with_scrollback(16, 6, 40);
    for (i, code) in [1, 2, 3].iter().enumerate() {
        e.feed(
            format!("\x1b]133;A\x07$ \x1b]133;B\x07a{i}\x1b]133;C\x07o\r\n\x1b]133;D;{code}\x07")
                .as_bytes(),
        );
    }
    e.drain_events();
    e
}

/// An erase that takes a `D` must not cost a command that fully survives its code.
///
/// This is the reachable half: ED retires a contiguous run of rows, so it can strip the
/// `D` off the end of a command whose text and start are safely above it. Before the
/// exit moved into the stream this answered `(1, "a1", None)`.
#[test]
fn a_command_keeps_its_exit_when_an_erase_takes_only_its_finished_mark() {
    let mut e = three_coded_commands();
    e.feed(b"\x1b[2;1H\x1b[0J"); // rows 2..5 retire — a1's D is on line 2

    assert_eq!(
        lines(&e),
        vec![(0, "a0".into(), Some(1)), (1, "a1".into(), Some(2))],
        "a1 survives whole and keeps its own code, though the mark that carried it is \
         gone"
    );
}

/// And a disposal that leaves a *hole* must not slide a code onto the wrong command.
///
/// ED cannot produce a hole — it retires a prefix or a suffix — but `remove_marker` is
/// public and does, so the pairing is pinned against the shape rather than against the
/// verb. Under the old query-time pairing this answered `[(0,"a0",Some(2)), …]`: `a0`
/// wearing `a1`'s exit code.
#[test]
fn a_hole_in_the_marks_does_not_move_an_exit_code_onto_another_command() {
    let mut e = three_coded_commands();
    let victims: Vec<_> = e
        .command_marks()
        .into_iter()
        .filter(|(_, line, _)| *line == 1)
        .map(|(id, _, _)| id)
        .collect();
    assert_eq!(victims.len(), 4, "a0's D plus the whole of a1");
    for id in victims {
        e.remove_marker(id);
    }

    assert_eq!(
        lines(&e),
        vec![(0, "a0".into(), Some(1)), (2, "a2".into(), Some(3))],
        "a0 keeps its own code and a1 is simply absent — a2 stays at its own document          line, since disposing a mark moves no content"
    );
}

/// The exit is resolved when `D` arrives, so losing the `D` mark afterwards cannot cost
/// a command that fully survives its code.
#[test]
fn a_surviving_command_keeps_its_exit_when_only_its_finished_mark_is_disposed() {
    let mut e = four_commands();
    let d_ids: Vec<_> = e
        .command_marks()
        .into_iter()
        .filter(|(_, _, k)| matches!(k, MarkerKind::CommandFinished(_)))
        .map(|(id, _, _)| id)
        .collect();
    assert_eq!(d_ids.len(), 4);

    for id in d_ids {
        e.remove_marker(id);
    }

    assert_eq!(
        lines(&e),
        vec![
            (0, "c0".into(), Some(0)),
            (1, "c1".into(), Some(0)),
            (2, "c2".into(), Some(0)),
            (3, "c3".into(), Some(0)),
        ]
    );
}

// ---- the text half ---------------------------------------------------------------

/// The producer no disposal rule reaches: a plain write over the command's columns,
/// with no erase verb anywhere.
#[test]
fn an_overwrite_of_the_command_row_does_not_re_borrow_its_cells() {
    let mut e = four_commands();
    e.feed(b"\x1b[1;3HZZ");

    assert_eq!(
        lines(&e)[0],
        (0, "c0".into(), Some(0)),
        "the command that ran, not the cells now standing where it was"
    );
}

/// ICH and DCH move the command's cells out from under the recorded columns without
/// blanking the row, which is the same class one verb over.
#[test]
fn an_in_line_shift_does_not_re_borrow_the_command_row() {
    let mut e = four_commands();
    e.feed(b"\x1b[1;3H\x1b[1P"); // DCH 1 inside the command text
    assert_eq!(lines(&e)[0].1, "c0");

    let mut e = four_commands();
    e.feed(b"\x1b[1;3H\x1b[3@"); // ICH 3 before it
    assert_eq!(lines(&e)[0].1, "c0");
}

/// A command that spans rows is captured whole, and the answer does not change when the
/// rows underneath it do.
#[test]
fn a_wrapped_command_is_captured_across_its_rows() {
    let mut e = Engine::with_scrollback(8, 6, 40);
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07abcdefghij\x1b]133;C\x07o\r\n\x1b]133;D;0\x07");
    e.drain_events();
    assert_eq!(lines(&e)[0].1, "abcdefghij");

    e.feed(b"\x1b[1;1H\x1b[2K");
    assert_eq!(lines(&e)[0].1, "abcdefghij", "still the captured text");
}

/// A command still being typed — `B` with no `C` — has no bound and no capture, so it
/// stays out of the answer exactly as before.
#[test]
fn a_command_with_no_output_start_is_still_omitted() {
    let mut e = four_commands();
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07typing");

    assert_eq!(
        lines(&e).len(),
        4,
        "the four finished ones, and not the fifth"
    );
}

/// The capture is bounded, for the reason `MAX_MARKERS` is: the *stream* allocates it,
/// so a stream that emits `B`, dumps a screenful and then `C` decides the size.
#[test]
fn a_capture_is_bounded_by_max_command_text() {
    let mut e = Engine::with_scrollback(64, 40, 4000);
    e.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07");
    for _ in 0..400 {
        e.feed(b"0123456789012345678901234567890123456789012345678901234567890123");
    }
    e.feed(b"\x1b]133;C\x07o\r\n\x1b]133;D;0\x07");

    let captured = &lines(&e)[0].1;
    assert!(
        captured.chars().count() <= justerm_core::MAX_COMMAND_TEXT,
        "captured {} chars, cap is {}",
        captured.chars().count(),
        justerm_core::MAX_COMMAND_TEXT
    );
    assert!(
        !captured.is_empty(),
        "truncation keeps the announceable prefix rather than dropping the answer"
    );
}
