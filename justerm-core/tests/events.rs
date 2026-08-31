//! Consumer event-surface tests (#12): OSC/BEL → drained events.
//!
//! Drive the whole path — feed the OSC/BEL bytes an app emits, then drain the
//! queue — so both `osc_dispatch`/`execute` and the pull-based queue are
//! covered. Both OSC terminators are exercised: BEL (`0x07`) and ST (`ESC \`).

use justerm_core::{Engine, TermEvent};

#[test]
fn osc2_sets_title_bel_terminated() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;hello\x07");
    assert_eq!(term.drain_events(), vec![TermEvent::Title("hello".into())]);
}

#[test]
fn osc0_sets_title_st_terminated() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]0;world\x1b\\");
    assert_eq!(term.drain_events(), vec![TermEvent::Title("world".into())]);
}

#[test]
fn bel_rings_bell() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x07");
    assert_eq!(term.drain_events(), vec![TermEvent::Bell]);
}

#[test]
fn osc7_reports_cwd() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]7;file://host/home/ki\x07");
    assert_eq!(
        term.drain_events(),
        vec![TermEvent::Cwd("file://host/home/ki".into())]
    );
}

#[test]
fn drain_empties_the_queue() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x07");
    assert_eq!(term.drain_events(), vec![TermEvent::Bell]);
    // A second drain with no new output is empty — events are consumed, not
    // re-reported (the pull counterpart to an ack).
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
}

#[test]
fn events_preserve_stream_order() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;t1\x07\x07\x1b]7;file://h/p\x07");
    assert_eq!(
        term.drain_events(),
        vec![
            TermEvent::Title("t1".into()),
            TermEvent::Bell,
            TermEvent::Cwd("file://h/p".into()),
        ]
    );
}

#[test]
fn osc8_hyperlink_emits_no_event() {
    // OSC 8 is per-cell state (slice #26), not an event surface concern — it
    // must not produce a TermEvent here.
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]8;;https://example.com\x07linked\x1b]8;;\x07");
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
}

#[test]
fn printing_does_not_emit_events() {
    let mut term = Engine::new(80, 24);
    term.feed(b"plain text\r\n");
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
}

// --- XTWINOPS 22/23, the title stack (#823) -------------------------------
//
// Measured on the RHEL 9 VM under real ptys: `vim`, `nvim`, `less`, `man`,
// `htop` and `tmux` all emit this pair, and `vim` is the one that uses the
// axis-limited forms. Every assertion below drives the public API only --
// `Engine::feed` in, `Engine::drain_events` out -- because the stack, its depth
// and the retained strings are implementation, and a test that read them would
// still pass if the event never fired, which is the only thing a consumer sees.

#[test]
fn xtwinops_pop_restores_the_pushed_title() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;sh\x07");
    term.feed(b"\x1b[22;0;0t"); // push both, the literal vim and nvim emit
    term.feed(b"\x1b]2;vi\x07");
    let _ = term.drain_events();
    term.feed(b"\x1b[23;0;0t"); // pop both
    assert_eq!(term.drain_events(), vec![TermEvent::Title("sh".into())]);
}

#[test]
fn xtwinops_nested_pushes_unwind_in_order() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;a\x07\x1b[22t\x1b]2;b\x07\x1b[22t\x1b]2;c\x07");
    let _ = term.drain_events();
    term.feed(b"\x1b[23t\x1b[23t");
    assert_eq!(
        term.drain_events(),
        vec![TermEvent::Title("b".into()), TermEvent::Title("a".into())]
    );
}

#[test]
fn xtwinops_pop_on_an_empty_stack_emits_nothing() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;only\x07");
    let _ = term.drain_events();
    term.feed(b"\x1b[23;0;0t");
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
}

#[test]
fn xtwinops_over_popping_leaves_the_current_title_alone() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;base\x07\x1b[22t\x1b]2;inner\x07");
    let _ = term.drain_events();
    // One push, three pops: the first restores, the rest find nothing and must
    // not resurrect anything or re-report the title they already restored.
    term.feed(b"\x1b[23t\x1b[23t\x1b[23t");
    assert_eq!(term.drain_events(), vec![TermEvent::Title("base".into())]);
}

#[test]
fn xtwinops_the_stack_is_ten_deep_and_drops_the_oldest() {
    let mut term = Engine::new(80, 24);
    // Eleven pushes, each of a distinct title. The bound drops the OLDEST while
    // the push still succeeds -- refusing the push instead would break the
    // pairing for the innermost levels, which are the ones unwound first.
    for i in 0..11 {
        term.feed(format!("\x1b]2;t{i}\x07").as_bytes());
        term.feed(b"\x1b[22t");
    }
    let _ = term.drain_events();
    for _ in 0..11 {
        term.feed(b"\x1b[23t");
    }
    let restored: Vec<TermEvent> = (1..=10)
        .rev()
        .map(|i| TermEvent::Title(format!("t{i}")))
        .collect();
    assert_eq!(term.drain_events(), restored);
}

#[test]
fn xtwinops_an_icon_only_pop_does_not_restore_the_window_title() {
    let mut term = Engine::new(80, 24);
    // vim pushes and pops each axis separately, so the two must not share a
    // stack -- alacritty's dispatch reads only params[0] and gets this wrong.
    term.feed(b"\x1b]2;outer\x07\x1b[22;2t\x1b]2;inner\x07");
    let _ = term.drain_events();
    term.feed(b"\x1b[23;1t"); // pop the ICON axis only
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
    term.feed(b"\x1b[23;2t"); // now the window axis
    assert_eq!(term.drain_events(), vec![TermEvent::Title("outer".into())]);
}

