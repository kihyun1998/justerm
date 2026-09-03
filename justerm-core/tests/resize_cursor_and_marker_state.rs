//! Two things `Term::resize` loses that it should carry, both surfaced by the #549 completeness
//! pass and both pre-existing.
//!
//! ## 1. `pending_wrap` is cursor state, not screen configuration
//!
//! Printing into the last column does **not** advance: the cursor parks there and the wrap happens
//! on the *next* print. `resize` reset that flag alongside the scroll margins — so after a resize
//! the next byte overwrote the last glyph instead of wrapping past it.
//!
//! This header used to name the tab stops beside the margins, as state that "does legitimately
//! reset". #849 measured that false: the table is extended rather than rebuilt now, and only the
//! margins still reset.
//!
//! All three references keep it, and the two that reflow repair it rather than dropping it:
//!
//! - **ghostty** never touches the live cursor's flag, and repairs the saved cursor's explicitly:
//!   *"If we had pending wrap set and we're no longer at the end of the line, we unset the pending
//!   wrap and move the cursor to reflect the correct next position"* (`terminal/Screen.zig:2092-2098`
//!   @ `e6e26e1`). That rule — clear it **and step forward** — is the one implemented here.
//! - **alacritty** lifts the cursor *outside the grid* before reflowing (`point.column += 1`,
//!   `alacritty_terminal/src/grid/resize.rs:113-116` grow and `:248-251` shrink) and restores it by
//!   clamping back and re-arming the flag (`:173-177`) @ `852e971`.
//! - **xterm.js** encodes the state as `x === cols`, so it is representable by construction and
//!   survives whatever the reflow does.
//!
//! ## 2. The alt-screen resize path discards marker **columns**
//!
//! On the alt screen the *primary* markers still reflow with the primary pane (#187). That branch
//! built its points as `(m.line, 0)` and wrote back only `m.line`, while the primary branch passes
//! and restores both. The column is what bounds OSC-133 command-text extraction (#166), so a
//! resize taken while a full-screen app is up truncated the recorded command.
//!
//! This is a plain omission against a sibling in the same function, not a divergence from anything.

use justerm_core::Engine;

// ---- 1. pending_wrap -------------------------------------------------------------------------

#[test]
fn a_column_resize_keeps_the_deferred_wrap() {
    // `"abcdef"` fills all six columns, so the cursor parks on the last one with the wrap pending.
    // After the resize the next byte must still wrap rather than overwrite `f`.
    let mut t = Engine::new(6, 10);
    t.feed(b"abcdef");

    t.resize(3, 10);
    t.feed(b"X");

    assert_eq!(t.accessible_text().trim_end(), "abcdefX");
}

#[test]
fn a_rows_only_resize_keeps_the_deferred_wrap() {
    // No reflow runs at all here, so this pins the reset itself rather than the point mapping —
    // the reset was unconditional and fired on every resize, not just a column change.
    //
    // Rows **grow** rather than shrink deliberately. A shrink from 10 rows evicts the top of the
    // screen into scrollback while leaving the cursor on row 0, so the content and the cursor part
    // company and `accessible_text` stops being a statement about this flag. Measured: the
    // no-pending-wrap control moves the same way, which is what identifies that as a different
    // behaviour rather than this one.
    let mut t = Engine::new(6, 10);
    t.feed(b"abcdef");

    t.resize(6, 12);
    t.feed(b"X");

    assert_eq!(t.accessible_text().trim_end(), "abcdefX");
}

#[test]
fn a_resize_that_moves_the_cursor_off_the_last_column_steps_it_forward_instead() {
    // ghostty's rule: the flag means "the cursor is logically one past its column". If the reflow
    // leaves it somewhere other than the last column, that logical position is representable, so
    // the flag is cleared and the cursor takes it. `"abcde"` at 6 columns leaves the cursor at
    // column 5 with no pending wrap; `"abcdef"` widened to 8 leaves it at column 6, in range.
    let mut t = Engine::new(6, 10);
    t.feed(b"abcdef");

    t.resize(8, 10);

    assert_eq!((t.cursor().row, t.cursor().col), (0, 6));
    t.feed(b"X");
    assert_eq!(t.accessible_text().trim_end(), "abcdefX");
}

#[test]
fn a_resize_without_a_deferred_wrap_does_not_move_the_cursor() {
    // The side condition: the step-forward must be gated on the flag, not on the column.
    let mut t = Engine::new(6, 10);
    t.feed(b"abc");
    assert_eq!(
        (t.cursor().row, t.cursor().col),
        (0, 3),
        "fixture: no pending wrap"
    );

    t.resize(8, 10);

    assert_eq!((t.cursor().row, t.cursor().col), (0, 3));
}

// ---- 2. alt-screen resize and the primary markers' columns -------------------------------------

/// A shell-integration transcript: prompt, command, output. The `B`/`C` columns bound the command
/// text, so losing a column truncates it.
fn shell_transcript(cols: usize) -> Engine {
    let mut t = Engine::new(cols, 6);
    t.feed(b"$ \x1b]133;B\x07echo hello world\x1b]133;C\x07");
    t.feed(b"\r\nhello world\r\n");
    t
}

#[test]
fn a_resize_taken_on_the_alt_screen_keeps_the_primary_markers_columns() {
    let mut on_alt = shell_transcript(20);
    on_alt.feed(b"\x1b[?1049h");
    on_alt.resize(6, 6);
    on_alt.feed(b"\x1b[?1049l");

    let mut on_primary = shell_transcript(20);
    on_primary.resize(6, 6);

    assert_eq!(
        on_alt.command_lines().first().map(|c| c.command.clone()),
        on_primary
            .command_lines()
            .first()
            .map(|c| c.command.clone()),
        "which screen was active when the resize happened must not change the recorded command"
    );
}

#[test]
fn the_primary_resize_control_is_unchanged() {
    // Pins the comparand of the test above: if the primary path ever regressed, that assert would
    // pass for the wrong reason.
    //
    // This expected value used to carry a trailing `\n`, annotated as "#549's end-of-line gap, not
    // this one" — a defect frozen as the comparand because the sibling test only needs the two
    // screens to *agree*. #562 closed that gap: the C mark ending a full row now bounds the command
    // as `col == cols` instead of being answered with the next row's column 0, which is the first
    // row of the following logical line.
    let mut t = shell_transcript(20);

    t.resize(6, 6);

    assert_eq!(
        t.command_lines().first().map(|c| c.command.clone()),
        Some("echo hello world".to_string()),
        "the command text is width-independent"
    );
}
