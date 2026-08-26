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

/// OSC 11 starts at the background slot, so its second spec is the cursor:
/// `OSC 11 ; a ; b` sets background=a **and** cursor=b.
///
/// **This assertion was inverted by #832, deliberately.** It shipped as
/// `osc11_stacks_from_bg_and_caps_at_cursor`, pinning `b` as *dropped*. That drop
/// was never a decision about OSC 11's second slot — it was a consequence of
/// #137 capping the stack at two slots, and #137 gave its reason as "beamterm has
/// no cursor concept yet". beamterm was replaced by `justerm-renderer` under
/// ADR-0018, which has had a cursor colour since #368, so the reason the cap
/// rested on no longer holds. xterm walks the same loop from each sequence's own
/// offset with no cap at the second slot (`misc.c:3679`), so removing ours
/// restores the reference behaviour rather than inventing one.
#[test]
fn osc11_stacks_from_bg_into_the_cursor_slot() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]11;rgb:11/11/11;rgb:22/22/22\x07");
    assert_eq!(
        t.drain_events(),
        vec![
            TermEvent::SetBackground("rgb:11/11/11".into()),
            TermEvent::SetCursorColor("rgb:22/22/22".into()),
        ]
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

// --- OSC 12 / 112, the cursor slot (#832) ---

/// **The degenerate form is the one real applications emit**, so it is the first
/// test here. `nvim` 0.8.0 emits `ESC ] 12 ; BEL` — an empty spec, no colour —
/// four to five times in a six-second session. An empty field addresses the slot
/// and leaves it alone: it is neither a set nor a reset (xterm `misc.c:3687`,
/// where a separator appearing where a name should be yields a null name, and
/// only a non-null name is set or queried).
///
/// Without this rule, opening an editor fires four spurious cursor-colour
/// changes each carrying an empty string.
#[test]
fn osc12_with_an_empty_spec_relays_nothing() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]12;\x07"); // the captured bytes, verbatim
    assert_eq!(t.drain_events(), vec![]);
    assert_eq!(t.drain_replies(), b""); // and it is not a query either
}

/// OSC 12 with no field at all — not even a separator — is the same non-event.
/// This is xterm's other empty case (`misc.c:3684-3685`, nothing left in the string).
#[test]
fn osc12_with_no_field_at_all_relays_nothing() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]12\x07");
    assert_eq!(t.drain_events(), vec![]);
}

/// OSC 12 sets the cursor colour — the raw spec forwarded, like fg and bg. The
/// engine holds no colour here either; the consumer owns the palette.
#[test]
fn osc12_sets_the_cursor_colour() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]12;#ff0000\x07");
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::SetCursorColor("#ff0000".into())]
    );
}

/// OSC 12 `?` is a query the consumer answers, mirroring OSC 10/11. The reply
/// envelope is the engine's; the spec is the consumer's.
#[test]
fn osc12_query_and_report_cursor_color() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]12;?\x1b\\");
    assert_eq!(t.drain_events(), vec![TermEvent::QueryCursorColor]);

    t.report_cursor_color("rgb:ff/00/00");
    assert_eq!(t.drain_replies(), b"\x1b]12;rgb:ff/00/00\x1b\\");
}

/// OSC 112 puts the cursor colour back, the third member of the 110/111/112
/// reset family. `nvim` emits it six times in a default session.
#[test]
fn osc112_resets_the_cursor_colour() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]112\x07");
    assert_eq!(t.drain_events(), vec![TermEvent::ResetCursorColor]);

    t.feed(b"\x1b]112\x1b\\"); // both terminators
    assert_eq!(t.drain_events(), vec![TermEvent::ResetCursorColor]);
}

// --- the empty-spec rule, on the slots that already shipped (#832) ---

/// The same rule on the foreground and background slots, which share the
/// handler. This is a **fix to shipped behaviour**: before #832 these relayed
/// `SetForeground("")` / `SetBackground("")`, handing the consumer an empty
/// string to parse as a colour.
#[test]
fn osc10_and_osc11_with_an_empty_spec_relay_nothing() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]10;\x07");
    t.feed(b"\x1b]11;\x07");
    assert_eq!(t.drain_events(), vec![]);
}

/// An empty field **skips its slot and still advances to the next** — it does not
/// end the stack. So `OSC 10 ; ; <spec>` is the documented way to address the
/// background alone, and it must relay a background change and *no* foreground
/// change (xterm `misc.c:3687-3692`: the null name is skipped, then the parse
/// steps past the separator).
#[test]
fn osc10_an_empty_first_field_addresses_the_background_alone() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]10;;#112233\x07");
    assert_eq!(
        t.drain_events(),
        vec![TermEvent::SetBackground("#112233".into())]
    );
}

// --- the stack, now three slots deep (#832) ---

/// One sequence can fill all three slots, which is what removing the two-slot cap
/// buys. xterm walks `[fg, bg, cursor, …]` from the sequence's own offset
/// (`misc.c:3679-3696`; the slot order is `OSC_TEXT_FG = 10` then BG then CURSOR,
/// `ptyx.h:1018-1020`).
#[test]
fn osc10_stacks_fg_bg_then_cursor() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]10;rgb:11/11/11;rgb:22/22/22;rgb:33/33/33\x07");
    assert_eq!(
        t.drain_events(),
        vec![
            TermEvent::SetForeground("rgb:11/11/11".into()),
            TermEvent::SetBackground("rgb:22/22/22".into()),
            TermEvent::SetCursorColor("rgb:33/33/33".into()),
        ]
    );
}

/// The stack still ends somewhere: xterm's next slots are the pointer colours
/// (`OSC_MOUSE_FG` = 13, `OSC_MOUSE_BG` = 14), which justerm does not model, so a
/// fourth spec is dropped rather than mis-addressed.
#[test]
fn osc10_stack_stops_after_the_cursor_slot() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]10;a;b;c;d\x07");
    assert_eq!(
        t.drain_events(),
        vec![
            TermEvent::SetForeground("a".into()),
            TermEvent::SetBackground("b".into()),
            TermEvent::SetCursorColor("c".into()),
        ]
    );
}

/// A `?` works in the cursor slot inside a stack, like every other slot.
#[test]
fn osc11_stack_queries_the_cursor_slot() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b]11;rgb:11/11/11;?\x07");
    assert_eq!(
        t.drain_events(),
        vec![
            TermEvent::SetBackground("rgb:11/11/11".into()),
            TermEvent::QueryCursorColor,
        ]
    );
}
