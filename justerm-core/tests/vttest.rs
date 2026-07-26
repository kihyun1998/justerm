//! Issue #8 — a vttest-*style* conformance harness.
//!
//! Real vttest is interactive/visual and esctest needs a query/response path
//! the engine does not have yet (DSR/CPR replies — #11/#12). So this is an
//! in-process, data-driven net: feed known VT input, dump the whole screen, and
//! compare against an inline golden. Unlike the per-cell assertions in
//! `vt_compliance.rs`, a full-screen golden catches changes *anywhere* — the
//! "systematic net" that surfaces hidden state we did not think to assert.
//!
//! This file is a growing net: add cases as dogfood reveals tail behaviour.
//!
//! **A capture is pinned on two surfaces, not one** (#554). The char dump below is
//! blind to the soft-wrap link, because that link is not a character — it decides
//! whether two rows are *one logical line*, so a capture whose lines are silently
//! merged produces a byte-identical dump. That blindness was measured, not
//! theorised: throughout #540 every golden here stayed green while the engine
//! merged two unrelated logical lines. Capture tests therefore go through
//! `check_capture`, which pins the char grid **and** the logical lines in one call
//! so neither half can be added without the other.

use justerm_core::Engine;

/// Render the active screen to a deterministic text dump: one bar-delimited line
/// per row (so trailing spaces stay visible), then a cursor line. Chars + cursor
/// only — attributes/colours are a later layer.
fn dump(term: &Engine) -> String {
    let grid = term.grid();
    let mut s = String::new();
    for row in 0..grid.rows() {
        s.push('|');
        for col in 0..grid.cols() {
            s.push(grid.cell(row, col).c());
        }
        s.push_str("|\n");
    }
    let cur = term.cursor();
    s.push_str(&format!(
        "cursor=({},{}) visible={}\n",
        cur.row, cur.col, cur.visible
    ));
    s
}

/// Render the buffer's **logical** lines — soft-wrapped rows joined back into the
/// one line they belong to, which is what copy, search and accessible text see.
///
/// This is the surface `dump` structurally cannot show. The wrap link is not a
/// character: it decides whether two rows are one line, so a capture whose logical
/// lines are silently merged produces a byte-identical char dump. Throughout #540
/// the char goldens were green while `accessible_text` returned `"abcdefghmnop"`
/// for what should have been two lines.
///
/// Both references keep the two surfaces separate for the same reason — ghostty's
/// dump takes an explicit `unwrap` option (*"Whether to unwrap soft-wrapped
/// lines"*, `terminal/formatter.zig:88-90` @ `e6e26e1`) and its resize/reflow tests
/// assert the wrapped and unwrapped forms of one screen.
fn logical_lines(term: &Engine) -> String {
    let mut s = String::from("--- logical lines ---\n");
    for line in term.accessible_text().lines() {
        s.push_str(&format!("{:?}\n", line.trim_end()));
    }
    s
}

/// Pin a **capture** on both surfaces at once: the char grid and the logical lines.
///
/// One call, so a capture cannot be added with only half its goldens — which is the
/// failure this helper exists to prevent, and the one #554 records: the corpus was
/// blind to an entire class of defect because every golden asserted characters only.
/// Deliberately not folded into `dump`: nine synthetic `check` cases share that
/// function, and a logical-line section means nothing at `check(5, 2, b"hi", …)`
/// scale while forcing all nine expectations to be rewritten.
fn check_capture(term: &Engine, chars: &str, logical: &str) {
    assert_eq!(dump(term), chars, "char grid");
    assert_eq!(logical_lines(term), logical, "logical lines");
}

/// Feed `input` into a fresh `cols`×`rows` engine and assert its screen dump
/// equals `expected`.
fn check(cols: usize, rows: usize, input: &[u8], expected: &str) {
    let mut term = Engine::new(cols, rows);
    term.feed(input);
    assert_eq!(dump(&term), expected);
}

#[test]
fn print_basic() {
    check(
        5,
        2,
        b"hi",
        "\
|hi   |
|     |
cursor=(0,2) visible=true
",
    );
}

