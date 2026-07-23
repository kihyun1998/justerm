//! DECSET mode 2027 grapheme-cluster segmentation (#295, subsumes #301).
//!
//! Mode 2027 (default OFF) opts an application into UAX #29 grapheme-cluster width: a ZWJ
//! family / skin-tone / flag / emoji+VS16 sequence is clustered into ONE cell (its full text in
//! the side-table) instead of one cell per scalar. OFF preserves the per-char (wcwidth-compatible)
//! behaviour verbatim, so the cursor stays in sync with wcwidth apps (the reason clustering is
//! opt-in — cf. #301). Verified against ghostty/kitty (both gate clustering behind ?2027).

use justerm_core::{Color, Engine, decode, encode};

/// OSC 8 open, for the extended-attr carry tests below.
const LINK_OPEN: &[u8] = b"\x1b]8;;https://example.com\x07";
/// Mode 2027 on, underline armed, underline colour = indexed 1, hyperlink open.
const EXT_ATTR_PEN: &[u8] = b"\x1b[?2027h\x1b[4m\x1b[58:5:1m";

#[test]
fn mode_2027_tracked_and_decrqm_reports_it() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2027$p"); // off → reset
    assert_eq!(t.drain_replies(), b"\x1b[?2027;2$y");
    t.feed(b"\x1b[?2027h\x1b[?2027$p"); // on → set
    assert_eq!(t.drain_replies(), b"\x1b[?2027;1$y");
    t.feed(b"\x1b[?2027l\x1b[?2027$p"); // back off → reset
    assert_eq!(t.drain_replies(), b"\x1b[?2027;2$y");
}

const FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"; // 👨‍👩‍👧

#[test]
fn mode_2027_clusters_a_zwj_family_into_one_cell() {
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2027h");
    t.feed(FAMILY.as_bytes());
    let g = t.grid();
    // ONE wide cell: 👨 lead at col 0, its spacer at 1, and col 2 is BLANK — the family did not
    // spill into a second cell.
    assert_eq!(g.cell(0, 0).c(), '\u{1F468}', "lead is 👨");
    assert!(g.cell(0, 0).is_wide(), "the cluster cell is wide");
    assert!(g.cell(0, 1).is_wide_spacer(), "col 1 is the wide spacer");
    assert_eq!(g.cell(0, 2).c(), ' ', "col 2 blank — no second cell");
    // Cursor advanced by 2 (one wide cell), not 6.
    assert_eq!(t.cursor().col, 2, "cursor after one wide cell");
    // The whole sequence is preserved in the cell + its side-table (copy fidelity).
    assert_eq!(
        t.accessible_text().trim_end(),
        FAMILY,
        "full cluster preserved"
    );
}

#[test]
fn mode_off_family_stays_three_cells_wcwidth_compatible() {
    let mut t = Engine::new(80, 24);
    t.feed(FAMILY.as_bytes()); // no ?2027h — default per-char path
    let g = t.grid();
    assert_eq!(g.cell(0, 0).c(), '\u{1F468}', "👨 cell");
    assert_eq!(
        g.cell(0, 2).c(),
        '\u{1F469}',
        "👩 is its own cell (per-char)"
    );
    assert_eq!(g.cell(0, 4).c(), '\u{1F467}', "👧 is its own cell");
    assert_eq!(
        t.cursor().col,
        6,
        "6 columns — matches wcwidth apps (no desync)"
    );
}

#[test]
fn mode_2027_clusters_a_flag_and_promotes_a_narrow_base_to_wide() {
    let flag = "\u{1F1F0}\u{1F1F7}"; // 🇰🇷 — two regional indicators, each per-char width 1
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2027h");
    t.feed(flag.as_bytes());
    let g = t.grid();
    // The first RI landed NARROW (width 1); the second RI joins AND promotes the cluster to a
    // WIDE cell — a flag is one double-width emoji. Column count stays 2 (1+1 → one wide), so no
    // wcwidth desync even though the cell model changed.
    assert_eq!(g.cell(0, 0).c(), '\u{1F1F0}', "flag lead");
    assert!(
        g.cell(0, 0).is_wide(),
        "promoted to wide when the 2nd RI joined"
    );
    assert!(g.cell(0, 1).is_wide_spacer(), "spacer written on promotion");
    assert_eq!(t.cursor().col, 2, "cursor after one wide cell");
    assert_eq!(t.accessible_text().trim_end(), flag, "full flag preserved");
}

