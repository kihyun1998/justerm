//! The terminal state model: a `vte::Perform` that maps parsed VT actions onto
//! the grid, cursor, and pen. This is where the "hidden VT state" lives —
//! pending-wrap, the wide-char spacer, and the pen (BCE seam).

use std::collections::VecDeque;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use vte::{Params, Perform};

use crate::cell::{Cell, CellFlags};
use crate::color::Color;
use crate::cursor::{Cursor, CursorShape, Pen};
use crate::damage::{LineBounds, LineDamage, ScrollOp, TermDamage};
use crate::event::TermEvent;
use crate::grid::{ExtAttrs, Grid, Row};
use crate::input::{
    KeyEvent, MouseEncoding, MouseEvent, MouseProtocol, encode_focus, encode_key, encode_mouse,
    encode_paste,
};
use crate::logical::LogicalLine;
use crate::search::Match;
use crate::selection::{BufferPoint, Selection};
use crate::serialize::{
    Frame, FrameKind, MarkerId, MarkerKind, MarkerLine, MarkerPosition, Overlay, Span,
};

/// Buffer-walk primitives shared by every read surface (#585). A child module, so
/// it reaches `Term`'s private fields directly — no field is widened for it.
mod walk;

/// The search query surface (#586) — finding matches, and the highlight set the
/// consumer pushes back. Stands on `walk`, whose `pub(super)` reaches a sibling
/// module because both are descendants of `term`.
mod search;

/// The selection surface (#587) — gestures, the anchor fixups the write path drives,
/// and text extraction. Stands on `walk` like its siblings.
mod selection;

/// Owns the authoritative screen state and applies VT actions to it.
pub struct Term {
    grid: Grid,
    /// The inactive screen. Swapped with `grid` on alt-screen enter/leave; holds
    /// whichever of primary/alternate is not currently shown. The alt screen has
    /// no scrollback (#3 only rings the primary).
    alt_grid: Grid,
    cursor: Cursor,
    /// Cursor saved on alt-screen enter (DEC 1049), restored on leave.
    saved_cursor: Cursor,
    /// Whether the alternate screen is currently active. Guards enter/leave so a
    /// double-enter or double-leave is a no-op.
    on_alt: bool,
    /// One flag per column: is there a tab stop here? Explicit per-column state
    /// (HTS sets, TBC clears), not a fixed modulo. Default = every 8th column.
    tabs: Vec<bool>,
    /// Origin mode (DECOM ?6): when set, cursor addressing is relative to the
    /// scroll region's top margin (and clamped to it).
    origin_mode: bool,
    /// Autowrap (DECAWM ?7): default on. When off, a glyph past the right margin
    /// pins the cursor to the last column and overwrites in place instead of
    /// wrapping to the next line (matches xterm.js) (#63).
    autowrap: bool,
    /// Insert mode (IRM, the non-private SM/RM mode 4): default off (replace).
    /// When on, a printed glyph shifts the row's tail right first (#64).
    insert_mode: bool,
    /// New-line mode (LNM, the non-private SM/RM mode 20): default off. When on,
    /// a line feed also carriage-returns (`convertEol`). Output-only — the Enter
    /// key still encodes CR, matching xterm.js (#71).
    newline_mode: bool,
    /// Reverse wraparound (DEC ?45): default off. When on, a *backspace* at
    /// column 0 of a soft-wrapped row moves back to the end of the previous row
    /// (BS only, soft wraps only — matches xterm.js) (#80).
    reverse_wraparound: bool,
    /// Bracketed-paste mode (DEC ?2004). The engine owns the flag; the input
    /// encoder (#11) reads it to decide whether to wrap pasted text in markers.
    bracketed_paste: bool,
    /// Synchronized output (DEC ?2026): the app brackets a frame of output so the
    /// renderer can paint it atomically. The engine only *tracks* the flag — the
    /// consumer owns the paint-hold and the spec-mandated timeout (#73).
    synchronized_output: bool,
    /// Color-scheme-update notifications (DEC ?2031): the app asked to be told
    /// when the light/dark scheme changes. The engine is theme-agnostic — it only
    /// tracks the flag; the consumer (which knows the scheme) drives the ?997
    /// notification via `report_color_scheme` (#85).
    color_scheme_updates: bool,
    /// Grapheme-cluster mode (DEC ?2027, default OFF): the app opted into UAX #29 grapheme-cluster
    /// width — a ZWJ / skin-tone / flag / emoji+VS16 sequence is clustered into ONE cell instead of
    /// one cell per scalar (#295). OFF keeps the per-char (wcwidth-compatible) behaviour so the
    /// cursor stays in sync with wcwidth apps — clustering is opt-in for exactly that reason (#301).
    grapheme_clustering: bool,
    /// win32-input-mode (DEC ?9001): the app asked for keys as raw Windows
    /// key-records. The engine only *tracks* the flag — the raw record encoding
    /// (`CSI Vk;Sc;Uc;Kd;Cs;Rc _`) is a non-goal (raw passthrough, no semantic
    /// conversion), left to the ConPTY consumer; `encode_key` is unchanged (#86).
    win32_input_mode: bool,
    /// Application cursor keys (DECCKM ?1): when set, cursor keys / Home / End
    /// encode as SS3 rather than CSI (see `input.rs`).
    app_cursor_keys: bool,
    /// Application keypad mode (DECNKM ?66 / DECKPAM `ESC =` / DECKPNM `ESC >`):
    /// tracked for protocol completeness + DECRQM, but NOT yet acted on in key
    /// encoding — xterm.js tracks it the same way and never reads it (#74).
    application_keypad: bool,
    /// VT52 compatibility mode (DECANM ?2 *reset*): when set, `esc_dispatch` is
    /// re-routed into the pre-ANSI VT52 dialect (`ESC A`-style sequences) instead
    /// of the ANSI meaning. `ESC <` clears it. Default off (ANSI). (#84)
    vt52_mode: bool,
    /// VT52 `ESC Y row col` direct-addressing state (#84). vte tokenizes `ESC Y`
    /// as a final and returns to ground, so the two coordinate bytes arrive as
    /// `print()` calls — not part of the escape sequence. This counts them down
    /// (2 → 1 → 0; 0 = not addressing) and `vt52_y_row` parks the first (row)
    /// until the second (col) lands. Each byte decodes as `value - 0x20`.
    vt52_y_pending: u8,
    vt52_y_row: usize,
    /// Mouse tracking mode — what events the app asked to be reported
    /// (?1000/?1002/?1003). `Off` by default.
    mouse_protocol: MouseProtocol,
    /// Mouse coordinate encoding (default X10 vs ?1006 SGR).
    mouse_encoding: MouseEncoding,
    /// Focus in/out reporting (?1004): emit `CSI I`/`CSI O` on focus change.
    focus_events: bool,
    /// Kitty keyboard-protocol progressive-enhancement flags currently in effect
    /// (bit0 disambiguate, bit1 report-events, bit2 alt-keys, bit3 all-as-escape,
    /// bit4 associated-text). 0 = legacy. `encode_key` consults these (#23).
    kitty_flags: u8,
    /// Saved `kitty_flags` for the protocol's push/pop stack (`CSI > u` pushes,
    /// `CSI < u` pops). Capped depth — overflow drops the oldest entry.
    kitty_stack: Vec<u8>,
    /// Consumer events (title / bell / cwd) accumulated since the last
    /// `drain_events` (#12). Pull, not push — see `event.rs`.
    events: Vec<TermEvent>,
    /// Outbound reply bytes (DA/DSR/DECRQM query answers, #27) accumulated
    /// during `feed` for the consumer to write back to the PTY. Raw bytes →
    /// PTY, kept separate from typed `events` → UI.
    replies: Vec<u8>,
    /// Hyperlink side-table (OSC 8): each entry is one link's URI, referenced by
    /// `Cell.link` (1-based). Append-only (#26).
    hyperlink_pool: Vec<String>,
    /// The hyperlink currently open (OSC 8 with a URI), stamped onto every glyph
    /// written until closed (OSC 8 with empty URI). Ambient pen-like state — not
    /// part of the pen/SGR, and *not* cleared by an SGR reset.
    current_link: Option<core::num::NonZeroU32>,
    /// Scroll region top/bottom margins (DECSTBM), 0-based inclusive. A
    /// line-feed at `scroll_bottom` scrolls only rows `[scroll_top..=scroll_bottom]`.
    /// Default = the full screen.
    scroll_top: usize,
    scroll_bottom: usize,
    /// Lines that have scrolled off the top of the primary screen, oldest at the
    /// front. Accrues only on a top-anchored, primary-screen scroll.
    scrollback: VecDeque<Row>,
    /// How many lines the viewport is scrolled up from the bottom. 0 = following
    /// the live screen; clamped to `[0, scrollback.len()]`.
    display_offset: usize,
    /// Maximum scrollback lines retained; the oldest are evicted past this.
    scrollback_limit: usize,
    /// A spare row buffer recycled across full-screen scrolls: the cap-evicted
    /// oldest line is parked here and reused as the next scroll's blank bottom,
    /// so a steady-state flood allocates nothing (ADR-0009).
    recycled_row: Option<Row>,
    /// Per-line damage bounds since the last `reset_damage` (ack), one per row.
    line_damage: Vec<LineBounds>,
    /// A first-class scroll recorded since the last `reset_damage`.
    scroll: Option<ScrollOp>,
    /// The whole screen changed (alt switch / clear / later resize+flood) — the
    /// renderer must redraw everything.
    full_damage: bool,
    /// The cursor `(row, col)` at the last `reset_damage` (ack) — where the
    /// consumer last saw the caret. A pure cursor move records no content
    /// damage, so `damage()` folds this *old* cell plus the current one into the
    /// frame; without it a cell-invert caret ghosts at the old spot (mirrors
    /// Alacritty's `last_cursor`). #38.
    prev_cursor: (usize, usize),
    /// The live selection, in absolute buffer coordinates. `None` when nothing
    /// is selected. See `selection.rs`.
    selection: Option<Selection>,
    /// The search highlights the consumer asked to paint (#108). Search
    /// matches are consumer-owned (it drives next/prev), so the engine holds only
    /// the set handed back via `set_search_highlights`, and `frame()` projects it
    /// onto the viewport — the same anchoring path as the selection.
    search_highlights: Vec<Match>,
    /// The *active* (current) search match (#428), stored as its absolute span
    /// (#436) — designated by the consumer (next/prev is its policy) either as
    /// an index into `search_highlights` (resolved to the span at call time) or
    /// directly by span, which a capping backend uses for a past-cap match
    /// (xterm creates its active decoration from the found result, OUTSIDE the
    /// capped highlight list). A span is NOT structurally tied to the set, so
    /// every path that voids the set must void this too: `set_search_highlights`
    /// (hand-over reset, #428) and `invalidate_search_highlights` (the single
    /// funnel for eviction / every region scroll incl. the accrual sub-region
    /// branch (#449) / reflow / both alt swaps) — a stale span would otherwise
    /// keep painting coordinates that now hold other text.
    active_search_highlight: Option<Match>,
    /// Engine-owned decoration markers (#118), split per buffer like xterm's
    /// `BufferSet` (#177 S0): each a stable id bound to an absolute buffer line
    /// that re-anchors through eviction/scroll/reflow like a selection anchor. The
    /// active buffer's list is selected by `on_alt` — `markers`/`markers_mut`.
    /// `alt_markers` stays empty while the alt guards (#158/#164) are in place; it
    /// is disposed on alt-leave (xterm `clearAllMarkers`). `next_marker_id` hands
    /// out monotonic ids across both buffers so ids never alias.
    normal_markers: Vec<Marker>,
    alt_markers: Vec<Marker>,
    next_marker_id: u32,
    /// Cursor state saved by DECSC (ESC 7), restored by DECRC (ESC 8). A slot
    /// separate from `saved_cursor` (which is the alt-screen save). Defaults to
    /// home/default so a DECRC with no prior DECSC restores a sane state.
    decsc: SavedCursor,
    /// SCS-designated character sets G0..G3 (#62). `gl` indexes the active (GL)
    /// set, switched by SI (→G0) / SO (→G1). First cut uses G0/G1.
    charsets: [Charset; 4],
    gl: usize,
}

/// A character set designated by SCS (#62). First cut: ASCII (default), DEC
/// Special Graphics (line-drawing), and UK. G2/G3 and the GR half are later.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Charset {
    #[default]
    Ascii,
    DecSpecialGraphics,
    Uk,
}

impl Charset {
    /// Map one GL byte (a `char` in the 7-bit range) through this set. ASCII and
    /// any out-of-range char pass through; UK swaps `#`→£; DEC Special Graphics
    /// translates `_`..`~` to the line-drawing / symbol glyphs.
    fn map(self, c: char) -> char {
        match self {
            Charset::Ascii => c,
            Charset::Uk if c == '#' => '£',
            Charset::Uk => c,
            Charset::DecSpecialGraphics => dec_special_graphics(c),
        }
    }
}

/// The VT100 DEC Special Graphics set: bytes `_`..`~` (0x5F..0x7E) map to the
/// box-drawing and symbol glyphs. Matches xterm/alacritty; anything outside the
/// range passes through unchanged.
fn dec_special_graphics(c: char) -> char {
    // Keys ``..`~` only — `_` (0x5F) is deliberately absent, matching xterm.js /
    // alacritty (it passes through as a literal underscore), not the strict-DEC
    // "0x5F = blank" reading.
    match c {
        '`' => '◆',
        'a' => '▒',
        'b' => '␉',
        'c' => '␌',
        'd' => '␍',
        'e' => '␊',
        'f' => '°',
        'g' => '±',
        'h' => '␤',
        'i' => '␋',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        other => other,
    }
}

/// Default scrollback retention when not specified.
const DEFAULT_SCROLLBACK: usize = 10_000;

/// The narrowest screen the engine represents: **two columns**.
///
/// A width-2 glyph occupies a `WIDE_CHAR` lead *and* the `WIDE_CHAR_SPACER` that
/// stands for its second half, so one column cannot hold one — and a pair with only
/// one half written is the malformed state every repair path in this crate keys off
/// (ADR-0025 D4). `Term::with_scrollback` and [`Term::resize`] clamp `cols` up to
/// this, which is what makes D4 (*both halves of a pair move together*)
/// unconditionally satisfiable rather than true only above some unstated width.
///
/// Both references that a terminal *engine* can be compared to forbid one column for
/// exactly this reason — alacritty's `MIN_COLUMNS = 2` and xterm.js's
/// `MINIMUM_COLS = 2` — and the third (ghostty) permits it only by destroying the
/// glyph.
///
/// The clamp is **silent and pull-only**: a `resize(1, rows)` during a pane drag is
/// widened rather than rejected, and no event reports it. Both references instead
/// make the clamped size the one that travels outward — alacritty derives its
/// `WindowSize` from the clamped `SizeInfo`, xterm.js fires `onResize` with the
/// clamped pair — so a justerm consumer must do that correlation itself: read the
/// width back from [`Term::grid`] / the frame header and size the PTY from *that*,
/// never from the value it requested. Sizing a PTY to one column leaves the
/// application rendering for a width the buffer does not have (#547).
pub const MIN_COLUMNS: usize = 2;

/// The state DECSC (ESC 7) saves and DECRC (ESC 8) restores: position, pen/SGR,
/// pending-wrap, and origin mode (per ADR-0004 — DECRC restores origin mode,
/// which Alacritty omits). Cursor *visibility* is deliberately not part of this
/// (DECTCEM is separate from DECSC).
#[derive(Clone, Copy, Default)]
struct SavedCursor {
    row: usize,
    col: usize,
    pen: Pen,
    pending_wrap: bool,
    origin_mode: bool,
    /// SCS charset state at save time — DECSC/DECRC round-trip the designated
    /// sets and the active GL shift (#62).
    charsets: [Charset; 4],
    gl: usize,
}

/// An engine-owned decoration marker (#118): a stable id bound to an absolute
/// buffer line. The line shifts in lockstep with eviction/region scroll/reflow
/// (the same coordinate moves the selection anchor tracks); the marker is
/// dropped when its line leaves the buffer.
struct Marker {
    id: MarkerId,
    line: usize,
    /// The cursor column at emit time (#166). Meaningful for OSC-133 command
    /// marks — CommandStart(B)/OutputStart(C) columns bound the *typed command*
    /// (excluding the prompt), like VSCode's `commandStartX`/`commandExecutedX`.
    /// Plain `add_marker` decorations are row-granular and carry `col = 0`.
    ///
    /// **Domain is `[0, cols]`, not `[0, cols - 1]` (#562)** — a bound, not a cell.
    /// A command that exactly fills its row ends *one past* the last column, and
    /// that value is what `extract_lines` wants: it clips `[b_col, c_col)`, so the
    /// exclusive end absorbs it through `.min(cells.len())`. Storing `cursor.col`
    /// alone (the cursor is held at `cols - 1` with `pending_wrap`) cost such a
    /// command its last character with no resize involved. The **inclusive** side
    /// cannot absorb it, so `extract_lines` steps a `from` of `cells.len()` to the
    /// next line rather than selecting an empty run and flushing a `\n`.
    col: usize,
    /// Plain for a `add_marker` decoration; a command-boundary role for an
    /// OSC 133 mark (#158). All kinds share the anchor/eviction machinery.
    kind: MarkerKind,
}

/// One executed shell command recovered from OSC-133 marks (#166), for
/// screen-reader command navigation. The consumer jumps prompt-to-prompt over
/// these and announces `command` + a success/fail signal from `exit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    /// The command's jump anchor as a *document* line — the logical-line index of
    /// the CommandStart(B) mark within [`Term::accessible_text`], so the consumer
    /// reveals the right row of the accessible view (soft-wrapped rows collapse to
    /// one logical line). This is core's analog of VSCode's
    /// `bufferToEditorLineMapping`; the frame-mode web side has no wrap info to
    /// map it itself.
    pub line: usize,
    /// The typed command text, prompt- and output-excluded (B→C columns).
    pub command: String,
    /// The CommandFinished(D) exit code, if the shell reported one and the
    /// command has finished.
    pub exit: Option<i32>,
}

/// Collect per-line damage bounds into damaged `LineDamage` spans (undamaged
/// lines dropped). Shared by `damage` (content-only) and `frame_damage`
/// (content + cursor cells).
fn bounds_to_lines(bounds: &[LineBounds]) -> Vec<LineDamage> {
    bounds
        .iter()
        .enumerate()
        .filter(|(_, b)| b.is_damaged())
        .map(|(line, b)| {
            let (left, right) = b.span();
            LineDamage { line, left, right }
        })
        .collect()
}

