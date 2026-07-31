//! #582 — a frame's parts are read against the frame's own declared geometry.
//!
//! `decode` validated a span with `right < left` and nothing else, so a frame could
//! declare a 4×2 grid and carry a span claiming column 8 of line 99 and still decode
//! `Ok`. That is not a cosmetic inconsistency: `justerm-web`'s `cell-mirror.ts` keeps
//! the viewport as one flat array and writes `cells[line * cols + x]`, so a column past
//! `cols` does not throw — it lands in the *next row's* slot and silently overwrites it.
//! The user-visible failure is a screen reader announcing, and a copy producing,
//! characters that are not on that line.
//!
//! **The trap that costs the next reader an hour**, and note that this change *moves* it.
//! Before: editing `right` upward failed with `Truncated`, not `BadSpan`, because the cell
//! payload is length-derived — a widened span without matching cells is rejected on
//! *length* and never reaches the bounds question. After: a widening past `cols` stops at
//! the new bounds check first, so only a widening that stays *inside* the frame still
//! reaches `Truncated`, and both orderings are pinned below. What is unchanged is the
//! reproduction the issue prescribes: keep the payload internally valid and shrink the
//! *declared* geometry. Nothing about the payload is malformed; only its relationship to
//! the frame is.
//!
//! **Reference posture** — the rows are in `docs/agents/reference-facts.md` § "Validating a
//! decoded payload against its own declared geometry"; what matters here is the part that
//! is *not* the simple story. The convergent rule across all three references is **validate
//! where the coordinate is computed, index freely inside**, which `Term::damage_span` and
//! `justerm-renderer`'s `FrameGrid::validate` already follow and `decode` alone did not.
//! But the closest analogue of all — ghostty's `verifyIntegrity`, which checks exactly this
//! class (a sparse side-map entry must pair with a cell carrying the bit) and whose doc
//! names *"deserialization"* — is **debug-gated**, and its production restore path validates
//! nothing.
//!
//! The distinction that survives is **who produced the payload**. Ghostty deserializes its
//! own snapshot in its own process, so an integrity failure there is a ghostty bug and
//! belongs in an assert. `decode` reads bytes a consumer hands back over its own transport
//! (ADR-0008; `tests/robustness.rs` names them attacker-influenced), so the same failure is
//! *input* — rejected, not asserted. That split is the shape of this change: `decode`
//! rejects, `encode` only `debug_assert!`s, because encode's input is a `Span` justerm
//! built, which is ghostty's case exactly.
//!
//! The *positions* a frame carries (overlay spans, cursor) stay unchecked: consumers
//! resolve them by scan rather than by index, and clamping a position is the consumer's
//! call under ADR-0017 — core has nothing there to reject.
//!
//! No `WIRE_VERSION` bump: the byte layout is unchanged and a bump would make every
//! published v14 decoder reject byte-identical frames. Measured before the change — 383
//! engine frames (6 recorded PTY captures × 3 geometries, plus 11 synthetic streams × 3
//! geometries × 4 states) violated none of these bounds, so nothing `encode` produces today
//! becomes undecodable. The capture half of that sweep is kept below as a standing test
//! (255 frames); the random half now lives in `robustness.rs`'s resize lane, which is where
//! the odd geometry actually comes from.

use justerm_core::{
    Color, CursorShape, DecodeError, Engine, Frame, FrameKind, ScrollOp, decode, encode,
};
use std::num::NonZeroU32;

/// Header offsets (`serialize.rs`): MAGIC(2) · VERSION(1) · has_scroll(1) · kind(1) ·
/// cols(2) · rows(2). Named rather than inlined so a header change breaks one place.
const COLS_AT: usize = 5;
const ROWS_AT: usize = 7;

/// Overwrite the *one* occurrence of `needle` in `buf` with `patch`, asserting there is
/// exactly one. The assert is the point: these fixtures reach a group entry by its bytes
/// rather than by a computed offset, and a layout change must fail loudly here instead of
/// silently patching some other field into the shape the test expects.
fn patch_unique(buf: &mut [u8], needle: &[u8], patch: &[u8]) {
    assert_eq!(
        needle.len(),
        patch.len(),
        "patch must not resize the buffer"
    );
    let hits: Vec<usize> = buf
        .windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one occurrence of {needle:?} in the wire buffer, found {}",
        hits.len()
    );
    buf[hits[0]..hits[0] + patch.len()].copy_from_slice(patch);
}