#[test]
fn mode_2027_promotes_a_text_base_plus_vs16_to_wide() {
    // ▶ (U+25B6) is per-char width 1; under mode 2027 a following VS16 (U+FE0F) forces emoji
    // presentation → the cluster becomes width 2 (this is the width-2 #301 wanted, now opt-in).
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2027h");
    t.feed("\u{25B6}\u{FE0F}".as_bytes());
    let g = t.grid();
    assert_eq!(g.cell(0, 0).c(), '\u{25B6}');
    assert!(
        g.cell(0, 0).is_wide(),
        "VS16 promoted ▶ to width 2 under mode 2027"
    );
    assert!(g.cell(0, 1).is_wide_spacer());
    assert_eq!(t.cursor().col, 2);
}

#[test]
fn mode_2027_clusters_a_skin_tone_sequence_no_promotion_needed() {
    // 👍🏽 = 👍 (base, already width 2) + 🏽 (U+1F3FB modifier, Extend). The modifier joins the
    // already-wide base — one wide cell, no width change.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2027h");
    t.feed("\u{1F44D}\u{1F3FB}".as_bytes());
    let g = t.grid();
    assert_eq!(g.cell(0, 0).c(), '\u{1F44D}', "👍 lead");
    assert!(g.cell(0, 0).is_wide());
    assert_eq!(
        g.cell(0, 2).c(),
        ' ',
        "modifier did not spill into a new cell"
    );
    assert_eq!(t.cursor().col, 2);
    assert_eq!(t.accessible_text().trim_end(), "\u{1F44D}\u{1F3FB}");
}

#[test]
fn mode_off_flag_and_vs16_stay_per_char() {
    // With mode 2027 OFF (default), the wcwidth-compatible path is preserved verbatim.
    let mut t = Engine::new(80, 24);
    t.feed("\u{1F1F0}\u{1F1F7}".as_bytes()); // flag → two narrow RI cells
    let g = t.grid();
    assert_eq!(g.cell(0, 0).c(), '\u{1F1F0}', "1st RI cell");
    assert!(
        !g.cell(0, 0).is_wide(),
        "RI stays narrow (per-char width 1)"
    );
    assert_eq!(g.cell(0, 1).c(), '\u{1F1F7}', "2nd RI is its own cell");
    assert_eq!(t.cursor().col, 2, "two narrow cells");

    let mut t2 = Engine::new(80, 24);
    t2.feed("\u{25B6}\u{FE0F}".as_bytes()); // ▶️ → base ▶ (width 1) + VS16 as width-0 combining
    let g2 = t2.grid();
    assert_eq!(g2.cell(0, 0).c(), '\u{25B6}');
    assert!(
        !g2.cell(0, 0).is_wide(),
        "▶ stays width 1 without mode 2027 (no VS16 promotion)"
    );
    assert_eq!(t2.cursor().col, 1, "one narrow cell — wcwidth agrees");
}

#[test]
fn mode_2027_promotion_at_last_column_relocates_the_cluster_to_the_next_line() {
    // #303: a narrow base that must promote-to-wide but sits at the last column has no spacer room,
    // so the WHOLE cluster relocates to the next line as a wide cell and the row soft-wraps —
    // matching ghostty. (2 cols → the 1st RI lands at the last column; 2 rows → no scroll.)
    let mut t = Engine::new(2, 2);
    t.feed(b"\x1b[?2027h");
    t.feed("X".as_bytes()); // (0,0)
    t.feed("\u{1F1F0}".as_bytes()); // 🇰 → narrow at (0,1) last column, cursor pending-wrap
    t.feed("\u{1F1F7}".as_bytes()); // 🇷 joins → promote → no room → relocate to row 1
    let g = t.grid();
    // Row 0 soft-wraps: 'X' stays, the vacated last column carries WRAPLINE.
    assert_eq!(g.cell(0, 0).c(), 'X');
    assert!(
        g.cell(0, 1).is_wrapline(),
        "row 0 soft-wraps at the vacated last column"
    );
    // The flag is relocated to row 1 as ONE wide cell.
    assert_eq!(
        g.cell(1, 0).c(),
        '\u{1F1F0}',
        "flag lead relocated to row 1"
    );
    assert!(g.cell(1, 0).is_wide(), "and is now wide");
    assert!(g.cell(1, 1).is_wide_spacer(), "with its spacer");
    // The two rows read back as ONE logical line (soft-wrap join, vacated leading spacer skipped):
    // "X" + the full flag — proving logical_lines/accessible_text stay coherent across relocation.
    assert_eq!(t.accessible_text().trim_end(), "X\u{1F1F0}\u{1F1F7}");
}