impl Term {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_scrollback(cols, rows, DEFAULT_SCROLLBACK)
    }

    pub fn with_scrollback(cols: usize, rows: usize, scrollback_limit: usize) -> Self {
        // Both clamps mirror `resize` exactly, so a screen cannot be born at a size a
        // resize would refuse. They are not the same *kind* of rule, though: the width
        // floor is a published contract (#547 — one column was supported and no longer
        // is), while the row floor is `resize`'s own long-standing "a terminal is never
        // 0-tall" that this constructor merely failed to enforce while carrying the
        // same `scroll_bottom: rows - 1` below. That gap was a subtract-overflow panic
        // on `rows == 0`, not a degenerate screen.
        let cols = cols.max(MIN_COLUMNS);
        let rows = rows.max(1);
        Term {
            grid: Grid::new(cols, rows),
            alt_grid: Grid::new(cols, rows),
            cursor: Cursor::default(),
            saved_cursor: Cursor::default(),
            on_alt: false,
            origin_mode: false,
            autowrap: true,
            insert_mode: false,
            newline_mode: false,
            reverse_wraparound: false,
            bracketed_paste: false,
            synchronized_output: false,
            color_scheme_updates: false,
            grapheme_clustering: false,
            win32_input_mode: false,
            app_cursor_keys: false,
            application_keypad: false,
            vt52_mode: false,
            vt52_y_pending: 0,
            vt52_y_row: 0,
            mouse_protocol: MouseProtocol::Off,
            mouse_encoding: MouseEncoding::Default,
            focus_events: false,
            kitty_flags: 0,
            kitty_stack: Vec::new(),
            events: Vec::new(),
            replies: Vec::new(),
            hyperlink_pool: Vec::new(),
            current_link: None,
            tabs: default_tabs(cols),
            scroll_top: 0,
            scroll_bottom: rows - 1,
            scrollback: VecDeque::new(),
            display_offset: 0,
            scrollback_limit,
            recycled_row: None,
            line_damage: vec![LineBounds::undamaged(cols); rows],
            scroll: None,
            full_damage: false,
            prev_cursor: (0, 0), // matches the default cursor's home position
            selection: None,
            search_highlights: Vec::new(),
            active_search_highlight: None,
            normal_markers: Vec::new(),
            alt_markers: Vec::new(),
            next_marker_id: 0,
            decsc: SavedCursor::default(),
            charsets: [Charset::Ascii; 4],
            gl: 0,
        }
    }

    /// What changed since the last `reset_damage()` — line ranges, each with a
    /// changed column span. See ADR-0003.
    pub fn damage(&self) -> TermDamage {
        if self.full_damage {
            return TermDamage::Full;
        }
        // Scrolled up under follow-bottom "stay": the viewport is frozen, so
        // screen changes below it are not visible — report nothing. (A user
        // scroll that moves the viewport sets full_damage above.)
        if self.display_offset > 0 {
            return TermDamage::Partial(Vec::new());
        }
        TermDamage::Partial(bounds_to_lines(&self.line_damage))
    }

    /// Render damage: content damage plus the cursor cells, for [`Term::frame`].
    ///
    /// A pure cursor move changes no cell *content*, so [`Term::damage`] (which
    /// stays content-only, the cadence/flow-control primitive) would miss it —
    /// yet a cell-invert caret must clear its old spot and ink the new one. So
    /// the frame producer folds the old (last-acked) + current cursor cells in,
    /// but only when the cursor actually moved: a still cursor needs no redraw,
    /// keeping an idle frame empty. Mirrors Alacritty's `last_cursor`. #38.
    fn frame_damage(&self) -> TermDamage {
        if self.full_damage {
            return TermDamage::Full;
        }
        if self.display_offset > 0 {
            return TermDamage::Partial(Vec::new());
        }
        let cur = self.cursor.point();
        if cur == self.prev_cursor {
            return TermDamage::Partial(bounds_to_lines(&self.line_damage));
        }
        let mut bounds = self.line_damage.clone();
        bounds[cur.0].expand(cur.1, cur.1);
        let pr = self.prev_cursor.0.min(self.grid.rows() - 1);
        let pc = self.prev_cursor.1.min(self.grid.cols() - 1);
        bounds[pr].expand(pc, pc);
        TermDamage::Partial(bounds_to_lines(&bounds))
    }

    /// Clear accumulated damage. The consumer calls this after applying a frame
    /// (the ack); the next `damage()` reflects only changes since.
    pub fn reset_damage(&mut self) {
        for b in &mut self.line_damage {
            b.reset();
        }
        self.scroll = None;
        self.full_damage = false;
        // The consumer has now seen the caret at the current position; the next
        // frame's cursor-move damage is measured from here (#38).
        self.prev_cursor = self.cursor.point();
    }

    /// Mark the whole screen damaged (alt switch / clear / flood, and a consumer
    /// reattach that needs a full re-sync — see [`crate::Engine::mark_fully_damaged`]).
    pub fn mark_fully_damaged(&mut self) {
        self.full_damage = true;
    }

    /// Record that columns `[left, right]` of `row` changed.
    ///
    /// Both columns are **clamped to the last column, and asserted in debug** (#536).
    ///
    /// Ten of the fourteen call sites derive their bound from a cursor column or from `cols`.
    /// **Four derive it from a wide pair's width**, and that is the shape worth centralising:
    /// `write_glyph`'s `col + width - 1` (which had no guard — this issue), `promote_cluster_to_wide`'s
    /// `col + 1` (guarded by its own `col + 1 >= cols` early return), `demote_cluster_to_narrow`'s
    /// `(col + 1).min(cols - 1)` (self-clamped), and `relocate_cluster_wide`'s literal `(0, 1)`
    /// (valid only because `MIN_COLUMNS = 2`, #547). Three carried a private guard and one did not.
    ///
    /// No reference has this shape to port a clamp from: alacritty computes damage ranges too
    /// (`term/mod.rs:1406`, `:1649` @ `852e971`) but always from a column or `columns()` — its print
    /// path records no damage at all, relying on the previous and current cursor *points* to bracket
    /// the line — while xterm.js tracks whole rows (`markDirty(y)`) and ghostty a per-row
    /// `dirty: bool`. alacritty's `LineDamageBounds::expand`, which this one is a copy of, is equally
    /// unguarded.
    ///
    /// The two halves do different jobs:
    ///
    /// - the **`debug_assert` is the detector**. An out-of-range bound is stored silently and
    ///   detonates later, when `frame()` slices the row, so the stack trace accuses the reader
    ///   rather than the writer. That delay is what #536 was filed about, and the assert collapses
    ///   it — a bad caller dies here, at the site that recorded it (measured: an injected off-by-one
    ///   moved the panic from `frame()`'s slice to this line).
    /// - the **clamp is the release backstop**, and it clamps *toward a false positive*. justerm is
    ///   a library, so a panic crosses into the consumer's process; over-damaging repaints a cell
    ///   that did not change, which costs nothing a consumer can see. ghostty states the asymmetry
    ///   as a rule: *"Dirty tracking may have false positives but should never have false negatives.
    ///   A false negative would result in a visual artifact on the screen."* (`page.zig:1993-1995`).
    ///
    /// **`left` is guarded for that reason, and it is the axis that can actually lose a cell.**
    /// Clamping `right` cannot under-report — columns past the last do not exist. But `LineBounds`
    /// marks a line undamaged with `left = cols, right = 0` and `is_damaged()` is `left <= right`,
    /// so a single `expand` with `left > right` on an otherwise-clean line leaves the line reading
    /// as undamaged and **drops its whole span silently**. Unreachable from the ten column-derived
    /// sites today; guarded because that is precisely the failure ghostty's rule forbids.
    ///
    /// `row` is deliberately left to panic on the index, and that is **not** in tension with
    /// `frame_damage` clamping a row fifty lines below (`prev_cursor.0.min(rows - 1)`). The two
    /// rows are different kinds of thing under the same rule: `prev_cursor` is a *stale remembered*
    /// coordinate that a shrinking resize may have put out of range, so clamping it repaints the
    /// nearest surviving cell — a false positive. `row` here is a *live computed* index for the
    /// mutation just made, so clamping it would damage a different line than the one that changed:
    /// a false negative on the real line, which is the outcome the rule forbids.
    fn damage_span(&mut self, row: usize, left: usize, right: usize) {
        let last = self.grid.cols().saturating_sub(1);
        debug_assert!(
            left <= right && right <= last,
            "damage_span({row}, {left}, {right}) is not a span inside [0, {last}]"
        );
        self.line_damage[row].expand(left.min(last), right.min(last));
    }

    /// The first-class scroll recorded since the last `reset_damage`, if any.
    /// Suppressed while scrolled up — a content scroll must not shift the frozen
    /// viewport.
    pub fn scroll_delta(&self) -> Option<ScrollOp> {
        if self.display_offset > 0 {
            return None;
        }
        self.scroll
    }

    /// Build a serializable [`Frame`] from the current damage + grid + grapheme
    /// pool (#6). `Full` ships every row; `Partial` ships the damaged spans. The
    /// global side-table is remapped to **frame-local** indices — the engine pool
    /// is append-only and leaky, so a frame carries only the clusters its cells
    /// reference, renumbered, with each cell's `extra` rewritten to the local id.
    pub fn frame(&self) -> Frame {
        let cols = self.grid.cols();
        let rows = self.grid.rows();
        let (kind, line_spans): (FrameKind, Vec<(usize, usize, usize)>) = match self.frame_damage()
        {
            TermDamage::Full => (
                FrameKind::Full,
                (0..rows).map(|l| (l, 0, cols - 1)).collect(),
            ),
            TermDamage::Partial(lines) => (
                FrameKind::Partial,
                lines
                    .into_iter()
                    .map(|d| (d.line, d.left, d.right))
                    .collect(),
            ),
        };

        let mut side_table: Vec<Vec<char>> = Vec::new();
        // Same frame-local renumber for the hyperlink side-table (#26).
        let mut link_table: Vec<String> = Vec::new();
        let mut link_remap = vec![0u16; self.hyperlink_pool.len() + 1];
        // Cells come from the viewport at `display_offset`, not the live grid:
        // viewport row `line` is absolute buffer line `top + line` (scrollback
        // when scrolled up, the live grid when `display_offset == 0`, where
        // `top == scrollback.len()` and this is identical to reading the grid).
        // Without this, a wire consumer — cells reach it only through `frame()` —
        // could never display scrollback (#48).
        let top = self.scrollback.len() - self.display_offset;
        let mut spans = Vec::with_capacity(line_spans.len());
        for (line, left, right) in line_spans {
            let mut cells = Vec::with_capacity(right - left + 1);
            let mut combining = std::collections::BTreeMap::new();
            let mut links = std::collections::BTreeMap::new();
            let mut ucolors = std::collections::BTreeMap::new();
            let row = self.abs_row(top + line);
            let last_col = row.len().saturating_sub(1);
            for col in left..=right {
                let mut cell = row[col];
                // Soft-wrap is a row property (#538), but the wire has no per-row slot — so it is
                // *derived* back onto the last cell's WRAPLINE bit here, which keeps the format
                // byte-identical and is why moving the storage needed no VERSION bump. The bit is
                // therefore wire-only: on a live grid it is never set, and `Row::is_wrapped` is
                // the question to ask.
                if col == last_col && row.is_wrapped() {
                    cell.insert_flags(CellFlags::WRAPLINE);
                }
                // Combining clusters and hyperlinks live in the row's maps; each
                // tagged cell contributes its reference to the frame, recorded on
                // the span by span-relative column (the cell holds only the bit).
                if let Some(marks) = row.combining_at(col) {
                    side_table.push(marks.to_vec());
                    let idx = core::num::NonZeroU32::new(side_table.len() as u32)
                        .expect("side_table just pushed, len >= 1");
                    combining.insert(col - left, idx);
                }
                if let Some(lidx) = row.link_at(col) {
                    // Renumber the global pool index to a contiguous frame-local
                    // one (only referenced URIs ship), same as the old per-cell link.
                    let l = lidx.get() as usize;
                    if link_remap[l] == 0 {
                        link_table.push(self.hyperlink_pool[l - 1].clone());
                        link_remap[l] = link_table.len() as u16;
                    }
                    let fidx = core::num::NonZeroU32::new(link_remap[l] as u32)
                        .expect("link_remap just set, nonzero");
                    links.insert(col - left, fidx);
                }
                // Underline colour (SGR 58, #520): a colour reference, not a
                // side-table index, so it rides the span inline. `ucolor_at` is
                // flag-gated + already Default-filtered (the stamp only fires on an
                // underlined cell), so a present entry is a real non-default colour.
                if let Some(color) = row.ucolor_at(col) {
                    ucolors.insert(col - left, color);
                }
                cells.push(cell);
            }
            spans.push(Span {
                line: line as u16,
                left: left as u16,
                right: right as u16,
                cells,
                combining,
                links,
                ucolors,
            });
        }

        Frame {
            cols: cols as u16,
            rows: rows as u16,
            kind,
            // The live cursor: position in screen coords + DECTCEM visibility.
            // Reported, not drawn — the consumer renders the caret (#38).
            cursor_row: self.cursor.row as u16,
            cursor_col: self.cursor.col as u16,
            // Hidden while scrolled up: the live cursor is off the frozen
            // viewport, and a cell-invert caret would otherwise ink over
            // scrollback. Consistent with the frozen-damage policy (no cursor
            // damage is emitted while scrolled) and with xterm.js / alacritty,
            // which hide the caret when it falls outside the visible rows (#48).
            cursor_visible: self.cursor.visible && self.display_offset == 0,
            cursor_shape: self.cursor.shape,
            cursor_blink: self.cursor.blink,
            // Viewport scroll position for the consumer's scrollbar (ADR-0013).
            display_offset: self.display_offset as u32,
            scrollback_len: self.scrollback.len() as u32,
            // The mouse tracking mode as a routing mask (#129): which mouse events
            // the app wants, derived from the protocol by the single source
            // `encode_mouse` shares. The consumer routes app-vs-local on it.
            mouse_events: self.mouse_protocol.wanted_events(),
            // Alt-screen flag (#149): buffer-global state the consumer can't
            // derive from viewport damage; the a11y announce policy gates on it.
            alt_screen: self.on_alt,
            scroll: self.scroll_delta(),
            spans,
            side_table,
            link_table,
            // Interaction overlays projected onto this viewport (#108): the
            // engine-owned selection and the consumer-supplied search highlights,
            // each re-projected here so the scroll offset is applied once, by the
            // same authority that projects the cells.
            overlay: Overlay {
                selection: self.selection_range(),
                matches: self
                    .search_highlights
                    .iter()
                    .flat_map(|m| self.match_spans(m))
                    .collect(),
                // The consumer-designated active match (#428), projected through
                // the same `match_spans` math — usually also present in `matches`
                // above (the renderer's ranking resolves the overlap, #424), but
                // a span designation may sit OUTSIDE a capped hand-over (#436).
                active_match: self
                    .active_search_highlight
                    .as_ref()
                    .map(|m| self.match_spans(m))
                    .unwrap_or_default(),
                markers: self.marker_positions(),
                marker_lines: self.all_marker_lines(),
            },
        }
    }

    /// Record a scroll of rows `[top, bottom]` by `count` (positive = up).
    ///
    /// Damage is indexed by row position, so it must follow the content the
    /// scroll just moved: rotate the bounds the same way and mark the newly
    /// exposed line fully damaged (it is new blank content for the consumer).
    fn record_scroll(&mut self, top: usize, bottom: usize, count: isize) {
        let cols = self.grid.cols();
        match count {
            1 => {
                self.line_damage[top..=bottom].rotate_left(1);
                self.line_damage[bottom] = LineBounds::fully_damaged(cols);
            }
            -1 => {
                self.line_damage[top..=bottom].rotate_right(1);
                self.line_damage[top] = LineBounds::fully_damaged(cols);
            }
            _ => {}
        }
        // Accumulate repeated scrolls of the same region into one op (flow
        // control). A *different* region cannot be expressed as one op, so
        // degrade to full rather than silently dropping the earlier scroll.
        match self.scroll {
            Some(op) if op.top == top && op.bottom == bottom => {
                self.scroll = Some(ScrollOp {
                    top,
                    bottom,
                    count: op.count + count,
                });
            }
            None => self.scroll = Some(ScrollOp { top, bottom, count }),
            Some(_) => {
                self.scroll = None;
                self.mark_fully_damaged();
            }
        }
    }

    /// Number of lines currently held in scrollback history.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Whether the app has an open synchronized-output block (DEC ?2026, #73).
    pub fn synchronized_output(&self) -> bool {
        self.synchronized_output
    }

    /// Whether the app enabled color-scheme-update notifications (DEC ?2031, #85).
    pub fn color_scheme_updates(&self) -> bool {
        self.color_scheme_updates
    }

    /// Whether the app enabled grapheme-cluster mode (DEC ?2027, #295): emoji ZWJ / skin-tone /
    /// flag / VS16 sequences are clustered into one cell. OFF (default) is per-char, wcwidth-compat.
    pub fn grapheme_clustering(&self) -> bool {
        self.grapheme_clustering
    }

    /// Whether the app enabled win32-input-mode (DEC ?9001, #86). The engine does
    /// not encode the raw key-records itself (a non-goal); a ConPTY consumer reads
    /// this to decide whether to emit them.
    pub fn win32_input_mode(&self) -> bool {
        self.win32_input_mode
    }

    /// Queue a color-scheme report (`CSI ? 997 ; 1 n` dark / `; 2 n` light) on the
    /// reply channel. The consumer calls this to answer a `ColorSchemeQuery` event
    /// or, when its scheme changes and `color_scheme_updates()` is set, to send the
    /// unsolicited notification. The engine never stores or interprets the scheme
    /// (#85).
    pub fn report_color_scheme(&mut self, dark: bool) {
        let ps = if dark { 1 } else { 2 };
        self.replies
            .extend_from_slice(format!("\x1b[?997;{ps}n").as_bytes());
    }

    /// OSC 10/11 set/query the default fg/bg, stacking the `;`-separated specs
    /// across the `[foreground, background]` slots — xterm's
    /// `_setOrReportSpecialColor` offset loop (#137). OSC 10 starts at slot 0
    /// (fg → bg), OSC 11 at slot 1 (bg). A `?` spec is a query. xterm's 3rd slot
    /// (cursor / OSC 12) is out of scope, so the stack caps at two slots — extra
    /// specs are dropped.
    fn special_color(&mut self, params: &[&[u8]], start: usize) {
        for (i, &spec) in params[1..].iter().enumerate() {
            let event = match start + i {
                0 if spec == b"?" => TermEvent::QueryForeground,
                0 => TermEvent::SetForeground(String::from_utf8_lossy(spec).into_owned()),
                1 if spec == b"?" => TermEvent::QueryBackground,
                1 => TermEvent::SetBackground(String::from_utf8_lossy(spec).into_owned()),
                _ => break, // past [fg, bg] — cursor (OSC 12) unsupported
            };
            self.events.push(event);
        }
    }

    /// Answer an OSC 4 palette query (#122): wrap the consumer-supplied spec for
    /// `index` in the OSC 4 reply envelope, ST-terminated.
    pub fn report_palette_color(&mut self, index: u8, spec: &str) {
        self.replies
            .extend_from_slice(format!("\x1b]4;{index};{spec}\x1b\\").as_bytes());
    }

    /// Answer an OSC 10 foreground query (#122): wrap the consumer-supplied spec
    /// in the OSC 10 reply envelope, ST-terminated.
    pub fn report_foreground(&mut self, spec: &str) {
        self.replies
            .extend_from_slice(format!("\x1b]10;{spec}\x1b\\").as_bytes());
    }

    /// Answer an OSC 11 background query (#122): wrap the consumer-supplied spec
    /// (it knows its palette) in the OSC 11 reply envelope, ST-terminated. The
    /// engine formats the envelope only — it never knows the colour.
    pub fn report_background(&mut self, spec: &str) {
        self.replies
            .extend_from_slice(format!("\x1b]11;{spec}\x1b\\").as_bytes());
    }

    /// The cells of visible row `i` (0..rows) at the current scroll position.
    /// The viewport windows into `[history.. ; screen..]`: rows above
    /// `scrollback.len()` come from history, the rest from the live screen.
    pub fn viewport_line(&self, i: usize) -> &[Cell] {
        let top = self.scrollback.len() - self.display_offset;
        let idx = top + i;
        if idx < self.scrollback.len() {
            &self.scrollback[idx]
        } else {
            self.grid.row(idx - self.scrollback.len())
        }
    }

    /// Scroll the viewport up by `n` lines into history (clamped to the oldest).
    pub fn scroll_up(&mut self, n: usize) {
        let target = (self.display_offset + n).min(self.scrollback.len());
        self.set_display_offset(target);
    }

    /// Scroll the viewport down by `n` lines toward the live screen.
    pub fn scroll_down(&mut self, n: usize) {
        let target = self.display_offset.saturating_sub(n);
        self.set_display_offset(target);
    }

    /// Jump the viewport back to the live screen (follow the bottom).
    pub fn scroll_to_bottom(&mut self) {
        self.set_display_offset(0);
    }

    /// Move the viewport. A user scroll changes which lines are visible, so the
    /// whole viewport is repainted (full damage) when the offset actually moves.
    fn set_display_offset(&mut self, offset: usize) {
        // The alt screen has no scrollback to view; scroll intents are no-ops.
        if self.on_alt {
            return;
        }
        if offset != self.display_offset {
            self.display_offset = offset;
            self.mark_fully_damaged();
        }
    }

    // ---- selection -----------------------------------------------------------

    /// Map a viewport cell `(row, col)` to an absolute buffer point. The top
    /// visible row is `scrollback.len() - display_offset`, so viewport row `i`
    /// is that plus `i`.
    fn viewport_to_abs(&self, row: usize, col: usize) -> BufferPoint {
        let top = self.scrollback.len() - self.display_offset;
        BufferPoint {
            line: top + row,
            col,
        }
    }

    /// The primary-screen grid, wherever it currently lives — swapped into
    /// `alt_grid` while on the alt screen (#192). Command marks anchor *primary*
    /// content, so extracting their text must read this, not the active grid.
    fn primary_grid(&self) -> &Grid {
        if self.on_alt {
            &self.alt_grid
        } else {
            &self.grid
        }
    }

    /// The hyperlink-pool index at **screen** `(row, col)` (the live grid), or
    /// `None` — flag-gated through the row's link map. Resolve to the URI with
    /// [`Term::hyperlink`]. Mirrors `grid().cell(row, col)`.
    pub(crate) fn screen_link_at(&self, row: usize, col: usize) -> Option<core::num::NonZeroU32> {
        self.grid.row_ref(row).link_at(col)
    }

    /// The underline colour (SGR 58, #520) at screen `(row, col)`, as a theme-agnostic
    /// reference. `Color::Default` means "follow the fg" — the common case, and what an
    /// unset cell returns. Mirror of [`Term::screen_link_at`].
    pub(crate) fn screen_underline_color_at(&self, row: usize, col: usize) -> Color {
        self.grid.row_ref(row).ucolor_at(col).unwrap_or_default()
    }

    /// The hyperlink-pool index at **viewport** `(row, col)` (visible window,
    /// history included at the current scroll), or `None`. Mirrors
    /// `viewport_line(row)`.
    pub(crate) fn viewport_link_at(&self, row: usize, col: usize) -> Option<core::num::NonZeroU32> {
        let idx = self.scrollback.len() - self.display_offset + row;
        self.abs_row(idx).link_at(col)
    }

    /// The viewport's logical lines (#113/ADR-0017): each line's text plus a
    /// per-char map to its viewport `(row, col)`. Wide-char spacers are skipped
    /// and trailing blanks trimmed (so the text is 1:1 with `cells`). Empty rows
    /// are dropped. The cell-aware assembly the consumer can't do in frame mode.
    pub fn viewport_logical_lines(&self) -> Vec<LogicalLine> {
        let rows = self.grid.rows();
        let total = self.scrollback.len() + rows;
        let top = self.scrollback.len() - self.display_offset; // abs line of viewport row 0
        let bottom = top + rows; // abs lines [top, bottom) are on screen

        // If viewport row 0 is a wrap-continuation, walk up into scrollback to
        // the logical line's true start so an edge-spanning URL still matches —
        // floored, because on alt that scrollback is the *primary* buffer's.
        let floor = self.abs_floor();
        let mut start = top;
        while start > floor && self.abs_row(start - 1).is_wrapped() {
            start -= 1;
        }

        let mut out = Vec::new();
        let mut line = start;
        while line < bottom {
            // Accumulate one logical line forward while each row soft-wraps; the
            // tail may run past `bottom` (off-screen below) — included too.
            let mut text = String::new();
            let mut map: Vec<(i32, usize)> = Vec::new();
            let mut cur = line;
            loop {
                let cells = self.abs_line(cur);
                for (col, cell) in cells.iter().enumerate() {
                    if cell.is_spacer() {
                        continue;
                    }
                    // Signed viewport row: < 0 above the top, >= rows below.
                    let vrow = cur as i32 - top as i32;
                    text.push(cell.c());
                    map.push((vrow, col));
                    // Combining marks (#45) ride the same cell — append each and
                    // map it to that cell so `text` stays 1:1 with `cells`.
                    if let Some(marks) = self.combining_at(cur, col) {
                        for &m in marks {
                            text.push(m);
                            map.push((vrow, col));
                        }
                    }
                }
                let soft = self.abs_row(cur).is_wrapped();
                if soft && cur + 1 < total {
                    cur += 1;
                } else {
                    break;
                }
            }
            // Trim trailing blanks (only the last row can have them), keeping
            // `text` and `cells` in lockstep.
            let trimmed = text.trim_end();
            map.truncate(trimmed.chars().count());
            text.truncate(trimmed.len());
            if !text.is_empty() {
                out.push(LogicalLine { text, cells: map });
            }
            line = cur + 1;
        }
        out
    }

    /// Register a decoration marker at viewport `row`, returning its stable id
    /// (#118). The row is resolved to an absolute buffer line (like a selection
    /// anchor), so the marker tracks that content through scroll/eviction/reflow.
    /// The active buffer's marker list (#177 S0) — alt while on the alt screen,
    /// else normal. Add/rotate/project operate on this; primary-scoped queries
    /// (`command_marks`/`command_lines`) and scrollback eviction read
    /// `normal_markers` directly.
    fn markers(&self) -> &Vec<Marker> {
        if self.on_alt {
            &self.alt_markers
        } else {
            &self.normal_markers
        }
    }

    /// Mutable [`Self::markers`].
    fn markers_mut(&mut self) -> &mut Vec<Marker> {
        if self.on_alt {
            &mut self.alt_markers
        } else {
            &mut self.normal_markers
        }
    }

    pub fn add_marker(&mut self, row: usize) -> MarkerId {
        // On the alt screen this anchors an *alt-scoped* marker (#187): per-buffer
        // storage (#186) keeps it out of the primary list, and it is disposed on
        // alt-leave — xterm's per-buffer `addMarker` + `clearAllMarkers`. No dead
        // sentinel is needed anymore; `markers_mut` routes to the active buffer.
        let line = self.viewport_to_abs(row, 0).line;
        self.push_marker(line, 0, MarkerKind::Plain)
    }

    /// Push a marker anchored at absolute `(line, col)` with `kind`, returning its
    /// id. The shared core of `add_marker` (viewport row, `col = 0`) and OSC-133
    /// command marks (cursor line + column) — one place owns id allocation + the
    /// `markers` list.
    fn push_marker(&mut self, line: usize, col: usize, kind: MarkerKind) -> MarkerId {
        let id = MarkerId(self.next_marker_id);
        self.next_marker_id += 1;
        self.markers_mut().push(Marker {
            id,
            line,
            col,
            kind,
        });
        id
    }

    /// Record an OSC 133 command-boundary mark at the cursor's current line
    /// (#158). Ignored on the alt screen: unlike the decoration guards that
    /// per-buffer storage retired (#187), this one stands on a *semantic* — OSC
    /// 133 is shell integration, which only runs on the primary screen, so an alt
    /// 133 is meaningless (there is no command to bound). Command nav/announce read
    /// the *normal* buffer's marks (`command_marks`/`command_lines`, primary-scoped
    /// since #186), so even a stray alt 133 could not reach them — but there is no
    /// value in creating an alt-scoped command mark nothing consumes (#188). The
    /// cursor line is `scrollback ++ screen`-absolute, independent of
    /// `display_offset` (the cursor is always in the grid, never scrollback).
    fn add_command_mark(&mut self, kind: MarkerKind) {
        if self.on_alt {
            return;
        }
        let line = self.scrollback.len() + self.cursor.row;
        // `cursor.col` alone is one column short whenever the command exactly filled the row: the
        // cursor that has just written the last cell is held *at* `cols - 1` with `pending_wrap`
        // set, because "one past the last column" is not a column (#562). A command mark's column
        // is an **exclusive** bound on the command text, so it wants precisely that unrepresentable
        // value — `extract_lines` clips `[b_col, c_col)` and `.min(cells.len())` absorbs it.
        //
        // Without this, `$ ` + `abcd` at 6 columns recorded `abc`: no resize involved, so this half
        // of #562 was reachable on a screen that never changed size.
        let col = self.cursor.col + usize::from(self.cursor.pending_wrap);
        self.push_marker(line, col, kind);
    }

    /// The OSC 133 command-boundary marks in buffer order — `(id, absolute line,
    /// kind)` (#158). Plain decoration markers (#118) are excluded. The consumer
    /// pairs prompt/command/finished marks and drives navigation/announce policy
    /// (#160); core only parses and anchors them.
    pub fn command_marks(&self) -> Vec<(MarkerId, usize, MarkerKind)> {
        // Primary-scoped: OSC-133 shell integration marks live on the normal
        // buffer, so command nav/announce read it even while on the alt screen.
        self.normal_markers
            .iter()
            .filter(|m| m.kind != MarkerKind::Plain)
            .map(|m| (m.id, m.line, m.kind))
            .collect()
    }

    /// The executed shell commands recovered from OSC-133 marks, in buffer order
    /// (#166) — the data behind screen-reader command navigation. Each
    /// [`CommandLine`] pairs a CommandStart(B) with the following OutputStart(C)
    /// to extract the *typed command* (the prompt before B and the output after C
    /// excluded via the captured columns, VSCode `extractCommandLine` parity), and
    /// attaches the trailing CommandFinished(D) exit. A command still being typed
    /// (B with no C yet) is not navigable — its text has no bound — so it is
    /// omitted until output starts.
    pub fn command_lines(&self) -> Vec<CommandLine> {
        let mut out: Vec<CommandLine> = Vec::new();
        // (B line, B col) awaiting its matching C. Marks arrive in buffer order.
        let mut pending: Option<(usize, usize)> = None;
        // Primary-scoped (see `command_marks`): the normal buffer's marks.
        for m in &self.normal_markers {
            match m.kind {
                MarkerKind::CommandStart => pending = Some((m.line, m.col)),
                MarkerKind::OutputStart => {
                    if let Some((b_line, b_col)) = pending.take() {
                        // Columns bound the command precisely even though output was
                        // written after C — `extract_lines` reads current cells but
                        // clips to `[b_col, c_col)`, excluding both prompt and output.
                        // Command marks anchor primary content — read the primary
                        // grid so the text is right even while on the alt screen (#192).
                        // Where the *typed* command begins, which is not always where B was
                        // emitted: a prompt that ends its row leaves B past that row's content, and
                        // the command really starts on the next line. Normalised **here** rather
                        // than inside `extract_lines`, because the two answers differ by caller —
                        // a selection that starts in a line's trailing blanks does contain the
                        // break that follows, a command does not — and because `doc_line_of` needs
                        // the same value. Feeding it the raw `b_line` reported the command one
                        // document line early, which is the a11y "jump to previous command" target.
                        let (b_line, b_col) =
                            self.command_start(self.primary_grid(), b_line, b_col, m.line);
                        let command =
                            self.extract_lines(self.primary_grid(), b_line, b_col, m.line, m.col);
                        out.push(CommandLine {
                            line: self.doc_line_of(self.primary_grid(), b_line),
                            command,
                            exit: None,
                        });
                    }
                }
                MarkerKind::CommandFinished(exit) => {
                    // The exit belongs to the most recent command not yet closed;
                    // the `is_none` guard stops a stray D from clobbering a code.
                    if let Some(last) = out.last_mut()
                        && last.exit.is_none()
                    {
                        last.exit = exit;
                    }
                }
                MarkerKind::Plain | MarkerKind::PromptStart => {}
            }
        }
        out
    }

    /// Advance an OSC-133 `CommandStart` position past any hard-ended line that holds no command
    /// text at or after it, stopping before `end` (the matching `OutputStart`).
    ///
    /// B is emitted at the cursor, so a prompt that fills — or merely ends — its row leaves the mark
    /// in that row's trailing blanks (#562). Two things then go wrong if the raw position is used:
    /// `extract_lines` selects an empty run and, because the row is hard-ended, flushes it with a
    /// `\n` the command never contained; and `doc_line_of` names the prompt's line rather than the
    /// command's. Both were reachable **without any resize** — the row only has to end before its
    /// width, which an 8-column row holding a 6-column prompt does.
    ///
    /// Only hard-ended rows advance. On a soft-wrapped row the continuation is the same logical
    /// line, its trailing blanks are real content (a space at a wrap boundary was typed), and no
    /// `\n` is flushed there anyway.
    fn command_start(&self, grid: &Grid, line: usize, col: usize, end: usize) -> (usize, usize) {
        let (mut line, mut col) = (line, col);
        while line < end && !self.row_in(grid, line).is_wrapped() {
            let cells = self.line_in(grid, line);
            if col < cells.len() && !cells[col..].iter().all(Cell::is_blank) {
                break;
            }
            line += 1;
            col = 0;
        }
        (line, col)
    }

    /// The document (logical) line index that absolute buffer line `abs` renders
    /// into within [`Term::accessible_text`] — the number of hard line-ends before
    /// it (soft-wrapped rows share one logical line). Primary-screen coordinates,
    /// matching `accessible_text`'s `start = 0` for the primary screen; command
    /// marks are primary-only. O(abs) per call — fine for an on-demand query over
    /// the handful of commands in a session.
    fn doc_line_of(&self, grid: &Grid, abs: usize) -> usize {
        (0..abs)
            .filter(|&l| !self.row_in(grid, l).is_wrapped())
            .count()
    }

    /// Remove a marker by id (#118). Disposing it fires `MarkerDisposed` so the
    /// consumer's cleanup is one path whether the marker left by eviction or by
    /// this explicit call (xterm's `dispose()` likewise always fires onDispose).
    /// A no-op for an unknown/already-disposed id.
    pub fn remove_marker(&mut self, id: MarkerId) {
        // Id-based, buffer-agnostic: search both lists (ids are unique across
        // buffers) so a marker is removed whichever screen it lives on (#177 S0).
        let before = self.normal_markers.len() + self.alt_markers.len();
        self.normal_markers.retain(|m| m.id != id);
        self.alt_markers.retain(|m| m.id != id);
        if self.normal_markers.len() + self.alt_markers.len() != before {
            self.events.push(TermEvent::MarkerDisposed(id));
        }
    }

    /// Shift markers down one absolute line after the oldest history line is
    /// evicted; a marker *on* that line (abs 0) has left the buffer, so it is
    /// disposed and announced (#118) — the marker analogue of
    /// `selection_evict_oldest`, but a list with per-marker disposal.
    /// The marker analogue of `selection_shift_below_margin` (#449) — primary
    /// only, because the accrual branch that needs it is primary-only.
    fn markers_shift_below_margin(&mut self, from: usize) {
        for m in &mut self.normal_markers {
            if m.line >= from {
                m.line += 1;
            }
        }
    }

    fn markers_evict_oldest(&mut self) {
        // Scrollback eviction is primary-only (the alt screen has none).
        let mut disposed = Vec::new();
        self.normal_markers.retain_mut(|m| {
            if m.line == 0 {
                disposed.push(m.id);
                false
            } else {
                m.line -= 1;
                true
            }
        });
        for id in disposed {
            self.events.push(TermEvent::MarkerDisposed(id));
        }
    }

    /// Rotate markers within an in-screen region scroll of absolute lines
    /// `[top, bottom]` (`up` = a line dropped at `top`, else at `bottom`) — the
    /// marker analogue of `selection_rotate_region`. A marker on the dropped edge
    /// has left the buffer, so it is disposed and announced (#118).
    fn markers_rotate_region(&mut self, top: usize, bottom: usize, up: bool) {
        let mut disposed = Vec::new();
        self.markers_mut().retain_mut(|m| {
            if m.line < top || m.line > bottom {
                return true; // outside the region — unchanged
            }
            let dropped_edge = if up { top } else { bottom };
            if m.line == dropped_edge {
                disposed.push(m.id);
                false
            } else {
                m.line = if up { m.line - 1 } else { m.line + 1 };
                true
            }
        });
        for id in disposed {
            self.events.push(TermEvent::MarkerDisposed(id));
        }
    }

    /// The active buffer's markers projected onto the current viewport — one
    /// `MarkerPosition` per marker whose line is visible, off-screen markers
    /// omitted. The alt screen projects its own (alt-scoped) markers now (#187);
    /// they are disposed on alt-leave, so a primary frame never shows them.
    fn marker_positions(&self) -> Vec<MarkerPosition> {
        let top = self.scrollback.len() - self.display_offset;
        let rows = self.grid.rows();
        self.markers()
            .iter()
            .filter_map(|m| {
                let row = m.line.checked_sub(top)?;
                (row < rows).then_some(MarkerPosition {
                    id: m.id,
                    row,
                    kind: m.kind,
                })
            })
            .collect()
    }

    /// Every live marker's absolute buffer line (#120 S3) — the off-viewport
    /// superset of `marker_positions`, for the overview ruler. No viewport filter:
    /// a marker scrolled out of view is still reported (that is the ruler's job),
    /// its `line` in the same `[0, scrollback + rows)` frame as the header's
    /// `scrollback_len`/`display_offset`.
    fn all_marker_lines(&self) -> Vec<MarkerLine> {
        self.markers()
            .iter()
            .map(|m| MarkerLine {
                id: m.id,
                line: m.line as u32,
            })
            .collect()
    }

    /// Resize the screen to `cols` x `rows`. Rows dropped off the top (on shrink)
    /// enter scrollback. Column reflow of soft-wrapped lines is layered on top
    /// separately (#7). The whole screen is damaged.
    ///
    /// `cols` is widened to [`MIN_COLUMNS`] — a narrower screen cannot hold a
    /// width-2 glyph, so it is clamped rather than represented (#547).
    pub fn resize(&mut self, cols: usize, rows: usize) {
        // A terminal is never 0-tall; clamp so the math below (rows - 1) can't
        // underflow. Columns clamp to MIN_COLUMNS, not 1: chunking by cols needs a
        // non-zero width, but a *wide glyph* needs two (#547).
        let cols = cols.max(MIN_COLUMNS);
        let rows = rows.max(1);
        let old_cols = self.grid.cols();
        let limit = self.scrollback_limit;

        // A reflow moves match coordinates (and can change the match set), so the
        // query-derived highlights are invalidated; the consumer re-searches at
        // the new width. The selection re-anchors below — it is user-authored.
        self.invalidate_search_highlights();

        // Both screens are resized. Scrollback pairs with the PRIMARY screen
        // (whichever is active) — the alt screen has no history of its own.
        // `reflow: true` is the *primary* pane's setting and is deliberately a constant, **not**
        // `self.autowrap`. ghostty gates its equivalent on DECAWM — `.reflow =
        // self.modes.get(.wraparound)` (`terminal/Terminal.zig` `resize`) — and the reading that
        // makes that coherent is the one this file just accepted for the alt screen: an application
        // that turns autowrap off is placing lines itself, so its content is a layout rather than a
        // flow, and re-wrapping it changes what it drew.
        //
        // Not followed here, for three reasons, and they are recorded rather than filed because
        // nothing observable is known to break either way (measured: with DECAWM off, a full
        // 6-column row still re-splits into two rows at width 3 exactly as it does with DECAWM on;
        // the difference against ghostty is that ghostty truncates that row instead).
        //
        // - **The wrap flag is not a lie.** `Row::is_wrapped` means "this row continues into the
        //   next", which after a re-split is simply true. DECAWM governs the **write** path — where
        //   a glyph goes when the cursor is at the last column — not how stored content is laid out
        //   again later. Dropping the flag would make `"abcdef"` extract as `"abc\ndef"`.
        // - **The mode is global and momentary; the buffer is neither.** DECAWM is read at resize
        //   time and would decide the fate of history written under the opposite setting. A TUI that
        //   turns it off while drawing would, on a resize landing in that window, leave every
        //   properly wrapped line in scrollback un-reflowed.
        // - **It costs content.** Not reflowing truncates each row to the new width, so the tail of
        //   a long line leaves the grid. justerm keeps it.
        //
        // There is no per-row signal to be finer with: a row written under DECAWM off and a row that
        // merely ended early are both simply unwrapped. "Do not re-split an unwrapped logical line"
        // would break ordinary reflow, since a line that exactly fills its width carries no wrap
        // flag either.
        let dims = ReflowDims {
            old_cols,
            cols,
            rows,
            limit,
            reflow: true,
        };
        let scrollback = std::mem::take(&mut self.scrollback);
        if self.on_alt {
            // Active = alt (cursor, no scrollback); inactive = primary. Selection
            // is primary-only and cleared on alt enter, so no anchors to track.
            // Alt markers still ride this pane, but **not because it reflows** — since #567 it does
            // not, so a marker's content no longer moves under it and the old reason here ("justerm
            // column-reflows the alt grid, so a marker must follow its content") is retracted. What
            // they ride it for is the row *fit*: a shrink still drops rows off the top, and a marker
            // on one of those has left a screen with no history to hold it. Their stored line is
            // `base + alt_row` (base = primary scrollback len), so convert to alt-local rows here
            // and re-anchor on the new base afterward — the primary scrollback below may rewrap and
            // change length even when the alt grid does not move at all.
            let old_base = scrollback.len();
            let alt_pts: Vec<(usize, usize)> = self
                .alt_markers
                .iter()
                .map(|m| (m.line - old_base, m.col))
                .collect();
            let alt = self.grid.take_lines();
            let r_alt = reflow_pane(
                alt,
                VecDeque::new(),
                self.cursor.point(),
                &alt_pts,
                ReflowDims {
                    limit: 0,
                    reflow: false,
                    ..dims
                },
            );
            self.grid.set_screen(r_alt.screen, cols, rows);
            self.cursor.set_point(r_alt.cursor, rows, cols);

            // Primary is inactive here, but markers anchor *primary* content, so
            // they reflow with it (the selection is already cleared on alt enter).
            // `(line, col)`, not `(line, 0)`: the column is what bounds OSC-133 command-text
            // extraction (#166), and discarding it here truncated the recorded command for any
            // resize taken while a full-screen app was up. The primary branch below has always
            // passed and restored both; this one is the sibling that did not.
            let marker_pts: Vec<(usize, usize)> = self
                .normal_markers
                .iter()
                .map(|m| (m.line, m.col))
                .collect();
            let primary = self.alt_grid.take_lines();
            let r = reflow_pane(
                primary,
                scrollback,
                self.saved_cursor.point(),
                &marker_pts,
                dims,
            );
            self.alt_grid.set_screen(r.screen, cols, rows);
            self.scrollback = r.scrollback;
            self.saved_cursor.set_point(r.cursor, rows, cols);
            for (i, m) in self.normal_markers.iter_mut().enumerate() {
                m.line = r.extras[i].0.saturating_sub(r.evicted);
                m.col = r.extras[i].1;
            }
            // The alt half lives in a different frame from the primary one above, and adding the
            // two was the defect: `extras` count from the top of the alt pane's own history, and
            // the alt screen **has** no history — every row the shrink pushed off the top is gone,
            // not archived. Passing the primary's scrollback limit made `reflow_pane` keep them,
            // so a rows-only resize (no reflow at all) reported a marker four lines past the end of
            // the buffer. The limit is `0` here because that is what an alt screen's history is.
            //
            // A marker whose row went with it is **disposed**, matching what the alt screen already
            // does when a row leaves by scrolling (`markers_rotate_region` fires `MarkerDisposed`
            // for the marker on the departing edge). Silently relocating it to row 0 would put a
            // decoration on content it was never attached to.
            let new_base = self.scrollback.len();
            let mut alt_disposed = Vec::new();
            let mut i = 0;
            self.alt_markers.retain_mut(|m| {
                let (line, col) = r_alt.extras[i];
                i += 1;
                match line.checked_sub(r_alt.evicted) {
                    Some(row) if row < rows => {
                        m.line = new_base + row;
                        // The column rides along for the same reason as the primary half, but
                        // **unpinned**: `add_marker` always passes column 0, and I could not get an
                        // OSC-133 mark (the only column-bearing kind) to appear in `alt_markers` at
                        // all. That is a gap in my knowledge, not evidence the column is
                        // structurally zero — `push_marker` takes a column and `markers_mut` routes
                        // by active buffer, so the field is reachable in principle. Carrying it
                        // keeps the two halves stating one invariant; if the alt path really is
                        // marker-column-free, this line is a no-op.
                        m.col = col;
                        true
                    }
                    _ => {
                        alt_disposed.push(m.id);
                        false
                    }
                }
            });
            for id in alt_disposed {
                self.events.push(TermEvent::MarkerDisposed(id));
            }
        } else {
            // Active = primary (cursor, scrollback); inactive = alt. The selection
            // anchors (absolute) reflow alongside the cursor so they keep their
            // content across a column change.
            let sel_pts: Vec<(usize, usize)> = self
                .selection
                .as_ref()
                .map(|s| {
                    vec![
                        (s.anchor.point.line, s.anchor.point.col),
                        (s.focus.point.line, s.focus.point.col),
                    ]
                })
                .unwrap_or_default();
            // Markers reflow on the same pane by (line, col) — the column matters
            // for OSC-133 command marks, whose B/C columns bound the extracted
            // command text (#166). They ride after the selection points so each
            // reads its own reflowed slot back from `extras` (#118).
            let mut pts = sel_pts.clone();
            pts.extend(self.normal_markers.iter().map(|m| (m.line, m.col)));

            let primary = self.grid.take_lines();
            let r = reflow_pane(primary, scrollback, self.cursor.point(), &pts, dims);
            self.grid.set_screen(r.screen, cols, rows);
            self.scrollback = r.scrollback;
            self.cursor.set_point(r.cursor, rows, cols);
            if let Some(sel) = &mut self.selection {
                // A selection endpoint is **UI** state, so its reading of `col == cols` (#562) is
                // neither the cursor's nor a mark's: it is clamped into the grid. UI state may not
                // move the application's content to make room for itself — the criterion that
                // decided this, and the one ghostty encodes by clamping every non-cursor pin before
                // it can widen a row (`terminal/PageList.zig:1576-1585` @ `e6e26e1`) while leaving
                // the cursor pin unclamped (`:1602-1606`).
                sel.anchor.point = BufferPoint {
                    line: r.extras[0].0.saturating_sub(r.evicted),
                    col: r.extras[0].1.min(cols - 1),
                };
                sel.focus.point = BufferPoint {
                    line: r.extras[1].0.saturating_sub(r.evicted),
                    col: r.extras[1].1.min(cols - 1),
                };
            }
            let marker_off = sel_pts.len();
            for (i, m) in self.normal_markers.iter_mut().enumerate() {
                m.line = r.extras[marker_off + i].0.saturating_sub(r.evicted);
                m.col = r.extras[marker_off + i].1;
            }

            let alt = self.alt_grid.take_lines();
            let r = reflow_pane(
                alt,
                VecDeque::new(),
                (0, 0),
                &[],
                ReflowDims {
                    limit: 0,
                    reflow: false,
                    ..dims
                },
            );
            self.alt_grid.set_screen(r.screen, cols, rows);
        }

        // Carry the deferred wrap across the resize — it is *cursor* state, and it used to be
        // reset here alongside the margins and tab stops, which are screen configuration and do
        // legitimately reset. Losing it meant the next byte overwrote the last glyph instead of
        // wrapping past it, on a column resize *and* on a rows-only one where no reflow runs at
        // all.
        //
        // The flag means "the cursor is logically one past the column it sits on". Where the
        // reflow leaves it somewhere other than the last column that logical position **is**
        // representable, so the flag is cleared and the cursor takes it instead — ghostty's rule,
        // stated in its own words for the saved cursor: *"If we had pending wrap set and we're no
        // longer at the end of the line, we unset the pending wrap and move the cursor to reflect
        // the correct next position"* (`terminal/Screen.zig:2092-2098` @ `e6e26e1`). alacritty
        // reaches the same place from the other side, lifting the cursor outside the grid before
        // reflowing and clamping it back afterwards (`grid/resize.rs:113-116`, `:248-251`,
        // `:173-177` @ `852e971`); xterm.js needs no rule because `x === cols` is representable.
        //
        // `col + 1` cannot overflow the row: the branch requires `col != cols - 1`, and `col` is
        // already clamped below `cols` by `Cursor::set_point`.
        if self.cursor.pending_wrap && self.cursor.col != cols - 1 {
            self.cursor.pending_wrap = false;
            self.cursor.col += 1;
        }
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        self.tabs = default_tabs(cols);
        self.display_offset = self.display_offset.min(self.scrollback.len());

        // Damage tracking is sized to the screen; a resize repaints everything,
        // so drop any pending scroll op (it points at the old rows).
        self.line_damage = vec![LineBounds::undamaged(cols); rows];
        self.scroll = None;
        self.mark_fully_damaged();
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// Whether bracketed-paste mode (DEC ?2004) is enabled. The input encoder
    /// (#11) reads this to decide whether to wrap pasted text in markers.
    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    // ---- input encoding (#11) ------------------------------------------------

    /// Encode a key event to bytes using the active cursor-key mode (DECCKM)
    /// and the kitty keyboard-protocol flags (`encode_key` consults both).
    pub fn encode_key(&self, ev: KeyEvent) -> Option<Vec<u8>> {
        encode_key(
            &ev,
            self.app_cursor_keys,
            self.application_keypad,
            self.kitty_flags,
        )
    }

    /// Encode a mouse event using the active tracking mode + encoding. `None`
    /// when reporting is off or the event is filtered by the mode.
    pub fn encode_mouse(&self, ev: MouseEvent) -> Option<Vec<u8>> {
        encode_mouse(&ev, self.mouse_protocol, self.mouse_encoding)
    }

    /// Encode pasted text, wrapping it in bracketed-paste markers when ?2004 is
    /// on.
    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        encode_paste(text, self.bracketed_paste)
    }

    /// Encode a focus change (`CSI I`/`CSI O`), or `None` when focus reporting
    /// (?1004) is off.
    pub fn encode_focus(&self, focused: bool) -> Option<Vec<u8>> {
        encode_focus(focused, self.focus_events)
    }

    /// Take the consumer events queued since the last drain, emptying the queue.
    pub fn drain_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut self.events)
    }

    /// Take the reply bytes queued since the last drain (DA/DSR/DECRQM answers),
    /// emptying the buffer. The consumer writes them back to the PTY.
    pub fn drain_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// Device Status Report (CSI Ps n): 6 = cursor position, 5 = operating
    /// status. Queues the reply for `drain_replies` (#27).
    fn device_status_report(&mut self, param: u16) {
        match param {
            6 => {
                // CSI row;col R, 1-based — region-relative under origin mode
                // (the coordinate system the app is addressing in).
                let row = if self.origin_mode {
                    self.cursor.row.saturating_sub(self.scroll_top)
                } else {
                    self.cursor.row
                } + 1;
                let col = self.cursor.col + 1;
                self.replies
                    .extend_from_slice(format!("\x1b[{row};{col}R").as_bytes());
            }
            5 => self.replies.extend_from_slice(b"\x1b[0n"), // status: OK
            _ => {}
        }
    }

    /// Kitty keyboard-protocol negotiation (#23). `lead` is the leading CSI
    /// intermediate: `?` query, `>` push, `=` set, `<` pop.
    fn kitty_dispatch(&mut self, lead: u8, params: &Params) {
        match lead {
            // Query → report the current flags as `CSI ? flags u` (#27 channel).
            b'?' => self
                .replies
                .extend_from_slice(format!("\x1b[?{}u", self.kitty_flags).as_bytes()),
            // Push: save the current flags, then set the new ones (default 0).
            b'>' => {
                const KITTY_STACK_CAP: usize = 16;
                if self.kitty_stack.len() >= KITTY_STACK_CAP {
                    self.kitty_stack.remove(0); // drop the oldest on overflow
                }
                self.kitty_stack.push(self.kitty_flags);
                self.kitty_flags = param_or(params, 0, 0) as u8;
            }
            // Pop `n` (default 1): restore from the stack, 0 once empty.
            b'<' => {
                for _ in 0..param_or(params, 0, 1) {
                    self.kitty_flags = self.kitty_stack.pop().unwrap_or(0);
                }
            }
            // Set in place (no push): mode 1 replace, 2 or-in, 3 and-not.
            b'=' => {
                let flags = param_or(params, 0, 0) as u8;
                self.kitty_flags = match param_or(params, 1, 1) {
                    1 => flags,
                    2 => self.kitty_flags | flags,
                    3 => self.kitty_flags & !flags,
                    _ => self.kitty_flags,
                };
            }
            _ => {}
        }
    }

    /// DECRQM (CSI ? Ps $ p): report whether DEC private mode `Ps` is set —
    /// `CSI ? Ps ; val $ y` with val 1=set, 2=reset, 0=not recognized (#27).
    fn decrqm(&mut self, mode: u16) {
        let state = match mode {
            1 => Some(self.app_cursor_keys),
            // DECANM (#84): set = ANSI mode (the normal state), reset = VT52.
            2 => Some(!self.vt52_mode),
            6 => Some(self.origin_mode),
            // DECCOLM: derived from the actual width, never a tracked flag — a
            // flag would lie if the consumer ignored the resize request (#82).
            3 => Some(self.grid.cols() == 132),
            7 => Some(self.autowrap),
            45 => Some(self.reverse_wraparound),
            9 => Some(self.mouse_protocol == MouseProtocol::X10),
            66 => Some(self.application_keypad),
            12 => Some(self.cursor.blink),
            25 => Some(self.cursor.visible),
            // Mouse tracking is a single-state enum (the levels are mutually
            // exclusive — an app enables one), so querying ?1000 while ?1002 is
            // active reports "reset". Faithful to that model.
            1000 => Some(self.mouse_protocol == MouseProtocol::Normal),
            1002 => Some(self.mouse_protocol == MouseProtocol::ButtonEvent),
            1003 => Some(self.mouse_protocol == MouseProtocol::AnyEvent),
            1004 => Some(self.focus_events),
            1006 => Some(self.mouse_encoding == MouseEncoding::Sgr),
            1015 => Some(self.mouse_encoding == MouseEncoding::Urxvt),
            1005 => Some(self.mouse_encoding == MouseEncoding::Utf8),
            1016 => Some(self.mouse_encoding == MouseEncoding::SgrPixels),
            47 | 1047 | 1049 => Some(self.on_alt),
            2004 => Some(self.bracketed_paste),
            2026 => Some(self.synchronized_output),
            2027 => Some(self.grapheme_clustering),
            2031 => Some(self.color_scheme_updates),
            9001 => Some(self.win32_input_mode),
            _ => None,
        };
        let val = match state {
            Some(true) => 1,
            Some(false) => 2,
            None => 0,
        };
        self.replies
            .extend_from_slice(format!("\x1b[?{mode};{val}$y").as_bytes());
    }

    /// Resolve a cell's `link` index (OSC 8) to its URI, or `None` if the index
    /// is out of range. The renderer reads `Cell.link`, then this, to make a
    /// cell clickable (#26).
    pub fn hyperlink(&self, link: core::num::NonZeroU32) -> Option<&str> {
        self.hyperlink_pool
            .get(link.get() as usize - 1)
            .map(String::as_str)
    }

    // ---- cursor / scroll primitives ------------------------------------------

    /// Move down one line. At the bottom margin, scroll the region instead;
    /// below the region, just descend (no scroll). Column is unchanged (raw LF;
    /// CR is what returns to column 0).
    /// An ordinary line feed — `LF`/`VT`/`FF`, `IND` and `NEL`. None of them serves a wrap.
    fn linefeed(&mut self) {
        self.linefeed_inner(false);
    }

    /// A line feed, carrying the one fact the shift itself cannot see: whether the auto-wrap asked
    /// for it.
    ///
    /// `serves_wrap` is the **bottom** seam's exemption in `shift_region`, the mirror of
    /// `evicts_to_scrollback` for the top one. When `wrapline` drives this, the blank that lands at
    /// the region's bottom *is* where the wrapped text is about to go — so the row that #540 would
    /// call "the one that lost its continuation to the blank" is in fact the row whose continuation
    /// that blank **is** (#557).
    ///
    /// xterm.js threads the identical fact through the identical seam, in the opposite direction:
    /// `BufferService.scroll(eraseAttr, isWrapped)` stamps the *destination* row
    /// (`common/services/BufferService.ts:68`/`:77` @ `699f553`), and exactly one of its four
    /// non-test callers passes `true` — the auto-wrap branch of `_print` (`InputHandler.ts:588`),
    /// not `lineFeed`, `index` or the ED-2 loop.
    fn linefeed_inner(&mut self, serves_wrap: bool) {
        // New-line mode (LNM ?20): a line feed also returns to column 0 (#71).
        if self.newline_mode {
            self.carriage_return();
        }
        if self.cursor.row == self.scroll_bottom {
            // A top-anchored primary-screen scroll pushes the evicted top line
            // into scrollback history.
            if self.scroll_top == 0 && !self.on_alt {
                // Scrollback accrues whenever the scroll is top-anchored on the
                // primary screen (`scroll_top == 0`) — but the O(1) ring handshake
                // only applies to a *full-screen* scroll (`scroll_bottom` at the
                // last row). A top-anchored *sub-region* (`[0..k]`, k < rows-1)
                // still accrues, yet must scroll only its region, so it keeps the
                // copy + region scroll. These are distinct predicates (ADR-0009).
                let evicted = if self.scroll_bottom == self.grid.rows() - 1 {
                    // Full-screen hot path: move the evicted top row out, install
                    // a recycled blank as the new bottom (zero-alloc steady state).
                    let blank = self
                        .recycled_row
                        .take()
                        .unwrap_or_else(|| Row::from_cells(Vec::with_capacity(self.grid.cols())));
                    let evicted = self.grid.scroll_up_recycle(blank);
                    // The one row-shifting path that does not route through `shift_region` (it
                    // needs the primitive that *returns* the evicted row), so it records its own
                    // scroll op — and owes no seam clear, which is now an argument rather than a
                    // coincidence: the top seam is exempt because this evicts into scrollback
                    // (adjacency is preserved one row back), and the bottom seam is exempt because
                    // this branch only runs at `scroll_bottom == rows - 1`, where a wrap on the
                    // last row is the state the scroll exists to serve.
                    self.record_scroll(self.scroll_top, self.scroll_bottom, 1);
                    evicted
                } else {
                    // Top-anchored sub-region: copy row 0, then region-scroll
                    // `[0..=scroll_bottom]` (rows below stay fixed).
                    //
                    // #449: the fixed rows keep their GRID position while
                    // scrollback grows, so their content's concatenated absolute
                    // index shifts +1 — re-anchor the content-tracking anchors
                    // (selection, markers; alacritty's swap-back of the fixed
                    // bottom lines is the screen-relative equivalent of this)
                    // and invalidate the query-derived highlights
                    // (drop-not-re-anchor policy, #108). In-region and
                    // scrollback content keeps stable indices — untouched.
                    let below = self.scrollback.len() + self.scroll_bottom + 1;
                    self.selection_shift_below_margin(below);
                    self.markers_shift_below_margin(below);
                    self.invalidate_search_highlights();
                    let evicted = self.grid.row_owned(0);
                    self.shift_region(
                        self.scroll_top,
                        self.scroll_bottom,
                        false,
                        true,
                        serves_wrap,
                    );
                    evicted
                };
                self.scrollback.push_back(evicted);
                // Follow-bottom = stay: if the user is scrolled up, bump the
                // offset so the same lines stay in view instead of being yanked
                // to the bottom.
                if self.display_offset > 0 {
                    self.display_offset = (self.display_offset + 1).min(self.scrollback.len());
                }
                // Cap: evict the oldest line past the limit. The view is anchored
                // to history, so dropping the front shifts the offset down too
                // (xterm.js trims ybase and ydisp together) — also keeps the
                // offset within `[0, len]`. The evicted row is parked for reuse.
                if self.scrollback.len() > self.scrollback_limit {
                    self.recycled_row = self.scrollback.pop_front();
                    // Every absolute index just shifted down by one; move the
                    // selection with it so its anchors keep their content.
                    self.selection_evict_oldest();
                    // Query-derived highlights can't survive the index shift (see
                    // the method doc); selection re-anchors, highlights invalidate.
                    self.invalidate_search_highlights();
                    // Markers are persistent anchors: shift them down with the
                    // index, disposing any whose line was the evicted one (#118).
                    self.markers_evict_oldest();
                    if self.display_offset > 0 {
                        // Scrolled up: evicting the oldest line advanced the
                        // viewport, so it must be repainted (the "frozen while
                        // scrolled" rule does not apply when the view itself moved).
                        self.display_offset -= 1;
                        self.mark_fully_damaged();
                    }
                }
            } else {
                // Region (top margin > 0) or alt-screen scroll: the evicted line
                // does NOT enter scrollback, so content moves *within* the screen
                // and absolute indices in the region shift. Rotate the selection
                // up so it follows; an endpoint on the dropped line clears it.
                let base = self.scrollback.len();
                self.selection_rotate_region(
                    base + self.scroll_top,
                    base + self.scroll_bottom,
                    true,
                );
                // Rotate the active buffer's markers with the content (#187):
                // per-buffer storage (#186) scopes them, so an alt scroll rotates
                // *alt* marks and leaves the frozen primary list untouched — no
                // guard needed. `markers_rotate_region` routes via `markers_mut`.
                self.markers_rotate_region(base + self.scroll_top, base + self.scroll_bottom, true);
                self.invalidate_search_highlights();
                self.shift_region(
                    self.scroll_top,
                    self.scroll_bottom,
                    false,
                    false,
                    serves_wrap,
                );
            }
        } else if self.cursor.row + 1 < self.grid.rows() {
            self.cursor.row += 1;
        }
    }

    /// DECSTBM (CSI r): set the top/bottom scroll margins (1-based inclusive).
    /// An invalid region (top ≥ bottom) is ignored.
    fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let bottom = bottom.min(self.grid.rows());
        if top >= bottom {
            return;
        }
        self.scroll_top = top - 1;
        self.scroll_bottom = bottom - 1;
        self.goto(0, 0); // DECSTBM homes the cursor (absolute)
    }

    // ---- alt screen (DEC 1049) -----------------------------------------------

    /// Enter the alternate screen: save the cursor, swap in the other grid, and
    /// clear it.
    /// Save the cursor into the alt-screen slot — `?1048` set, and the first
    /// half of `?1049` enter (#72).
    fn save_alt_cursor(&mut self) {
        self.saved_cursor = self.cursor;
    }

    /// Restore the cursor from the alt-screen slot — `?1048` reset, and the
    /// second half of `?1049` leave. DECTCEM visibility is a standalone mode, not
    /// part of the save, so preserve it across the restore (#38/#72).
    fn restore_alt_cursor(&mut self) {
        let visible = self.cursor.visible;
        self.cursor = self.saved_cursor;
        self.cursor.visible = visible;
    }

    /// Switch to the (cleared) alternate buffer without touching the cursor —
    /// `?47`/`?1047` set, and the second half of `?1049` enter (#72).
    fn switch_to_alt(&mut self) {
        if self.on_alt {
            return;
        }
        std::mem::swap(&mut self.grid, &mut self.alt_grid);
        self.grid.clear();
        self.on_alt = true;
        self.display_offset = 0; // the alt screen has no scrollback to view
        self.selection = None; // a selection cannot survive a screen swap
        self.invalidate_search_highlights(); // matches index the primary buffer
        self.mark_fully_damaged();
    }

    /// Switch back to the primary buffer without touching the cursor —
    /// `?47`/`?1047` reset, and the first half of `?1049` leave (#72).
    fn switch_to_primary(&mut self) {
        if !self.on_alt {
            return;
        }
        // Dispose the alt buffer's markers on leave — xterm `activateNormalBuffer`
        // → `clearAllMarkers` (#177 S0). Empty while the alt guards stand, so this
        // fires nothing today; it's the seam the alt-marker slices (#187) build on.
        for m in self.alt_markers.drain(..) {
            self.events.push(TermEvent::MarkerDisposed(m.id));
        }
        std::mem::swap(&mut self.grid, &mut self.alt_grid);
        self.on_alt = false;
        self.display_offset = 0; // return to the primary at its bottom
        self.selection = None; // a selection cannot survive a screen swap
        self.invalidate_search_highlights(); // matches index the swapped-out buffer
        self.mark_fully_damaged();
    }

    fn enter_alt_screen(&mut self) {
        if self.on_alt {
            return;
        }
        self.save_alt_cursor();
        self.switch_to_alt();
    }

    /// Leave the alternate screen: swap the primary grid back in and restore the
    /// saved cursor.
    fn leave_alt_screen(&mut self) {
        if !self.on_alt {
            return;
        }
        self.switch_to_primary();
        self.restore_alt_cursor();
    }

    /// RI (ESC M): move up one line. At the top margin, scroll the region down
    /// instead.
    fn reverse_index(&mut self) {
        if self.cursor.row == self.scroll_top {
            // RI never enters scrollback; the region scrolls down within the
            // screen, so absolute indices in it shift down. Rotate the selection.
            let base = self.scrollback.len();
            self.selection_rotate_region(base + self.scroll_top, base + self.scroll_bottom, false);
            // Rotate the active buffer's markers (#187) — alt-scoped on the alt
            // screen, so no guard (see `linefeed`).
            self.markers_rotate_region(base + self.scroll_top, base + self.scroll_bottom, false);
            self.invalidate_search_highlights();
            self.shift_region(self.scroll_top, self.scroll_bottom, true, false, false);
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
        }
    }

    // ---- cursor save/restore (DECSC / DECRC) ---------------------------------

    /// DECSC (ESC 7): save the cursor position, pen, pending-wrap, and origin
    /// mode. Visibility is not saved (DECTCEM is separate).
    fn save_cursor(&mut self) {
        self.decsc = SavedCursor {
            row: self.cursor.row,
            col: self.cursor.col,
            pen: self.cursor.pen,
            pending_wrap: self.cursor.pending_wrap,
            origin_mode: self.origin_mode,
            charsets: self.charsets,
            gl: self.gl,
        };
    }

    /// DECRC (ESC 8): restore what DECSC saved. Origin mode is restored (per
    /// ADR-0004); visibility is left as-is. The position is clamped to the
    /// current screen in case it shrank since the save.
    fn restore_cursor(&mut self) {
        let s = self.decsc;
        self.cursor.row = s.row.min(self.grid.rows() - 1);
        self.cursor.col = s.col.min(self.grid.cols() - 1);
        self.cursor.pen = s.pen;
        self.cursor.pending_wrap = s.pending_wrap;
        self.origin_mode = s.origin_mode;
        self.charsets = s.charsets;
        self.gl = s.gl;
    }

    /// RIS (ESC c) — full reset to the power-on state (#53). Reconstruct every
    /// screen/mode field to its construction default (preserving only the
    /// dimensions and the scrollback cap), but keep the consumer-bound output
    /// queues (`replies`/`events`) that accrued earlier in this `feed`, and
    /// signal a full repaint. The vte parser lives outside `Term`, so replacing
    /// `self` does not disturb in-progress parsing. Mirrors xterm.js fullReset.
    fn full_reset(&mut self) {
        let replies = std::mem::take(&mut self.replies);
        let mut events = std::mem::take(&mut self.events);
        // RIS wipes the buffer, so every marker's line is gone — announce each
        // disposal so the consumer drops its decorations (and isn't confused when
        // the reset id counter reissues the same ids). The events survive the
        // reset below (#118).
        events.extend(
            self.normal_markers
                .iter()
                .chain(&self.alt_markers)
                .map(|m| TermEvent::MarkerDisposed(m.id)),
        );
        let (cols, rows) = (self.grid.cols(), self.grid.rows());
        *self = Term::with_scrollback(cols, rows, self.scrollback_limit);
        self.replies = replies;
        self.events = events;
        self.mark_fully_damaged();
    }

    /// DECSTR (CSI ! p) — soft reset (#53). Resets a defined subset of modes to
    /// their defaults *without* destroying screen content or scrollback, moving
    /// the active cursor, or touching the mouse/focus reporting subsystem. Per
    /// xterm.js softReset, autowrap returns to ON (the xterm default), not off.
    fn soft_reset(&mut self) {
        self.cursor.visible = true;
        self.cursor.pen = Pen::default();
        self.scroll_top = 0;
        self.scroll_bottom = self.grid.rows() - 1;
        self.origin_mode = false;
        self.app_cursor_keys = false;
        self.bracketed_paste = false;
        self.grapheme_clustering = false; // ?2027 back to the wcwidth-compat default (#295)
        self.autowrap = true; // xterm default is ON (not the VT100 "off")
        self.insert_mode = false;
        self.charsets = [Charset::Ascii; 4];
        self.gl = 0;
        self.decsc = SavedCursor::default();
    }

    fn carriage_return(&mut self) {
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
    }

    /// DECSCUSR (CSI Ps SP q): set the caret shape + blink (#89). 0/2 = steady
    /// block, 1 = blinking block; 3/4 = blinking/steady underline; 5/6 =
    /// blinking/steady bar (odd = blink). 0 resets to the default (steady block).
    /// An unknown param leaves the style unchanged. Mirrors xterm.js.
    fn set_cursor_style(&mut self, param: u16) {
        let (shape, blink) = match param {
            0 | 2 => (CursorShape::Block, false),
            1 => (CursorShape::Block, true),
            3 => (CursorShape::Underline, true),
            4 => (CursorShape::Underline, false),
            5 => (CursorShape::Bar, true),
            6 => (CursorShape::Bar, false),
            _ => return,
        };
        self.cursor.shape = shape;
        self.cursor.blink = blink;
    }

    /// Backspace (BS, 0x08): move the cursor one column left. With reverse
    /// wraparound (?45) a backspace at column 0 of a *soft-wrapped* row moves
    /// back to the last column of the previous row — undoing one autowrap. Only
    /// soft wraps reverse (the previous row carries `WRAPLINE`); a hard CR/LF
    /// line does not. BS only (not cursor-left), matching xterm.js (#80).
    fn backspace(&mut self) {
        self.cursor.pending_wrap = false;
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
            return;
        }
        if self.reverse_wraparound
            && self.cursor.row > self.scroll_top
            && self.cursor.row <= self.scroll_bottom
        {
            let prev = self.cursor.row - 1;
            let last = self.grid.cols() - 1;
            if self.grid.row_ref(prev).is_wrapped() {
                self.grid.row_mut(prev).set_wrapped(false);
                self.cursor.row = prev;
                self.cursor.col = last;
            }
        }
    }

    /// Auto-wrap at end of line: line-feed then return to column 0.
    fn wrapline(&mut self) {
        self.linefeed_inner(true);
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
    }

    // ---- tab stops (HT / HTS / TBC) ------------------------------------------

    /// HT: advance to the next set tab stop, or the last column if none remain
    /// (no wrap).
    fn put_tab(&mut self) {
        let cols = self.grid.cols();
        let mut col = self.cursor.col;
        while col + 1 < cols {
            col += 1;
            if self.tabs[col] {
                break;
            }
        }
        self.cursor.col = col;
        self.cursor.pending_wrap = false;
    }

    /// HTS (ESC H): set a tab stop at the cursor column.
    fn set_tab_stop(&mut self) {
        let col = self.cursor.col;
        self.tabs[col] = true;
    }

    /// TBC (CSI g): clear the tab stop at the cursor (mode 0) or all stops
    /// (mode 3).
    fn clear_tab_stop(&mut self, mode: u16) {
        match mode {
            0 => {
                let col = self.cursor.col;
                self.tabs[col] = false;
            }
            3 => self.tabs.iter_mut().for_each(|t| *t = false),
            _ => {}
        }
    }

    // ---- printing ------------------------------------------------------------

    /// The extended attributes the pen currently stamps onto a cell it writes: the open OSC 8
    /// hyperlink (#26/#46) and a non-default underline colour (SGR 58, #520).
    ///
    /// The colour is gated on the UNDERLINE attribute — an underline colour is meaningless on a
    /// cell that draws no underline, and xterm likewise does not persist it there
    /// (`AttributeData isEmpty()` ignores the colour; `InputHandler.test.ts:2084`). That keeps
    /// it off the wire for cells that never draw it (ADR-0020: no inert per-cell payload). SGR 58
    /// is the *underline* colour, so STRIKETHROUGH alone does not arm it.
    ///
    /// One place, because there are three sites that write a pen-built cell (the glyph, its wide
    /// spacer, and the vacated wrap column) — mirroring the pen half of `Row::ext_attrs_at`, so a
    /// later rider is added here rather than at each of them (#521/#528).
    fn pen_ext_attrs(&self) -> ExtAttrs {
        let ucolor = self.cursor.pen.underline_color;
        let armed =
            ucolor != Color::Default && self.cursor.pen.flags.contains(CellFlags::UNDERLINE);
        ExtAttrs::from_pen(self.current_link, armed.then_some(ucolor))
    }

    /// Free a cell that has stopped being part of a glyph — the *structural repair* every
    /// overwrite, erase and row-shift owes the no-orphan invariant when it destroys one half of
    /// a width-2 glyph, plus the spacer a mode-2027 demotion no longer needs.
    ///
    /// This is **not** an erase. The app asked for something at a *different* column; freeing
    /// this one is the engine keeping its own invariant. But it is still a mutation, so it
    /// **damages** — and that is the half every site used to forget, because each function
    /// damaged its own range and the repaired cell lies outside it by construction (that is what
    /// makes it a repair). A frame-mode consumer therefore kept painting the destroyed glyph.
    /// Bundling the reset with its damage is the point of this helper: a repair site added later
    /// cannot forget the half that has no compiler behind it (#530).
    ///
    /// The cell it leaves is a **blank carrying the current background** — the same rule
    /// `clear_cells` already applies to a BCE erase, extended to the repair, so one sentence
    /// covers both: *a blank cell carries the current background.* A bare `Cell::default()`
    /// would punch an uncoloured notch into a coloured run, which no reference implementation
    /// does.
    ///
    /// Deliberately the pen's **background only** — not its full attributes, and this is not a
    /// compromise between references: it is byte-for-byte xterm.js's `_eraseAttrData()`
    /// (`DEFAULT_ATTR_DATA` + `curAttr.bg & ~0xFC000000`, i.e. default everything plus the pen's
    /// background colour), which is what its `replaceCells` / `insertCells` / `deleteCells`
    /// repairs are handed — eight of the twelve sites here. Only xterm's *print* path uses the
    /// whole pen. Taking the whole pen
    /// (xterm.js `setCellFromCodepoint(x, 0, 1, curAttr)`) would plant the pen's hyperlink and,
    /// worse, its DECSCA protection onto a cell the app never wrote — a cell no later erase could
    /// clear. Taking the *cell's own* attributes (alacritty `clear_wide`, which keeps `extra`)
    /// would leave the destroyed glyph's hyperlink alive and clickable, the defect #529 is filed
    /// against. Both were considered and rejected; the maintainer chose this on 2026-07-24 and it
    /// is theirs to reverse (see #530 for what they were shown).
    ///
    /// What used to be recorded here as a known limitation is **resolved** (#538, ADR-0025 D1):
    /// `reset()` still clears the whole content word, but the soft-wrap link is no longer part of
    /// it. The live flag is on the `Row`, so freeing the last column — here or on the erase path —
    /// cannot break a wrap; `CellFlags::WRAPLINE` is wire-only, derived onto the last cell at
    /// encode time and never read back (`cell.rs`). Ending a wrap is now an explicit per-verb call
    /// (`end_wrap`), which is the shape both references already had and the reason the move was
    /// made: ghostty and xterm.js hold the flag on the row/line, and xterm.js takes `clearWrap` as
    /// an explicit argument on its erase helper (`_eraseInBufferLine`, `InputHandler.ts:1175`)
    /// rather than letting a cell clear decide it.
    ///
    /// Known cost, accepted rather than overlooked: with DECSCA the freed cell loses its
    /// protection. ghostty has the same hole and flags it in its own source; justerm does not
    /// implement DECSCA today, so revisit if it lands.
    fn free_cell(&mut self, row: usize, col: usize) {
        let bg = self.cursor.pen.bg;
        let cell = self.grid.cell_mut(row, col);
        cell.reset();
        cell.set_bg(bg);
        self.damage_span(row, col, col);
    }

    /// Will the next `wrapline()` actually reach another row?
    ///
    /// `wrapline` → `linefeed` advances in exactly two cases: the cursor sits at the scroll
    /// region's bottom (so the region scrolls under it), or it has a row below it on screen.
    /// Parked *below* a DECSTBM region on the last row it does neither — it silently stays put.
    ///
    /// Both wide-at-boundary paths must ask before they commit anything, because both destroy
    /// content on the assumption that the row is about to change: `write_glyph` blanks the column
    /// it is leaving, and `relocate_cluster_wide` writes its cluster to `(cursor.row, 0..=1)`
    /// *after* the wrap — which is the same row when nothing advanced, so it lands on live cells.
    /// Reasoning only about the vacated source column misses that second case entirely.
    ///
    /// This mirrors `linefeed`'s own condition; the two must be read together.
    fn wrapline_advances(&self) -> bool {
        self.cursor.row == self.scroll_bottom || self.cursor.row + 1 < self.grid.rows()
    }

    /// Blank the last column as the soft-wrap artefact it is, when a width-2 glyph could not fit
    /// there (#528). Shared by the two paths that reach this state: `write_glyph`'s wide-at-boundary
    /// wrap and `relocate_cluster_wide`'s promoted cluster.
    ///
    /// The column is **written**, not merely flagged: a blank built from the current pen, exactly
    /// as every reference does it — xterm.js `setCellFromCodepoint(col, 0, 1, curAttr)`
    /// (`InputHandler.ts:609-611`; `BufferLine.ts:244-251` takes the pen's fg/bg *and* its
    /// `extended` link/colour), ghostty `printCell(0, .spacer_head)` (`Terminal.zig:1410-1412`,
    /// whose `printCell` stamps the cursor's hyperlink), alacritty `write_at_cursor(' ')` under a
    /// `LEADING_WIDE_CHAR_SPACER` template (`mod.rs:1108-1113`, assigning `extra` from it).
    ///
    /// Flagging in place instead left the previous occupant's glyph, hyperlink and underline colour
    /// alive in a cell every text reader skips — so a renderer drew a character that could not be
    /// copied, searched or announced (#528). Building from `Pen::cell` also clears the presence
    /// bits, so no stale side-map entry can be read back through the new cell.
    ///
    /// WRAPLINE marks the row a continuation rather than a hard line-end (search, logical lines
    /// #113 and reflow #7 all read it); the leading-spacer marker makes the text extractors skip
    /// the blank instead of joining `"ab한"` → `"ab 한"`.
    ///
    /// The marker is **alacritty's** `LEADING_WIDE_CHAR_SPACER` (`term/cell.rs`) — ghostty calls
    /// the same thing `.spacer_head`. It is *not* xterm's: xterm.js has no marker at all, writing
    /// a bare null cell and re-inferring the artefact at reflow time from "ends in null and the
    /// following line starts with a wide char". That difference has a consequence here — in
    /// xterm.js a lost marker degrades to an empty cell that trimming drops anyway, whereas
    /// justerm writes `' '`, so the marker is the *only* thing keeping this column out of the
    /// extracted text.
    fn vacate_for_wrap(&mut self, row: usize, col: usize) {
        // Writing this column makes the vacate an overwrite like any other, so it inherits the
        // no-orphan obligation every other overwrite site carries (`write_glyph`,
        // `promote_cluster_to_wide`, `insert_chars`, `delete_chars`, the erase path): if the
        // column was the *spacer* of a wide glyph, blanking it destroys the spacer marker and
        // strands the lead. That is unrecoverable rather than merely untidy — every repair path
        // keys off `is_wide_spacer()`, so once the marker is gone no later write, ECH, EL, ICH
        // or DCH can ever clear the orphan.
        if col > 0 && self.grid.cell(row, col).is_wide_spacer() {
            self.free_cell(row, col - 1);
        }
        let mut vacated = self.cursor.pen.cell(' ');
        vacated.set_leading_spacer();
        *self.grid.cell_mut(row, col) = vacated;
        self.begin_wrap(row);
        let ext = self.pen_ext_attrs();
        self.grid.row_mut(row).set_ext_attrs(col, ext);
        // The cell's contents changed, so a frame-mode consumer must be told or it keeps painting
        // the old glyph (ADR-0003: every mutation site records damage). The *repaired* lead above
        // is damaged by `free_cell`, which owns that pairing for all twelve repair sites — this
        // site used to hand-roll it, and keeping both left neither able to discriminate.
        self.damage_span(row, col, col);
    }

    /// Write one glyph at the cursor, handling deferred wrap and the wide-char
    /// spacer, then advance the cursor (deferring the wrap if it hits the edge).
    fn write_glyph(&mut self, c: char, width: usize) {
        let cols = self.grid.cols();

        // Resolve a deferred last-column wrap before placing the next glyph.
        // The row being left soft-wrapped: mark its last cell so reflow (#7) can
        // tell it from a hard CR/LF line-end.
        if self.cursor.pending_wrap {
            let row = self.cursor.row;
            // Claim the wrap only if there will *be* a next row to continue into. Parked below a
            // DECSTBM region on the last row, `wrapline` → `linefeed` advances nothing and the
            // glyph overwrites this same row from column 0 — so the wrap never happened, and a
            // flag set here is permanently false: the cursor never leaves, nothing clears it, and
            // it survives into `backspace`'s reverse-wraparound, reflow, and every text reader.
            //
            // The predicate is not new and neither is its rationale: `wrapline_advances` was
            // written for exactly this state and is already asked by both wide-at-boundary paths.
            // This narrow path was the one caller that committed without asking. (Surfaced by the
            // #540 completeness pass, which found a row-shift verb inheriting the bogus flag and
            // merging two unrelated logical lines.)
            if self.wrapline_advances() {
                self.begin_wrap(row);
            }
            self.wrapline();
        }

        // A width-2 glyph that cannot fit in the last column wraps first — unless
        // autowrap is off, in which case it is dropped (xterm.js `continue`), not
        // squeezed or wrapped.
        if width == 2 && self.cursor.col + 1 >= cols {
            if !self.autowrap {
                return;
            }
            // …but only if the wrap actually happens: vacating for a wrap that never occurs
            // blanks a column holding a live glyph.
            if self.wrapline_advances() {
                self.vacate_for_wrap(self.cursor.row, cols - 1);
            }
            self.wrapline();
        }

        // Insert mode (IRM): open a `width`-wide gap at the cursor first, shifting
        // the row's tail right (off-edge cells discarded, wide halves repaired),
        // then write into the gap — mirrors xterm.js's insertCells (#64).
        if self.insert_mode {
            self.insert_chars(width);
        }

        let (row, col) = (self.cursor.row, self.cursor.col);

        // Overwriting either half of a pair that wrapped from the row above ends that pair, so the
        // row above's artefact record is void (#534). The one exception is the in-place same-width
        // overwrite — a wide lead replaced by another wide lead at the same column — which is
        // ghostty's `if (cell.wide != wide)` escape and the reason this is asked *before* the
        // write rather than after it. Note IRM has already run its own check inside `insert_chars`
        // by the time this would fire, on the pre-shift state, which is the correct one.
        if col <= 1 && self.wrapped_pair_at_row_start(row) && !(col == 0 && width == 2) {
            self.void_wrap_artefact_above(row);
        }

        // Overwriting one half of an existing wide glyph orphans the other —
        // clear it so no stray lead/spacer is left behind.
        let last = col + width - 1;
        if col > 0 && self.grid.cell(row, col).is_wide_spacer() {
            self.free_cell(row, col - 1);
        }
        if last + 1 < cols && self.grid.cell(row, last).is_wide() {
            self.free_cell(row, last + 1);
        }

        let mut cell = self.cursor.pen.cell(c);
        if width == 2 {
            cell.insert_flags(CellFlags::WIDE_CHAR);
        }
        *self.grid.cell_mut(row, col) = cell;
        // Stamp the pen's extended attrs — the open hyperlink (#26/#46) and a non-default
        // underline colour (#520) — into the row's side maps.
        let ext = self.pen_ext_attrs();
        self.grid.row_mut(row).set_ext_attrs(col, ext);

        // The trailing column of a wide glyph carries a distinct spacer marker —
        // and the same link + underline colour, so a hover/selection/underline over
        // either half agrees.
        if width == 2 && col + 1 < cols {
            let mut spacer = self.cursor.pen.cell(' ');
            spacer.insert_flags(CellFlags::WIDE_CHAR_SPACER);
            *self.grid.cell_mut(row, col + 1) = spacer;
            self.grid.row_mut(row).set_ext_attrs(col + 1, ext);
        }

        // Record damage for the cell(s) just written.
        self.damage_span(row, col, col + width - 1);

        // Advance. Reaching/passing the last column sets pending-wrap instead of
        // wrapping eagerly — the cursor parks on the last column.
        let new_col = col + width;
        if new_col >= cols {
            self.cursor.col = cols - 1;
            // With autowrap off (DECAWM ?7l) the cursor pins to the last column
            // and the next glyph overwrites in place — no deferred wrap (#63).
            self.cursor.pending_wrap = self.autowrap;
        } else {
            self.cursor.col = new_col;
        }
    }

    /// Attach a combining mark (width-0 code point) to the grapheme it modifies —
    /// the cell the cursor just left. With pending-wrap the cursor still sits on
    /// the just-written last-column glyph, so attach in place (no back-up, no
    /// deferred wrap); otherwise step back one column, and once more over a
    /// wide-char spacer to reach its lead. Stored in the grapheme side-table.
    fn push_combining(&mut self, c: char) {
        let row = self.cursor.row;
        let mut col = if self.cursor.pending_wrap {
            self.cursor.col
        } else {
            self.cursor.col.saturating_sub(1)
        };
        if self.grid.cell(row, col).is_wide_spacer() {
            col = col.saturating_sub(1);
        }
        // Append the mark to the row's combining map at this column (setting the
        // cell's combining bit). No global pool — the cluster rides the row.
        self.grid.row_mut(row).push_combining(col, c);
        self.damage_span(row, col, col);
    }

    /// Mode 2027 (#295): if `c` **extends** the previous cell's grapheme cluster (UAX #29), append
    /// it to that cell's side-table — no new cell, no cursor advance — and return `true`. Otherwise
    /// return `false` so `print` takes the normal per-scalar path (a break starts a new cell).
    ///
    /// The break state is reconstructed fresh from the previous cell's stored cluster (base scalar +
    /// side-table marks) rather than persisted across calls, so cursor moves / CR-LF can't corrupt
    /// it (mirrors ghostty). Width promotion for a narrow base (a flag's second RI, a text-base +
    /// VS16) is handled by the caller in a later step; here the base's existing width holds.
    fn try_grapheme_join(&mut self, c: char) -> bool {
        let row = self.cursor.row;
        // Locate the previous cluster's base cell, exactly as `push_combining`: with pending-wrap
        // the cursor still sits on the last glyph; else step back one, and over a wide spacer.
        let col = if self.cursor.pending_wrap {
            self.cursor.col
        } else if self.cursor.col == 0 {
            return false; // nothing precedes on this row
        } else {
            self.cursor.col - 1
        };
        let col = if self.grid.cell(row, col).is_wide_spacer() {
            col.saturating_sub(1)
        } else {
            col
        };
        // Reconstruct the previous cluster's text: base scalar + any already-joined scalars.
        let mut prev = String::new();
        prev.push(self.grid.cell(row, col).c());
        if let Some(marks) = self.grid.row_ref(row).combining_at(col) {
            prev.extend(marks.iter().copied());
        }
        if !crate::grapheme::grapheme_extends(&prev, c) {
            return false;
        }
        // Join: ride the side-table (no new cell).
        self.grid.row_mut(row).push_combining(col, c);
        // Width promotion: a flag's second regional indicator, or a text-base + VS16, grows the
        // cluster to width 2. `UnicodeWidthStr` gives the cluster width (RI-pair → 2, VS16 → 2). If
        // the base cell is still narrow, widen it in place.
        let cluster_w = {
            prev.push(c);
            UnicodeWidthStr::width(prev.as_str())
        };
        if cluster_w == 2 && !self.grid.cell(row, col).is_wide() {
            self.promote_cluster_to_wide(row, col);
        } else if cluster_w == 1 && self.grid.cell(row, col).is_wide() {
            // The mirror case: a default-wide emoji + VS15 (text selector) shrinks to width 1.
            self.demote_cluster_to_narrow(row, col);
        }
        self.damage_span(row, col, col);
        true
    }

    /// Shrink a wide cluster cell back to a single-width cell (#295): a default-wide emoji joined by
    /// VS15 (U+FE0E, the text selector) requests text presentation → width 1. Remove `WIDE_CHAR`,
    /// free the spacer, and back the cursor up over it (the inverse of `promote_cluster_to_wide`).
    fn demote_cluster_to_narrow(&mut self, row: usize, col: usize) {
        let cols = self.grid.cols();
        self.grid
            .cell_mut(row, col)
            .remove_flags(CellFlags::WIDE_CHAR);
        if col + 1 < cols {
            self.free_cell(row, col + 1); // free the now-unused spacer
        }
        // The cluster shrank 2→1: the cursor sat just past the wide cell (col+2, or pending-wrap on
        // the last column); it now sits just past the single-width cell at col+1.
        self.cursor.pending_wrap = false;
        self.cursor.col = (col + 1).min(cols - 1);
        self.damage_span(row, col, (col + 1).min(cols - 1));
    }

    /// Widen a narrow base cell to a double-width cluster in place (#295): set `WIDE_CHAR`, write
    /// its spacer, and step the cursor over it. Only reached when a joining scalar (flag's 2nd RI,
    /// VS16) promotes the cluster to width 2. A base pinned at the last column has no room for a
    /// spacer — relocation is a later step; until then it stays narrow (rare, renders single-width).
    fn promote_cluster_to_wide(&mut self, row: usize, col: usize) {
        let cols = self.grid.cols();
        if col + 1 >= cols {
            // No spacer room at the last column: relocate the whole cluster to the next line as a
            // wide cell (the row soft-wraps), mirroring write_glyph's wide-at-boundary wrap (#303).
            self.relocate_cluster_wide(row, col);
            return;
        }
        // Overwriting col+1 with the spacer can orphan the far half of a WIDE glyph standing there
        // (the cursor may have been repositioned before the joining scalar arrived). Reset that
        // orphan, exactly as write_glyph does (2462-2470), so no dangling spacer survives.
        if self.grid.cell(row, col + 1).is_wide() && col + 2 < cols {
            self.free_cell(row, col + 2);
        }
        self.grid
            .cell_mut(row, col)
            .insert_flags(CellFlags::WIDE_CHAR);
        // The spacer is the lead's second half, so it takes the LEAD's extended attrs — the
        // hyperlink and underline colour riding the row's side maps — exactly as write_glyph
        // stamps both halves of a wide write. `pen.cell(' ')` carries neither (and the pen may
        // have moved on since the base was printed), so they are re-attached here; a base with
        // none clears whatever the overwritten column held (#521).
        let ext = self.grid.row_ref(row).ext_attrs_at(col);
        let mut spacer = self.cursor.pen.cell(' ');
        spacer.insert_flags(CellFlags::WIDE_CHAR_SPACER);
        *self.grid.cell_mut(row, col + 1) = spacer;
        self.grid.row_mut(row).set_ext_attrs(col + 1, ext);
        // The cursor sat at col+1 (just past the narrow base); move it over the new spacer, applying
        // the same last-column pending-wrap rule as a wide write.
        let new_col = col + 2;
        if new_col >= cols {
            self.cursor.col = cols - 1;
            self.cursor.pending_wrap = self.autowrap;
        } else {
            self.cursor.col = new_col;
        }
        self.damage_span(row, col, col + 1);
    }

    /// Relocate a last-column narrow cluster to the next line as a wide cell (#303): its base +
    /// side-table marks move to `(next_row, 0..=1)` and the vacated last column becomes a soft-wrap
    /// (WRAPLINE + leading spacer), exactly as `write_glyph` wraps a wide glyph that can't fit. With
    /// autowrap off it stays narrow.
    ///
    /// The destination is an **overwrite**, so it owes the no-orphan repair every other overwrite
    /// site owes (#529, ADR-0025 D4) — see the comment at that site for why justerm restates it
    /// once per wide-writing path where the references get it structurally.
    ///
    /// The `cols < 2` arm is **unreachable since #547** —
    /// `MIN_COLUMNS = 2` is the floor on every path that sets a width — and is kept only as a
    /// bounds guard for the `col + 1` writes below, not as a described behaviour.
    fn relocate_cluster_wide(&mut self, row: usize, col: usize) {
        let cols = self.grid.cols();
        if cols < 2 || !self.autowrap || !self.wrapline_advances() {
            // Nowhere to place a wide cell — leave it narrow. `!wrapline_advances()` joins the
            // other two for the same reason: with no next row, the relocation would write the
            // cluster over columns 0-1 of the *current* row and destroy whatever is there.
            return;
        }
        // Capture the base cell (glyph + attrs), its marks, and its extended attrs before
        // vacating. The extended attrs (hyperlink, underline colour) must be read HERE and not
        // after the move: they live in the *source row's* side maps, and `wrapline()` below may
        // scroll — after which that row is a different (or recycled) `Row` (#521).
        let base = *self.grid.cell(row, col);
        let marks: Vec<char> = self
            .combining_at(row, col)
            .map(<[char]>::to_vec)
            .unwrap_or_default();
        let ext = self.grid.row_ref(row).ext_attrs_at(col);
        // Vacate the last column as a soft-wrap artefact — the same step `write_glyph` takes for a
        // wide glyph that cannot fit, and now literally the same code, so the two cannot drift
        // apart again (#528; they held opposite behaviours until then).
        self.vacate_for_wrap(row, col);
        // Advance to the next line (scrolls if at the bottom); cursor lands at col 0.
        self.wrapline();
        let nr = self.cursor.row;
        // The destination is an overwrite like any other, so it owes the same no-orphan repair
        // `write_glyph` performs for its own trailing column (#529, D4): the spacer about to land
        // on `(nr, 1)` half-destroys a wide glyph standing there, stranding its far half at
        // `(nr, 2)` — a `WIDE_CHAR_SPACER` with no lead to its left, still carrying the destroyed
        // glyph's hyperlink and underline colour. Asked *before* the writes, on the pre-write
        // state, exactly as `write_glyph`'s `last + 1` check is.
        //
        // Two of the three references have this exact site, and both repair it without a rule of
        // their own, because they write a pair as two *separate* cell writes and the repair lives
        // in the write:
        //   - xterm.js names the case outright — *"Combining character widens 1 column to 2. Move
        //     old character to next line."* (`InputHandler.ts:583-611` @ 699f553,
        //     `copyCellsFrom(oldRow, oldCol, 0, oldWidth, false)` at `:605-607`). The relocation
        //     leaves `x == 2`, so its once-per-run right-edge repair (`:668-669`) lands on exactly
        //     the orphaned column.
        //   - ghostty relocates in `Terminal.zig:1188-1252` @ e6e26e1 and reaches the repair
        //     through `cursorRight(1); printCell(0, .spacer_tail)` (`:1251-1252`) — that second
        //     `printCell` runs the `cell.wide != wide` switch (`:1484`) whose `.wide` arm clears
        //     the neighbouring lead's tail (`:1489-1499`).
        //   - alacritty has **no** counterpart: a width-0 codepoint returns early through
        //     `push_zerowidth` (`term/mod.rs:1069-1085` @ 852e971), so a cluster never changes
        //     width and nothing is ever relocated. Its orphan repair (`:994-1008`) is still the
        //     mechanism reference, reached the same way — one repair per `write_at_cursor`.
        // justerm writes both halves in one step, so the repair is not structural here and each
        // wide-writing path restates it — this is the third (`write_glyph`,
        // `promote_cluster_to_wide`, and now the relocation).
        //
        // What justerm does **not** copy is ghostty's reach-back at this site: its `.wide` arm
        // also clears the previous row's `.spacer_head` (`:1504-1506`, gated `cursor.y > 0 and
        // cursor.x <= 1`) — the very marker this relocation set seven statements earlier
        // (`:1200`). Derived from source, not executed. Suppressing it here is #534's rule
        // verbatim: a repair keyed on a state predicate must not fire while that state is
        // mid-construction.
        //
        // The other two obligations `write_glyph` carries are N/A here, recorded because an
        // unexplained omission is what gets re-litigated:
        //   - the *left*-orphan repair asks `col > 0`, and the lead lands at column 0.
        //   - `void_wrap_artefact_above(nr)` would clear a record that `vacate_for_wrap` **just
        //     set**, in both the advance case (`nr == row + 1`, so its target `nr - 1` is `row`)
        //     and the scroll case (`nr == row`, the source rotated up to `row - 1`). Firing it
        //     would be self-clobbering, not merely redundant — the same shape as #534's
        //     mid-construction rule. Measured after a repairing relocation: `is_row_wrapped(0)`
        //     and `(0, cols-1).is_leading_spacer()` both hold.
        //
        // `2 < cols` is a live bound, not defence in depth. The print paths cannot leave a
        // `WIDE_CHAR` lead in the last column — `write_glyph` wraps rather than write one there
        // and `promote_cluster_to_wide` relocates rather than promote in place — but `Row::resize`
        // can: the alt screen resizes without reflowing (#567), so truncating a row through a pair
        // strands its lead in the final column. The relocation then meets `is_wide() == true` at
        // `cols == 2`, and without the bound reads `(nr, 2)` on a two-column grid — an
        // out-of-bounds panic in a library, inside a consumer's process, reachable by shrinking a
        // window over a CJK glyph. Pinned by `min_columns.rs::
        // a_relocation_beside_a_truncated_wide_lead_does_not_index_past_the_row`.
        if 2 < cols && self.grid.cell(nr, 1).is_wide() {
            self.free_cell(nr, 2);
        }
        // Re-place the base as a wide lead + spacer, re-attaching the marks fresh (drop the combining
        // bit so push_combining starts a clean cluster at the new column).
        let mut lead = base;
        lead.set_combined(false);
        lead.insert_flags(CellFlags::WIDE_CHAR);
        *self.grid.cell_mut(nr, 0) = lead;
        for m in marks {
            self.grid.row_mut(nr).push_combining(0, m);
        }
        // Re-attach the extended attrs to BOTH halves at the new row. `lead` copied the base's
        // presence bits but not its map entries, so without this the bit is set with nothing
        // behind it — the read is gated and silently returns the default, and the frame stops
        // round-tripping (the cell encodes as linked with no index).
        self.grid.row_mut(nr).set_ext_attrs(0, ext);
        let mut spacer = self.cursor.pen.cell(' ');
        spacer.insert_flags(CellFlags::WIDE_CHAR_SPACER);
        *self.grid.cell_mut(nr, 1) = spacer;
        self.grid.row_mut(nr).set_ext_attrs(1, ext);
        // Cursor just past the wide cell (pending-wrap if it fills a 2-column row).
        if cols <= 2 {
            self.cursor.col = cols - 1;
            self.cursor.pending_wrap = self.autowrap;
        } else {
            self.cursor.col = 2;
            self.cursor.pending_wrap = false;
        }
        self.damage_span(nr, 0, 1);
    }

    // ---- cursor movement (CSI A/B/C/D/G/d/H/f) -------------------------------

    fn move_up(&mut self, n: usize) {
        self.cursor.row = self.cursor.row.saturating_sub(n);
        self.cursor.pending_wrap = false;
    }

    fn move_down(&mut self, n: usize) {
        self.cursor.row = (self.cursor.row + n).min(self.grid.rows() - 1);
        self.cursor.pending_wrap = false;
    }

    fn move_forward(&mut self, n: usize) {
        self.cursor.col = (self.cursor.col + n).min(self.grid.cols() - 1);
        self.cursor.pending_wrap = false;
    }

    fn move_back(&mut self, n: usize) {
        self.cursor.col = self.cursor.col.saturating_sub(n);
        self.cursor.pending_wrap = false;
    }

    fn set_col(&mut self, col: usize) {
        self.cursor.col = col.min(self.grid.cols() - 1);
        self.cursor.pending_wrap = false;
    }

    fn set_row(&mut self, row: usize) {
        self.cursor.row = row.min(self.grid.rows() - 1);
        self.cursor.pending_wrap = false;
    }

    fn goto(&mut self, row: usize, col: usize) {
        // Origin mode addresses rows relative to the scroll region's top margin
        // and clamps to its bottom; otherwise rows are absolute to the screen.
        let (offset, max_row) = if self.origin_mode {
            (self.scroll_top, self.scroll_bottom)
        } else {
            (0, self.grid.rows() - 1)
        };
        self.cursor.row = (row + offset).min(max_row);
        self.cursor.col = col.min(self.grid.cols() - 1);
        self.cursor.pending_wrap = false;
    }

    // ---- erase (CSI J / K) ---------------------------------------------------

    /// Clear cells `from..to` on `row`.
    ///
    /// Background Color Erase (BCE): erased cells carry the current SGR
    /// background only — fg and text attributes reset to default (matches
    /// xterm/alacritty, where the fill is `cursor.template.bg.into()`).
    ///
    /// **Cleared concern, with its validity condition — an empty range would break the pair
    /// invariant.** With `from == to` the first guard below still frees the lead at `from - 1`
    /// while the second is skipped (`to > from` is false) and the fill loop does nothing, so the
    /// spacer at `from` would survive its lead — an ADR-0025 D4 break, and the exact lead-less
    /// orphan the word walk must then treat as opaque. This is unreachable **as long as every
    /// caller passes a non-empty range**, which holds today: `ECH` clamps to
    /// `(col + n).min(cols)` with `n >= 1` (both `CSI X` and `CSI 0 X` erase one cell), and every
    /// `EL`/`ED` site passes `0..cols` or `0..=cursor`. A future caller that can pass an empty
    /// range must guard here first.
    fn clear_cells(&mut self, row: usize, from: usize, to: usize) {
        let cols = self.grid.cols();
        // Erasing either half of a pair that wrapped from the row above ends it, so that row's
        // artefact record is void (#534). `from <= 1` rather than `from == 0` because erasing from
        // column 1 destroys the spacer and the no-orphan repair below then frees the lead. ghostty
        // reaches the same row from its erase path — `Screen.splitCellBoundary`'s `x == 0 or x ==
        // 1` branch (`Screen.zig:1873` @ `e6e26e1`), called from `eraseChars` (`Terminal.zig:3159`).
        if from <= 1 && to > from && self.wrapped_pair_at_row_start(row) {
            self.void_wrap_artefact_above(row);
        }
        // Don't orphan a wide char straddling the erase boundary.
        if from > 0 && self.grid.cell(row, from).is_wide_spacer() {
            self.free_cell(row, from - 1);
        }
        if to > from && to < cols && self.grid.cell(row, to - 1).is_wide() {
            self.free_cell(row, to);
        }

        let bg = self.cursor.pen.bg;
        for col in from..to {
            let cell = self.grid.cell_mut(row, col);
            cell.reset();
            cell.set_bg(bg);
        }
        if to > from {
            self.damage_span(row, from, to - 1);
        }
    }

    /// End `row`'s soft wrap, because something just destroyed the content that was continuing
    /// onto the next row.
    ///
    /// Which verbs owe this is **not** derivable from the erased range — it is a per-verb rule,
    /// and both references spell it out call site by call site rather than inferring it:
    ///
    /// | verb | ends the wrap? | xterm | ghostty |
    /// |---|---|---|---|
    /// | `EL 0` (erase right) | **yes**, at any column | `ClearRight` → `LineClrWrapped` unconditionally (`util.c:1871`) | `cursorResetWrap()` in `eraseLine(.right)` |
    /// | `ECH` | **yes**, at any column | same `ClearRight` (`util.c:1961`) | `cursorResetWrap()` in `eraseChars` |
    /// | `DCH` | **yes** | `screen.c` | `cursorResetWrap()` — *"Our row's soft-wrap is always reset"* |
    /// | `EL 1` (erase left) | no | `ClearLeft`, no clear | no |
    /// | `ICH` | no | no | no |
    ///
    /// The shape behind the three that do: each destroys content **from the cursor rightward**, so
    /// "this row continues past its last column" can no longer be asserted. Erasing leftward or
    /// inserting blanks leaves the tail — and whatever it flowed into — intact.
    ///
    /// **`EL 2` is a deliberate divergence.** justerm ends the wrap; xterm does not (`ClearLine`,
    /// `util.c:1905`, has no `LineClrWrapped`) and ghostty copies that with a comment naming it —
    /// *"it seems like complete should reset the soft-wrap state of the line but in xterm it does
    /// not."* justerm differs because it *joins* logical lines for `accessible_text` / `search` /
    /// selection text, so a blanked-but-still-wrapped row visibly merges two lines in copy — a
    /// consequence xterm does not carry. Recorded rather than silently matched or silently
    /// Mark `row` as soft-wrapping into the next one — and damage the cell the bit rides on.
    ///
    /// The exact mirror of [`Term::end_wrap`], and it exists for the mirror of that function's
    /// reason. The flag lives on the `Row` (#538) and reaches a consumer only as the last cell's
    /// `WRAPLINE`, derived at encode time. Every other cell-carried fact changes when that cell is
    /// written, so damage covers it for free; this one does not, and a `Partial` frame would never
    /// ship the bit — a frame-mode consumer rebuilding logical lines from cells then keeps the two
    /// rows *split* forever, the exact dual of the "joined forever" that `end_wrap` guards.
    ///
    /// `end_wrap` took that obligation in #540; the set side never did. It stayed invisible because
    /// a wrap normally moves the cursor to the next row, and `frame_damage` tops the frame up with
    /// the old cursor cell. When a **scroll serves the wrap** the cursor keeps its row index, so
    /// nothing tops it up — which is how #557 surfaced it.
    ///
    /// Damaging here rather than at each caller is what keeps this true for set sites added later,
    /// the same argument `end_wrap`'s comment makes.
    fn begin_wrap(&mut self, row: usize) {
        self.grid.row_mut(row).set_wrapped(true);
        let last = self.grid.cols() - 1;
        self.damage_span(row, last, last);
    }

    /// diverged; see #538.
    fn end_wrap(&mut self, row: usize) {
        self.grid.row_mut(row).set_wrapped(false);
        // The flag is stored on the `Row` but rides the wire on the row's **last cell**, derived
        // at encode time. Every other cell-carried fact changes only when that cell is written,
        // so damage covers it for free; this one does not, and a `Partial` frame would never
        // re-ship the bit — leaving a frame-mode consumer with two rows joined forever. Damaging
        // here rather than at each caller is what keeps that true for call sites added later.
        let last = self.grid.cols() - 1;
        self.damage_span(row, last, last);
        // The wrap artefact goes with the wrap. The marker's claim is "the last column is the
        // blank a width-2 glyph vacated **because this row continues onto the next**", so a row
        // that stops continuing cannot hold one (ADR-0025 D3 — position is part of the test, and
        // so is the wrap it is positioned in). Coupling the two here is what makes the row-shift
        // seams and every wrap-ending erase a single rule instead of a clear per verb: ghostty
        // couples them in one function the same way — `Screen.cursorResetWrap`
        // (`terminal/Screen.zig:1524` @ `e6e26e1`, spacer-head clear at `:1539-1545`), reached from
        // `deleteChars` / `eraseChars` / `eraseLine`. It early-returns on `if (!page_row.wrap)`;
        // this one clears unconditionally, which is strictly safer.
        //
        // Most callers erase through this column anyway, so the clear is redundant for them; the
        // ones it is *not* redundant for are the row-shift seams (#540's `shift_region`, which
        // ends a wrap without touching a cell) and `delete_chars`, whose marker rides the shift.
        // The leftward erases are the mirror case — they blank this column while the wrap
        // legitimately survives — and go through `drop_artefact_if_erased` instead.
        //
        // One wrap-ending path deliberately does *not* reach here: `shift_region`'s `top == 0`
        // seam, whose row is in scrollback rather than the grid. It couples the same two clears
        // inline; see the comment there.
        self.grid.cell_mut(row, last).clear_leading_spacer();
    }

    /// The pair that wrapped into `row` is about to be destroyed or moved, so the artefact record
    /// on the row **above** it is void — drop it. **Call before the mutation.**
    ///
    /// The marker makes a claim with two clauses: this row soft-wraps (owned by `end_wrap`), and
    /// its last column is the blank *that specific pair* vacated. This is the second clause, and
    /// the rule behind every call site is one sentence: **the record survives only an in-place
    /// same-width overwrite.** Anything else that reaches columns 0/1 of the continuation — a
    /// narrow write, an erase, a shift in either direction — ends the pair the record was about,
    /// and a wide lead that arrives afterwards by some other route did not *wrap* from anywhere.
    ///
    /// Both references gate on that, and both gate on the state **before** the write rather than
    /// after it:
    ///
    /// - ghostty `Terminal.zig:1484` @ `e6e26e1` — the whole wide-repair `switch` sits under
    ///   `if (cell.wide != wide)`, so a wide glyph overwritten by another wide glyph skips it; the
    ///   reach-back stanza then appears in the `.wide` (`:1501-1506`) and `.spacer_tail`
    ///   (`:1529-1532`) arms only.
    /// - alacritty `term/mod.rs:994` @ `852e971` — the reach-back at `:1004-1008` is inside
    ///   `if cursor_cell.flags.intersects(WIDE_CHAR | WIDE_CHAR_SPACER)`, but with no
    ///   width-unchanged escape, so it drops a record that is still true. Alacritty is the outlier
    ///   of the two and justerm follows ghostty.
    ///
    /// Asking *after* the mutation instead looks equivalent and is not: it answers "is some wide
    /// lead standing at column 0", which a `DCH` that pulls the *next* wide glyph left also
    /// satisfies, and which a two-step placement (a narrow base promoted to wide by VS16 under
    /// mode 2027, or IRM's insert-then-write) satisfies only at the end. Both were measured
    /// disagreeing with the rule above before this took its current form.
    ///
    /// The erase and intra-row-shift call sites are **ported, not derived**: ghostty's
    /// `Screen.splitCellBoundary` (`Screen.zig:1831`, the `x == 0 or x == 1` branch at `:1873`)
    /// reaches up one row and clears the previous row's spacer head, and it is called from
    /// `deleteChars` (`Terminal.zig:3107-3109`) and `eraseChars` (`:3159-3160`). Only justerm's
    /// `ICH` site has no counterpart — ghostty's `insertBlanks` (`:2988`) calls it nowhere.
    ///
    /// `row == 0` does not mean "no row above": on the primary screen the text readers walk
    /// `[scrollback ++ grid]` as one buffer (`abs_floor() == 0`), so the row above grid row 0 is
    /// the last **scrollback** row and it can carry the marker. Alacritty reaches the same row for
    /// the same reason — its `topmost_line()` is `Line(-history_size)` (`grid/mod.rs:504`), so
    /// `point.line - 1` indexes into history; ghostty is the one that stops at the viewport
    /// (`cursor.y > 0`). On the alt screen `abs_floor()` is the screen top, so no join crosses the
    /// boundary and there is nothing to repair.
    ///
    /// No damage is owed by either branch, and for a stronger reason than #540's: the marker is a
    /// `content` bit outside `CONTENT_MARKER_MASK`, so `Cell::flags()` never sees it and it does
    /// not cross the wire at all. The `damage_span` below is defensive, not load-bearing.
    fn void_wrap_artefact_above(&mut self, row: usize) {
        if row > 0 {
            let last = self.grid.cols() - 1;
            if self.grid.cell(row - 1, last).is_leading_spacer() {
                self.grid.cell_mut(row - 1, last).clear_leading_spacer();
                self.damage_span(row - 1, last, last);
            }
        } else if !self.on_alt
            && let Some(cell) = self.scrollback.back_mut().and_then(|r| r.last_mut())
        {
            cell.clear_leading_spacer();
        }
    }

    /// Is a wide pair standing at columns 0..=1 of `row` — i.e. is there a record for
    /// `void_wrap_artefact_above` to void? A cheap pre-mutation test the four call sites share, so
    /// the rule lives in one place rather than being re-derived per verb (ADR-0025 D2).
    fn wrapped_pair_at_row_start(&self, row: usize) -> bool {
        self.grid.cell(row, 0).is_wide()
    }

    /// Drop a wide-wrap artefact marker that has outlived the wrap it belonged to, without
    /// touching the wrap itself.
    ///
    /// The mirror of the marker clean-up inside `end_wrap`, for the verbs that erase *leftward*:
    /// `EL 1` and `ED 1` correctly leave the wrap alone (the row's tail still flows onward), but
    /// they can still clear the last column, and then the artefact's blank turns into visible
    /// text that a reflow bakes in permanently. Only the marker goes; the wrap is the caller's
    /// business.
    fn drop_artefact_if_erased(&mut self, row: usize, from: usize, to: usize) {
        let last = self.grid.cols() - 1;
        if from <= last && to > last {
            self.grid.cell_mut(row, last).clear_leading_spacer();
        }
    }

    /// Shift `[top..=bottom]` by one line — up unless `down` — and end the wraps the shift
    /// falsified. Every row-shifting verb (IL/DL/SU/SD and the region paths in LF/RI) goes
    /// through here so the repair cannot be forgotten at a call site (ADR-0025 D2).
    ///
    /// The wrap flag claims "this row continues into the **next** row", so it is a statement about
    /// *adjacency*, and rotating whole `Row`s keeps it true for free: both halves of a pair inside
    /// the region move by the same line, so the claim still describes the same neighbour. Only the
    /// two seams falsify it, where a row's next neighbour changed underneath it:
    ///
    /// - **`top - 1`**, just outside the region. Its continuation rotated away (up-shift) or was
    ///   pushed down (down-shift), so whatever now sits at `top` is a stranger. This is the seam
    ///   that merges two unrelated logical lines in copy/search/accessible text (#540's repro).
    /// - **the row that lost its continuation to the blank** — `bottom - 1` after an up-shift (the
    ///   blank lands at `bottom`), `bottom` after a down-shift (its continuation rotated up to
    ///   `top` and was blanked there). The down-shift form is the one that reaches *outside* the
    ///   region: the stale claim points at `bottom + 1`, a row the verb never touched.
    ///
    /// Damaging matters as much as clearing, and `end_wrap` does both: `top - 1` is outside the
    /// region, so the scroll op the caller records does not cover it and a `Partial` frame would
    /// never re-ship the derived `WRAPLINE` bit.
    ///
    /// **Each seam has exactly one exemption, and both are facts about the caller that this
    /// function cannot see** — which is why they are parameters rather than tests:
    ///
    /// - `evicts_to_scrollback` exempts the **top** seam: a linefeed pushes row 0 into scrollback,
    ///   so the readers' `[scrollback ++ grid]` walk finds the continuation one row further back
    ///   and adjacency survives.
    /// - `serves_wrap` exempts the **bottom** seam: the shift was asked for by `wrapline`, so the
    ///   blank it exposes at `bottom` is not a stranger that displaced a continuation — it *is*
    ///   the continuation, about to be written into (#557).
    ///
    /// Both are one-sided on purpose. A wrap-serving scroll still falsifies the top seam, and a
    /// scrollback-evicting linefeed still falsifies the bottom one when no wrap asked for it.
    ///
    /// **No reference implements this rule**, so it is derived rather than ported — ADR-0004, the
    /// spec is the authority for VT semantics, above any implementation:
    ///
    /// - **ghostty** clears the wrap on *every* row a full-width IL/DL touches
    ///   (`terminal/Terminal.zig:2746-2752`, `:2906-2912` @ `e6e26e1`). The clear runs *before* the
    ///   row swap at `:2936-2939`, so both ends stay false: an interior pair is split, not
    ///   preserved. It still never reaches the row above the shifted range.
    /// - **alacritty** has no `WRAPLINE` clear on any scroll path (@ `852e971`).
    /// - **xterm.js** splices whole line objects and never touches `isWrapped`
    ///   (`common/InputHandler.ts:1345-1402` @ `699f553`). Its opposite polarity — "I continue the
    ///   *previous* row" (`common/buffer/Buffer.ts:566-570`) — moves the exposure to the mirrored
    ///   seam rather than removing it: a spliced-in line keeps a continuation claim about a
    ///   predecessor it never met.
    ///
    /// The seam row's wide-wrap *marker* is the same shift's other half, and it now rides along:
    /// `end_wrap` clears both (#534), and the `top == 0` branch below — the one seam whose row is
    /// not a grid row — couples them inline for the same reason.
    ///
    /// **Validity condition for clearing at the seams rather than everywhere.** ghostty clears the
    /// wrap and the spacer head on *every* row a full-width IL/DL touches, and its own comment
    /// gives two reasons: it splits interior pairs, **and** it supports left/right margins
    /// (DECSLRM), where a partial-row shift can break an interior pair without moving its
    /// neighbour. justerm rotates whole `Row`s and implements no DECSLRM, so an interior pair and
    /// its continuation always move together and seam-only is sound. If left/right margins ever
    /// land, this rule and #534's marker rule break at the same time — neither is safe under a
    /// shift that moves part of a row.
    fn shift_region(
        &mut self,
        top: usize,
        bottom: usize,
        down: bool,
        evicts_to_scrollback: bool,
        serves_wrap: bool,
    ) {
        if down {
            self.grid.scroll_down_region(top, bottom);
        } else {
            self.grid.scroll_up_region(top, bottom);
        }
        // Recording the scroll op is part of shifting, not a step a caller adds after: damage is
        // indexed by row position, so `record_scroll` rotates `line_damage` with the content. A
        // seam clear damaged *before* that rotation is carried to the wrong row — and on a
        // down-shift it lands on `top`, which `record_scroll` immediately overwrites with
        // `fully_damaged`. The clear then never reaches the wire at all: the model splits the
        // rows, a `Partial` frame does not say so, and the consumer keeps them joined forever.
        // Ordering it here is what makes that unrepeatable at a sixth call site.
        self.record_scroll(top, bottom, if down { -1 } else { 1 });
        if top > 0 {
            self.end_wrap(top - 1);
        } else if !evicts_to_scrollback && !self.on_alt {
            // `top == 0` does not mean "no row above": on the primary the text readers walk
            // `[scrollback ++ grid]` as one buffer (`abs_floor() == 0`), so the row above grid row
            // 0 is the last *scrollback* row and it can wrap into the screen. A full-screen SU /
            // DL / RI therefore leaves this issue's defect one row higher, outside the grid.
            //
            // `evicts_to_scrollback` is what keeps `linefeed` out: it pushes grid row 0 into
            // scrollback, so the continuation is re-attached one row further back and the claim
            // stays true — clearing there would split a line the scroll preserved. On the alt
            // screen `abs_floor()` is the screen top, so no join crosses the boundary at all.
            //
            // No damage is owed with the clear, unlike `end_wrap`'s grid form: a scrollback row
            // only reaches the wire while `display_offset > 0`, and there `damage()` returns an
            // empty `Partial` (`term.rs`, the frozen-viewport short-circuit) while any scroll that
            // *moves* the viewport marks full damage. Valid as long as that short-circuit holds.
            //
            // The artefact marker goes with the wrap here exactly as it does in `end_wrap`, and
            // this branch is the reason that coupling cannot simply live in `end_wrap`: it is the
            // one wrap-ending path whose row is not a grid row, so it does not call it. Leaving it
            // out left #534's defect alive one row above the grid — reachable from every
            // `scroll_region_lines` verb, since all of them pass `evicts_to_scrollback: false`,
            // and visible as a word selection one cell too wide plus a reflow that bakes the
            // stranded marker mid-row.
            if let Some(row) = self.scrollback.back_mut() {
                row.set_wrapped(false);
                if let Some(cell) = row.last_mut() {
                    cell.clear_leading_spacer();
                }
            }
        }
        // The blank lands at `bottom` going up and at `top` going down, so the row that lost its
        // continuation is the one just above it. Going down that is `top - 1`, already cleared
        // above; going up it is `bottom - 1`, which for a one-row region is that same row.
        //
        // The up-shift form needs the `bottom + 1` guard, and it is not defensive — without it the
        // clear destroys a **live** wrap. A row at the screen's bottom edge that wraps is the
        // ordinary soft-wrap-at-the-last-row state: `wrapline` sets the flag and the linefeed
        // scrolls precisely so the continuation has somewhere to land, which is the *next* row
        // after this shift. Its claim is about a row that does not exist yet, so the shift makes it
        // true rather than false.
        //
        // **The rest of that guard's original rationale was too narrow, and #557 is what it cost.**
        // It read: *"the link is only broken when there is a stationary row below the region
        // (`bottom + 1 < rows`): then the continuation stayed put while its lead moved up."* A
        // stationary row below is **necessary but not sufficient**. At a *region's* bottom the same
        // wrapline-asked-for scroll happens with `bottom + 1 < rows` perfectly true, and the clear
        // then split the logical line the scroll existed to continue. The geometry was never the
        // discriminator; **why the shift is happening** is — which is what `serves_wrap` carries.
        //
        // The guard stays anyway: it is the screen-bottom case of the same fact, and it also holds
        // for a *non*-wrap-serving linefeed at the screen edge.
        //
        // One invariant is still worth naming, because it was not true when this guard was first
        // written: **a row only claims a wrap if a next row will exist for it**. A row parked below
        // a DECSTBM region kept a permanent false claim, and this guard preserved it — the #540
        // completeness pass merged two unrelated logical lines through exactly that hole. The claim
        // is now gated at its set site (`write_glyph` asks `wrapline_advances`), so the guard's
        // premise holds. Valid as long as that gate stays.
        let orphaned = if serves_wrap {
            // The blank this shift just exposed is the continuation the wrap is waiting for, so
            // there is nothing to falsify — see the `serves_wrap` note on `linefeed_inner` (#557).
            None
        } else if down {
            Some(bottom)
        } else if bottom + 1 < self.grid.rows() {
            bottom.checked_sub(1)
        } else {
            None
        };
        if let Some(row) = orphaned {
            self.end_wrap(row);
        }
    }

    fn erase_display(&mut self, mode: u16) {
        let (cols, rows) = (self.grid.cols(), self.grid.rows());
        let (cr, cc) = (self.cursor.row, self.cursor.col);
        match mode {
            0 => {
                // Erases this row's tail and every row below, so nothing can continue from here
                // — and the rows below cannot continue either.
                self.clear_cells(cr, cc, cols);
                self.end_wrap(cr);
                for row in (cr + 1)..rows {
                    self.clear_cells(row, 0, cols);
                    self.end_wrap(row);
                }
            }
            1 => {
                // Leftward: this row's tail survives, so its own wrap does. The rows *above* are
                // gone entirely.
                for row in 0..cr {
                    self.clear_cells(row, 0, cols);
                    self.end_wrap(row);
                }
                self.clear_cells(cr, 0, cc + 1);
                self.drop_artefact_if_erased(cr, 0, cc + 1);
                // Covering the whole row means nothing continues from it. xterm.js has a
                // dedicated arm for exactly this case, in its own words: *"Deleted entire
                // previous line. This next line can no longer be wrapped."*
                // (`InputHandler.ts:1248-1252` — under its continuation polarity that assignment
                // is this engine's `end_wrap(cr)`.) `EL 1` has no such arm there, and none here.
                if cc + 1 == cols {
                    self.end_wrap(cr);
                }
            }
            2 => {
                for row in 0..rows {
                    self.clear_cells(row, 0, cols);
                    self.end_wrap(row);
                }
            }
            _ => {}
        }
    }

    /// Erase in line (EL): 0 = cursor→end, 1 = start→cursor, 2 = whole line.
    fn erase_line(&mut self, mode: u16) {
        let cols = self.grid.cols();
        let (cr, cc) = (self.cursor.row, self.cursor.col);
        match mode {
            // Erase right — ends the wrap at any column (xterm's `ClearRight`).
            0 => {
                self.clear_cells(cr, cc, cols);
                self.end_wrap(cr);
            }
            // Erase left — the tail survives, so the wrap does. The artefact marker does not:
            // if the erase reached the last column it just blanked the cell the marker described.
            1 => {
                self.clear_cells(cr, 0, cc + 1);
                self.drop_artefact_if_erased(cr, 0, cc + 1);
            }
            // Erase the whole line — see `end_wrap`: a deliberate divergence from xterm.
            2 => {
                self.clear_cells(cr, 0, cols);
                self.end_wrap(cr);
            }
            _ => {}
        }
    }

    // ---- intra-line editing (ICH / DCH / ECH) --------------------------------

    /// ECH (CSI Pn X): erase `n` cells in place from the cursor — no shift.
    /// BCE-filled (via `clear_cells`); pending-wrap is left untouched.
    fn erase_chars(&mut self, n: usize) {
        let cols = self.grid.cols();
        let (row, col) = (self.cursor.row, self.cursor.col);
        let to = (col + n).min(cols);
        self.clear_cells(row, col, to);
        // Destroys content from the cursor rightward, so the row can no longer be continuing —
        // unconditionally, at any column and for any `n`. Both references do exactly this (see
        // `end_wrap`): xterm routes ECH through the same `ClearRight` as `EL 0`, ghostty calls
        // `cursorResetWrap()` in `eraseChars`.
        self.end_wrap(row);
    }

    /// ICH (CSI Pn @): insert `n` blanks at the cursor, shifting the rest of the
    /// line right; cells pushed past the right edge are lost. The opened gap is
    /// BCE-filled; pending-wrap is left untouched.
    fn insert_chars(&mut self, n: usize) {
        let cols = self.grid.cols();
        let (r, col) = (self.cursor.row, self.cursor.col);
        let n = n.min(cols - col);
        if n == 0 {
            return;
        }
        // Shifting a wrapped pair out of columns 0/1 ends it, so the row above's artefact record
        // is void (#534). Asked **before** the shift, which is what keeps IRM correct: `write_glyph`
        // routes its wide-at-boundary insert through here *after* `vacate_for_wrap` has just set
        // the marker on the row above, and a post-shift test would see the freshly blanked gap and
        // clear the marker inside its own SET site's critical section. Pre-shift the question is
        // about the pair that was actually there, which is the one the record is about.
        if col <= 1 && self.wrapped_pair_at_row_start(r) {
            self.void_wrap_artefact_above(r);
        }
        let bg = self.cursor.pen.bg;
        let row = self.grid.row_mut(r);
        // Shift [col .. cols-n) right by n; the tail falls off the edge. The
        // combining map follows the moved cells (the bit travels with the raw
        // copy, the cluster data must too).
        row.copy_within(col..cols - n, col + n);
        row.move_maps(col..cols - n, col + n);
        for cell in &mut row[col..col + n] {
            cell.reset();
            cell.set_bg(bg);
        }
        // Repair wide-char halves split at the seams (no-orphan invariant):
        // a lead just before the gap lost its spacer; the first shifted cell may
        // be a spacer whose lead did not move.
        if col > 0 && self.grid.cell(r, col - 1).is_wide() {
            self.free_cell(r, col - 1);
        }
        if col + n < cols && self.grid.cell(r, col + n).is_wide_spacer() {
            self.free_cell(r, col + n);
        }
        // A lead shifted to the last column lost its spacer off the edge.
        if self.grid.cell(r, cols - 1).is_wide() {
            self.free_cell(r, cols - 1);
        }
        // Note ICH needs no repair to *this* row's marker: a right shift always pushes the last
        // column off the edge, so it discards a marker rather than carrying one inward —
        // measured, and pinned by `ich_discards_the_marker_off_the_edge`.
        self.damage_span(r, col, cols - 1);
    }

    /// DCH (CSI Pn P): delete `n` cells at the cursor, shifting the tail left; the
    /// vacated cells at the right are BCE-blanked. Pending-wrap is left untouched.
    fn delete_chars(&mut self, n: usize) {
        let cols = self.grid.cols();
        let (r, col) = (self.cursor.row, self.cursor.col);
        let n = n.min(cols - col);
        if n == 0 {
            return;
        }
        // The shift pulls the tail left and blanks the far end, so the row stops continuing —
        // ghostty says it outright (*"Our row's soft-wrap is always reset"* in `deleteChars`,
        // `Terminal.zig:3133` @ `e6e26e1`).
        //
        // **Before the shift, not after** (#534): `end_wrap` clears the artefact marker at the
        // *last* column, and the marker is a cell bit that the shift carries inward with every
        // other cell. Ending the wrap afterwards would clear a column the marker has already left,
        // stranding it mid-row where it describes nothing (ADR-0025 D3) and silently swallows the
        // blank between two runs in copy, search and accessible text. Same shape as #540's
        // `record_scroll` ordering: the clear has to happen where the state still is.
        self.end_wrap(r);
        // Deleting a wrapped pair out of columns 0/1 ends it, so the row above's artefact record
        // is void — and this is where the "ask before, not after" rule earns its keep twice over:
        // a `DCH` can pull the *next* wide glyph left into column 0, which a post-shift "is a wide
        // lead standing here?" test happily accepts even though the pair the record was about has
        // been deleted. ghostty asks the same question at the same point:
        // `Screen.splitCellBoundary(cursor.x)` from `deleteChars` (`Terminal.zig:3107` @ `e6e26e1`),
        // whose `x == 0 or x == 1` branch reaches up a row and clears the spacer head.
        if col <= 1 && self.wrapped_pair_at_row_start(r) {
            self.void_wrap_artefact_above(r);
        }
        let bg = self.cursor.pen.bg;
        let row = self.grid.row_mut(r);
        // Shift [col+n .. cols) left to [col ..); BCE-fill the vacated tail. The
        // combining map follows the moved cells.
        row.copy_within(col + n..cols, col);
        row.move_maps(col + n..cols, col);
        for cell in &mut row[cols - n..cols] {
            cell.reset();
            cell.set_bg(bg);
        }
        // Repair wide-char halves split by the deletion (no-orphan invariant):
        // a lead just before the cut lost its spacer; the cell now at the cursor
        // may be a spacer whose lead was deleted.
        if col > 0 && self.grid.cell(r, col - 1).is_wide() {
            self.free_cell(r, col - 1);
        }
        if self.grid.cell(r, col).is_wide_spacer() {
            self.free_cell(r, col);
        }
        self.damage_span(r, col, cols - 1);
    }

    // ---- line/region editing (IL / DL / SU / SD) -----------------------------

    /// Scroll rows `[top..=bottom]` by `n` lines, BCE-filling the exposed lines.
    /// `down` inserts blanks at the top (content moves down); otherwise content
    /// moves up and blanks appear at the bottom. Reuses the one-line region scroll
    /// primitives (so damage + scroll-op accumulation come for free), then fills
    /// the exposed lines with the current SGR background.
    fn scroll_region_lines(&mut self, top: usize, bottom: usize, n: usize, down: bool) {
        let height = bottom - top + 1;
        let n = n.min(height);
        if n == 0 {
            return;
        }
        // Anchors (selection #3, markers #118/#158) live at absolute buffer lines;
        // SU/SD/IL/DL don't accrue scrollback, so `base` is stable across the loop.
        let base = self.scrollback.len();
        for _ in 0..n {
            self.shift_region(top, bottom, down, false, false);
            // Rotate anchors with the content, like `linefeed`/`reverse_index`
            // (#162). `up` = content moved up = the non-`down` case. Markers rotate
            // with the active buffer (#187) — alt-scoped on the alt screen, so no
            // guard; the selection is cleared on alt enter.
            self.selection_rotate_region(base + top, base + bottom, !down);
            self.markers_rotate_region(base + top, base + bottom, !down);
        }
        self.invalidate_search_highlights();
        // BCE-fill the n exposed lines (the primitives blank to default).
        let bg = self.cursor.pen.bg;
        let (fill_top, fill_end) = if down {
            (top, top + n)
        } else {
            (bottom + 1 - n, bottom + 1)
        };
        let cols = self.grid.cols();
        for r in fill_top..fill_end {
            for c in 0..cols {
                let cell = self.grid.cell_mut(r, c);
                cell.reset();
                cell.set_bg(bg);
            }
        }
    }

    /// SU (CSI Pn S): scroll the scroll region up by `n`.
    fn scroll_up_lines(&mut self, n: usize) {
        self.scroll_region_lines(self.scroll_top, self.scroll_bottom, n, false);
    }

    /// SD (CSI Pn T): scroll the scroll region down by `n`.
    fn scroll_down_lines(&mut self, n: usize) {
        self.scroll_region_lines(self.scroll_top, self.scroll_bottom, n, true);
    }

    /// IL (CSI Pn L): insert `n` blank lines at the cursor, scrolling
    /// `[cursor..=scroll_bottom]` down. A no-op when the cursor is outside the
    /// scroll region.
    fn insert_lines(&mut self, n: usize) {
        let cur = self.cursor.row;
        if cur < self.scroll_top || cur > self.scroll_bottom {
            return;
        }
        self.scroll_region_lines(cur, self.scroll_bottom, n, true);
    }

    /// DL (CSI Pn M): delete `n` lines at the cursor, scrolling
    /// `[cursor..=scroll_bottom]` up. A no-op when the cursor is outside the
    /// scroll region.
    fn delete_lines(&mut self, n: usize) {
        let cur = self.cursor.row;
        if cur < self.scroll_top || cur > self.scroll_bottom {
            return;
        }
        self.scroll_region_lines(cur, self.scroll_bottom, n, false);
    }

    // ---- SGR (CSI m) ---------------------------------------------------------

    fn sgr(&mut self, params: &Params) {
        let pen = &mut self.cursor.pen;
        let mut iter = params.iter();
        while let Some(param) = iter.next() {
            let code = param.first().copied().unwrap_or(0);
            match code {
                0 => pen.reset(),
                1 => pen.flags.insert(CellFlags::BOLD),
                2 => pen.flags.insert(CellFlags::DIM),
                3 => pen.flags.insert(CellFlags::ITALIC),
                4 => pen.flags.insert(CellFlags::UNDERLINE),
                5 => pen.flags.insert(CellFlags::BLINK),
                7 => pen.flags.insert(CellFlags::INVERSE),
                8 => pen.flags.insert(CellFlags::HIDDEN),
                9 => pen.flags.insert(CellFlags::STRIKETHROUGH),
                22 => pen.flags.remove(CellFlags::BOLD | CellFlags::DIM),
                23 => pen.flags.remove(CellFlags::ITALIC),
                24 => pen.flags.remove(CellFlags::UNDERLINE),
                25 => pen.flags.remove(CellFlags::BLINK),
                27 => pen.flags.remove(CellFlags::INVERSE),
                28 => pen.flags.remove(CellFlags::HIDDEN),
                29 => pen.flags.remove(CellFlags::STRIKETHROUGH),
                30..=37 => pen.fg = Color::Indexed((code - 30) as u8),
                38 => {
                    if let Some(c) = parse_extended_color(param, &mut iter) {
                        pen.fg = c;
                    }
                }
                39 => pen.fg = Color::Default,
                40..=47 => pen.bg = Color::Indexed((code - 40) as u8),
                48 => {
                    if let Some(c) = parse_extended_color(param, &mut iter) {
                        pen.bg = c;
                    }
                }
                49 => pen.bg = Color::Default,
                // Underline colour (SGR 58 / 59, #520) — same extended-colour grammar
                // as 38/48 (colon `58:2:r:g:b` / `58:5:n`, or legacy semicolon), so it
                // reuses `parse_extended_color` verbatim. 59 returns to "follow the fg".
                58 => {
                    if let Some(c) = parse_extended_color(param, &mut iter) {
                        pen.underline_color = c;
                    }
                }
                59 => pen.underline_color = Color::Default,
                // bright foreground/background (aixterm) → palette 8..=15.
                90..=97 => pen.fg = Color::Indexed((code - 90 + 8) as u8),
                100..=107 => pen.bg = Color::Indexed((code - 100 + 8) as u8),
                _ => {}
            }
        }
    }
}