#[test]
fn a_span_reaching_past_the_declared_width_is_rejected() {
    // The issue's reproduction, verbatim: a genuinely 9-cell span, then the *header's*
    // declared width shrunk to 4. The payload stays internally valid, so nothing but a
    // bounds check can catch it.
    let mut e = Engine::new(9, 2);
    e.feed(b"abcdefghi");
    let frame = e.frame();
    assert_eq!(
        (frame.spans[0].left, frame.spans[0].right),
        (0, 8),
        "the fixture only means something if the span really spans all nine columns"
    );

    let mut bytes = encode(&frame);
    assert_eq!(decode(&bytes), Ok(frame), "unpatched, it round-trips");

    bytes[COLS_AT..COLS_AT + 2].copy_from_slice(&4u16.to_le_bytes());
    assert_eq!(
        decode(&bytes),
        Err(DecodeError::BadSpan),
        "a span whose last column is past the frame's own width is malformed input"
    );
}

#[test]
fn widening_the_span_within_the_frame_is_still_rejected_on_length_not_on_bounds() {
    // The trap the issue warns about, pinned so nobody re-derives it — and pinned in the
    // form that *survives* this change. The cell payload's length follows from
    // `right - left + 1`, so raising `right` demands cells that are not there and the
    // reader runs off the end before any bounds question is asked.
    //
    // The half of the trap that this change retires: widening `right` past `cols` now
    // stops at `BadSpan` instead, because the bounds check runs first. So the two failures
    // are ordered, and only a widening that stays *inside* the frame still reaches
    // `Truncated`. That is why this fixture damages three columns of a nine-column frame
    // rather than the whole row.
    let mut e = Engine::new(9, 2);
    e.feed(b"ab"); // deliberately short: filling the row parks the cursor at the last
    e.reset_damage(); // column, and the damage bracket then spans the full width
    e.feed(b"\x1b[2;1Hxyz"); // damage a few columns of the *other* row
    let frame = e.frame();
    // Take the narrowest span rather than the first: the damage bracket spans out to the
    // previous cursor column, so which row is narrow depends on where the cursor was.
    let narrow = frame
        .spans
        .iter()
        .min_by_key(|s| s.right - s.left)
        .expect("the frame has spans");
    let (line, left, right) = (narrow.line, narrow.left, narrow.right);
    assert!(
        right + 1 < frame.cols,
        "fixture: the span must be narrower than the frame ({right} vs {} cols) for the \
         widening to stay inside it",
        frame.cols
    );
    // Build the needle from the span itself rather than from a guess: the damage bracket
    // includes the previous cursor column, so the span is wider than the three characters
    // just written and hardcoding it silently patched nothing.
    let triple = |r: u16| {
        let mut v = line.to_le_bytes().to_vec();
        v.extend_from_slice(&left.to_le_bytes());
        v.extend_from_slice(&r.to_le_bytes());
        v
    };

    let mut bytes = encode(&frame);
    patch_unique(&mut bytes, &triple(right), &triple(frame.cols - 1));
    assert_eq!(
        decode(&bytes),
        Err(DecodeError::Truncated),
        "a widening that stays inside the frame is a length failure, not a bounds failure"
    );

    // And the other half, for the same fixture: past `cols`, bounds wins.
    let mut bytes = encode(&frame);
    patch_unique(&mut bytes, &triple(right), &triple(255));
    assert_eq!(decode(&bytes), Err(DecodeError::BadSpan));
}

#[test]
fn a_span_on_a_line_past_the_declared_height_is_rejected() {
    // The row axis. The issue established this one by reading the loop; it is measured
    // here — before the fix, a frame declaring one row carried spans on lines 0, 1 and 2
    // and decoded `Ok`.
    let mut e = Engine::new(4, 3);
    e.feed(b"aaaa\r\nbbbb\r\ncccc");
    let frame = e.frame();
    assert!(
        frame.spans.iter().any(|s| s.line == 2),
        "the fixture needs a span on the row the shrunk header will exclude"
    );

    let mut bytes = encode(&frame);
    bytes[ROWS_AT..ROWS_AT + 2].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(decode(&bytes), Err(DecodeError::BadSpan));
}

