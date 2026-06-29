//! #122 dynamic colour — OSC 4/10/11 (set/query) and 104/110/111 (reset) relayed
//! as `TermEvent`s for the theme-aware consumer to apply, mirroring the
//! `ColorSchemeQuery` pattern. The engine stays theme-agnostic: it forwards the
//! raw spec string (never parses hex), cells keep their `Indexed` references, and
//! a query is answered by the consumer via a `report_*` reply. Both OSC
//! terminators (BEL `0x07`, ST `ESC \`) are exercised. xterm.js cross-checked.

use justerm_core::{Color, Engine, TermEvent};

/// OSC 11 sets the default background — the engine forwards the raw spec, not a
/// parsed colour (it holds no palette; the consumer applies it).
/// `printf '\033]11;#1e1e2e\033\\'`.
#[test]
fn osc11_sets_default_background() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]11;#1e1e2e\x07");
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::SetBackground("#1e1e2e".into())]
    );
}

/// OSC 10 sets the default foreground — same forward-the-raw-spec shape, ST
/// terminator + `rgb:` form here.
#[test]
fn osc10_sets_default_foreground() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]10;rgb:ff/00/00\x1b\\");
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::SetForeground("rgb:ff/00/00".into())]
    );
}

/// OSC 4 sets one ANSI palette entry `index` to `spec`. The cell still
/// references `Indexed(index)`; only the consumer's palette[index] changes.
#[test]
fn osc4_sets_a_palette_color() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]4;1;rgb:ff/00/00\x07");
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::SetPaletteColor {
            index: 1,
            spec: "rgb:ff/00/00".into()
        }]
    );
}

/// OSC 4 with a `?` spec for an index is a palette query (per pair), answered via
/// `report_palette_color` — distinct from a set on that index.
#[test]
fn osc4_query_and_report_palette_color() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]4;5;?\x07");
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::QueryPaletteColor { index: 5 }]
    );

    t.report_palette_color(5, "rgb:00/00/ff");
    assert_eq!(t.drain_replies(), b"\x1b]4;5;rgb:00/00/ff\x1b\\");
}

/// OSC 104 resets palette entries to the theme default: no argument resets the
/// whole table (None), `104 ; i ; j` resets each named index.
#[test]
fn osc104_resets_palette_entries() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]104\x07"); // no arg → reset all
    assert_eq!(t.drain_events(), vec![TermEvent::ResetPaletteColor(None)]);

    t.feed(b"\x1b]104;1;2\x07"); // reset specific indices
    assert_eq!(
        t.drain_events(),
        vec![
            TermEvent::ResetPaletteColor(Some(1)),
            TermEvent::ResetPaletteColor(Some(2)),
        ]
    );
}

/// OSC 110 / 111 reset the default foreground / background to the theme default.
#[test]
fn osc110_111_reset_default_fg_bg() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]110\x07");
    assert_eq!(t.drain_events(), vec![TermEvent::ResetForeground]);

    t.feed(b"\x1b]111\x07");
    assert_eq!(t.drain_events(), vec![TermEvent::ResetBackground]);
}

/// A `?` spec is a QUERY, not a set: the theme-agnostic engine doesn't know the
/// colour, so it relays a query event for the consumer to answer (like
/// `ColorSchemeQuery`) — it must not be mistaken for `SetBackground("?")`.
#[test]
fn osc11_query_emits_a_query_event() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]11;?\x07");
    assert_eq!(t.drain_events(), vec![TermEvent::QueryBackground]);
}

/// The consumer answers a background query by handing back the spec; the engine
/// wraps it in the OSC 11 reply envelope (ST-terminated), mirroring
/// `report_color_scheme`. The spec value is the consumer's (it knows its
/// palette); only the envelope is the engine's.
#[test]
fn report_background_queues_the_osc11_reply() {
    let mut t = Engine::new(80, 24);
    t.report_background("rgb:1e/1e/2e");
    assert_eq!(t.drain_replies(), b"\x1b]11;rgb:1e/1e/2e\x1b\\");
}

/// OSC 10 `?` is a foreground query, answered via `report_foreground`.
#[test]
fn osc10_query_and_report_foreground() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]10;?\x07");
    assert_eq!(t.drain_events(), vec![TermEvent::QueryForeground]);

    t.report_foreground("rgb:ff/ff/ff");
    assert_eq!(t.drain_replies(), b"\x1b]10;rgb:ff/ff/ff\x1b\\");
}

