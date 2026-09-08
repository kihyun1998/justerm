//! Grapheme-cluster segmentation for the streaming parser (#295, DECSET mode 2027).
//!
//! justerm-core processes input one `char` at a time (`Term::print`). Under mode 2027 the parser
//! must decide, per incoming scalar, whether it **extends** the previous cell's grapheme cluster
//! (ride the side-table, no new cell) or **breaks** (start a new cell). This is the incremental
//! form of UAX #29 extended grapheme-cluster segmentation.
//!
//! The break decision is delegated to `unicode-segmentation` (the full UAX #29 rule set:
//! GB9 Extend/ZWJ, GB9a SpacingMark, GB9b Prepend, GB11 emoji-ZWJ, GB12/GB13 regional-indicator
//! pairing) rather than hand-rolled — the rules need large Unicode property tables that would rot.
//!
//! ## No break state is persisted, and the cluster is not rebuilt either (#867)
//!
//! Persisting a break state across `print` calls would have to be repaired at every cursor-moving
//! verb. xterm.js does persist one — a packed integer on the parser, cleared at every C0 execute
//! and every CSI/ESC/OSC dispatch (`src/common/parser/EscapeSequenceParser.ts:676` @ `699f553`) —
//! and that granularity is **not** behaviour-preserving here: `grapheme_cluster.rs`'s
//! `mode_2027_promotion_repairs_an_orphaned_wide_half_at_col_plus_one` feeds CUP and then requires
//! the next scalar to join the cluster the cursor landed on. So the state stays in the cell.
//!
//! What #867 removed is the *reconstruction*. [`joins_cluster`] asks `unicode-segmentation` about a
//! bounded **tail** of the stored cluster and widens that tail only when the rules themselves ask
//! for more context (`GraphemeIncomplete::PreContext`), so a cluster that keeps growing no longer
//! costs O(L) per scalar. ghostty pays that O(L) — `src/terminal/Terminal.zig:1150-1168` @
//! `e6e26e1` builds a fresh `BreakState` per print and re-walks every stored codepoint — with a
//! per-step cost small enough to hide it. Ours was two full segmentation passes plus two
//! allocations per scalar, and 90 KB of open ZWJ output cost 26 s inside one `feed()`.
//!
//! ## A variation selector on a non-emoji base is KEPT (#317 §1, decided 2026-08-18)
//!
//! `x` + VS16 is not an emoji sequence — the selector changes nothing about how `x` is drawn or how
//! wide it is. justerm still joins it into the side-table, because UAX #29 puts it there: VS16/VS15
//! are `Extend`, so the segmenter says yes and the scalar rides along. The only place it is
//! observable is **text extraction** — a copy of that cell yields the extra scalar.
//!
//! ghostty drops it: *"the terminal does not store those selectors in the cell, so callers must also
//! restore their grapheme break state and leave prev unchanged"* (`src/unicode/grapheme.zig:56` @
//! `e6e26e16`). Recorded here because the divergence is **narrower than it looks, and #317's body
//! described it wrongly** as a disagreement about UAX #29. It is not one: ghostty's own
//! `graphemeWidth('x', 0xFE0F)` returns `len = 2` (`:315`), so both implementations agree the
//! selector is *in the cluster*. They differ one layer down, on whether the cell **stores** what the
//! cluster contains — and ghostty's own comment states the cost of its answer, which is that every
//! caller now has to repair a break state the storage layer discarded.
//!
//! justerm keeps it, on the tie-breaker for this layer: VT semantics answer to **the spec**, above
//! any implementation including ours (ADR-0004). Widths are identical either way, so nothing on
//! screen distinguishes them; what a cell hands back is the cluster the spec says it is.

use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