/// A minimal 3-row frame carrying nothing but the scroll op under test, so the assertion
/// cannot be satisfied by the span check instead.
fn frame_with_scroll(top: usize, bottom: usize) -> Frame {
    Frame {
        cols: 8,
        rows: 3,
        kind: FrameKind::Partial,
        cursor_row: 0,
        cursor_col: 0,
        cursor_visible: true,
        cursor_shape: CursorShape::Block,
        cursor_blink: false,
        display_offset: 0,
        scrollback_len: 0,
        mouse_events: Default::default(),
        alt_screen: false,
        scroll: Some(ScrollOp {
            top,
            bottom,
            count: 1,
        }),
        spans: vec![],
        link_table: vec![],
        overlay: Default::default(),
    }
}

#[test]
fn a_scroll_region_reaching_past_the_declared_height_is_rejected() {
    // The scroll op is a write index in a consumer, exactly like a span: `cell-mirror.ts`
    // hands `scrollTop/scrollBottom/scrollCount` to `shiftRegion`, which assigns
    // `cells[y * cols + x]` for every `y` in `top..=bottom` with nothing bounding it
    // against `rows`.
    //
    // The family already decided this in the other direction, which is why it belongs in
    // the same change rather than in a sibling issue: `justerm-renderer`'s
    // `FrameGrid::validate` rejects the same value with `DamageError::ScrollOutsideGrid`,
    // and its doc records why — a `line == rows` off-by-one trapped the wasm module and
    // left it poisoned for every later call (#355). Core is the one boundary in the family
    // that has no producer to trust and, until now, no check of its own.
    let frame = frame_with_scroll(0, 5);
    assert_eq!(decode(&encode(&frame)), Err(DecodeError::BadSpan));
}

#[test]
fn an_empty_scroll_region_is_left_alone_even_when_it_points_past_the_end() {
    // `top > bottom` describes no rows at all, so no consumer iterates it — the renderer's
    // validate() says so explicitly ("an empty region, not an error"). Pinned so a later
    // tightening has to argue with a test rather than with a comment: rejecting this would
    // be new strictness with no failure behind it.
    let frame = frame_with_scroll(5, 0);
    assert_eq!(decode(&encode(&frame)), Ok(frame));
}

/// A frame carrying one coloured underline at a known column, so the group entry can be
/// found by its bytes: `(col u16, colour u32)`, the colour packed by `encode_color`.
fn frame_with_one_ucolor() -> (justerm_core::Frame, u32) {
    let mut e = Engine::new(9, 2);
    e.feed(b"\x1b[4m\x1b[58:2::9:9:9mABCDEFGHI");
    let frame = e.frame();
    let packed = justerm_core::encode_color(Color::Rgb(9, 9, 9));
    (frame, packed)
}

#[test]
fn an_underline_colour_keyed_past_the_end_of_its_span_is_rejected() {
    let (frame, packed) = frame_with_one_ucolor();
    assert!(
        frame.spans[0].ucolors.contains_key(&0),
        "fixture: column 0 carries the colour"
    );
    let mut bytes = encode(&frame);

    let mut needle = 0u16.to_le_bytes().to_vec();
    needle.extend_from_slice(&packed.to_le_bytes());
    let mut patch = 9999u16.to_le_bytes().to_vec();
    patch.extend_from_slice(&packed.to_le_bytes());
    patch_unique(&mut bytes, &needle, &patch);

    assert_eq!(
        decode(&bytes),
        Err(DecodeError::BadSpan),
        "a group entry keyed outside the span it rides on is malformed input"
    );
}

#[test]
fn a_combining_cluster_keyed_past_the_end_of_its_span_is_rejected() {
    let mut e = Engine::new(4, 2);
    e.feed("ae\u{0301}f".as_bytes());
    let frame = e.frame();
    assert!(
        !frame.spans[0].combining.is_empty(),
        "fixture: the span carries a combining cluster"
    );
    let mut bytes = encode(&frame);

    // entry = (col u16, len u32, chars…): col 1, one char U+0301.
    let mut needle = 1u16.to_le_bytes().to_vec();
    needle.extend_from_slice(&1u32.to_le_bytes());
    needle.extend_from_slice(&0x0301u32.to_le_bytes());
    let mut patch = 9999u16.to_le_bytes().to_vec();
    patch.extend_from_slice(&1u32.to_le_bytes());
    patch.extend_from_slice(&0x0301u32.to_le_bytes());
    patch_unique(&mut bytes, &needle, &patch);

    assert_eq!(decode(&bytes), Err(DecodeError::BadSpan));
}