/// Parse `38`/`48`/`58` extended colour (foreground / background / underline colour, #520), in
/// either form:
/// - sub-parameter (colon) form inline in `param`: `38:5:n`, `38:2:r:g:b`
///   (optionally `38:2:cs:r:g:b` with a colorspace id), or
/// - legacy (semicolon) form: pull the following top-level params from `iter`.
///
/// The colon RGB form is **count-based** (`off = if param.len() >= 6 { 3 } else { 2 }`): a 5-param
/// `38:2:r:g:b` (no colorspace slot) reads RGB(r,g,b) directly, while a 6-param `38:2:cs:r:g:b` — or
/// `38:2::r:g:b` with an *empty* cs, the form kitty/nvim actually emit — skips the colorspace slot.
/// The short 5-param form is **non-conformant to T.416 / ISO-8613-6** (the de-jure standard always
/// carries a colorspace field), but tolerating it is the **ecosystem-dominant** behaviour, verified
/// against real source (2026-07, #520): VTE (`src/sgr.hh`, branches on `n > 4`), foot (`csi.c`,
/// `sub.idx >= 5`) and alacritty (`ansi.rs`, `params.len() > 4`) all count the sub-parameters and
/// decode the short form as RGB(r,g,b), exactly as here. VTE's own comment calls it a "common
/// misinterpretation of the standard" (foot: "bastard version") that it supports anyway; **only
/// xterm.js is strict** (always consumes a colorspace slot, so it misreads the short form). So a
/// difference from xterm here is deliberate leniency shared with the non-xterm ecosystem, not a
/// defect — the ADR-0004 spec-faithfulness is about not *omitting* behaviour, not about rejecting a
/// widely-emitted non-standard input.
fn parse_extended_color<'a, I>(param: &[u16], iter: &mut I) -> Option<Color>
where
    I: Iterator<Item = &'a [u16]>,
{
    if param.len() > 1 {
        // Colon sub-parameter form: kind is param[1].
        match param[1] {
            2 => {
                // 38:2:r:g:b (len 5) or 38:2:cs:r:g:b (len 6, colorspace skipped).
                let off = if param.len() >= 6 { 3 } else { 2 };
                let r = *param.get(off)? as u8;
                let g = *param.get(off + 1)? as u8;
                let b = *param.get(off + 2)? as u8;
                Some(Color::Rgb(r, g, b))
            }
            5 => Some(Color::Indexed(*param.get(2)? as u8)),
            _ => None,
        }
    } else {
        // Legacy semicolon form: kind, then its operands, are separate params.
        match iter.next()?.first().copied()? {
            2 => {
                let r = iter.next()?.first().copied()? as u8;
                let g = iter.next()?.first().copied()? as u8;
                let b = iter.next()?.first().copied()? as u8;
                Some(Color::Rgb(r, g, b))
            }
            5 => Some(Color::Indexed(iter.next()?.first().copied()? as u8)),
            _ => None,
        }
    }
}

