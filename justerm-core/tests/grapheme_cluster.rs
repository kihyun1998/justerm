//! DECSET mode 2027 grapheme-cluster segmentation (#295, subsumes #301).
//!
//! Mode 2027 (default OFF) opts an application into UAX #29 grapheme-cluster width: a ZWJ
//! family / skin-tone / flag / emoji+VS16 sequence is clustered into ONE cell (its full text in
//! the side-table) instead of one cell per scalar. OFF preserves the per-char (wcwidth-compatible)
//! behaviour verbatim, so the cursor stays in sync with wcwidth apps (the reason clustering is
//! opt-in — cf. #301). Verified against ghostty/kitty (both gate clustering behind ?2027).

use justerm_core::{Engine, decode, encode};

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
fn mode_2027_promotion_at_last_column_stays_narrow_known_gap() {
    // KNOWN GAP (#303): a narrow base that would promote-to-wide but sits at the last column has
    // no room for its spacer, so it stays narrow. Documented here so the tail edge is tracked, not
    // silent — the common (non-edge) promotion is covered above.
    let mut t = Engine::new(2, 1); // 2 columns: put the 1st RI at the last column
    t.feed(b"\x1b[?2027h");
    t.feed("X".as_bytes()); // col 0
    t.feed("\u{1F1F0}".as_bytes()); // 🇰 → narrow at col 1 (last), cursor pending-wrap
    t.feed("\u{1F1F7}".as_bytes()); // 🇷 joins but cannot promote (no col 2)
    let g = t.grid();
    assert!(
        !g.cell(0, 1).is_wide(),
        "last-column flag stays narrow until relocation lands (#303)"
    );
    // The full flag is still preserved in the side-table (copy fidelity holds).
    assert!(t.accessible_text().contains("\u{1F1F0}\u{1F1F7}"));
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
