//! #158 (A1) — OSC 133 command-detection: parsing `133;A/B/C/D` into semantic
//! command marks (kind + optional exit) on the #118 marker primitive. Core VT
//! mechanism only; the wire (#159) and web nav/announce (#160) are follow-ups.
//!
//! Hidden-state matrix grounded in VSCode `commandDetectionCapability.ts`:
//! exit-less D, empty/invalid exit, D-without-B/C, alt-screen suppression,
//! nested/duplicate A, unknown subcommand, plain-vs-command marker separation.

use justerm_core::{Engine, MarkerKind};

/// Collect just the kinds of the command marks, in buffer order.
fn kinds(t: &Engine) -> Vec<MarkerKind> {
    t.command_marks().into_iter().map(|(_, _, k)| k).collect()
}

/// A full prompt→command→output→finished cycle records four kinded marks in
/// order, the last carrying the parsed exit code. This is the shape VSCode's
/// CommandDetection builds a `TerminalCommand` from.
#[test]
fn full_cycle_records_four_kinded_marks() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;A\x07user$ \x1b]133;B\x07ls\x1b]133;C\x07out\r\n\x1b]133;D;0\x07");

    assert_eq!(
        kinds(&t),
        vec![
            MarkerKind::PromptStart,
            MarkerKind::CommandStart,
            MarkerKind::OutputStart,
            MarkerKind::CommandFinished(Some(0)),
        ]
    );
}

/// `D` with no exit field → `CommandFinished(None)`. VSCode leaves exitCode
/// `undefined` here (bar a bash-history hack we don't replicate).
#[test]
fn finished_without_exit_is_none() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;D\x07");

    assert_eq!(kinds(&t), vec![MarkerKind::CommandFinished(None)]);
}

/// A non-zero exit (e.g. 130 = SIGINT) round-trips as the parsed integer.
#[test]
fn finished_parses_nonzero_exit() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;D;130\x07");

    assert_eq!(kinds(&t), vec![MarkerKind::CommandFinished(Some(130))]);
}

/// An empty (`D;`) or non-numeric exit field is treated as absent, not a crash
/// or a bogus code.
#[test]
fn finished_empty_or_invalid_exit_is_none() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;D;\x07");
    t.feed(b"\x1b]133;D;abc\x07");

    assert_eq!(
        kinds(&t),
        vec![
            MarkerKind::CommandFinished(None),
            MarkerKind::CommandFinished(None),
        ]
    );
}

/// Each mark anchors at the cursor's current line, so the marks track where the
/// prompt/command/output actually sit as the cursor advances down the screen.
#[test]
fn marks_anchor_at_cursor_line() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;A\x07"); // cursor at row 0
    t.feed(b"cmd\r\n"); // cursor now at row 1
    t.feed(b"\x1b]133;C\x07");

    let marks = t.command_marks();
    assert_eq!(marks[0].1, 0, "PromptStart anchors on row 0");
    assert_eq!(marks[1].1, 1, "OutputStart anchors on row 1");
}

/// Alt-screen apps (vim, less) don't emit shell-integration marks. A 133 on the
/// alt screen is ignored — not for the old aliasing reason (per-buffer storage,
/// #186/#187, retired that) but on a *semantic*: OSC 133 is primary-screen shell
/// integration, so an alt 133 marks nothing meaningful (#188).
#[test]
fn alt_screen_marks_are_ignored() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b[?1049h"); // enter alt screen
    t.feed(b"\x1b]133;A\x07\x1b]133;D;0\x07");

    assert!(t.command_marks().is_empty());
}

/// #188: command navigation is primary-scoped. Even while on the alt screen with
/// an alt decoration marker present (#187), `command_marks`/`command_lines` read
/// the *normal* buffer, so the primary command history stays intact for nav
/// (#166) / announce (#160) and the alt marker never leaks into it.
#[test]
fn command_nav_is_primary_scoped_on_alt() {
    let mut t = Engine::new(40, 24);
    // A primary command cycle: `ls`, exit 0.
    t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07\r\n\x1b]133;D;0\x07");

    // Enter alt and drop a decoration marker there.
    t.feed(b"\x1b[?1049h");
    let _alt = t.add_marker(2);

    // command_marks shows ONLY the primary command's marks — the alt decoration
    // marker (a Plain kind in the alt buffer) never leaks in.
    assert_eq!(
        t.command_marks()
            .into_iter()
            .map(|(_, _, k)| k)
            .collect::<Vec<_>>(),
        vec![
            MarkerKind::PromptStart,
            MarkerKind::CommandStart,
            MarkerKind::OutputStart,
            MarkerKind::CommandFinished(Some(0)),
        ],
    );
    // command_lines is entry-scoped to the primary too: exactly the one primary
    // command, with its exit from the D mark.
    let cmds = t.command_lines();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].exit, Some(0));
    // NB: the command *text* is not asserted here — while on the alt screen
    // `extract_lines` reads the active (alt) grid, so it comes back empty. That is
    // a separate, pre-existing bug tracked as #192; the mark/entry scoping this
    // test locks in is unaffected by it.
}