#[test]
fn a_hyperlink_reference_keyed_past_the_end_of_its_span_is_rejected() {
    let mut e = Engine::new(6, 2);
    e.feed(b"\x1b]8;;http://x\x07ab\x1b]8;;\x07");
    let frame = e.frame();
    assert_eq!(
        frame.spans[0].links.keys().copied().collect::<Vec<_>>(),
        vec![0, 1],
        "fixture: two adjacent linked cells, which is what makes the group findable below"
    );
    let mut bytes = encode(&frame);

    // The group is `count u32` then `(col u16, idx u32)` per entry. Match the count *and*
    // both entries: `(0, 1)` alone is six bytes of small integers that occur elsewhere in
    // the buffer (measured — three matches), and a patch applied to the wrong one would
    // silently test nothing.
    let entries = |first_col: u16| {
        let mut v = 2u32.to_le_bytes().to_vec();
        v.extend_from_slice(&first_col.to_le_bytes());
        v.extend_from_slice(&1u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&1u32.to_le_bytes());
        v
    };
    patch_unique(&mut bytes, &entries(0), &entries(9999));

    assert_eq!(decode(&bytes), Err(DecodeError::BadSpan));
}

/// A real engine frame with two impossible group keys grafted on through `Span`'s `pub`
/// fields — the only way to build one. `Term::frame` inserts `col - left` for `col` in
/// `left..=right`, and `decode` reads a `u16` and now rejects an out-of-range one, so
/// neither of the two paths that *produce* a `Span` can reach this state.
fn frame_with_keys_outside_their_span() -> Frame {
    let mut e = Engine::new(9, 2);
    // The hyperlink is here so `link_table` has a real entry for the grafted link key to
    // point at — all three groups must be exercised, not two. A mutation that unfiltered
    // the *link* write loop passed the whole suite until this fixture covered it.
    e.feed(b"\x1b]8;;http://x\x07\x1b[4m\x1b[58:5:1mABCDEFGHI\x1b]8;;\x07");
    let mut frame = e.frame();
    let idx = NonZeroU32::new(1).expect("1 is non-zero");
    let span = &mut frame.spans[0];
    span.ucolors.clear();
    span.ucolors.insert(2, Color::Indexed(1)); // in range: must survive
    span.ucolors.insert(65539, Color::Indexed(1)); // out of range: must not be written
    span.combining.clear();
    span.combining.insert(65540, vec!['\u{0301}']);
    span.links.clear();
    span.links.insert(1, idx); // in range: must survive
    span.links.insert(65541, idx); // out of range: must not be written
    frame
}

/// The detector half. An out-of-span key cannot come from the engine, so one reaching
/// `encode` is a justerm bug, and the assert accuses the *producer* at the site instead of
/// letting a dropped entry surface later as a missing colour nobody can trace. Same split
/// as `Term::damage_span`, which documents it in full.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "past the end of its")]
fn encode_names_the_producer_of_a_key_outside_its_span() {
    let _ = encode(&frame_with_keys_outside_their_span());
}

/// The backstop half — what a consumer's release build actually does. justerm is a
/// library, so a panic crosses into someone else's process; dropping the entry is the
/// failure direction that costs nothing visible.
///
/// **This one runs only under `cargo test --release`**, which the gate matrix does not
/// include — it is pinned, not gated. Verified by hand on this change (see the PR).
#[cfg(not(debug_assertions))]
#[test]
fn encode_drops_a_key_outside_its_span_rather_than_narrowing_it_onto_a_live_cell() {
    // The reason this is not merely symmetry with the decode side: the group's column is
    // `usize` in memory and `u16` on the wire, so `col as u16` did not *lose* the entry —
    // it moved it onto a different, live column. Measured before the fix:
    // `ucolors = {65539: …}` over a 9-cell span encoded to key 3 and (since #531's re-arm)
    // armed the live cell 'D'; `combining = {65540: …}` armed 'E'. ghostty states the rule
    // for the same situation in one line — "Never manufacture an out-of-bounds pin"
    // (`PageList.zig:4930`).
    let frame = frame_with_keys_outside_their_span();
    let decoded = decode(&encode(&frame)).expect("the encoded frame is still decodable");
    let dspan = &decoded.spans[0];

    assert_eq!(
        dspan.ucolors.keys().copied().collect::<Vec<_>>(),
        vec![2],
        "the in-range entry survives and the impossible one is not written at all"
    );
    assert!(
        dspan.combining.is_empty(),
        "the same rule holds for every group, not just the one the issue named"
    );
    let armed: Vec<(usize, char)> = dspan
        .cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_ucolored())
        .map(|(i, c)| (i, c.c()))
        .collect();
    assert_eq!(
        armed,
        vec![(2, 'C')],
        "and no unrelated live glyph is coloured — 65539 used to land on 'D' at column 3"
    );
    assert!(
        !dspan.cells.iter().any(|c| c.is_combined()),
        "nor marked as carrying a cluster it does not have"
    );
    assert_eq!(
        dspan.links.keys().copied().collect::<Vec<_>>(),
        vec![1],
        "the link group answers the same way — every group, or the rule is not a rule"
    );
    let linked: Vec<usize> = dspan
        .cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_linked())
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        linked,
        vec![1],
        "and 65541 does not narrow onto column 5 and link a glyph that carries no URI"
    );
}

