//! #621 — the wire carries what the engine legitimately holds.
//!
//! Every fixture here starts from a real [`Engine`] and goes out through `feed`, because
//! that is the only starting point that can observe this defect. `tests/robustness.rs`'s
//! round-trip property is driven from a *decoded* frame, so it proves
//! `decode ∘ encode ∘ decode == decode` — a fixed point of the decoder's own output, which
//! by construction cannot see an encode-side asymmetry. Measured on the sibling defect
//! (#531): the buggy build satisfied that property while failing the assertion below.
//!
//! The four cases are not one defect in four costumes; they fail differently, and the
//! difference is the whole reason this issue is not #582's:
//!
//! - a cluster or URI longer than the old `u16` length prefix → `Err(BadTag)`, loud;
//! - a viewport holding more combining cells than the old `u16` table count → the count
//!   wraps, `encode` writes more records than it declared, and `decode` returns **`Ok`**
//!   on a frame whose combining references point past the end of the table;
//! - more distinct URIs in one frame than the old `u16` frame-local index → a **panic**
//!   inside `Term::frame` itself, before any byte reaches the wire.
//!
//! None of that is malformed input. The engine parsed it correctly and stores it
//! correctly; the fields were simply too small for values it legitimately holds. That is
//! why the fix is capacity (widen what is rare, delete what is per-cell) and not
//! validation — whether `decode` should *reject* an out-of-range group key stays #582's
//! question, and nothing here answers it.

use justerm_core::{Engine, decode, encode};

/// Just past `u16::MAX`, so a fixture crosses the old ceiling without paying for the
/// distance beyond it. Every count below is derived from this rather than restated, so
/// the fixtures cannot drift apart from the threshold they exist to cross.
const PAST_U16: usize = u16::MAX as usize + 1;

#[test]
fn a_combining_cluster_longer_than_the_old_prefix_survives_the_wire() {
    // One cell, one very long grapheme cluster. `Row::push_combining` caps nothing — and
    // no reference caps it either (xterm.js appends to a JS string, alacritty pushes onto
    // an unbounded `Vec<char>`, ghostty's `GraphemeAllocOutOfMemory` is an allocator
    // failure that its own caller resolves by *growing*), so the engine is right to hold
    // this and the wire was wrong to be unable to describe it.
    let mut e = Engine::new(10, 2);
    let mut stream = String::from("a");
    for _ in 0..PAST_U16 {
        stream.push('\u{0301}');
    }
    e.feed(stream.as_bytes());

    let frame = e.frame();
    assert_eq!(
        decode(&encode(&frame)).expect("a long cluster must round-trip, not fail to decode"),
        frame,
    );
}

#[test]
fn a_hyperlink_uri_longer_than_the_old_prefix_survives_the_wire() {
    // The OSC 8 half of the same shape. A URI this long is unusual, not invalid: nothing
    // in the OSC 8 path bounds it, so the engine stores what the stream gave it.
    let mut e = Engine::new(10, 2);
    let uri = "h".repeat(PAST_U16);
    e.feed(format!("\x1b]8;;{uri}\x07A").as_bytes());

    let frame = e.frame();
    assert_eq!(
        decode(&encode(&frame)).expect("a long URI must round-trip, not fail to decode"),
        frame,
    );
    assert_eq!(
        frame.link_table[0].len(),
        PAST_U16,
        "the engine stores the whole URI — if this shrinks, the fix capped the input \
         instead of widening the field, which is the one option prior art ruled out",
    );
}