/// Reflow one screen (joined with its `scrollback`) to `cols` x `rows`, tracking
/// `point` (a cursor in screen coordinates). Returns the new screen rows, the new
/// scrollback (capped to `limit`), and the new point. The alt screen passes an
/// empty scrollback and discards the returned one.
/// The fixed dimensions a resize reflows toward.
#[derive(Clone, Copy)]
struct ReflowDims {
    old_cols: usize,
    cols: usize,
    rows: usize,
    limit: usize,
    /// Whether a column change may **re-split** this pane's content, or only re-fit its rows.
    ///
    /// False for the alt screen (#567). Reflow re-splits a long line so history stays readable at
    /// the new width — it assumes the content is text that *flows*. The alt screen has no history,
    /// its content is a **layout** rather than a paragraph (re-wrapping htop's columns means
    /// nothing), and the application already knows the new size and repaints. All three references
    /// take the same position with the same shape — one flag on the same resize function:
    /// ghostty `alt.resize(.{ .reflow = false })`, alacritty `grid.resize(!is_alt, …)`, xterm.js
    /// gating on `_hasScrollback` with the alt buffer built as `new Buffer(false, …)`.
    ///
    /// It is not merely wasted work: measured on a real `htop` recording taken across a live
    /// `SIGWINCH`, re-splitting leaves debris in the cells htop does not overwrite, because htop
    /// repaints **without** clearing. `vim` hides it by erasing first.
    reflow: bool,
}