#[test]
fn mode_2027_last_column_relocation_scrolls_when_at_the_bottom() {
    // On a single-row screen the relocation's wrap scrolls: the vacated 'X' row moves to
    // scrollback and the flag lands wide on the (now sole) visible row.
    let mut t = Engine::new(2, 1);
    t.feed(b"\x1b[?2027h");
    t.feed("X\u{1F1F0}\u{1F1F7}".as_bytes()); // X, then the flag clustered at the last column
    let g = t.grid();
    assert_eq!(
        g.cell(0, 0).c(),
        '\u{1F1F0}',
        "flag scrolled onto the visible row"
    );
    assert!(g.cell(0, 0).is_wide());
    assert!(g.cell(0, 1).is_wide_spacer());
    assert_eq!(t.scrollback_len(), 1, "the 'X' row scrolled into history");
    // The whole document still reads "X🇰🇷" (scrollback + screen).
    assert_eq!(t.accessible_text().trim_end(), "X\u{1F1F0}\u{1F1F7}");
}

#[test]
fn mode_2027_no_relocation_at_last_column_with_autowrap_off() {
    // With autowrap off (?7l) a last-column base never gets pending-wrap, so `try_grapheme_join`
    // steps back to col-1 (the wrong cell) — the flag never forms and nothing relocates: the 2nd RI
    // overwrites the 1st in place. (ghostty instead picks the prev cell by `codepoint()==0` and keeps
    // the *1st* RI narrow; both are degenerate — one RI is always lost with autowrap off. Upstream of
    // #303's relocation charter.) The last cell stays narrow; row 1 is untouched.
    let mut t = Engine::new(2, 2);
    t.feed(b"\x1b[?2027h\x1b[?7l"); // grapheme mode on, autowrap OFF
    t.feed("X\u{1F1F0}\u{1F1F7}".as_bytes());
    let g = t.grid();
    assert!(
        !g.cell(0, 1).is_wide(),
        "no wide cluster at the last column"
    );
    assert_eq!(g.cell(1, 0).c(), ' ', "nothing relocated to row 1");
}

#[test]
fn mode_2027_vs15_narrows_a_default_wide_emoji() {
    // Lens-2 divergence: a default-WIDE emoji + VS15 (U+FE0E, the *text* selector) requests text
    // presentation → width 1 (ghostty + kitty narrow it). ⌚ (U+231A) is width 2; under mode 2027 a
    // following VS15 must DEMOTE the cell: remove WIDE_CHAR, free the spacer, back the cursor up.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2027h");
    t.feed("\u{231A}".as_bytes()); // ⌚ wide at col 0-1, cursor → col 2
    t.feed("\u{FE0E}".as_bytes()); // VS15 joins → cluster width 1 → demote
    let g = t.grid();
    assert_eq!(g.cell(0, 0).c(), '\u{231A}');
    assert!(!g.cell(0, 0).is_wide(), "VS15 demoted ⌚ to width 1");
    assert!(!g.cell(0, 1).is_wide_spacer(), "old spacer freed");
    assert_eq!(t.cursor().col, 1, "cursor backed up to a single-width cell");
}

#[test]
fn mode_2027_promotion_repairs_an_orphaned_wide_half_at_col_plus_one() {
    // Regression (Lens-1 breakage 1): promotion overwrites col+1 with a spacer. If a WIDE glyph was
    // standing there (cursor repositioned before the joining scalar arrived), its other half must be
    // repaired — no orphaned WIDE_CHAR_SPACER may survive, exactly as write_glyph does.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2027h");
    t.feed("\u{25B6}".as_bytes()); // ▶ narrow at col 0, cursor → col 1
    t.feed("\u{4E2D}".as_bytes()); // 中 wide lead at col 1, spacer at col 2, cursor → col 3
    t.feed(b"\x1b[1;2H"); // CUP to row 1 col 2 (0-based col 1) — cursor now just after ▶
    t.feed("\u{FE0F}".as_bytes()); // VS16 joins ▶, promotes → spacer overwrites 中's lead at col 1
    let g = t.grid();
    assert!(g.cell(0, 0).is_wide(), "▶ promoted to wide");
    assert!(g.cell(0, 1).is_wide_spacer(), "its spacer at col 1");
    assert!(
        !g.cell(0, 2).is_wide_spacer(),
        "中's orphaned spacer at col 2 must be repaired (reset), not left dangling"
    );
}