/// Autowrap: the 4th char wraps to the next row (deferred last-column wrap).
#[test]
fn autowrap() {
    check(
        3,
        2,
        b"abcd",
        "\
|abc|
|d  |
cursor=(1,1) visible=true
",
    );
}

/// Pending-wrap: a case that ends the instant the last column is filled. The
/// cursor line distinguishes deferred wrap (parked at the last column) from an
/// eager wrap (already moved to the next row) — which the `autowrap` case above
/// cannot, because it writes past the boundary and the two converge.
#[test]
fn pending_wrap_parks_cursor() {
    check(
        3,
        2,
        b"abc",
        "\
|abc|
|   |
cursor=(0,2) visible=true
",
    );
}

/// HT advances to the 8-column tab stop.
#[test]
fn tab_stop() {
    check(
        20,
        1,
        b"\tX",
        "\
|        X           |
cursor=(0,9) visible=true
",
    );
}

/// Scroll region: IND at the bottom margin scrolls rows [2..=3] up; the rows
/// outside the region (A at top, D at bottom) stay fixed.
#[test]
fn scroll_region_index() {
    check(
        4,
        4,
        b"\x1b[2;3r\x1b[1;1HA\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[3;1H\x1bD",
        "\
|A   |
|C   |
|    |
|D   |
cursor=(2,0) visible=true
",
    );
}

/// Alt-screen: after ?1049h the screen is fresh; the primary content (AB) is
/// hidden and only the alt write (Z) shows.
#[test]
fn alt_screen() {
    check(
        5,
        2,
        b"AB\x1b[?1049h\x1b[1;1HZ",
        "\
|Z    |
|     |
cursor=(0,1) visible=true
",
    );
}

// ===========================================================================
// #20 dogfood — synthetic "editor redraw" goldens combining the editing CSIs.
// A full-screen golden catches cross-interactions a per-op test cannot. (The
// real captured vim/htop streams are the HITL remainder of #20.)
// ===========================================================================

/// #20 dogfood — REAL captured `htop` session (80x24, EPEL on RHEL; see
/// tests/fixtures/capture-dogfood.sh). htop is an ncurses TUI on the alt screen
/// (?1049h), so this snapshots just before it leaves (?1049l), like the vim
/// alt-screen golden. It exercises a heavier mix than top: SGR colour gauges
/// (the per-core `[||  1.3%]` meters and Mem/Swp bars), G0 charset designation
/// (ESC(B), a non-ASCII sort glyph (▽ in the `CPU%▽MEM%` header), VPA (CSI d),
/// SGR mouse-tracking enable/disable (?1006;1000h/l), and a teardown
/// Erase-in-Display: on SIGINT htop homes to row 24 and emits CSI J, wiping the
/// F1..F10 function-key bar — so the bottom row is intentionally blank here.
/// Frozen bytes keep the snapshot deterministic despite htop's live data.
#[test]
fn htop_capture_redraw() {
    let raw = include_bytes!("fixtures/htop.raw");
    let altcut = raw.windows(8).position(|w| w == b"\x1b[?1049l").unwrap();
    let mut term = Engine::new(80, 24);
    term.feed(&raw[..altcut]);
    check_capture(
        &term,
        include_str!("fixtures/htop.altscreen.golden"),
        include_str!("fixtures/htop.altscreen.logical.golden"),
    );
}

/// #20 dogfood — REAL captured `top` session (80x24, procps-ng on RHEL; see
/// tests/fixtures/capture-dogfood.sh, which uses `top` when htop/EPEL is
/// unavailable). A live-monitor TUI exercises a different CSI mix than an
/// editor: per-cell SGR colour + bold + reverse-video (the column header),
/// G0 charset designation (ESC(B), EL clears, full-screen home-and-repaint,
/// and a bottom-row LF scroll that pushes the "top -" header line off the top.
/// The char-only dump does not render attributes, but exact text placement
/// proves every escape was consumed rather than printed — and replaying frozen
/// bytes keeps the snapshot deterministic despite top's live system data.
#[test]
fn top_capture_redraw() {
    let mut term = Engine::new(80, 24);
    term.feed(include_bytes!("fixtures/top.raw"));
    check_capture(
        &term,
        include_str!("fixtures/top.golden"),
        include_str!("fixtures/top.logical.golden"),
    );
}