/// The result of reflowing one pane.
struct PaneReflow {
    screen: Vec<Row>,
    scrollback: VecDeque<Row>,
    /// The cursor's new screen-relative position.
    cursor: (usize, usize),
    /// Each tracked extra point's new position **in this pane's own `[history ++ screen]` frame**,
    /// index-aligned with the `extra_abs` argument — *before* any history the caller discards.
    ///
    /// Reported raw, with `evicted` beside it, because the two callers translate differently and
    /// doing it here silently picked the primary's answer for both: the primary keeps its history,
    /// so an extra's absolute line only moves by what the cap threw away, while the alt pane has no
    /// history at all and everything above the screen is *gone*. Adding the alt result to the
    /// primary's scrollback length then produced a line the buffer does not have — reachable
    /// without any reflow, on a rows-only resize.
    extras: Vec<(usize, usize)>,
    /// Rows that left the buffer entirely off the front of this pane's history. For the primary
    /// that is the scrollback cap's eviction; for the alt pane, whose limit is `0` because it has
    /// no history, it is every row the shrink pushed off the top. An extra whose raw line is below
    /// this **is not in the buffer any more** — the caller decides what that means for its kind.
    evicted: usize,
}

/// Reflow one pane (its `scrollback` joined with `screen`) to `dims`, tracking
/// the screen-relative cursor `point` plus any `extra_abs` points given in
/// **absolute** `[scrollback ++ screen]` coordinates (selection anchors).
fn reflow_pane(
    screen: Vec<Row>,
    scrollback: VecDeque<Row>,
    point: (usize, usize),
    extra_abs: &[(usize, usize)],
    dims: ReflowDims,
) -> PaneReflow {
    let scroll_len = scrollback.len();
    let mut all: Vec<Row> = scrollback.into();
    all.extend(screen);

    // The cursor is screen-relative; lift it to absolute, then track it together
    // with the already-absolute extras.
    let mut pts: Vec<(usize, usize)> = Vec::with_capacity(1 + extra_abs.len());
    pts.push((scroll_len + point.0, point.1));
    pts.extend_from_slice(extra_abs);

    let pts = if dims.reflow && dims.cols != dims.old_cols {
        let (reflowed, np) = crate::grid::reflow(all, dims.cols, &pts);
        all = reflowed;
        np
    } else {
        pts
    };

    // The cursor can land one row past everything the reflow emitted — "just after the content"
    // when the content ends on a full row (#562). That row is real, and while the pane is shorter
    // than the screen the caller's fit supplies it for free. When the content already fills the
    // pane it has to be bought, and the price is one row of history: the pane **scrolls**, which is
    // what a terminal does when content grows past the bottom. Without it the cursor was pulled
    // back onto the last glyph and the next byte destroyed a character — the ordinary shell shape,
    // a prompt at the bottom of a full screen.
    //
    // Five earlier designs made `reflow` itself materialise the row and were rejected on
    // measurements (a cursor at column 59 resized to width 4 emptied the buffer; a blank-line
    // exemption turned 22 alt lines into 21). `reflow` cannot see this pane's budget, so it spent
    // what it did not have. Here the budget is in scope, and it is the gate: a pane with no history
    // cannot pay — the displaced row would be destroyed rather than archived — so it keeps clamping.
    //
    // `limit > 0`, deliberately, and not "is this the alt screen": since #567 the alt panes pass
    // `limit: 0` because that is what an alt screen's history is, so they are excluded by the budget
    // rather than by a branch. That branch is what the design carrying this rule was rejected for
    // needing.
    //
    // This **amends** ADR-0025 rather than reading it narrowly: `reflow` does not create rows; the
    // seam may, when the pane can pay. What that record measured is that materialising
    // *unconditionally* destroys content.
    let cursor_abs = pts[0].0 + usize::from(pts[0].1 == dims.cols);
    if dims.limit > 0 {
        while all.len() <= cursor_abs {
            all.push(Row::blank(dims.cols));
        }
    }

    let split = all.len().saturating_sub(dims.rows);
    let history: Vec<Row> = all.drain(0..split).collect();
    let mut sb: VecDeque<Row> = history.into();
    let mut dropped = 0usize;
    while sb.len() > dims.limit {
        sb.pop_front();
        dropped += 1;
    }

    // `reflow` may answer `col == cols` — "just after the last cell", which is a real place in the
    // logical line and no place in the grid (#562). The **cursor's** reading of it is the next
    // *write* position, so a full row means the start of the row after; the caller's row fit
    // provides that row (`Grid::set_screen` pads at the bottom). A mark reads the same value the
    // opposite way and keeps it verbatim — see `Term::resize`.
    let cursor_row = pts[0].0.saturating_sub(split);
    let cursor = if pts[0].1 == dims.cols {
        (cursor_row + 1, 0)
    } else {
        (cursor_row, pts[0].1)
    };

    // The bound on a tracked line belongs **here**, not inside `reflow`: this is where the final
    // geometry is known. The screen is padded to `dims.rows` whatever `reflow` emitted, so this
    // pane's last addressable line is `split + dims.rows - 1`. Bounding against `reflow`'s own row
    // count instead clamped away rows the fit was about to create (#562), while still being the
    // only thing standing between an out-of-range anchor and a panic in the consumer's process —
    // selection anchors and marks are written back raw, unlike the cursor (`Cursor::set_point`).
    // Expressed in this pane's own frame, so it is the same frame `extras` and `evicted` are in.
    let max_line = split + dims.rows - 1;

    // The cursor returns to screen-relative (its absolute index minus the history split). The
    // extras stay in this pane's frame — see the field docs for why they are not shifted here.
    PaneReflow {
        cursor,
        extras: pts[1..]
            .iter()
            .map(|&(l, c)| (l.min(max_line), c))
            .collect(),
        evicted: dropped,
        screen: all,
        scrollback: sb,
    }
}