#[test]
fn xtwinops_a_window_only_push_is_not_restored_by_an_icon_only_pop() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;win\x07\x1b[22;2t\x1b]2;other\x07");
    let _ = term.drain_events();
    term.feed(b"\x1b[23;1t\x1b[23;1t\x1b[23;1t");
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
}

#[test]
fn xtwinops_both_spellings_of_the_both_axes_form_agree() {
    // nvim emits BOTH `22;0t` and `22;0;0t` for the same operation, so an
    // absent third parameter and an explicit zero must not diverge.
    let mut two = Engine::new(80, 24);
    two.feed(b"\x1b]2;a\x07\x1b[22;0t\x1b]2;b\x07");
    let _ = two.drain_events();
    two.feed(b"\x1b[23;0t");

    let mut three = Engine::new(80, 24);
    three.feed(b"\x1b]2;a\x07\x1b[22;0;0t\x1b]2;b\x07");
    let _ = three.drain_events();
    three.feed(b"\x1b[23;0;0t");

    assert_eq!(two.drain_events(), vec![TermEvent::Title("a".into())]);
    assert_eq!(three.drain_events(), vec![TermEvent::Title("a".into())]);
}

#[test]
fn xtwinops_a_pop_with_no_title_change_still_reports_the_title() {
    let mut term = Engine::new(80, 24);
    // Nothing changed between the push and the pop, but the consumer is still
    // told what the title now is: the references fire their title notification
    // unconditionally, and a consumer cannot know the stack's contents.
    term.feed(b"\x1b]2;same\x07\x1b[22t");
    let _ = term.drain_events();
    term.feed(b"\x1b[23t");
    assert_eq!(term.drain_events(), vec![TermEvent::Title("same".into())]);
}

#[test]
fn xtwinops_a_push_before_any_title_restores_the_empty_one() {
    let mut term = Engine::new(80, 24);
    // This is the shape every measured application actually emits: the push
    // happens at startup, BEFORE the application sets its own title. Restoring
    // the empty string is what tells the consumer to go back to its default.
    term.feed(b"\x1b[22;0;0t\x1b]2;vim\x07");
    let _ = term.drain_events();
    term.feed(b"\x1b[23;0;0t");
    assert_eq!(term.drain_events(), vec![TermEvent::Title(String::new())]);
}

#[test]
fn xtwinops_an_unrecognised_axis_acts_on_no_axis() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;base\x07\x1b[22;3t\x1b]2;inner\x07");
    let _ = term.drain_events();
    term.feed(b"\x1b[23;0t"); // nothing was ever pushed
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
}

#[test]
fn xtwinops_the_third_parameter_is_ignored() {
    // DELIBERATE DIVERGENCE from the spec, pinned here so it cannot drift
    // silently. ctlseqs.txt:1698 gives the third parameter (1..10) direct slot
    // access WITHOUT pushing or popping, and xterm implements it. justerm
    // treats it as an ordinary push/pop -- measured reach of the slot form is
    // zero across seven programs, all of /usr/bin and /usr/lib64, every
    // candidate terminfo, and every other implementation. See #823.
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;base\x07\x1b[22;2;3t\x1b]2;inner\x07");
    let _ = term.drain_events();
    term.feed(b"\x1b[23;2;3t");
    assert_eq!(term.drain_events(), vec![TermEvent::Title("base".into())]);
}

#[test]
fn xtwinops_a_reset_between_push_and_pop_makes_the_pop_a_no_op() {
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;before\x07\x1b[22t");
    term.feed(b"\x1bc"); // RIS -- the stack is terminal state and dies with it
    let _ = term.drain_events();
    term.feed(b"\x1b[23t");
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
}

#[test]
fn xtwinops_the_alt_screen_does_not_disturb_the_title_stack() {
    let mut term = Engine::new(80, 24);
    // The title is not buffer state, so swapping grids must leave it alone --
    // every measured application pushes, enters the alt screen, leaves it and
    // then pops.
    term.feed(b"\x1b]2;shell\x07\x1b[22;0;0t\x1b]2;vim\x07");
    term.feed(b"\x1b[?1049h");
    term.feed(b"\x1b[?1049l");
    let _ = term.drain_events();
    term.feed(b"\x1b[23;0;0t");
    assert_eq!(term.drain_events(), vec![TermEvent::Title("shell".into())]);
}

#[test]
fn other_window_operations_remain_ignored() {
    // This slice recognises two operations inside `CSI t`, not `CSI t` itself.
    // A regression here would mean the new arm swallowed its neighbours.
    let mut term = Engine::new(80, 24);
    term.feed(b"\x1b]2;kept\x07");
    let _ = term.drain_events();
    term.feed(b"\x1b[18t\x1b[14t\x1b[11t\x1b[24t\x1b[t");
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
    assert_eq!(term.drain_replies(), Vec::<u8>::new());
    // Asserting "no event" cannot see the failure this guards against: a push
    // is SILENT, so an unknown operation that fell through to one would look
    // identical here. The pop is what makes the stack observable — if any of
    // the five above pushed, this restores a title and emits.
    term.feed(b"\x1b[23t");
    assert_eq!(term.drain_events(), Vec::<TermEvent>::new());
}
