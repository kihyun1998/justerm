//! #660 — a selection made *on* the alt screen must not outlive a resize.
//!
//! `Engine::frame()` panicked: on the alt screen, extending a selection to the last
//! viewport row and then shrinking the grid made `selection_range()` index past the end
//! of the grid (`grid.rs`, "index out of bounds: the len is 2 but the index is 2"),
//! reached through four ordinary public calls.
//!
//! **The premise that broke, and where it is written down.** `Term::resize`'s alt branch
//! does not re-anchor the selection, and says why: *"the selection is already cleared on
//! alt enter"*. That is true — `self.selection = None` runs on both screen swaps
//! (`term.rs`, *"a selection cannot survive a screen swap"*) — but it says nothing about a
//! selection made **while the alt screen is already up**, which is the ordinary act of
//! copying text out of vim or htop. A premise that holds at one instant was read as an
//! invariant holding for the lifetime of the screen.
//!
//! **The variable was isolated before the fix, and the first theory was wrong.** The
//! obvious reading is that the primary screen survives because a shrink pushes rows into
//! scrollback, so the stale absolute index lands in history instead of past the grid.
//! Measured, that is false — a primary engine with `with_scrollback(.., 0)` does *not*
//! panic:
//!
//! ```text
//! alt, default scrollback              PANIC
//! primary, default scrollback          OK   spans=1
//! primary, scrollback limit 0          OK   spans=2
//! alt, selection inside shrunk grid    OK
//! ```
//!
//! What actually separates them is **reflow**: the primary pane re-anchors user-authored
//! points through `reflow_pane` (`term.rs`, *"The selection re-anchors below — it is
//! user-authored"*), and the alt pane deliberately does not reflow at all. So the anchor
//! keeps its old absolute line, and nothing brings it back into range.
//!
//! **Reference posture — both speak, on complementary axes, and neither lets a selection
//! survive a resize unchanged** (pinned trees, verified 2026-07-31):
//!
//! - alacritty clears on a **width** change and rotates by the line delta otherwise —
//!   `if old_cols != num_cols { self.selection = None }`
//!   (`alacritty_terminal/src/term/mod.rs:680-682`);
//! - xterm.js clears on a **height** change, and its comment names this bug class
//!   directly: *"Clear selection when resizing vertically. This experience could be
//!   improved, this is the simple option to fix the buggy behavior"*, citing its own issue
//!   5300 (`src/browser/services/SelectionService.ts:156-160`).
//!
//! justerm's primary path is already alacritty's rotate branch, done through reflow. The
//! alt path is the one with no answer, and the repro is a rows shrink — xterm.js's exact
//! case, where the answer is to drop it.
//!
//! **Dropping is a policy choice, not a capability limit — and the first version of this
//! file said otherwise, which is worth keeping visible.** It claimed the alt pane has
//! "nothing to re-anchor through". Measurably false: the alt branch of `resize` makes its
//! own `reflow_pane` call *with* tracked points and already uses the returned `extras` /
//! `evicted` to rotate and dispose alt markers — `reflow: false` disables the column
//! re-split, not the tracking. What makes dropping the right trade is that a marker is one
//! point with a binary fate while a selection is two ordered endpoints, so a shrink that
//! destroys the row under one and not the other has no "dispose" answer. Installing a false
//! *"cannot"* as the justification for fixing a false *"cannot"* is this issue's own failure
//! mode, one layer up.
//!
//! **Two routes the anchor-side fix does not reach**, both found by the completeness pass
//! and both measured before being covered below: an out-of-range viewport row needs no
//! resize and no alt screen at all (`selection_extend(3, …)` on a 3-row grid panicked in
//! `frame()`), and `selection_range` read `abs_line` *before* its own visibility filter
//! while its sibling `match_spans` bounds first. Fixed at the anchor (`viewport_to_abs`
//! clamps) and at the read (the loop bounds), respectively.
//!
//! **A third route survived both of those and the pass's own sweep table, and was found by
//! hammering instead of reading** — see `growing_the_alt_screen_drops_it_too_and_not_for_the
//! _obvious_reason`. An alt resize reflows the *primary* pane, whose history is what
//! `scrollback` holds while the alt screen is up, so the base an anchor is measured from
//! moves even on a **grow** that touches no alt row; `selection_text` then walked off the
//! end through `extract_lines`, a reader neither of the fixes above covers. That is why the
//! gate is any geometry change rather than a shrink, and why the sweep is checked in
//! (`no_verb_leaves_an_anchor_the_readers_cannot_walk`) rather than deleted.

use justerm_core::{Engine, SelectionType, Side};