/// Default tab stops: one every 8 columns (incl. column 0), matching xterm.
fn default_tabs(cols: usize) -> Vec<bool> {
    (0..cols).map(|i| i % 8 == 0).collect()
}

/// First sub-parameter of CSI param `idx`, or `default` when absent or zero
/// (a zero/omitted numeric param means "1" for cursor movement and "0" for
/// erase — callers pass the right default).
fn param_or(params: &Params, idx: usize, default: u16) -> u16 {
    match params.iter().nth(idx).and_then(|p| p.first().copied()) {
        Some(v) if v != 0 => v,
        _ => default,
    }
}

impl Term {
    /// Apply one DEC private mode set (`'h'`) or reset (`'l'`). DECSET/DECRST
    /// carry a list of modes, so `csi_dispatch` folds this over every parameter
    /// (#56); each mode is an independent toggle, not a stack.
    fn set_dec_private_mode(&mut self, action: char, mode: u16) {
        match (action, mode) {
            ('h', 1049) => self.enter_alt_screen(),
            ('l', 1049) => self.leave_alt_screen(),
            // Legacy alt-screen variants (#72): ?47/?1047 switch the buffer
            // without saving the cursor; ?1048 saves/restores the cursor without
            // switching. ?1049 is the two combined.
            ('h', 47) | ('h', 1047) => self.switch_to_alt(),
            ('l', 47) | ('l', 1047) => self.switch_to_primary(),
            ('h', 1048) => self.save_alt_cursor(),
            ('l', 1048) => self.restore_alt_cursor(),
            ('h', 6) => {
                // DECOM: set homes the cursor to the region top.
                self.origin_mode = true;
                self.goto(0, 0);
            }
            ('l', 6) => self.origin_mode = false, // unset leaves the cursor put
            ('h', 7) => self.autowrap = true,     // DECAWM
            ('l', 7) => self.autowrap = false,
            ('h', 45) => self.reverse_wraparound = true, // reverse wraparound (#80)
            ('l', 45) => self.reverse_wraparound = false,
            // DECCOLM (#82): the engine is dimension-free, so emit a request the
            // consumer may honor by resizing — no screen/cursor/margin change here.
            ('h', 3) => self.events.push(TermEvent::ColumnMode { cols: 132 }),
            ('l', 3) => self.events.push(TermEvent::ColumnMode { cols: 80 }),
            ('h', 25) => self.cursor.visible = true, // DECTCEM show
            ('l', 25) => self.cursor.visible = false, // DECTCEM hide
            ('h', 12) => self.cursor.blink = true,   // att610 cursor blink (#81)
            ('l', 12) => self.cursor.blink = false,
            ('h', 2004) => self.bracketed_paste = true,
            ('l', 2004) => self.bracketed_paste = false,
            ('h', 2026) => self.synchronized_output = true, // synchronized output (#73)
            ('l', 2026) => self.synchronized_output = false,
            ('h', 2027) => self.grapheme_clustering = true, // grapheme-cluster mode (#295)
            ('l', 2027) => self.grapheme_clustering = false,
            ('h', 2031) => self.color_scheme_updates = true, // color-scheme notifications (#85)
            ('l', 2031) => self.color_scheme_updates = false,
            ('h', 9001) => self.win32_input_mode = true, // win32-input-mode (#86)
            ('l', 9001) => self.win32_input_mode = false,

            // Input-encoding modes (#11): DECCKM, mouse tracking + encoding,
            // focus reporting. Each set assigns the level; each reset clears
            // it (apps enable/disable the same mode, not a stack).
            ('h', 1) => self.app_cursor_keys = true, // DECCKM
            ('l', 1) => self.app_cursor_keys = false,
            ('h', 66) => self.application_keypad = true, // DECNKM (#74)
            ('l', 66) => self.application_keypad = false,
            // DECANM (#84): set = ANSI (the normal state); reset enters VT52. Only
            // the reset is meaningful — `?2h` is a no-op (already ANSI).
            ('l', 2) => self.vt52_mode = true,
            ('h', 9) => self.mouse_protocol = MouseProtocol::X10, // X10 mouse (#70)
            ('h', 1000) => self.mouse_protocol = MouseProtocol::Normal,
            ('h', 1002) => self.mouse_protocol = MouseProtocol::ButtonEvent,
            ('h', 1003) => self.mouse_protocol = MouseProtocol::AnyEvent,
            ('l', 9) | ('l', 1000) | ('l', 1002) | ('l', 1003) => {
                self.mouse_protocol = MouseProtocol::Off
            }
            ('h', 1006) => self.mouse_encoding = MouseEncoding::Sgr,
            ('l', 1006) => self.mouse_encoding = MouseEncoding::Default,
            ('h', 1015) => self.mouse_encoding = MouseEncoding::Urxvt,
            ('l', 1015) => self.mouse_encoding = MouseEncoding::Default,
            ('h', 1005) => self.mouse_encoding = MouseEncoding::Utf8,
            ('l', 1005) => self.mouse_encoding = MouseEncoding::Default,
            ('h', 1016) => self.mouse_encoding = MouseEncoding::SgrPixels,
            ('l', 1016) => self.mouse_encoding = MouseEncoding::Default,
            ('h', 1004) => self.focus_events = true,
            ('l', 1004) => self.focus_events = false,

            _ => {} // other DEC modes are later slices
        }
    }