/// OSC 4 carries multiple `index ; spec` pairs in one sequence — each becomes its
/// own event (xterm's `while slots > 1` pair loop).
#[test]
fn osc4_sets_multiple_palette_colors_in_one_sequence() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]4;1;rgb:ff/00/00;2;rgb:00/ff/00\x07");
    assert_eq!(
        t.drain_events(),
        vec![
            TermEvent::SetPaletteColor {
                index: 1,
                spec: "rgb:ff/00/00".into()
            },
            TermEvent::SetPaletteColor {
                index: 2,
                spec: "rgb:00/ff/00".into()
            },
        ]
    );
}

// --- theme-agnostic guard + parsing edges (spike-promoted) ---

/// The identity guard: an OSC 4 set must NOT change how the engine represents a
/// cell. A glyph in indexed colour 1 still serializes as `Indexed(1)` — the
/// engine never applied (or even parsed) the palette value, so it stays
/// theme-agnostic. Only the consumer's palette[1] changes, off to the side.
#[test]
fn osc4_set_leaves_cells_as_indexed_references() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]4;1;rgb:ff/00/00\x07"); // app redefines palette entry 1
    let _ = t.drain_events();

    t.feed(b"\x1b[31mX"); // SGR 31 → fg = indexed 1, print 'X'
    let frame = t.frame();
    assert_eq!(frame.spans[0].cells[0].fg(), Color::Indexed(1)); // not Rgb
}

/// A `?` may appear mid multi-pair: OSC 4 sets one entry and queries another in
/// the same sequence — each pair is classified independently.
#[test]
fn osc4_mixes_set_and_query_per_pair() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]4;1;rgb:ff/00/00;2;?\x07");
    assert_eq!(
        t.drain_events(),
        vec![
            TermEvent::SetPaletteColor {
                index: 1,
                spec: "rgb:ff/00/00".into()
            },
            TermEvent::QueryPaletteColor { index: 2 },
        ]
    );
}

/// Malformed OSC 4 fields are dropped, never panic: an out-of-range index
/// (`999`), a non-numeric index, and a dangling index with no spec all yield no
/// events.
#[test]
fn osc4_malformed_fields_are_dropped() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]4;999;red\x07"); // > u8
    t.feed(b"\x1b]4;notanum;red\x07"); // non-numeric index
    t.feed(b"\x1b]4;1\x07"); // dangling index, no spec
    assert_eq!(t.drain_events(), vec![]);
}

/// Raw-forward is format-agnostic: the engine never parses the spec, so every
/// XParseColor form (16-bit `rgb:`, `#RRGGBB`, long `#hex`) reaches the consumer
/// verbatim for it to interpret.
#[test]
fn spec_forms_pass_through_verbatim() {
    for (seq, spec) in [
        (
            b"\x1b]11;rgb:1e1e/1e1e/2e2e\x07".as_slice(),
            "rgb:1e1e/1e1e/2e2e",
        ),
        (b"\x1b]11;#1e1e2e\x07".as_slice(), "#1e1e2e"),
    ] {
        let mut t = Engine::new(80, 24);
        t.feed(seq);
        assert_eq!(
            t.drain_events(),
            vec![TermEvent::SetBackground(spec.into())]
        );
    }
}

/// OSC 10 stacks its `;`-separated specs across [fg, bg] (xterm's offset loop):
/// `OSC 10 ; a ; b` sets foreground=a then background=b in one sequence (#137).
#[test]
fn osc10_stacks_fg_then_bg() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]10;rgb:11/11/11;rgb:22/22/22\x07");
    assert_eq!(
        t.drain_events(),
        vec![
            TermEvent::SetForeground("rgb:11/11/11".into()),
            TermEvent::SetBackground("rgb:22/22/22".into()),
        ]
    );
}

/// OSC 11 starts at the background slot, so a third spec would be the cursor —
/// out of scope. `OSC 11 ; a ; b` sets background=a and drops b (cursor cap).
#[test]
fn osc11_stacks_from_bg_and_caps_at_cursor() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]11;rgb:11/11/11;rgb:22/22/22\x07");
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::SetBackground("rgb:11/11/11".into())] // cursor slot dropped
    );
}

/// A `?` works per slot inside a stack: `OSC 10 ; fg ; ?` sets the foreground and
/// queries the background.
#[test]
fn osc10_stack_mixes_set_and_query_per_slot() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]10;rgb:11/11/11;?\x07");
    assert_eq!(
        t.drain_events(),
        vec![
            TermEvent::SetForeground("rgb:11/11/11".into()),
            TermEvent::QueryBackground,
        ]
    );
}