#[test]
fn mode_2027_clustered_cell_survives_encode_decode_roundtrip() {
    // DoD real proof (ADR-0005): a clustered family produced by the ENGINE survives the wire —
    // the clustered cell + its side-table round-trip encode→decode intact (not a hand-built frame).
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2027h");
    t.feed(FAMILY.as_bytes());
    let frame = t.frame();
    let decoded = decode(&encode(&frame)).expect("decode");
    assert_eq!(decoded, frame, "clustered cell + side_table round-trips");
    // The decoded side-table actually carries the joined emoji (👩, 👧 rode the cluster).
    let joined: String = decoded.side_table.iter().flatten().collect();
    assert!(
        joined.contains('\u{1F469}') && joined.contains('\u{1F467}'),
        "joined emoji live in the round-tripped side_table"
    );
}

#[test]
fn reflow_after_a_wide_char_wrap_injects_no_phantom_space() {
    // #303-surfaced pre-existing bug: a wide char that wrapped at the boundary leaves a
    // leading-spacer placeholder in the vacated last column. Reflow must DROP it on the soft-wrap
    // join — else a column resize injects a phantom space into every text reader.
    let mut t = Engine::new(2, 2);
    t.feed("a\u{D55C}".as_bytes()); // 'a' + 한 (wide) → 한 wraps to row 1
    assert_eq!(t.accessible_text().trim_end(), "a\u{D55C}");
    t.resize(3, 2); // widen — 한 now fits on one line
    assert_eq!(
        t.accessible_text().trim_end(),
        "a\u{D55C}",
        "no phantom space after reflow"
    );

    // The relocation path (#303) is a second producer of leading spacers — same invariant.
    let mut t2 = Engine::new(2, 2);
    t2.feed(b"\x1b[?2027h");
    t2.feed("X\u{1F1F0}\u{1F1F7}".as_bytes()); // relocate flag to row 1
    assert_eq!(t2.accessible_text().trim_end(), "X\u{1F1F0}\u{1F1F7}");
    t2.resize(3, 2);
    assert_eq!(
        t2.accessible_text().trim_end(),
        "X\u{1F1F0}\u{1F1F7}",
        "relocated cluster survives resize with no phantom space (#303)"
    );
    assert_eq!(
        t2.search("X\u{1F1F0}\u{1F1F7}").len(),
        1,
        "still searchable across reflow"
    );
}

// ---- width promotion carries the whole extended-attr family (#521) ------------
//
// A cell's hyperlink (#46) and underline colour (#520) ride per-row, column-keyed
// side maps gated by a presence bit. When a width promotion *moves* a cell
// (`relocate_cluster_wide`) or *grows* it (`promote_cluster_to_wide`), the bit
// travels with the copied cell but the map entry does not follow by itself — so
// the family must be re-attached explicitly, the way xterm.js's `copyCellsFrom`
// re-keys `_combined` **and** `_extendedAttrs` together (`BufferLine.ts`
// `_copyCellMapsFrom`).

#[test]
fn mode_2027_relocation_carries_the_extended_attrs_to_both_halves() {
    // ▶ lands in the LAST column as a narrow base; the VS16 promotes it to width 2,
    // which does not fit — so the whole cluster relocates to (1, 0..=1).
    //
    // The link and colour are RELEASED before the VS16 arrives. That separates the two
    // sources this test is about: the relocated cell takes what the *cell* carried (stamped
    // when ▶ was printed), while the column it vacates is blanked from the *pen*, which by
    // then holds nothing. With the pen still open both sources agree and the assertions
    // below could not tell them apart (#521/#528).
    let mut t = Engine::new(2, 24);
    t.feed(EXT_ATTR_PEN);
    t.feed(LINK_OPEN);
    t.feed("X\u{25B6}".as_bytes());
    t.feed(b"\x1b]8;;\x07\x1b[59m\x1b[24m"); // close the link, drop the colour + underline
    t.feed("\u{FE0F}".as_bytes()); // now promote → relocate

    assert_eq!(t.grid().cell(1, 0).c(), '\u{25B6}', "relocated lead");
    assert!(t.grid().cell(1, 0).is_wide(), "promoted to wide");
    assert!(t.grid().cell(1, 1).is_wide_spacer(), "its spacer");

    let link = t.link_at(1, 0).expect("the hyperlink rode the relocation");
    assert_eq!(t.hyperlink(link), Some("https://example.com"));
    assert_eq!(t.link_at(1, 1), Some(link), "both halves agree on the link");
    assert_eq!(t.underline_color_at(1, 0), Color::Indexed(1));
    assert_eq!(
        t.underline_color_at(1, 1),
        Color::Indexed(1),
        "both halves agree on the underline colour"
    );

    // Right reason: the attrs MOVED with the glyph, they were not duplicated. The vacated
    // column is blanked from the pen, which no longer holds either — so it keeps neither,
    // while the relocated cell above keeps both. (Were the link still open, the blank would
    // legitimately take it: a vacated column is a pen-written blank, not an erasure —
    // `wide_wrap_vacate.rs` covers that half.)
    assert_eq!(t.link_at(0, 1), None, "vacated column keeps no link");
    assert_eq!(t.underline_color_at(0, 1), Color::Default);
    // …and 'X', printed under the same pen, is untouched.
    assert_eq!(t.link_at(0, 0), Some(link));
    // The cluster itself still survives the relocation (#303).
    assert_eq!(t.accessible_text().trim_end(), "X\u{25B6}\u{FE0F}");
}