#[test]
fn no_frame_this_engine_produces_is_rejected_by_its_own_decoder() {
    // The change's central safety claim, made permanent. It was measured once as a
    // throwaway probe — 383 frames, zero violations — and a deleted probe is a claim with
    // a decaying half-life: the checks bound *published* behaviour, so the day a new verb
    // emits an out-of-frame span the failure is a consumer's frames going dark, not a red
    // test. This replays the recorded PTY captures (vim, htop, top, less, `ls
    // --hyperlink`, a kitty/neovim stream) at three geometries, taking a frame every 512
    // bytes, and asserts each one survives its own wire.
    //
    // Not a substitute for `robustness.rs`'s random lane, which starts from arbitrary
    // *bytes*; this one starts from real terminal output, which is the material that
    // actually shapes damage spans, scroll ops and the sparse groups.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let mut raws: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .expect("fixtures dir")
        .filter_map(|e| {
            let p = e.expect("dir entry").path();
            (p.extension().is_some_and(|x| x == "raw")).then_some(p)
        })
        .collect();
    raws.sort();
    assert!(
        raws.len() >= 6,
        "the corpus is the evidence: {} captures found",
        raws.len()
    );

    let mut frames = 0usize;
    for path in &raws {
        let bytes = std::fs::read(path).expect("capture");
        for (cols, rows) in [(80usize, 24usize), (40, 10), (2, 2)] {
            let mut e = Engine::new(cols, rows);
            for chunk in bytes.chunks(512) {
                e.feed(chunk);
                let frame = e.frame();
                // `is_ok()`, not equality — and the distinction was measured, not assumed.
                // `C_LEADING_SPACER` is engine-internal and never reaches the wire
                // (`cell.rs`: "stays in the content word and never reaches `flags()` / the
                // wire"), so a frame holding one decodes fine and is not a fixed point.
                // Real captures contain them; asserting equality here pinned a contract the
                // engine deliberately does not hold. The claim under test is rejection.
                assert!(
                    decode(&encode(&frame)).is_ok(),
                    "{} at {cols}x{rows} produced a frame its own decoder rejects",
                    path.display()
                );
                frames += 1;
            }
        }
    }
    // A floor, not the count: the corpus grows as captures are recorded, and a test that
    // pins the exact number breaks on the addition rather than on the defect. 255 today.
    assert!(frames >= 250, "only {frames} frames checked");
}

#[test]
fn an_engine_frame_with_every_group_populated_is_still_a_wire_fixed_point() {
    // The guard on the guard: the new rejection must not touch what the engine actually
    // produces. Driven from a real `Engine` rather than from decoded bytes, because the
    // round-trip property in `tests/robustness.rs` starts from a *decoded* frame and is
    // therefore blind by construction to an encode-side change (measured on #531).
    let mut e = Engine::new(12, 3);
    e.feed(b"\x1b]8;;http://x\x07li\x1b]8;;\x07");
    e.feed("\x1b[4m\x1b[58:5:2mne\u{0301}s\r\n".as_bytes());
    e.feed(b"second row\r\n");
    let frame = e.frame();
    let span = &frame.spans[0];
    assert!(
        !span.links.is_empty() && !span.ucolors.is_empty(),
        "fixture: the frame must actually exercise the groups it claims to"
    );
    assert!(
        frame.spans.iter().any(|s| !s.combining.is_empty()),
        "fixture: and a combining cluster somewhere in the frame"
    );

    assert_eq!(decode(&encode(&frame)), Ok(frame));
}