// ===========================================================================
// #20 dogfood — REAL captured vim session (80x24). Recorded via script(1) on
// RHEL with a scripted keystroke driver; see tests/fixtures/capture-dogfood.sh.
// The raw byte stream (script header/footer stripped) is replayed verbatim, so
// these goldens exercise the exact CSI mix a real editor emits — alt-screen
// enter/leave, DECSTBM scroll regions, IL/scroll-based line insert & delete,
// ICH/DCH, ECH, wide (Hangul) status text, and bottom-row LF scroll.
// ===========================================================================

/// Feed the whole real vim stream. vim opens the alt screen (?1049h) and
/// restores the primary one on quit (?1049l), so the engine ends back on an
/// empty primary screen with the cursor home — proving alt-screen save/restore
/// survives a full real session.
#[test]
fn vim_capture_restores_primary_on_quit() {
    let mut term = Engine::new(80, 24);
    term.feed(include_bytes!("fixtures/vim_redraw.raw"));
    check_capture(
        &term,
        include_str!("fixtures/vim_redraw.full.golden"),
        include_str!("fixtures/vim_redraw.full.logical.golden"),
    );
}

/// Feed up to the alt-screen teardown (?1049l) to assert the editor screen vim
/// actually drew. Note the buffer's first line ("inserted near the top") has
/// scrolled off the top: just before leaving the alt screen vim emits CR CR LF
/// with the cursor on the bottom row, and an LF there scrolls the whole screen
/// up one — standard VT behaviour (matches xterm/alacritty). The Hangul save
/// message on the status row shows wide cells followed by spacer cells.
#[test]
fn vim_capture_altscreen_redraw() {
    let raw = include_bytes!("fixtures/vim_redraw.raw");
    let altcut = raw.windows(8).position(|w| w == b"\x1b[?1049l").unwrap();
    let mut term = Engine::new(80, 24);
    term.feed(&raw[..altcut]);
    check_capture(
        &term,
        include_str!("fixtures/vim_redraw.altscreen.golden"),
        include_str!("fixtures/vim_redraw.altscreen.logical.golden"),
    );
}

/// Editor-style edit: type three lines, then with the cursor saved (DECSC) open
/// a line above the last two (IL), tighten line 0 (ICH) and line 1 (DCH), and
/// restore the cursor (DECRC). The cursor line proves DECRC returned it to where
/// DECSC saved it (3,4) rather than where IL left it.
#[test]
fn editor_redraw_insert_and_edit() {
    check(
        6,
        4,
        b"ABC\r\nDEF\r\nGHI\x1b7\x1b[1;1H\x1b[2@\x1b[2;2H\x1b[1P\x1b[3;1H\x1b[1L\x1b8",
        "\
|  ABC |
|DF    |
|      |
|GHI   |
cursor=(2,3) visible=true
",
    );
}

/// Editor-style delete: fill four rows, erase mid-row-0 (ECH), delete a whole
/// line pulling the rest up (DL), then clear to end-of-line (EL).
#[test]
fn editor_redraw_delete_and_erase() {
    check(
        6,
        4,
        b"xxxxxx\r\nyyyyyy\r\nzzzzzz\r\nwwwwww\x1b[1;3H\x1b[2X\x1b[2;1H\x1b[1M\x1b[3;4H\x1b[0K",
        "\
|xx  xx|
|zzzzzz|
|www   |
|      |
cursor=(2,3) visible=true
",
    );
}

// ===========================================================================
// #554 dogfood — SOFT WRAP under the row-shift verbs. The combination the four
// captures above structurally cannot contain: a program that emits IL/DL is a
// full-screen application, and a full-screen application positions every row
// with CUP, so it never lets the terminal continue a line. Replaying all four
// yields zero soft-wrapped rows, which is why the whole soft-wrap / wide-pair
// cluster (ADR-0025, spine #552) had no dogfood coverage — #540's defects were
// green against every capture in the corpus while merging two logical lines.
// Recorded by tests/fixtures/capture-softwrap.sh; see its header for the
// measurements behind that claim.
// ===========================================================================