    /// Dispatch one VT52 escape sequence (`ESC <final>`), reached only while
    /// `vt52_mode` is set (#84). VT52 is a pre-ANSI dialect: the cursor/erase
    /// finals map to the same `Term` primitives the ANSI path uses. `ESC <`
    /// returns to ANSI. Unknown finals are ignored.
    fn vt52_dispatch(&mut self, byte: u8) {
        match byte {
            b'A' => self.move_up(1),         // cursor up
            b'B' => self.move_down(1),       // cursor down
            b'C' => self.move_forward(1),    // cursor right
            b'D' => self.move_back(1),       // cursor left
            b'H' => self.goto(0, 0),         // cursor home
            b'I' => self.reverse_index(),    // reverse line feed
            b'J' => self.erase_display(0),   // erase cursor → end of screen
            b'K' => self.erase_line(0),      // erase cursor → end of line
            b'Y' => self.vt52_y_pending = 2, // direct address: two coord bytes follow
            // Identify (DECID): reply `ESC / Z` — "I am a VT52".
            b'Z' => self.replies.extend_from_slice(b"\x1b/Z"),
            b'=' => self.application_keypad = true, // enter alternate keypad
            b'>' => self.application_keypad = false, // exit alternate keypad
            b'<' => self.vt52_mode = false,         // exit VT52, return to ANSI
            // RIS (`ESC c`) is honored even here: it is a hard "recover from any
            // state" reset, and `full_reset` rebuilds `Term` with `vt52_mode`
            // cleared, so RIS always escapes VT52 back to ANSI. VT52 defines no
            // other meaning for `ESC c`.
            b'c' => self.full_reset(),
            // Graphics mode (`ESC F`/`ESC G`) is a documented non-goal: the VT52
            // graphics glyph set differs from DEC Special Graphics, so reusing that
            // charset would render the wrong glyphs. No-op rather than approximate.
            b'F' | b'G' => {}
            _ => {} // unknown VT52 finals are ignored
        }
    }