/// Whether `c` **extends** the cluster stored as `base` + `marks` rather than starting a new one —
/// the same UAX #29 question as segmenting `base·marks·c` and asking whether a boundary opened
/// before `c`, asked without materialising that string (#867).
///
/// The cursor is given a **tail** of the cluster. Every backward-looking rule in
/// `unicode-segmentation` — GB11's emoji lookback, GB12/GB13's regional-indicator parity, GB9c's
/// Indic conjunct scan — either decides from what it can see or reports `PreContext` and asks for
/// more; each decides *without* asking only when the chunk it was handed starts at offset 0
/// (`grapheme.rs:529`, `:565` in `unicode-segmentation` 1.13.3). So widening the tail on
/// `PreContext`, and finally handing over the whole cluster at offset 0, is a faithful substitute
/// for `provide_context` and reaches the same answer.
///
/// For the shapes that grow without bound — an open ZWJ run, a long `Extend` run — the rules stop
/// at the first scalar that decides them, so the tail never widens and the join is O(1).
pub(crate) fn joins_cluster(base: char, marks: &[char], c: char) -> bool {
    // A non-zero virtual prefix length. Only its *sign* is load-bearing: the cursor subtracts
    // `chunk_start` back out of every offset it computes and branches solely on
    // `chunk_start == 0` ("this chunk is the start of the text"), so any positive value behaves
    // identically.
    const VIRTUAL_PREFIX: usize = 4096;
    // Tail growth: start at the two scalars every rule needs at minimum, then quadruple, so a
    // cluster that genuinely needs deep context is reached in a logarithmic number of retries.
    const INITIAL_TAIL: usize = 2;

    let total = 1 + marks.len();
    let mut take = INITIAL_TAIL;
    loop {
        let take_now = take.min(total);
        let from_start = take_now == total;
        let mut chunk = String::new();
        if from_start {
            chunk.push(base);
            chunk.extend(marks.iter().copied());
        } else {
            chunk.extend(marks[marks.len() - take_now..].iter().copied());
        }
        let tail_bytes = chunk.len();
        chunk.push(c);
        let start = if from_start { 0 } else { VIRTUAL_PREFIX };
        let mut cursor = GraphemeCursor::new(start + tail_bytes, start + chunk.len(), true);
        match cursor.is_boundary(&chunk, start) {
            Ok(is_break) => return !is_break,
            // The rules want context the tail does not carry. Unreachable once `from_start` is
            // true, because a chunk starting at offset 0 is decided rather than deferred.
            Err(GraphemeIncomplete::PreContext(_)) if !from_start => {
                take = take.saturating_mul(4);
            }
            // No other variant is reachable: the boundary offset is inside the chunk by
            // construction and the cursor is never resumed. Answering "break" is the conservative
            // reading — it starts a new cell rather than corrupting one.
            Err(_) => return false,
        }
    }
}

/// Whether appending `c` can move the width `UnicodeWidthStr` reports for a cluster — `false` means
/// the width is provably unchanged and the oracle need not be consulted (#867). `first_join` says
/// `c` is the first scalar joined onto the base.
///
/// Two things move a cluster's width: a variation selector, and a scalar that occupies a column of
/// its own joining a base that does not yet. **Both only ever do so on the first join** — measured
/// against the oracle itself: repeating a selector never moved the answer (0 of 196 base/selector
/// combinations at depths 2..8), and a selector arriving after any other joined scalar left the
/// width where it already was. ghostty reaches the same shape from the other end — its
/// `graphemeWidthEffect` takes the *previous codepoint*, so a selector whose predecessor is itself
/// a selector fails `emoji_vs_base` and returns `.ignore` (`src/unicode/grapheme.zig:64` @
/// `e6e26e1`).
///
/// This is a **skip rule over an oracle that stays the authority**, not a replacement for it: when
/// it says "consult", `UnicodeWidthStr` over the whole cluster still decides. That distinction is
/// the whole design. An earlier attempt to answer the width outright instead of gating it got eight
/// ordinary inputs wrong — `x` + VS16 became a wide cell — and the suite did not notice, because
/// every width test used the one base where the shortcut happened to agree. So the predicate is
/// swept against the oracle in `width_gate_never_skips_a_real_change` rather than resting on the
/// reasoning above.
pub(crate) fn width_may_change(c: char, first_join: bool, base_is_wide: bool) -> bool {
    (is_variation_selector(c) && first_join) || (!base_is_wide && scalar_occupies_a_column(c))
}

/// The variation selectors VS1–VS16. VS16/VS15 request emoji and text presentation, and
/// `unicode-width` gives VS1–VS3 a width effect of their own on curly quotation marks
/// (`tables.rs:282-287` in `unicode-width` 0.2), so the whole block counts as width-relevant.
fn is_variation_selector(c: char) -> bool {
    ('\u{FE00}'..='\u{FE0F}').contains(&c)
}

