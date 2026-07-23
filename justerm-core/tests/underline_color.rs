//! SGR 58 / 59 — coloured underlines (#520). The underline draws in its own
//! colour, decoupled from the glyph fg; SGR 59 (and a full SGR reset) return it
//! to "follow the fg". The colour is a theme-agnostic reference (like fg/bg): the
//! engine stores `Default`/`Indexed`/`Rgb` and never resolves it.
//!
//! Driven through the public API — feed SGR + text, read the colour via
//! `Engine::underline_color_at` (it rides a per-row map, not the 12-byte cell,
//! mirroring the hyperlink map #46).

use justerm_core::{Color, Engine};

/// Feed a byte stream and return a fresh engine holding the result.
fn run(bytes: &[u8]) -> Engine {
    let mut t = Engine::new(80, 24);
    t.feed(bytes);
    t
}

#[test]
fn sgr58_rgb_colon_empty_colourspace_sets_the_underline_colour() {
    // `58:2::R:G:B` — the nvim/kitty form, with the empty colour-space field.
    let t = run(b"\x1b[4:3m\x1b[58:2::255:0:0mX");
    assert_eq!(t.grid().cell(0, 0).c(), 'X');
    assert_eq!(t.underline_color_at(0, 0), Color::Rgb(255, 0, 0));
}

#[test]
fn sgr58_rgb_colon_with_colourspace_id_skips_the_id() {
    // `58:2:C:R:G:B` — a colour-space id is present and must be skipped, exactly
    // as `parse_extended_color` already does for 38/48. (`4m` arms the underline so
    // the colour is stored — see the gating test below.)
    let t = run(b"\x1b[4m\x1b[58:2:1:0:200:0mX");
    assert_eq!(t.underline_color_at(0, 0), Color::Rgb(0, 200, 0));
}

#[test]
fn sgr58_indexed_colon_and_semicolon_agree() {
    let colon = run(b"\x1b[4m\x1b[58:5:9mX");
    let semi = run(b"\x1b[4m\x1b[58;5;9mX");
    assert_eq!(colon.underline_color_at(0, 0), Color::Indexed(9));
    assert_eq!(semi.underline_color_at(0, 0), Color::Indexed(9));
}

#[test]
fn sgr58_rgb_legacy_semicolon_sets_the_underline_colour() {
    let t = run(b"\x1b[4m\x1b[58;2;0;128;255mX");
    assert_eq!(t.underline_color_at(0, 0), Color::Rgb(0, 128, 255));
}

#[test]
fn the_colour_is_stored_only_where_an_underline_is_drawn() {
    // xterm parity (#520, Lens ②): an underline colour is meaningless on a cell that
    // draws no underline, and xterm does not persist it there (`AttributeData
    // isEmpty()` ignores the colour). So a colour armed with NO underline attribute
    // is not stored; arming the underline makes the same colour apply. This also
    // keeps the colour off the slice-2 wire for cells that never draw it (ADR-0020).
    let t = run(b"\x1b[58:2::255:0:0mA\x1b[4mB");
    assert_eq!(
        t.underline_color_at(0, 0),
        Color::Default,
        "colour armed but no underline drawn → not stored (follows fg)"
    );
    assert_eq!(
        t.underline_color_at(0, 1),
        Color::Rgb(255, 0, 0),
        "same armed colour, now with an underline → applied"
    );
}

#[test]
fn strikethrough_alone_does_not_arm_the_underline_colour() {
    // SGR 58 is the *underline* colour; a strikethrough-only cell (no underline)
    // does not carry it — it would strike in the fg.
    let t = run(b"\x1b[9m\x1b[58:2::255:0:0mX");
    assert_eq!(t.underline_color_at(0, 0), Color::Default);
}

#[test]
fn the_underline_colour_is_independent_of_the_glyph_fg() {
    // White text, red underline — the whole point of SGR 58: fg and underline
    // colour differ on one cell.
    let t = run(b"\x1b[38;2;255;255;255m\x1b[4m\x1b[58:2::255:0:0mX");
    assert_eq!(t.grid().cell(0, 0).fg(), Color::Rgb(255, 255, 255));
    assert_eq!(t.underline_color_at(0, 0), Color::Rgb(255, 0, 0));
}

#[test]
fn sgr59_resets_the_underline_colour_to_default() {
    // Underline stays on throughout; 59 resets only the colour, so the second glyph
    // underlines in the fg again (matches xterm: 59 touches colour only).
    let t = run(b"\x1b[4m\x1b[58:2::255:0:0mA\x1b[59mB");
    assert_eq!(t.underline_color_at(0, 0), Color::Rgb(255, 0, 0));
    assert_eq!(
        t.underline_color_at(0, 1),
        Color::Default,
        "59 returns to follow-the-fg"
    );
}

#[test]
fn sgr_reset_clears_the_underline_colour() {
    // A full SGR reset (0) drops the underline colour (and the underline) like every
    // other attribute.
    let t = run(b"\x1b[4m\x1b[58:2::255:0:0mA\x1b[0m\x1b[4mB");
    assert_eq!(t.underline_color_at(0, 0), Color::Rgb(255, 0, 0));
    assert_eq!(t.underline_color_at(0, 1), Color::Default);
}