    /// Consume one `ESC Y` coordinate byte (#84). The first byte is the row, the
    /// second the column; each decodes as `value - 0x20`. On the second byte the
    /// cursor is addressed (`goto` clamps out-of-range coordinates). Reached only
    /// from `print` while `vt52_y_pending > 0`.
    fn vt52_take_coord(&mut self, c: char) {
        let coord = (c as usize).saturating_sub(0x20);
        if self.vt52_y_pending == 2 {
            self.vt52_y_row = coord;
            self.vt52_y_pending = 1;
        } else {
            self.vt52_y_pending = 0;
            self.goto(self.vt52_y_row, coord);
        }
    }
}

impl Perform for Term {
    fn print(&mut self, c: char) {
        // VT52 `ESC Y` direct addressing (#84): vte delivers the two coordinate
        // bytes here (it returned to ground after the `Y` final), so intercept
        // them before they would be written as glyphs.
        if self.vt52_y_pending > 0 {
            self.vt52_take_coord(c);
            return;
        }
        // Translate through the active (GL) character set first (#62): under DEC
        // Special Graphics a printable byte becomes a line-drawing glyph.
        let c = self.charsets[self.gl].map(c);
        // Grapheme-cluster mode (DEC ?2027, #295): if `c` extends the previous cell's cluster,
        // join it there instead of placing a new cell. OFF → the per-char (wcwidth) path below.
        if self.grapheme_clustering && self.try_grapheme_join(c) {
            return;
        }
        match c.width() {
            // Zero-width (combining marks): the grapheme-cluster side-table is a
            // later slice; drop for now rather than mis-place it as its own cell.
            // A zero-width code point is a combining mark — attach it to the
            // previous base glyph rather than dropping it.
            Some(0) => self.push_combining(c),
            None => {}
            Some(width) => self.write_glyph(c, width),
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            // LF, VT, FF all line-feed.
            b'\n' | 0x0b | 0x0c => self.linefeed(),
            b'\r' => self.carriage_return(),
            0x08 => self.backspace(),
            b'\t' => self.put_tab(),
            0x07 => self.events.push(TermEvent::Bell), // BEL (#12)
            0x0e => self.gl = 1,                       // SO (LS1): GL = G1 (#62)
            0x0f => self.gl = 0,                       // SI (LS0): GL = G0
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        // Kitty keyboard-protocol negotiation: CSI > / = / < / ? ... u. The
        // leading intermediate distinguishes it from plain `CSI u` (SCORC) (#23).
        if action == 'u'
            && let Some(&lead) = intermediates.first()
            && matches!(lead, b'>' | b'<' | b'=' | b'?')
        {
            self.kitty_dispatch(lead, params);
            return;
        }
        // DEC private modes arrive with a '?' intermediate.
        if intermediates.first() == Some(&b'?') {
            // DECRQM (CSI ? Ps $ p) — report whether mode Ps is set. The '$'
            // intermediate distinguishes it from a plain `?...p`. It queries a
            // single mode, so it keys off the first parameter only.
            if action == 'p' && intermediates.contains(&b'$') {
                self.decrqm(param_or(params, 0, 0));
                return;
            }
            // Private DSR (CSI ? Ps n): ?996 = color-scheme query (#85). The
            // theme-agnostic engine relays it as an event for the consumer.
            if action == 'n' {
                if param_or(params, 0, 0) == 996 {
                    self.events.push(TermEvent::ColorSchemeQuery);
                }
                return;
            }
            // DECSET/DECRST carry a *list* of modes; apply set/reset to EVERY
            // parameter, not just the first — htop batches `?1006;1000h` into one
            // CSI, so folding only params[0] dropped the 1000 (#56).
            for mode in params.iter().filter_map(|p| p.first().copied()) {
                self.set_dec_private_mode(action, mode);
            }
            return;
        }
        // DECSTR soft reset: CSI ! p (#53).
        if intermediates.first() == Some(&b'!') && action == 'p' {
            self.soft_reset();
            return;
        }
        // DECSCUSR set cursor style: CSI Ps SP q (space intermediate) (#89). An
        // absent param means 1 (block blink); an explicit 0 means reset — so the
        // raw value matters and `param_or` (which folds 0 to its default) is wrong.
        if intermediates.first() == Some(&b' ') && action == 'q' {
            let param = params.iter().next().and_then(|p| p.first().copied());
            self.set_cursor_style(param.unwrap_or(1));
            return;
        }
        // Other private/intermediate sequences are later slices; ignore them
        // rather than misinterpret.
        if !intermediates.is_empty() {
            return;
        }
        match action {
            'A' => self.move_up(param_or(params, 0, 1) as usize),
            'B' | 'e' => self.move_down(param_or(params, 0, 1) as usize),
            'C' | 'a' => self.move_forward(param_or(params, 0, 1) as usize),
            'D' => self.move_back(param_or(params, 0, 1) as usize),
            'G' | '`' => self.set_col(param_or(params, 0, 1) as usize - 1),
            'd' => self.set_row(param_or(params, 0, 1) as usize - 1),
            'H' | 'f' => {
                let row = param_or(params, 0, 1) as usize - 1;
                let col = param_or(params, 1, 1) as usize - 1;
                self.goto(row, col);
            }
            'J' => self.erase_display(param_or(params, 0, 0)),
            'K' => self.erase_line(param_or(params, 0, 0)),
            'X' => self.erase_chars(param_or(params, 0, 1) as usize),
            '@' => self.insert_chars(param_or(params, 0, 1) as usize),
            'P' => self.delete_chars(param_or(params, 0, 1) as usize),
            'S' => self.scroll_up_lines(param_or(params, 0, 1) as usize),
            'T' => self.scroll_down_lines(param_or(params, 0, 1) as usize),
            'L' => self.insert_lines(param_or(params, 0, 1) as usize),
            'M' => self.delete_lines(param_or(params, 0, 1) as usize),
            'g' => self.clear_tab_stop(param_or(params, 0, 0)),
            'r' => {
                let rows = self.grid.rows() as u16;
                let top = param_or(params, 0, 1) as usize;
                let bottom = param_or(params, 1, rows) as usize;
                self.set_scroll_region(top, bottom);
            }
            'm' => self.sgr(params),
            's' => self.save_cursor(),    // SCOSC (CSI s) — alias of DECSC
            'u' => self.restore_cursor(), // SCORC (CSI u) — alias of DECRC
            // DA1 (primary device attributes, CSI c): advertise VT220 + ANSI
            // colour — the levels justerm actually implements (#27).
            'c' => self.replies.extend_from_slice(b"\x1b[?62;22c"),
            'n' => self.device_status_report(param_or(params, 0, 0)),
            // Non-private SM/RM. Folded over every parameter (modes can batch,
            // like the private path #56). IRM (4) and LNM (20) so far.
            'h' => {
                for m in params.iter().filter_map(|p| p.first().copied()) {
                    match m {
                        4 => self.insert_mode = true,
                        20 => self.newline_mode = true,
                        _ => {}
                    }
                }
            }
            'l' => {
                for m in params.iter().filter_map(|p| p.first().copied()) {
                    match m {
                        4 => self.insert_mode = false,
                        20 => self.newline_mode = false,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        // VT52 mode (#84): the pre-ANSI dialect reuses the same `ESC <final>`
        // tokens vte already produces, but with different meanings, so it is a
        // mode-gated branch here rather than a separate parser. All VT52 sequences
        // are intermediate-free; anything with an intermediate is not VT52.
        if self.vt52_mode && intermediates.is_empty() {
            self.vt52_dispatch(byte);
            return;
        }
        if let Some(&i) = intermediates.first() {
            // SCS: designate a charset to G0 (`ESC ( F`) or G1 (`ESC ) F`) (#62).
            if matches!(i, b'(' | b')') {
                let set = match byte {
                    b'0' => Charset::DecSpecialGraphics,
                    b'A' => Charset::Uk,
                    b'B' => Charset::Ascii,
                    _ => return, // other sets are later slices
                };
                self.charsets[if i == b'(' { 0 } else { 1 }] = set;
            }
            // Other intermediates (G2/G3 designators, etc.) are later slices.
            return;
        }
        match byte {
            b'D' => self.linefeed(), // IND (line-feed without CR)
            b'E' => {
                // NEL (next line): carriage return + line-feed.
                self.carriage_return();
                self.linefeed();
            }
            b'H' => self.set_tab_stop(),             // HTS
            b'M' => self.reverse_index(),            // RI
            b'7' => self.save_cursor(),              // DECSC
            b'8' => self.restore_cursor(),           // DECRC
            b'c' => self.full_reset(),               // RIS (#53)
            b'=' => self.application_keypad = true,  // DECKPAM (#74)
            b'>' => self.application_keypad = false, // DECKPNM
            _ => {}
        }
    }

    /// OSC dispatch (#12 event surface): title (0/2), cwd (7). OSC 8 hyperlink
    /// is per-cell state, handled in its own slice (#26), not here.
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // params[0] is the OSC number; params[1..] the payload fields.
        let Some(&number) = params.first() else {
            return;
        };
        match number {
            // OSC 0 = icon + window title, OSC 2 = window title. Both set title.
            b"0" | b"2" => {
                if let Some(&title) = params.get(1) {
                    self.events.push(TermEvent::Title(
                        String::from_utf8_lossy(title).into_owned(),
                    ));
                }
            }
            // OSC 7 = current working directory (a file:// URI).
            b"7" => {
                if let Some(&cwd) = params.get(1) {
                    self.events
                        .push(TermEvent::Cwd(String::from_utf8_lossy(cwd).into_owned()));
                }
            }
            // OSC 133 = FinalTerm/iTerm2 shell-integration command marks (#158):
            // `A` prompt start, `B` command start, `C` output start, `D[;exit]`
            // command finished. Each anchors a kinded marker at the cursor line;
            // pairing + navigation is consumer policy (#160). Unknown subcommands
            // (or none) are ignored. `D`'s exit field parses to `i32`, else None.
            b"133" => match params.get(1).copied() {
                Some(b"A") => self.add_command_mark(MarkerKind::PromptStart),
                Some(b"B") => self.add_command_mark(MarkerKind::CommandStart),
                Some(b"C") => self.add_command_mark(MarkerKind::OutputStart),
                Some(b"D") => {
                    let exit = params
                        .get(2)
                        .and_then(|p| core::str::from_utf8(p).ok())
                        .and_then(|s| s.parse::<i32>().ok());
                    self.add_command_mark(MarkerKind::CommandFinished(exit));
                }
                _ => {}
            },
            // OSC 8 = hyperlink: `OSC 8 ; params ; URI`. A non-empty URI opens a
            // link (interned + made current); an empty URI closes it. `params`
            // (e.g. `id=…`) is ignored for now — id-grouping is a later refinement.
            b"8" => {
                let uri = params.get(2).copied().unwrap_or(b"");
                if uri.is_empty() {
                    self.current_link = None;
                } else {
                    self.hyperlink_pool
                        .push(String::from_utf8_lossy(uri).into_owned());
                    self.current_link =
                        core::num::NonZeroU32::new(self.hyperlink_pool.len() as u32);
                }
            }
            // OSC 4 = set/query an ANSI palette entry: `OSC 4 ; index ; spec`
            // (#122). The engine forwards index + raw spec; the consumer applies
            // it to its palette (theme-agnostic — the cell keeps `Indexed`).
            b"4" => {
                // One event per `index ; spec` pair (xterm's `while slots > 1`).
                let mut rest = &params[1..];
                while let [idx, spec, tail @ ..] = rest {
                    rest = tail;
                    if let Ok(index) = String::from_utf8_lossy(idx).parse::<u8>() {
                        if *spec == b"?" {
                            self.events.push(TermEvent::QueryPaletteColor { index });
                        } else {
                            self.events.push(TermEvent::SetPaletteColor {
                                index,
                                spec: String::from_utf8_lossy(spec).into_owned(),
                            });
                        }
                    }
                }
            }
            // OSC 104 = reset palette entries (#122): no arg resets the whole
            // table, else one event per named index.
            b"104" => {
                if params.len() <= 1 {
                    self.events.push(TermEvent::ResetPaletteColor(None));
                } else {
                    for &idx in &params[1..] {
                        if let Ok(index) = String::from_utf8_lossy(idx).parse::<u8>() {
                            self.events.push(TermEvent::ResetPaletteColor(Some(index)));
                        }
                    }
                }
            }
            // OSC 10/11 = set/query the default foreground/background, stacking
            // specs across the [fg, bg] slots (#122, #137). OSC 10 starts at fg,
            // OSC 11 at bg. The engine forwards raw specs (theme-agnostic).
            b"10" => self.special_color(params, 0),
            b"11" => self.special_color(params, 1),
            // OSC 110 / 111 = reset the default foreground / background (#122).
            b"110" => self.events.push(TermEvent::ResetForeground),
            b"111" => self.events.push(TermEvent::ResetBackground),
            _ => {} // other OSCs are later slices
        }
    }
}
