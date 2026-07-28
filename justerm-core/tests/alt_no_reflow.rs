//! The alt screen resizes but does not **reflow** (#567).
//!
//! Reflow re-splits a long line so history stays readable at the new width. It assumes the content
//! is text that *flows*. The alt screen breaks that assumption three ways at once: it has no
//! history to make readable, its content is a **layout** rather than a paragraph (re-wrapping
//! htop's columns means nothing), and the application already knows the new size and repaints.
//!
//! All three references agree, with the same shape — one flag on the same resize function:
//! ghostty `alt.resize(.{ .reflow = false })` (`terminal/Terminal.zig`), alacritty
//! `grid.resize(!is_alt, …)` / `inactive_grid.resize(is_alt, …)` (`term/mod.rs:677-678`), xterm.js
//! `_isReflowEnabled → _hasScrollback` with the alt buffer built as `new Buffer(false, …)`
//! (`common/buffer/Buffer.ts:315-320`, `BufferSet.ts:47`).
//!
//! justerm did it anyway, and never decided to: `8f09d58` needed both screens to end up at the new
//! *dimensions* (leaving alt used to restore an old-sized grid — a mismatch that could panic a
//! damage-driven render), reached for the `reflow_pane` helper, and re-splitting came along with it.
//! That dimension fix is untouched here; only the re-split stops.

use justerm_core::Engine;

const HTOP_PRE: &[u8] = include_bytes!("fixtures/alt_resize_htop.pre.raw");
const HTOP_POST: &[u8] = include_bytes!("fixtures/alt_resize_htop.post.raw");
const VIM_PRE: &[u8] = include_bytes!("fixtures/alt_resize_vim.pre.raw");
const VIM_POST: &[u8] = include_bytes!("fixtures/alt_resize_vim.post.raw");

#[test]
fn an_alt_line_keeps_its_wrap_across_a_column_change() {
    // The positive contract. At 4 columns `"abcdefgh"` is a wrapped pair; widening to 8 would let
    // it fit on one row, and on the primary screen it would. Here it must not move, because nothing
    // may re-split a layout the application is about to repaint.
    let mut t = Engine::new(4, 4);
    t.feed(b"\x1b[?1049h");
    t.feed(b"abcdefgh");
    assert!(
        t.grid().is_row_wrapped(0),
        "fixture: row 0 wraps into row 1"
    );

    t.resize(8, 4);

    assert!(
        t.grid().is_row_wrapped(0),
        "the alt grid re-fits its rows; it does not re-split them"
    );
    assert_eq!(t.grid().cols(), 8, "but the dimensions still change");
    assert_eq!(t.grid().rows(), 4);
}

#[test]
fn the_primary_screen_still_reflows() {
    // The control that keeps this change from being read as "resize stopped reflowing". The same
    // bytes on the primary screen do unwrap, because there the content *is* a flow and there is
    // history to keep readable.
    let mut t = Engine::new(4, 4);
    t.feed(b"abcdefgh");
    assert!(t.grid().is_row_wrapped(0), "fixture");

    t.resize(8, 4);

    assert!(
        !t.grid().is_row_wrapped(0),
        "the primary line unwraps onto one row"
    );
}

#[test]
fn the_inactive_alt_grid_does_not_reflow_either() {
    // A resize taken from the primary screen also resizes the alt grid, and that half must follow
    // the same rule — otherwise the content re-splits while nobody is looking and the application
    // finds a layout it never drew when it switches back.
    let mut t = Engine::new(4, 6);
    t.feed(b"\x1b[?1049h");
    t.feed(b"abcdefgh");
    t.feed(b"\x1b[?1049l"); // back to primary; the alt grid keeps its content until re-entered

    t.resize(8, 6);
    t.feed(b"\x1b[?1049h");

    assert_eq!(t.grid().cols(), 8, "the alt grid took the new dimensions");
}

// ---------------------------------------------------------------------------
// the real round-trip: a live `SIGWINCH` mid-session (#567)
// ---------------------------------------------------------------------------
//
// Recorded on the RHEL VM with `expect`: the app is spawned on an 80x24 pty, the pty is narrowed to
// 40 columns — which is what raises `SIGWINCH` in the child — and the log is switched to a second
// file at exactly that moment. So `*.pre.raw` is the session up to the resize and `*.post.raw` is
// the application's own response to it. Replaying `pre → resize(40, 24) → post` is the only way to
// see what a user would actually see, because the answer depends on what the app does next.

#[test]
fn a_real_htop_repaint_is_not_polluted_by_a_re_split() {
    // htop repaints **without erasing** — measured, its response to `SIGWINCH` contains no
    // Erase-in-Display at all, only CUP-addressed overwrites. So every cell it does not write keeps
    // whatever was there, and re-splitting the alt grid at resize time leaves the old 80-column
    // layout fragmented across rows htop never touches. This is the measurement that made #567 a
    // correctness fix rather than a tidy-up: with the re-split, two unrelated rows fuse into
    // `0:00.08 /usr/li0/0ys201  77  root …`.
    assert_eq!(
        HTOP_POST
            .windows(4)
            .filter(|w| *w == b"\x1b[2J" || *w == b"\x1b[0J" || *w == b"\x1b[1J")
            .count(),
        0,
        "fixture: htop's SIGWINCH response erases nothing"
    );

    let mut t = Engine::new(80, 24);
    t.feed(HTOP_PRE);
    t.resize(40, 24);
    t.feed(HTOP_POST);

    let text = t.accessible_text();
    assert!(
        !text.contains("/usr/li0/0ys201"),
        "a re-split left debris in the cells htop did not repaint:\n{text}"
    );
}

#[test]
fn a_real_vim_repaint_is_unaffected_either_way() {
    // The counterpart, and the reason this had to be measured on two applications rather than one:
    // vim's response to `SIGWINCH` opens with `ESC[H ESC[2J` and repaints every row, so it overwrites
    // whatever the resize did and both designs agree exactly. Measured before the change and after —
    // identical final text. An app that clears cannot tell the difference; that is not evidence the
    // difference does not exist.
    assert!(
        VIM_POST.windows(4).any(|w| w == b"\x1b[2J"),
        "fixture: vim's SIGWINCH response clears the screen"
    );

    let mut t = Engine::new(80, 24);
    t.feed(VIM_PRE);
    t.resize(40, 24);
    t.feed(VIM_POST);

    assert!(
        t.accessible_text()
            .starts_with("The quick brown fox jumps over the lazy dog"),
        "vim's own repaint decides the screen: {:?}",
        t.accessible_text()
    );
}