/// Real `SIGWINCH` recordings from the RHEL VM, already in the repo for `alt_no_reflow`:
/// a full-screen application's own bytes before and after the terminal was resized under
/// it. Synthetic alt-screen material cannot stand in for these — the panic needs a
/// selection over content an *application* drew, and the repaint that follows is what
/// says the engine is still usable afterwards rather than merely not crashing.
const HTOP_PRE: &[u8] = include_bytes!("fixtures/alt_resize_htop.pre.raw");
const HTOP_POST: &[u8] = include_bytes!("fixtures/alt_resize_htop.post.raw");
const VIM_PRE: &[u8] = include_bytes!("fixtures/alt_resize_vim.pre.raw");
const VIM_POST: &[u8] = include_bytes!("fixtures/alt_resize_vim.post.raw");

/// An alt screen with three rows of content and a selection covering all of it.
fn alt_engine_with_a_full_selection() -> Engine {
    let mut e = Engine::new(4, 3);
    e.feed(b"\x1b[?1049h"); // alt screen
    e.feed(b"abc\r\ndef\r\nghi");
    e.selection_begin(0, 0, Side::Left, SelectionType::Char);
    e.selection_extend(2, 3, Side::Right); // the last viewport row
    e
}

#[test]
fn an_alt_screen_selection_does_not_survive_the_grid_shrinking_under_it() {
    let mut e = alt_engine_with_a_full_selection();
    assert!(
        !e.selection_range().is_empty(),
        "fixture: the selection must exist before the resize for this to assert anything"
    );

    e.resize(3, 2); // shrink rows 3 -> 2

    assert!(
        e.selection_range().is_empty(),
        "a shrink on a pane that does not reflow drops it rather than moving its ends by \
         two different rules"
    );
    assert_eq!(
        e.selection_text(),
        None,
        "and the text extraction agrees — a half-dropped selection is worse than none"
    );
}

#[test]
fn frame_still_builds_after_the_alt_screen_shrinks_under_a_selection() {
    // The user-visible half: `frame()` calls `selection_range()` (`term.rs`), so the panic
    // was not confined to a caller that asks for the selection — an alt-screen app plus a
    // live selection plus a window resize is an ordinary sequence, and justerm is a
    // library, so the panic crossed into the consumer's process.
    let mut e = alt_engine_with_a_full_selection();
    e.resize(3, 2);

    let frame = e.frame();
    assert_eq!((frame.cols, frame.rows), (3, 2));
    assert!(frame.overlay.selection.is_empty());
}

#[test]
fn a_width_only_shrink_on_the_alt_screen_drops_it_too() {
    // alacritty's axis. The alt pane does not reflow, so a narrower grid truncates each
    // row and an anchor's *column* goes stale exactly as its line does on the other axis —
    // there is no re-anchoring on either.
    let mut e = alt_engine_with_a_full_selection();
    e.resize(2, 3);

    assert!(e.selection_range().is_empty());
}

#[test]
fn a_resize_that_changes_nothing_leaves_an_alt_selection_alone() {
    // The side condition that stops the fix from being "clear the selection whenever
    // anyone calls resize": a no-op resize is not a resize. Without this, a consumer that
    // re-asserts its size on every frame (a `fit()` loop) would silently make selection on
    // the alt screen impossible, and every test above would still pass.
    let mut e = alt_engine_with_a_full_selection();
    let before = e.selection_range();
    e.resize(4, 3);

    assert_eq!(
        e.selection_range(),
        before,
        "same geometry, same selection — nothing moved under it"
    );
}

#[test]
fn a_primary_screen_selection_still_re_anchors_through_a_resize() {
    // The regression guard on the other side: the primary pane *does* reflow, and its
    // re-anchoring is the behaviour a user expects (the selection follows the text). This
    // fix must not reach it.
    let mut e = Engine::new(4, 3);
    e.feed(b"abc\r\ndef\r\nghi");
    e.selection_begin(0, 0, Side::Left, SelectionType::Char);
    e.selection_extend(2, 3, Side::Right);
    assert!(!e.selection_range().is_empty());

    e.resize(3, 2);

    assert!(
        !e.selection_range().is_empty(),
        "a reflowing pane re-anchors the selection instead of dropping it"
    );
}

#[test]
fn an_alt_selection_survives_everything_that_is_not_a_resize() {
    // Scoping in the other direction: scrolling the alt screen's own region moves content
    // under the selection and the engine already rotates the anchors with it (#162). A fix
    // that cleared on *any* buffer movement would break that, and no test above would say
    // so.
    let mut e = alt_engine_with_a_full_selection();
    let before = e.selection_range();

    // `\x1b[1;3r` homes the cursor, so a bare `\n` from there moves it down and scrolls
    // nothing — the first version of this test asserted movement that never happened.
    // Park the cursor on the last row of the region first.
    e.feed(b"\x1b[1;3r\x1b[3;1H");
    e.feed(b"\n"); // now the region really scrolls

    assert!(
        !e.selection_range().is_empty(),
        "content moved under the selection; the anchors rotate, they do not vanish"
    );
    assert_ne!(
        e.selection_range(),
        before,
        "and they really did move — an unchanged range would mean the rotation never ran"
    );
}

