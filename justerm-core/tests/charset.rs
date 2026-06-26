//! DEC character-set / line-drawing tests (#62): SCS designation, SI/SO shifts,
//! and the DEC Special Graphics → Unicode box-drawing translation. Driven through
//! the public API — feed the escape, then read the resolved glyph off the grid.

use justerm_core::Engine;

#[test]
fn dec_special_graphics_translates_q_to_horizontal_line() {
    // ESC ( 0 designates G0 = DEC Special Graphics; 'q' then renders as ─.
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b(0"); // designate G0 = DEC Special Graphics
    t.feed(b"q"); // GL is G0, so 'q' translates
    assert_eq!(t.grid().cell(0, 0).c(), '─');
}

#[test]
fn designating_ascii_stops_translation() {
    // ESC ( B returns G0 to ASCII — 'q' is literal again.
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b(0q"); // ─ at (0,0)
    t.feed(b"\x1b(Bq"); // literal q at (0,1)
    assert_eq!(t.grid().cell(0, 0).c(), '─');
    assert_eq!(t.grid().cell(0, 1).c(), 'q');
}

#[test]
fn si_so_switch_the_active_charset() {
    // G0 = ASCII, G1 = special graphics; SO selects G1, SI selects G0.
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b(B"); // G0 = ASCII
    t.feed(b"\x1b)0"); // G1 = DEC Special Graphics
    t.feed(b"\x0eq"); // SO → GL = G1, 'q' → ─
    assert_eq!(t.grid().cell(0, 0).c(), '─');
    t.feed(b"\x0fq"); // SI → GL = G0 (ASCII), 'q' literal
    assert_eq!(t.grid().cell(0, 1).c(), 'q');
}

#[test]
fn special_graphics_leaves_underscore_untranslated() {
    // xterm.js (and alacritty) omit `_` from the DEC Special Graphics table, so
    // it passes through as a literal underscore — verified against xterm.js's
    // Charsets.ts, not just the strict-DEC "0x5F = blank" reading (#62).
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b(0_"); // special graphics, then `_`
    assert_eq!(t.grid().cell(0, 0).c(), '_');
}

#[test]
fn uk_charset_maps_hash_to_pound() {
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b(A#"); // G0 = UK; '#' → £
    assert_eq!(t.grid().cell(0, 0).c(), '£');
}

#[test]
fn decsc_decrc_save_and_restore_the_charset() {
    // DECSC (ESC 7) saves the charset state; DECRC (ESC 8) restores it, so a
    // glyph after the restore translates with the saved set, not the current one.
    let mut t = Engine::new(10, 2);
    t.feed(b"\x1b(0"); // G0 = DEC Special Graphics
    t.feed(b"\x1b7"); // DECSC: save (G0 = special)
    t.feed(b"\x1b(B"); // G0 = ASCII now
    t.feed(b"\x1b8"); // DECRC: restore (G0 = special again, cursor home)
    t.feed(b"q"); // translates with the restored set → ─
    assert_eq!(t.grid().cell(0, 0).c(), '─');
}