/// Deterministic soft-wrap + every row-shift verb (IL/DL/SU/SD/RI, one set inside
/// a DECSTBM region). Content is written with **no CUP**: each line is simply
/// longer than the screen, so the terminal itself continues it — the state under
/// test. The shift verbs are emitted deliberately rather than by an application
/// redrawing, exactly as `undercurl_matrix.raw` is a deterministic printf; that is
/// what makes it the source of truth and reproducible without a VM.
///
/// The logical golden is where this capture earns its place. `WRAP-A` is 107
/// columns — 80 on its first row, 27 continuing — and a `DL` deletes the first
/// row, so the arithmetic pins the survivor: the golden must show a lone
/// 27-character line, not a 107-character one and not a merge with its new
/// neighbour. The char golden cannot tell those three apart.
#[test]
fn softwrap_shifts_capture() {
    let mut term = Engine::new(80, 24);
    term.feed(include_bytes!("fixtures/softwrap_shifts.raw"));
    check_capture(
        &term,
        include_str!("fixtures/softwrap_shifts.golden"),
        include_str!("fixtures/softwrap_shifts.logical.golden"),
    );
}

/// REAL captured `less -X` session (80x24, macOS `less` under `expect`; the other
/// captures are RHEL `script(1)` recordings — recorded here because no Linux VM
/// was available, and the stream is a valid one either way). Paging backward over
/// long lines emits 24 x `RI` (`ESC M`) across genuinely soft-wrapped content,
/// which is `reverse_index` — one of the five call sites #540 routed through
/// `Term::shift_region`, and the only one a real application reaches on its own.
///
/// The wraps live in scrollback rather than the grid, so the logical golden is the
/// only surface that shows them at all: each `LINEnn` must stay its own line, with
/// the 147-column ones joined and nothing merged across them.
#[test]
fn less_softwrap_capture() {
    let mut term = Engine::new(80, 24);
    term.feed(include_bytes!("fixtures/less_softwrap.raw"));
    check_capture(
        &term,
        include_str!("fixtures/less_softwrap.golden"),
        include_str!("fixtures/less_softwrap.logical.golden"),
    );
}

/// Deterministic soft wrap of a **wide** glyph, then the verbs that falsify the artefact marker
/// it leaves behind (#534). The other two soft-wrap captures supply wrapped rows but no wrapped
/// *pair*: replaying all six checked-in captures observes the marker **zero** times, so the wide
/// half of the ADR-0025 roster had no dogfood coverage even after #555 gave the rest of it some.
/// Measured before writing this; this capture observes it 43 times.
///
/// Each line is 79 narrow columns followed by `한`, so the terminal itself cannot fit the pair and
/// vacates the last column — no CUP places the content. Then one verb per line falsifies one of
/// the marker's two clauses: `A` overwrites the wrapped lead, `B` erases the continuation row,
/// `C` deletes the lead with `DCH`, `D` shifts the pair right with `ICH`, `E` removes the
/// continuation with `DL`.
///
/// **The char golden cannot see any of this.** The marker is a content bit, not a character, so
/// with the fix reverted `dump` is byte-identical while three of the logical lines lose the blank
/// the artefact turned back into — `A` reads `…xxxZ` instead of `…xxx Z`, and `B`'s `Q` sits one
/// column early. That is the discriminating check #554 demands of a new capture, run here.
///
/// `E` is deliberately **not** pinned by the golden: after #540 ended the wrap, its orphaned
/// marker sits on the last column of a hard-ended row, where a trailing blank is trimmed either
/// way. It is only observable through the word walk and through reflow, which is where
/// `wrap_artefact_lifecycle.rs` pins it. Recorded so the gap is not later read as coverage.
#[test]
fn softwrap_wide_capture() {
    let mut term = Engine::new(80, 24);
    term.feed(include_bytes!("fixtures/softwrap_wide.raw"));
    check_capture(
        &term,
        include_str!("fixtures/softwrap_wide.golden"),
        include_str!("fixtures/softwrap_wide.logical.golden"),
    );
}
