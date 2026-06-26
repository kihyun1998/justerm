//! DECCOLM 80/132-column mode tests (#82, DEC private mode ?3). justerm is
//! dimension-free, so ?3 only *emits a request event* the consumer may honor by
//! calling resize(); the engine clears/homes/resizes nothing itself (matching
//! xterm.js, which does nothing for DECCOLM unless the embedder opts in).

use justerm_core::{Engine, TermEvent};

#[test]
fn deccolm_set_emits_a_132_column_request() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?3h");
    assert_eq!(t.drain_events(), vec![TermEvent::ColumnMode { cols: 132 }]);
}

#[test]
fn deccolm_reset_emits_an_80_column_request() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?3l");
    assert_eq!(t.drain_events(), vec![TermEvent::ColumnMode { cols: 80 }]);
}

#[test]
fn deccolm_does_not_clear_or_home() {
    // The engine performs no screen/cursor mutation of its own — only the event.
    let mut t = Engine::new(80, 24);
    t.feed(b"hi"); // 'h'@(0,0), 'i'@(0,1), cursor at (0,2)
    t.feed(b"\x1b[?3h"); // DECCOLM
    t.drain_events();
    assert_eq!(
        t.grid().cell(0, 0).c(),
        'h',
        "DECCOLM must not clear the screen"
    );
    t.feed(b"X"); // cursor not homed → lands at (0,2)
    assert_eq!(
        t.grid().cell(0, 2).c(),
        'X',
        "DECCOLM must not home the cursor"
    );
}

#[test]
fn decrqm_reports_column_mode_from_actual_width() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?3$p"); // 80 cols → reset
    assert_eq!(t.drain_replies(), b"\x1b[?3;2$y");
    t.resize(132, 24); // consumer honors the request → now 132 wide
    t.feed(b"\x1b[?3$p"); // 132 cols → set
    assert_eq!(t.drain_replies(), b"\x1b[?3;1$y");
}