/// Step 4 — the real round-trip, on recorded PTY material rather than a synthetic stream.
///
/// The sequence is the one a user performs: a full-screen app is up, they drag a selection
/// across what it drew to copy it, and the window is resized under them (`SIGWINCH`). The
/// capture supplies both halves — the app's screen before, and its own repaint after — so
/// what is exercised is the engine's behaviour *between* two real byte streams.
///
/// Two assertions, and the second is the one that makes this more than a no-panic smoke
/// test: the app's post-resize repaint must still land, i.e. the engine is usable after
/// dropping the selection, not merely alive.
fn a_real_app_selection_survives_a_real_sigwinch(pre: &[u8], post: &[u8], label: &str) {
    let mut t = Engine::new(80, 24);
    t.feed(pre);
    assert!(
        t.grid().rows() == 24,
        "fixture: {label} is on a 24-row screen"
    );

    // Drag across the whole visible screen, ending on the last row — the anchor position
    // that made the shrink panic.
    t.selection_begin(0, 0, Side::Left, SelectionType::Char);
    t.selection_extend(23, 79, Side::Right);
    assert!(
        !t.selection_range().is_empty(),
        "fixture: {label} must have a live selection before the resize"
    );

    t.resize(40, 18); // both axes, and rows shrink — the reported case
    let frame = t.frame(); // panicked here before the fix (`frame` calls `selection_range`)

    assert!(
        frame.overlay.selection.is_empty(),
        "{label}: the anchor had no re-anchoring path, so it goes"
    );
    t.feed(post);
    assert!(
        !t.accessible_text().trim().is_empty(),
        "{label}: the application's own repaint still lands after the drop"
    );
}

#[test]
fn a_real_htop_selection_survives_a_real_sigwinch() {
    a_real_app_selection_survives_a_real_sigwinch(HTOP_PRE, HTOP_POST, "htop");
}

#[test]
fn a_real_vim_selection_survives_a_real_sigwinch() {
    a_real_app_selection_survives_a_real_sigwinch(VIM_PRE, VIM_POST, "vim");
}

// ---- the routes an anchor-side fix cannot reach (found by the completeness pass) --------

#[test]
fn a_selection_row_past_the_last_visible_one_is_clamped_at_the_anchor() {
    // No alt screen, no resize — just a row one past the end, which is what a pointer in
    // the sub-cell strip below the grid produces. Measured before the clamp: `row = 3` on a
    // 3-row grid panicked in `frame()`, and `row = 99` panicked in `selection_range`,
    // `selection_text` and the word extents too. The anchor-side fix above cannot reach any
    // of it, because none of it goes through `resize`.
    let mut e = Engine::new(4, 3);
    e.feed(b"abc\r\ndef\r\nghi");
    e.selection_begin(0, 0, Side::Left, SelectionType::Char);
    e.selection_extend(3, 3, Side::Right); // one past the last row

    let spans = e.selection_range();
    assert_eq!(
        spans.len(),
        3,
        "clamped to the last row, so the selection covers the whole visible grid"
    );
    assert!(
        spans.iter().all(|s| s.row < 3),
        "and nothing lands off-grid"
    );
    assert!(e.selection_text().is_some());
    let _ = e.frame();
}

#[test]
fn a_wildly_out_of_range_selection_row_is_survivable_on_every_read() {
    // The same clamp seen from the four readers that each walk the anchor differently —
    // one test per read path, because each reaches `abs_line` by its own route and the
    // first version of this fix only made *one* of them total.
    for ty in [
        SelectionType::Char,
        SelectionType::Word,
        SelectionType::Line,
        SelectionType::Block,
    ] {
        let mut e = Engine::new(4, 3);
        e.feed(b"abc\r\ndef\r\nghi");
        e.selection_begin(0, 0, Side::Left, ty);
        e.selection_extend(99, 3, Side::Right);

        let spans = e.selection_range();
        assert!(
            spans.iter().all(|s| s.row < 3),
            "{ty:?}: every span is on the grid"
        );
        let _ = e.selection_text();
        let _ = e.accessible_text();
        let _ = e.frame();
    }
}