/// Whether `c` would take a column of its own outside a cluster — the only way a joining scalar
/// that is not a selector can widen a narrow base (a flag's second regional indicator).
fn scalar_occupies_a_column(c: char) -> bool {
    unicode_width::UnicodeWidthChar::width(c).unwrap_or(0) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pre-#867 shape of this question, kept for the rule tests below: they are UAX #29 facts
    /// about a cluster written as text, and reading them that way is what makes them independent
    /// of how the cluster happens to be stored.
    fn extends(prev: &str, c: char) -> bool {
        let mut it = prev.chars();
        match it.next() {
            None => false,
            Some(base) => joins_cluster(base, &it.collect::<Vec<_>>(), c),
        }
    }

    // Independent UAX #29 truths — each case is a known grapheme-break fact, not a re-derivation of
    // the implementation.
    #[test]
    fn combining_mark_extends() {
        // é = 'e' + U+0301 combining acute (GB9 × Extend) → one grapheme.
        assert!(extends("e", '\u{0301}'));
    }

    #[test]
    fn zwj_extends_then_joined_emoji_extends() {
        // GB9 (× ZWJ): a ZWJ joins the preceding emoji…
        assert!(extends("\u{1F468}", '\u{200D}'), "👨 + ZWJ");
        // …and GB11 (ExtPict ZWJ × ExtPict): the emoji after the ZWJ joins too.
        assert!(extends("\u{1F468}\u{200D}", '\u{1F469}'), "👨‍ + 👩");
    }

    #[test]
    fn skin_tone_modifier_extends() {
        // 👍 + 🏽 (U+1F3FB, Emoji_Modifier = Extend) → one grapheme (GB9).
        assert!(extends("\u{1F44D}", '\u{1F3FB}'));
    }

    #[test]
    fn regional_indicator_pair_extends_but_the_third_breaks() {
        // GB12/GB13: RIs pair 2-by-2. The second RI joins the first (one flag)…
        assert!(extends("\u{1F1F0}", '\u{1F1F7}'), "🇰 + 🇷 = 🇰🇷");
        // …but a third RI starts a NEW flag (parity break).
        assert!(!extends("\u{1F1F0}\u{1F1F7}", '\u{1F1FA}'), "🇰🇷 + 🇺 breaks");
    }

    #[test]
    fn vs16_and_vs15_selectors_extend() {
        // A variation selector joins its base (GB9 × Extend), emoji (FE0F) or text (FE0E) alike.
        assert!(extends("\u{25B6}", '\u{FE0F}'), "▶ + VS16");
        assert!(extends("\u{25B6}", '\u{FE0E}'), "▶ + VS15");
    }

    #[test]
    fn a_new_base_scalar_breaks() {
        assert!(!extends("\u{1F468}", 'A'), "👨 + A breaks");
        assert!(!extends("A", 'B'), "A + B breaks");
        assert!(!extends("\u{4E2D}", '\u{6587}'), "中 + 文 breaks (CJK)");
    }

    #[test]
    fn empty_prefix_never_extends() {
        assert!(!extends("", 'A'));
        assert!(!extends("", '\u{1F468}'));
    }

    // The bounded tail is what #867 bought, so the cases that force it to widen are the ones worth
    // pinning: a fixed window would answer these differently.
    #[test]
    fn a_long_extend_run_before_a_zwj_still_reaches_its_emoji_base() {
        // GB11 is "ExtPict Extend* ZWJ × ExtPict". With 500 Extends between the base and the ZWJ,
        // the emoji lookback walks past any short tail and has to ask for more context.
        let mut marks: Vec<char> = std::iter::repeat_n('\u{0301}', 500).collect();
        marks.push('\u{200D}');
        assert!(
            joins_cluster('\u{1F468}', &marks, '\u{1F469}'),
            "the joining emoji still finds its pictographic base 502 scalars back"
        );
        // The same shape with a non-pictographic base must still break — otherwise the assertion
        // above would pass for a tail that simply never reached the base.
        assert!(
            !joins_cluster('x', &marks, '\u{1F469}'),
            "no pictographic base 502 scalars back, so GB11 does not apply"
        );
    }

    #[test]
    fn regional_indicator_parity_survives_the_tail() {
        // GB12/GB13 count regional indicators backwards from the boundary.
        assert!(joins_cluster('\u{1F1F0}', &[], '\u{1F1F7}'));
        assert!(!joins_cluster('\u{1F1F0}', &['\u{1F1F7}'], '\u{1F1FA}'));
    }

    // The gate is a claim *about the oracle*, so it is checked against the oracle. A wrong skip
    // changes a cell's width, which is the exact failure this predicate was written to avoid.
    #[test]
    fn width_gate_never_skips_a_real_change() {
        use unicode_width::UnicodeWidthStr;

        // Bases spanning the width classes the oracle distinguishes: ASCII, space, CJK, an
        // emoji-variation base, a default-wide emoji, Indic, Arabic, Hangul, a regional
        // indicator, and the Hebrew and Arabic letters that drive its ligature states.
        const BASES: [char; 14] = [
            'x',
            ' ',
            '中',
            '\u{25B6}',
            '\u{231A}',
            '\u{1F468}',
            '\u{1F600}',
            '\u{0915}',
            '\u{0600}',
            '\u{1100}',
            '\u{6F22}',
            '\u{1F1F0}',
            '\u{05D0}',
            '\u{0644}',
        ];
        // Joiners covering every width-moving shape the oracle has: both presentation selectors, a
        // VS1-3 selector, a plain mark, ZWJ, a second regional indicator, a skin-tone modifier, a
        // keycap, a spacing mark, a wide pictograph — and a **narrow** one.
        //
        // U+25B6 is in this list because leaving it out is what let a too-narrow gate pass. The
        // only reachable join that takes a narrow cluster to exactly 2 at a non-first position is
        // narrow-pictograph ZWJ narrow-pictograph (`▶‍▶`, 1 -> 2); with only wide pictographs here
        // every such join landed on 3, which the caller ignores, and the sweep could not tell the
        // two candidate gates apart.
        const JOINERS: [char; 11] = [
            '\u{FE0F}',
            '\u{FE0E}',
            '\u{FE01}',
            '\u{0301}',
            '\u{200D}',
            '\u{1F1F7}',
            '\u{1F3FB}',
            '\u{20E3}',
            '\u{0915}',
            '\u{1F469}',
            '\u{25B6}',
        ];

        // What a skip must preserve is the *action*, not the number, and the action is a function
        // of the cell's wide flag — which is **not** "the oracle's last answer was 2". The flag
        // moves only on an exactly-2 promotion or an exactly-1 demotion, so a cluster the oracle
        // measures at 3 leaves the flag exactly where it was. Modelling it as `width == 2` reported
        // `⌚ ZWJ ▶ + VS16` as a skipped change against a cell it wrongly believed was narrow.
        // So the flag is carried along each path, updated the way `try_grapheme_join` updates it.
        fn action(w: usize, is_wide: bool) -> (bool, bool) {
            (w == 2 && !is_wide, w == 1 && is_wide)
        }

        let mut skipped = 0usize;
        let mut consulted = 0usize;
        for base in BASES {
            // Walk every cluster this alphabet can build to depth 3, following only the joins the
            // segmenter actually allows — a cluster no input can produce proves nothing. Each path
            // carries the wide flag the engine would be holding at that point.
            let base_wide = UnicodeWidthStr::width(base.to_string().as_str()) == 2;
            let mut frontier: Vec<(Vec<char>, bool)> = vec![(Vec::new(), base_wide)];
            for _ in 0..3 {
                let mut next: Vec<(Vec<char>, bool)> = Vec::new();
                for (marks, is_wide) in &frontier {
                    for c in JOINERS {
                        if !joins_cluster(base, marks, c) {
                            continue;
                        }
                        let mut after = String::new();
                        after.push(base);
                        after.extend(marks.iter().copied());
                        after.push(c);
                        let w_true = UnicodeWidthStr::width(after.as_str());

                        // What the caller would use: the oracle when the gate says consult, and
                        // the cell's own shape when it says skip.
                        let consult = width_may_change(c, marks.is_empty(), *is_wide);
                        let w_used = if consult {
                            consulted += 1;
                            w_true
                        } else {
                            skipped += 1;
                            if *is_wide { 2 } else { 1 }
                        };
                        assert_eq!(
                            action(w_used, *is_wide),
                            action(w_true, *is_wide),
                            "the gate changed what the caller does: base U+{:04X} marks {:X?} + U+{:04X} (oracle {w_true}, used {w_used}, wide {is_wide})",
                            base as u32,
                            marks.iter().map(|m| *m as u32).collect::<Vec<_>>(),
                            c as u32,
                        );

                        let (promote, demote) = action(w_used, *is_wide);
                        let mut grown = marks.clone();
                        grown.push(c);
                        next.push((grown, (*is_wide || promote) && !demote));
                    }
                }
                frontier = next;
            }
        }
        // A sweep that skipped nothing would pass vacuously; one that consulted nothing would not
        // be exercising a gate at all.
        assert!(
            skipped > 0 && consulted > 0,
            "{skipped} skipped, {consulted} consulted"
        );
    }
}