#[test]
fn mode_2027_in_place_promotion_gives_the_spacer_the_leads_extended_attrs() {
    // Same promotion with room to grow: the base stays put and gains a spacer at
    // col+1, which `write_glyph` would have stamped with the same link + colour.
    let mut t = Engine::new(80, 24);
    t.feed(EXT_ATTR_PEN);
    t.feed(LINK_OPEN);
    t.feed("\u{25B6}\u{FE0F}".as_bytes());

    assert!(t.grid().cell(0, 0).is_wide());
    assert!(t.grid().cell(0, 1).is_wide_spacer());
    let link = t.link_at(0, 0).expect("the lead kept its link");
    assert_eq!(t.link_at(0, 1), Some(link), "both halves agree on the link");
    assert_eq!(t.underline_color_at(0, 0), Color::Indexed(1));
    assert_eq!(
        t.underline_color_at(0, 1),
        Color::Indexed(1),
        "both halves agree on the underline colour"
    );
}

#[test]
fn mode_2027_promotion_does_not_resurrect_a_stale_extended_attr() {
    // The spacer overwrites a column that HAD a live link + colour. The promoted
    // base has neither, so neither half may report one — a carry that only ever
    // *sets* would leave the old column's entry readable under the new cell.
    let mut t = Engine::new(80, 24);
    t.feed(b"\x1b[?2027h\x1b[4m\x1b[58:5:2m");
    t.feed(LINK_OPEN);
    t.feed(b"ab"); // both columns carry the link + indexed-2 underline colour
    t.feed(b"\x1b]8;;\x07\x1b[59m\x1b[24m"); // close the link, drop the colour + underline
    t.feed(b"\x1b[H"); // back to (0,0), over 'a'
    t.feed("\u{25B6}\u{FE0F}".as_bytes()); // narrow base then promote over 'b'

    assert!(t.grid().cell(0, 1).is_wide_spacer());
    assert_eq!(t.link_at(0, 0), None, "the new lead has no link");
    assert_eq!(t.link_at(0, 1), None, "and neither does its spacer");
    assert_eq!(t.underline_color_at(0, 0), Color::Default);
    assert_eq!(t.underline_color_at(0, 1), Color::Default);
}

#[test]
fn relocated_extended_attrs_survive_the_wire_round_trip() {
    // Real proof (ADR-0005): the carried link + colour reach a frame-mode consumer,
    // not just the in-process grid.
    let mut t = Engine::new(2, 24);
    t.feed(EXT_ATTR_PEN);
    t.feed(LINK_OPEN);
    t.feed("X\u{25B6}\u{FE0F}".as_bytes());
    let frame = t.frame();
    let decoded = decode(&encode(&frame)).expect("round-trips");
    // NB: `decoded == frame` is deliberately NOT asserted here. It does not hold for
    // *any* frame carrying an underline colour — `decode_cell` restores the link
    // presence bit (`set_linked(link != 0)`) but nothing restores `UCOLOR_PRESENT`
    // after the v13 ucolor group is read, so the cell bit comes back clear. That is a
    // pre-existing #520 codec asymmetry, independent of the carry under test (it
    // reproduces on a plain `\e[4m\e[58:5:1mA`), and the group below is what a
    // frame-mode consumer actually reads.
    let span = decoded
        .spans
        .iter()
        .find(|s| s.line == 1)
        .expect("row 1 is a damage span");
    let col = |c: usize| c - span.left as usize;
    assert_eq!(
        span.ucolors.get(&col(0)).copied(),
        Some(Color::Indexed(1)),
        "lead's underline colour on the wire"
    );
    assert_eq!(
        span.ucolors.get(&col(1)).copied(),
        Some(Color::Indexed(1)),
        "spacer's too"
    );
    assert!(span.links.contains_key(&col(0)), "lead's link on the wire");
    assert!(span.links.contains_key(&col(1)), "spacer's too");
}