#[test]
fn growing_the_alt_screen_drops_it_too_and_not_for_the_obvious_reason() {
    // The refinement that looks obviously right and is not, kept as a test because the
    // reasoning is the valuable part. A grow *pads* the alt pane rather than moving its
    // content, so no anchor looks invalidated, and an earlier version of this fix gated on
    // `<` to keep the selection here.
    //
    // A randomised sweep refuted that in one run. An alt resize also reflows the **primary**
    // pane, and on the alt screen `scrollback` *is* that primary history — so
    // `scrollback.len()` moves under an anchor whose absolute line was measured from the old
    // base, even though the alt grid itself never moved, and `selection_text` then walks off
    // the end. `resize`'s own marker path converts alt marks through `old_base` → new base
    // for precisely this reason and says so; the selection has no such conversion, which is
    // why the trigger is the geometry change rather than the direction of it.
    let mut e = alt_engine_with_a_full_selection();
    assert!(
        !e.selection_range().is_empty(),
        "fixture: a live selection first"
    );

    e.resize(4, 6); // grow rows: the alt grid pads, the primary below still rewraps
    assert!(
        e.selection_range().is_empty(),
        "a grow moves the base the anchor is measured from, so it goes too"
    );
    assert_eq!(e.selection_text(), None);
}

#[test]
fn no_verb_leaves_an_anchor_the_readers_cannot_walk() {
    // Standing coverage for the claim this change actually makes — "the panic class is
    // closed" — rather than for the one bug that started it. A completeness pass produced a
    // fourteen-row table concluding no path other than an alt resize could strand an anchor;
    // this hammer refuted it in one run by reaching `selection_text` through a *grow*, which
    // no row of that table covered. A negative result about a space is worth exactly as much
    // as the search that produced it, so the search is checked in.
    //
    // Deterministic: the LCG is seeded from the loop counter, so a failure names a seed.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
            &xs[(self.next() as usize) % xs.len()]
        }
    }

    // Every verb that moves content, changes geometry, or swaps a screen.
    const VERBS: &[&[u8]] = &[
        b"\x1b[?1049h",
        b"\x1b[?1049l",
        b"\x1b[?47h",
        b"\x1b[?1047l",
        b"\x1b[2J",
        b"\x1b[3J",
        b"\x1b[1J",
        b"\x1b[2K",
        b"\x1b[1;3r",
        b"\x1b[2;4r",
        b"\x1b[r",
        b"\x1b[S",
        b"\x1b[T",
        b"\x1b[L",
        b"\x1b[M",
        b"\x1bM",
        b"\x1b[?3h",
        b"\x1b[?3l",
        b"\x1bc",
        b"\x1b[!p",
        b"hello\r\n",
        b"\n\n\n",
        b"\x1b[10;10H",
        "wide 中文 text\r\n".as_bytes(),
    ];
    let types = [
        SelectionType::Char,
        SelectionType::Word,
        SelectionType::Line,
        SelectionType::Block,
    ];
    let sizes = [(2usize, 2usize), (4, 3), (9, 5), (20, 8), (80, 24)];

    for seed in 0..400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (c0, r0) = *rng.pick(&sizes);
        let mut e = if seed % 3 == 0 {
            Engine::with_scrollback(c0, r0, (seed % 7) as usize)
        } else {
            Engine::new(c0, r0)
        };
        e.feed(b"one\r\ntwo\r\nthree\r\nfour\r\n");
        e.selection_begin(0, 0, Side::Left, *rng.pick(&types));
        e.selection_extend(r0 - 1, c0 - 1, Side::Right);

        for step in 0..24 {
            match rng.next() % 10 {
                0 => {
                    let (c, r) = *rng.pick(&sizes);
                    e.resize(c, r);
                }
                1 => e.scroll_up((rng.next() % 5) as usize),
                2 => e.scroll_down((rng.next() % 5) as usize),
                3 => e.scroll_to_bottom(),
                4 => {
                    e.add_marker(e.grid().rows() - 1);
                }
                5 => {
                    let hits = e.search("o");
                    e.set_search_highlights(hits);
                }
                6 => {
                    // Re-anchor, deliberately out of range sometimes — the clamp's job.
                    let r = (rng.next() as usize) % (e.grid().rows() + 3);
                    let c = (rng.next() as usize) % (e.grid().cols() + 3);
                    e.selection_begin(0, 0, Side::Left, *rng.pick(&types));
                    e.selection_extend(r, c, Side::Right);
                }
                _ => e.feed(rng.pick(VERBS)),
            }

            // Every reader that walks the anchor, on every step.
            let (rows, cols) = (e.grid().rows(), e.grid().cols());
            for s in e.selection_range() {
                assert!(
                    s.row < rows && s.right < cols,
                    "seed {seed} step {step}: span {s:?} outside {cols}x{rows}"
                );
            }
            let _ = e.selection_text();
            let _ = e.accessible_text();
            let _ = e.frame();
        }
    }
}