/// An unknown 133 subcommand (future FinalTerm letters, typos) is ignored, not
/// mapped to a bogus kind.
#[test]
fn unknown_subcommand_is_ignored() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;Z\x07");
    t.feed(b"\x1b]133\x07"); // no subcommand field at all

    assert!(t.command_marks().is_empty());
}

/// Back-to-back prompt starts (a redraw after ctrl+l, or a shell that re-emits
/// A) each record a mark — the model is a flat stream of boundaries; pairing A
/// with its D is the consumer's job (#160), not core's.
#[test]
fn nested_prompt_starts_each_record() {
    let mut t = Engine::new(40, 24);
    t.feed(b"\x1b]133;A\x07\x1b]133;A\x07");

    assert_eq!(
        kinds(&t),
        vec![MarkerKind::PromptStart, MarkerKind::PromptStart]
    );
}

/// A primary-screen command mark must survive an alt-screen app that *scrolls*
/// (vim/less/htop) — the whole reason prompt marks exist. Regression for the
/// cross-buffer collision (#158): the alt grid occupies the same absolute-line
/// range as the primary marks, so an alt-screen region scroll used to rotate /
/// dispose primary marks even though primary content never moved.
#[test]
fn command_marks_survive_alt_screen_scroll() {
    let mut t = Engine::new(20, 5);
    t.feed(b"\x1b]133;A\x07"); // PromptStart at abs line 0 on the primary screen
    t.feed(b"\x1b[?1049h"); // enter the alt screen
    t.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\n"); // scroll the alt screen repeatedly
    t.feed(b"\x1b[?1049l"); // leave the alt screen

    let marks = t.command_marks();
    assert_eq!(marks.len(), 1, "primary mark survives the alt excursion");
    assert_eq!(marks[0].1, 0, "still anchored at abs line 0");
    assert_eq!(marks[0].2, MarkerKind::PromptStart);
}

/// The reverse-index path (RI at the top margin) is guarded too, not just
/// forward linefeed — a primary mark survives an alt app scrolling *up* (e.g.
/// `less` paging backward). Covers the second `markers_rotate_region` call site.
#[test]
fn command_marks_survive_alt_screen_reverse_scroll() {
    let mut t = Engine::new(20, 5);
    t.feed(b"\x1b]133;A\x07"); // PromptStart at abs line 0 (primary)
    t.feed(b"\x1b[?1049h"); // enter alt
    t.feed(b"\x1b[H"); // cursor home = scroll_top, so RI scrolls the region
    t.feed(b"\x1bM\x1bM\x1bM\x1bM\x1bM\x1bM"); // RI x6 → alt region scrolls down
    t.feed(b"\x1b[?1049l"); // leave alt

    let marks = t.command_marks();
    assert_eq!(marks.len(), 1, "primary mark survives reverse alt scroll");
    assert_eq!(marks[0].1, 0, "still anchored at abs line 0");
}

/// Plain decoration markers (#118 `add_marker`) are not command marks — they
/// carry no OSC-133 semantics and stay out of `command_marks`. Conversely a 133
/// mark is reported. The two marker sources share the anchor machinery but not
/// the query.
#[test]
fn plain_markers_excluded_command_marks_included() {
    let mut t = Engine::new(40, 24);
    t.feed(b"hello\r\n");
    let _plain = t.add_marker(0);
    assert!(
        t.command_marks().is_empty(),
        "plain marker is not a command mark"
    );

    t.feed(b"\x1b]133;A\x07");
    assert_eq!(t.command_marks().len(), 1, "133 mark is a command mark");
}