#[test]
fn a_viewport_with_more_combining_cells_than_the_old_count_survives_the_wire() {
    // The dangerous one, and the reason this is not merely a tight field. The old table
    // count was `u16` while the header's `cols` and `rows` are `u16` *each*, so a legal
    // viewport can hold far more cells than the count could describe. Measured before the
    // fix, at 400x200 with one mark per cell: the count wrapped 80000 -> 14464, `encode`
    // wrote all 80000 records anyway, and `decode` returned **`Ok`** with combining
    // references reaching index 79999 into a 14464-entry table.
    //
    // So the assertion that matters is not `is_ok()` — that was already true while the
    // frame was wrong. It is equality with the frame the engine actually holds.
    //
    // **Read this as a shape regression pin, not as evidence about the v14 code.** Measured
    // with a single-point mutation matrix over the whole fix: this test reddens under
    // exactly one reversion (dropping `decode`'s `set_combined` re-arm), and the much
    // cheaper cluster-length test above catches that same one. It cannot discriminate more,
    // because after v14 there is no viewport-scaled count left in the combining path to
    // overflow — the count is per span. What it still buys is a guard against *reintroducing*
    // a frame-wide table, which no cheaper test would notice. It is also the most expensive
    // test in this file (~66 000 cells), so if it ever needs to be dropped for time, drop it
    // knowing that and not on the belief that it is load-bearing for the current code.
    let cols = 300;
    let rows = 220;
    assert!(
        cols * rows > PAST_U16,
        "the fixture must cross the old count"
    );

    let mut e = Engine::new(cols, rows);
    let mut stream = String::new();
    for _ in 0..rows {
        for _ in 0..cols {
            stream.push('e');
            stream.push('\u{0301}');
        }
    }
    e.feed(stream.as_bytes());

    let frame = e.frame();
    let combining_cells: usize = frame.spans.iter().map(|s| s.combining.len()).sum();
    assert!(
        combining_cells > PAST_U16,
        "fixture must actually carry more than u16::MAX combining cells, got {combining_cells}",
    );
    // `assert!` rather than `assert_eq!` on purpose: these fixtures are ~66 000 cells, and
    // a failing `assert_eq!` Debug-prints both frames — 19.6 MB of output, measured. The
    // claim is identical; only the failure report is survivable.
    assert!(
        decode(&encode(&frame)).expect("must decode") == frame,
        "more combining cells than the old count fit — the frame must survive intact, \
         not decode Ok into fabricated references",
    );
}

#[test]
fn more_highlight_spans_than_the_old_count_survives_the_wire() {
    // The fifth case, and the one the first `as u16` sweep missed: it classified the
    // overlay groups' `(row, left, right)` *triples* as bounded — which they are — and
    // never looked at the count above them.
    //
    // Measured before the fix, with a one-character search over a large viewport:
    //
    //   1000x132   65 500 spans -> 65 500   Ok, equal
    //   1000x133   66 000 spans ->     464  Ok, *** and 928 marker-lines + 3 active
    //                                       spans FABRICATED from nothing ***
    //   1000x134   66 500 spans -> Err(BadTag)
    //
    // The fabrication is the tell, and it is worse than the truncation: a wrapped count
    // leaves the reader mid-group, so every group *after* it decodes from the wrong
    // offset. The engine had zero markers.
    let (cols, rows) = (1000, 133);
    let mut e = Engine::new(cols, rows);
    let line = "ax".repeat(cols / 2);
    for _ in 0..rows {
        e.feed(line.as_bytes());
        e.feed(b"\r\n");
    }
    e.set_search_highlights(e.search("a"));

    let frame = e.frame();
    assert!(
        frame.overlay.matches.len() > PAST_U16,
        "fixture must cross the old count, got {}",
        frame.overlay.matches.len(),
    );
    let decoded = decode(&encode(&frame)).expect("must decode");
    // Named separately from the whole-frame equality below: these two were invented by
    // the desync rather than merely lost, and a bare `!=` would not say so.
    assert!(
        decoded.overlay.marker_lines.is_empty() && decoded.overlay.active_match.is_empty(),
        "the engine had no markers and no active match — decoding any is the reader \
         walking out of the highlight group and into the next one",
    );
    assert!(decoded == frame, "the highlight group must survive intact");
}

#[test]
fn more_distinct_links_than_the_old_index_does_not_panic_the_engine() {
    // This one never reached the wire at all: `Term::frame` renumbers each referenced
    // pool entry to a frame-local index with `link_remap[l] = link_table.len() as u16`,
    // then wraps it in `NonZeroU32::new(…).expect("link_remap just set, nonzero")`. At the
    // 65536th distinct URI the cast yields 0 and the `expect` fires — a panic in the
    // engine, on input it parsed correctly.
    //
    // Absorbed into this issue rather than filed separately: that `u16` exists *because*
    // the wire's link field was `u16`, so removing the narrowing removes the panic. One
    // root, one fix.
    let cols = 300;
    let rows = 220;
    let mut e = Engine::new(cols, rows);

    let mut stream = String::new();
    for i in 0..PAST_U16 + 1 {
        // A distinct URI per cell, each one referenced exactly once.
        stream.push_str(&format!("\x1b]8;;h://{i}\x07x"));
    }
    e.feed(stream.as_bytes());

    let frame = e.frame();
    assert!(
        frame.link_table.len() > PAST_U16,
        "fixture must carry more than u16::MAX distinct URIs, got {}",
        frame.link_table.len(),
    );
    assert!(
        decode(&encode(&frame)).expect("must decode") == frame,
        "a frame with more distinct links than the old index could number must survive",
    );
}