#[test]
fn a_cell_with_no_sgr58_has_a_default_underline_colour() {
    let t = run(b"plain");
    assert_eq!(t.underline_color_at(0, 0), Color::Default);
}

#[test]
fn the_vm_captured_form_matrix_round_trips_every_encoding() {
    // Step-4 dogfood proof: a REAL byte stream captured on the Linux VM
    // (`tests/fixtures/capture-undercurl.sh`), carrying SGR 58 in all six encoding
    // forms. Each row is one colour-form; assert the first glyph of each coloured
    // word resolves to the right reference — this is the whole matrix parsed at once,
    // not a synthetic input I hand-wrote to match the parser.
    let t = run(include_bytes!("fixtures/undercurl_matrix.raw"));

    // Row 0: `58:2::255:0:0` (RGB, colon, empty colour-space) on "misspeled".
    assert_eq!(t.grid().cell(0, 0).c(), 'm');
    assert_eq!(t.underline_color_at(0, 0), Color::Rgb(255, 0, 0));
    // Row 1: `58:2:1:0:200:0` (RGB, colon, colour-space id 1 skipped) on "warnign".
    assert_eq!(t.underline_color_at(1, 0), Color::Rgb(0, 200, 0));
    // Row 2: `58:5:9` (indexed, colon) on "eror".
    assert_eq!(t.underline_color_at(2, 0), Color::Indexed(9));
    // Row 3: `58;2;0;128;255` (RGB, legacy semicolon) on "hyperlink".
    assert_eq!(t.underline_color_at(3, 0), Color::Rgb(0, 128, 255));
    // Row 4: `58;5;46` (indexed, legacy semicolon) on "hint".
    assert_eq!(t.underline_color_at(4, 0), Color::Indexed(46));
    // Row 5: `58:2::128:64:255` (RGB colon, empty cs) again. NOTE: the matrix uses
    // `4:0m` between rows to mean "underline off", but justerm parses `4:0` as code 4
    // → underline ON (the `:0` sub-parameter is the separate underline-*style* sibling,
    // out of #520's scope), so the underline is sticky-on across the capture. The
    // colour therefore stores here — the gating itself is proven by
    // `the_colour_is_stored_only_where_an_underline_is_drawn`, which controls the
    // underline explicitly with `4m`.
    assert_eq!(t.grid().cell(5, 0).c(), 'n');
    assert_eq!(t.underline_color_at(5, 0), Color::Rgb(128, 64, 255));
    // Row 6: fg and underline colour DIFFER — white text, red curl.
    assert_eq!(t.grid().cell(6, 0).fg(), Color::Rgb(255, 255, 255));
    assert_eq!(t.underline_color_at(6, 0), Color::Rgb(255, 0, 0));

    // ...and after each `59` the trailing label text returns to a default underline
    // colour — the reset really fires, it is not that every cell got a colour.
    let last = t.grid().cell(0, 20).c(); // somewhere in "  (58:2:: rgb curly)"
    assert_ne!(last, ' ', "row 0 has a trailing label past the word");
    assert_eq!(t.underline_color_at(0, 20), Color::Default);
}

#[test]
fn a_glyph_carries_its_link_and_underline_colour_together_through_an_ich_shift() {
    // Lens ② #4: the underline colour and the hyperlink live in SEPARATE per-row maps
    // here (xterm shares one object), so every cell-moving op must shift them in
    // lockstep. ICH (`move_maps`) shifts a linked + coloured glyph right by one; both
    // must arrive at the new column, and the vacated column must carry neither.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[4m\x1b[58:5:1m"); // underline on + indexed-1 underline colour
    t.feed(b"\x1b]8;;http://x\x07A\x1b]8;;\x07"); // 'A' under an open hyperlink
    assert_eq!(t.underline_color_at(0, 0), Color::Indexed(1));
    assert!(t.link_at(0, 0).is_some());

    t.feed(b"\x1b[1G\x1b[1@"); // cursor to col 0, insert one blank → shift 'A' to col 1
    assert_eq!(t.grid().cell(0, 1).c(), 'A', "A shifted right by ICH");
    assert_eq!(
        t.underline_color_at(0, 1),
        Color::Indexed(1),
        "colour moved with the glyph"
    );
    assert!(t.link_at(0, 1).is_some(), "link moved with the glyph too");
    assert_eq!(
        t.underline_color_at(0, 0),
        Color::Default,
        "vacated column carries no colour"
    );
    assert_eq!(t.link_at(0, 0), None, "vacated column carries no link");
}

#[test]
fn the_underline_colour_is_stamped_onto_both_halves_of_a_wide_glyph() {
    // A wide glyph occupies two cells; the underline colour must be on both, so a
    // renderer draws one continuous coloured underline (mirrors the link stamp).
    let t = run("\x1b[4m\x1b[58:2::255:0:0m中".as_bytes());
    assert_eq!(t.underline_color_at(0, 0), Color::Rgb(255, 0, 0));
    assert_eq!(
        t.underline_color_at(0, 1),
        Color::Rgb(255, 0, 0),
        "the wide glyph's spacer half carries it too"
    );
}
