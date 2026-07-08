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
use crate::grid::{Grid, Row};
use crate::input::{
    KeyEvent, MouseEncoding, MouseEvent, MouseProtocol, encode_focus, encode_key, encode_mouse,
    encode_paste,
};
use crate::logical::LogicalLine;
use crate::search::Match;
use crate::selection::{Anchor, BufferPoint, Selection, SelectionSpan, SelectionType, Side};
use crate::serialize::{
    Frame, FrameKind, MarkerId, MarkerKind, MarkerLine, MarkerPosition, Overlay, Span,
};

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
    /// The active search highlights the consumer asked to paint (#108). Search
    /// matches are consumer-owned (it drives next/prev), so the engine holds only
    /// the set handed back via `set_search_highlights`, and `frame()` projects it
    /// onto the viewport — the same anchoring path as the selection.
    search_highlights: Vec<Match>,
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

/// A selection resolved to absolute-coordinate bounds, ready for text extraction
/// or viewport-span projection. Columns are half-open (`from..to`).
enum Resolved {
    /// Char/Word/Line: a run that joins soft-wrapped rows. Columns apply to the
    /// first/last line; middle lines are whole.
    Linear {
        start_line: usize,
        from: usize,
        end_line: usize,
        to: usize,
    },
    /// Block: a rectangle — the same `from..to` columns on every row.
    Block {
        line0: usize,
        line1: usize,
        from: usize,
        to: usize,
    },
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
    fn damage_span(&mut self, row: usize, left: usize, right: usize) {
        self.line_damage[row].expand(left, right);
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
            let row = self.abs_row(top + line);
            for col in left..=right {
                let cell = row[col];
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
                cells.push(cell);
            }
            spans.push(Span {
                line: line as u16,
                left: left as u16,
                right: right as u16,
                cells,
                combining,
                links,
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

    /// The cells of absolute buffer line `line`, reading the screen portion from
    /// `grid` (scrollback is shared). Callers pick the active grid (`abs_line`) or
    /// the primary grid (`primary_grid`, for command-mark text on the alt screen).
    fn line_in<'a>(&'a self, grid: &'a Grid, line: usize) -> &'a [Cell] {
        if line < self.scrollback.len() {
            &self.scrollback[line]
        } else {
            grid.row(line - self.scrollback.len())
        }
    }

    /// The whole row of absolute buffer line `line` from `grid` (see `line_in`).
    fn row_in<'a>(&'a self, grid: &'a Grid, line: usize) -> &'a Row {
        if line < self.scrollback.len() {
            &self.scrollback[line]
        } else {
            grid.row_ref(line - self.scrollback.len())
        }
    }

    /// The cells of absolute buffer line `line` on the *active* screen.
    fn abs_line(&self, line: usize) -> &[Cell] {
        self.line_in(&self.grid, line)
    }

    /// The whole row of absolute buffer line `line` on the *active* screen.
    fn abs_row(&self, line: usize) -> &Row {
        self.row_in(&self.grid, line)
    }

    /// The combining marks at absolute `(line, col)` reading `grid`, or `None` —
    /// flag-gated through the row's map, so a stale entry is never surfaced.
    fn combining_in<'a>(&'a self, grid: &'a Grid, line: usize, col: usize) -> Option<&'a [char]> {
        self.row_in(grid, line).combining_at(col)
    }

    /// The combining marks at absolute `(line, col)` on the *active* screen.
    fn combining_at(&self, line: usize, col: usize) -> Option<&[char]> {
        self.combining_in(&self.grid, line, col)
    }

    /// The hyperlink-pool index at **screen** `(row, col)` (the live grid), or
    /// `None` — flag-gated through the row's link map. Resolve to the URI with
    /// [`Term::hyperlink`]. Mirrors `grid().cell(row, col)`.
    pub(crate) fn screen_link_at(&self, row: usize, col: usize) -> Option<core::num::NonZeroU32> {
        self.grid.row_ref(row).link_at(col)
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
        // the logical line's true start so an edge-spanning URL still matches.
        // On the alt screen the scrollback belongs to the *primary* buffer, so
        // the walk must stop at the screen top (`scrollback.len()`) — the alt
        // buffer is separate (selection clears on alt-swap for the same reason).
        let floor = if self.on_alt {
            self.scrollback.len()
        } else {
            0
        };
        let mut start = top;
        while start > floor
            && self
                .abs_line(start - 1)
                .last()
                .is_some_and(|c| c.is_wrapline())
        {
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
                let soft = cells.last().is_some_and(|c| c.is_wrapline());
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

    /// Literal search over the whole buffer (`[scrollback ++ screen]`), returning
    /// every non-overlapping match top-to-bottom in absolute coordinates. Matches
    /// cross soft-wrapped rows (one logical line) and skip wide-char spacers.
    /// Smart-case: a query with no uppercase matches case-insensitively.
    pub fn search(&self, query: &str) -> Vec<Match> {
        let q: Vec<char> = query.chars().collect();
        if q.is_empty() {
            return Vec::new();
        }
        let ci = !q.iter().any(|c| c.is_uppercase());
        // Fold to a single representative char so the haystack stays 1:1 with its
        // positions (rare multi-char case expansions take their first char).
        let fold = |c: char| {
            if ci {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c
            }
        };
        let needle: Vec<char> = q.iter().map(|&c| fold(c)).collect();
        let total = self.scrollback.len() + self.grid.rows();

        // On the alt screen the scrollback belongs to the *primary* buffer, so the
        // walk must start at the screen top (`scrollback.len()`): primary matches are
        // unreachable on alt, and a primary WRAPLINE row would otherwise soft-wrap-join
        // into the alt grid and corrupt the haystack at the boundary. Mirrors the
        // `viewport_logical_lines` floor (#113) — the alt buffer is separate (selection
        // clears on alt-swap for the same reason). (#144)
        let floor = if self.on_alt {
            self.scrollback.len()
        } else {
            0
        };
        let mut matches = Vec::new();
        let mut r = floor;
        while r < total {
            // Build the logical line at `r`: join soft-wrapped rows, recording
            // each char's source position and skipping wide-char spacers.
            let mut hay: Vec<char> = Vec::new();
            let mut pos: Vec<(usize, usize)> = Vec::new();
            let mut line = r;
            loop {
                let cells = self.abs_line(line);
                for (col, cell) in cells.iter().enumerate() {
                    if cell.is_spacer() {
                        continue;
                    }
                    hay.push(fold(cell.c()));
                    pos.push((line, col));
                    // Include the cell's grapheme side-table marks — combining marks, and under
                    // mode 2027 the joined emoji scalars (2nd RI, ZWJ-joined emoji, skin tone) —
                    // so a clustered scalar is findable, not just the base (#304). Each maps to the
                    // same cell column, mirroring `append_cell`'s base+marks extraction.
                    if let Some(marks) = self.combining_at(line, col) {
                        for &m in marks {
                            hay.push(fold(m));
                            pos.push((line, col));
                        }
                    }
                }
                let soft = cells.last().is_some_and(|c| c.is_wrapline());
                if soft && line + 1 < total {
                    line += 1;
                } else {
                    break;
                }
            }

            // Slide the needle over the logical line (non-overlapping).
            let mut i = 0;
            while needle.len() <= hay.len() && i + needle.len() <= hay.len() {
                if hay[i..i + needle.len()] == needle[..] {
                    let (start_line, start_col) = pos[i];
                    let (end_line, end_col) = pos[i + needle.len() - 1];
                    matches.push(Match {
                        start_line,
                        start_col,
                        end_line,
                        end_col,
                    });
                    i += needle.len();
                } else {
                    i += 1;
                }
            }
            r = line + 1;
        }
        matches
    }

    /// Scroll the viewport so a match's start line is visible (placed at the top
    /// when it sits in history; the live view when it is already on screen).
    pub fn search_scroll_to(&mut self, m: &Match) {
        let target = self.scrollback.len().saturating_sub(m.start_line);
        self.set_display_offset(target);
    }

    /// Project a match onto the current viewport as inclusive-column spans, one
    /// per visible row (off-screen parts dropped) — for the renderer to
    /// highlight, like `selection_range`.
    pub fn match_spans(&self, m: &Match) -> Vec<SelectionSpan> {
        let rows = self.grid.rows();
        let top = self.scrollback.len() - self.display_offset;
        let mut spans = Vec::new();
        for line in m.start_line..=m.end_line {
            if line < top {
                continue;
            }
            let row = line - top;
            if row >= rows {
                break;
            }
            let last = self.abs_line(line).len().saturating_sub(1);
            let left = if line == m.start_line { m.start_col } else { 0 };
            let right = if line == m.end_line {
                m.end_col.min(last)
            } else {
                last
            };
            if right >= left {
                spans.push(SelectionSpan { row, left, right });
            }
        }
        spans
    }

    /// Set the active search highlights to paint (#108). The consumer owns the
    /// `Vec<Match>` (it drives next/prev); handing it back here lets `frame()`
    /// project the highlights onto the viewport. An empty vec clears them.
    pub fn set_search_highlights(&mut self, matches: Vec<Match>) {
        self.search_highlights = matches;
    }

    /// Invalidate the held search highlights (#108). Called wherever a buffer
    /// mutation shifts the absolute coordinates the matches were found at — cap
    /// eviction, in-screen region/RI scroll, reflow. Search matches are
    /// query-derived (the engine holds matches, not the query, and the *set*
    /// itself may have changed), so unlike the user-authored selection they are
    /// dropped rather than re-anchored; the consumer re-searches on output
    /// (mirroring xterm/alacritty). Clearing avoids painting wrong content for
    /// the frame between the mutation and the consumer's refresh.
    fn invalidate_search_highlights(&mut self) {
        self.search_highlights.clear();
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
        self.push_marker(line, self.cursor.col, kind);
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

    /// The document (logical) line index that absolute buffer line `abs` renders
    /// into within [`Term::accessible_text`] — the number of hard line-ends before
    /// it (soft-wrapped rows share one logical line). Primary-screen coordinates,
    /// matching `accessible_text`'s `start = 0` for the primary screen; command
    /// marks are primary-only. O(abs) per call — fine for an on-demand query over
    /// the handful of commands in a session.
    fn doc_line_of(&self, grid: &Grid, abs: usize) -> usize {
        (0..abs)
            .filter(|&l| {
                !self
                    .line_in(grid, l)
                    .last()
                    .is_some_and(|c| c.is_wrapline())
            })
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

    /// Begin a selection of `ty` at viewport `(row, col)`, `side`.
    pub fn selection_begin(&mut self, row: usize, col: usize, side: Side, ty: SelectionType) {
        let anchor = Anchor {
            point: self.viewport_to_abs(row, col),
            side,
        };
        self.selection = Some(Selection {
            ty,
            anchor,
            focus: anchor,
        });
    }

    /// Extend the live selection's focus to viewport `(row, col)`, `side`.
    pub fn selection_extend(&mut self, row: usize, col: usize, side: Side) {
        let focus = Anchor {
            point: self.viewport_to_abs(row, col),
            side,
        };
        if let Some(sel) = &mut self.selection {
            sel.focus = focus;
        }
    }

    /// Clear the selection.
    pub fn selection_clear(&mut self) {
        self.selection = None;
    }

    /// Shift the selection up by one absolute line after the oldest history line
    /// is evicted by the scrollback cap. An endpoint clamps to the new top; if
    /// the whole selection was on the evicted line, it is cleared.
    fn selection_evict_oldest(&mut self) {
        let Some((a, f)) = self
            .selection
            .as_ref()
            .map(|s| (s.anchor.point.line, s.focus.point.line))
        else {
            return;
        };
        if a == 0 && f == 0 {
            self.selection = None;
            return;
        }
        if let Some(sel) = &mut self.selection {
            sel.anchor.point.line = a.saturating_sub(1);
            sel.focus.point.line = f.saturating_sub(1);
        }
    }

    /// Rotate the selection within an in-screen scroll of absolute lines
    /// `[top, bottom]`. `up` = content scrolled up (a line dropped at `top`);
    /// otherwise down (dropped at `bottom`). Called once per scrolled line (delta
    /// 1) by linefeed/RI/SU/SD/IL/DL.
    ///
    /// Mirrors alacritty `Selection::rotate`: an endpoint pushed past the region
    /// edge is *clamped* to that edge (upper → `top`/col 0/Left, lower →
    /// `bottom`/last col/Right; columns/side kept for Block), preserving the part
    /// of the selection still in the buffer. The whole selection clears only on a
    /// true *overtake* — the upper endpoint crossing the bottom while the lower
    /// stays inside, or the lower falling above the upper (a selection wholly on
    /// the dropped line). (#174: this replaced a policy that cleared on any
    /// endpoint touching the dropped edge, dropping still-valid content.)
    fn selection_rotate_region(&mut self, top: usize, bottom: usize, up: bool) {
        let (ty, anchor, focus) = match self.selection.as_ref() {
            Some(s) => (s.ty, s.anchor, s.focus),
            None => return,
        };
        let last_col = self.grid.cols().saturating_sub(1);
        // Order the endpoints by buffer position; the upper (`start`) clamps to
        // the region top, the lower (`end`) to the bottom. Remember which is the
        // anchor so the result writes back to the right field.
        let anchor_is_start = anchor.point <= focus.point;
        let (mut start, mut end) = if anchor_is_start {
            (anchor, focus)
        } else {
            (focus, anchor)
        };

        let (top_i, bottom_i) = (top as isize, bottom as isize);
        // The endpoint's line after the one-line scroll, or `None` if it's outside
        // the region (untouched). The dropped-edge line shifts *past* the edge (to
        // be clamped/overtaken below), matching alacritty's `line - delta`.
        let shift = |line: usize| -> Option<isize> {
            if line < top || line > bottom {
                None
            } else if up {
                Some(line as isize - 1)
            } else {
                Some(line as isize + 1)
            }
        };

        // Upper endpoint: clamp to the region top when pushed above it; clear if it
        // overtook the region bottom (down-scroll) while the lower stays inside.
        if let Some(nl) = shift(start.point.line) {
            if nl > bottom_i && (end.point.line as isize) <= bottom_i {
                self.selection = None;
                return;
            }
            if nl < top_i {
                start.point.line = top;
                if ty != SelectionType::Block {
                    start.point.col = 0;
                    start.side = Side::Left;
                }
            } else {
                start.point.line = nl as usize;
            }
        }
        // Lower endpoint: clear if it fell above the (rotated) upper endpoint;
        // else clamp to the region bottom when pushed below it.
        if let Some(nl) = shift(end.point.line) {
            if nl < start.point.line as isize {
                self.selection = None;
                return;
            }
            if nl > bottom_i {
                end.point.line = bottom;
                if ty != SelectionType::Block {
                    end.point.col = last_col;
                    end.side = Side::Right;
                }
            } else {
                end.point.line = nl as usize;
            }
        }

        if let Some(sel) = &mut self.selection {
            if anchor_is_start {
                (sel.anchor, sel.focus) = (start, end);
            } else {
                (sel.anchor, sel.focus) = (end, start);
            }
        }
    }

    /// The selection projected onto the current viewport: one inclusive-column
    /// span per visible row. Rows scrolled off-screen (above or below) are
    /// dropped. Empty when nothing is selected. See `SelectionSpan`.
    pub fn selection_range(&self) -> Vec<SelectionSpan> {
        let Some(resolved) = self.resolve() else {
            return Vec::new();
        };
        let rows = self.grid.rows();
        // Absolute index of viewport row 0.
        let top = self.scrollback.len() - self.display_offset;
        let mut spans = Vec::new();

        // Add a span for absolute `line` with inclusive cols `left..=right`, if
        // the line is currently visible.
        let mut push = |line: usize, left: usize, right: usize| {
            if line >= top {
                let row = line - top;
                if row < rows {
                    spans.push(SelectionSpan { row, left, right });
                }
            }
        };

        match resolved {
            Resolved::Linear {
                start_line,
                from,
                end_line,
                to,
            } => {
                for line in start_line..=end_line {
                    let len = self.abs_line(line).len();
                    let left = if line == start_line { from } else { 0 };
                    let right_excl = if line == end_line { to.min(len) } else { len };
                    if right_excl > left {
                        push(line, left, right_excl - 1);
                    }
                }
            }
            Resolved::Block {
                line0,
                line1,
                from,
                to,
            } => {
                if to > from {
                    for line in line0..=line1 {
                        push(line, from, to - 1);
                    }
                }
            }
        }
        spans
    }

    /// Resolve the live selection into absolute-coordinate bounds per type:
    /// a `Linear` run (char/word/line, which join soft wraps) or a `Block`
    /// rectangle. `None` when nothing is selected. Columns are half-open
    /// (`from..to`). Shared by `selection_text` and `selection_range`.
    fn resolve(&self) -> Option<Resolved> {
        let sel = self.selection.as_ref()?;
        let (start, end) = sel.ordered();
        Some(match sel.ty {
            SelectionType::Char => {
                // Half-open columns: each side decides if its own cell is in.
                let from = match start.side {
                    Side::Left => start.point.col,
                    Side::Right => start.point.col + 1,
                };
                let to = match end.side {
                    Side::Left => end.point.col,
                    Side::Right => end.point.col + 1,
                };
                Resolved::Linear {
                    start_line: start.point.line,
                    from,
                    end_line: end.point.line,
                    to,
                }
            }
            SelectionType::Word => {
                // Snap both ends to word boundaries (side is ignored).
                let ws = self.word_start(start.point);
                let we = self.word_end(end.point);
                Resolved::Linear {
                    start_line: ws.line,
                    from: ws.col,
                    end_line: we.line,
                    to: we.col + 1,
                }
            }
            SelectionType::Line => Resolved::Linear {
                start_line: start.point.line,
                from: 0,
                end_line: end.point.line,
                to: self.grid.cols(),
            },
            SelectionType::Block => {
                // Rectangular: the same column range on every row. Columns come
                // from the two anchors (min/max, with each edge's side).
                let cols = self.grid.cols();
                let (a, b) = (sel.anchor, sel.focus);
                let (lcol, lside, rcol, rside) = if a.point.col <= b.point.col {
                    (a.point.col, a.side, b.point.col, b.side)
                } else {
                    (b.point.col, b.side, a.point.col, a.side)
                };
                let from = match lside {
                    Side::Left => lcol,
                    Side::Right => lcol + 1,
                };
                let to = match rside {
                    Side::Left => rcol,
                    Side::Right => rcol + 1,
                };
                Resolved::Block {
                    line0: a.point.line.min(b.point.line),
                    line1: a.point.line.max(b.point.line),
                    from,
                    to: to.min(cols).max(from),
                }
            }
        })
    }

    /// The selected text (for copy), or `None` when nothing is selected.
    pub fn selection_text(&self) -> Option<String> {
        match self.resolve()? {
            Resolved::Linear {
                start_line,
                from,
                end_line,
                to,
            } => Some(self.extract_lines(&self.grid, start_line, from, end_line, to)),
            Resolved::Block {
                line0,
                line1,
                from,
                to,
            } => {
                // Each row independently — no soft-wrap joining.
                let mut out = String::new();
                for line in line0..=line1 {
                    let hi = to.min(self.abs_line(line).len());
                    let mut seg = String::new();
                    for col in from..hi {
                        self.append_cell(&self.grid, &mut seg, line, col);
                    }
                    out.push_str(seg.trim_end());
                    if line != line1 {
                        out.push('\n');
                    }
                }
                Some(out)
            }
        }
    }

    /// Append the text at absolute `(line, col)` — its base glyph plus any
    /// combining marks from the row's map — to `out`. Wide-char spacers
    /// contribute nothing.
    fn append_cell(&self, grid: &Grid, out: &mut String, line: usize, col: usize) {
        let cell = &self.line_in(grid, line)[col];
        if cell.is_spacer() {
            return;
        }
        out.push(cell.c());
        if let Some(marks) = self.combining_in(grid, line, col) {
            out.extend(marks);
        }
    }

    /// The whole buffer as one text document (#150): scrollback + screen assembled
    /// into logical lines (soft-wrap joined, wide-spacers skipped, trailing blanks
    /// trimmed at the logical end) — the accessible-view a screen reader reads as
    /// a document, distinct from the viewport row tree (#119). Reuses the
    /// selection extraction ([`extract_lines`](Self::extract_lines)) over the full
    /// range. On the alt screen only the alt buffer is shown — its "scrollback" is
    /// the *primary* buffer's, not this app's — mirroring `viewport_logical_lines`'
    /// alt floor.
    pub fn accessible_text(&self) -> String {
        let total = self.scrollback.len() + self.grid.rows();
        if total == 0 {
            return String::new();
        }
        let start = if self.on_alt {
            self.scrollback.len()
        } else {
            0
        };
        let mut doc = self.extract_lines(&self.grid, start, 0, total - 1, usize::MAX);
        // Trim *trailing* empty lines (blank screen rows below the content) — pure
        // noise to a listener, and what a fresh screen would otherwise emit. Keep
        // *internal* blank lines (paragraph breaks between command outputs) — a
        // document wants those, unlike the viewport tree which drops all empties.
        doc.truncate(doc.trim_end_matches('\n').len());
        doc
    }

    /// Concatenate the selected cells from `(start_line, from)` to
    /// `(end_line, to_end)` (half-open columns on the first/last line, whole
    /// lines between). Soft-wrapped rows (WRAPLINE) accumulate into one *logical*
    /// line so trailing-blank trimming happens only at the logical end — spaces
    /// at a wrap boundary are real content. A hard line-end flushes with `\n`.
    fn extract_lines(
        &self,
        grid: &Grid,
        start_line: usize,
        from: usize,
        end_line: usize,
        to_end: usize,
    ) -> String {
        let mut out = String::new();
        let mut current = String::new();
        for line in start_line..=end_line {
            let cells = self.line_in(grid, line);
            let left = if line == start_line { from } else { 0 };
            let right = if line == end_line {
                to_end.min(cells.len())
            } else {
                cells.len()
            };
            // A degenerate range (sides inverting one cell) gives left > right;
            // clamp to empty rather than panic on the slice.
            let right = right.max(left);
            for col in left..right {
                self.append_cell(grid, &mut current, line, col);
            }

            let is_last = line == end_line;
            let soft = cells.last().is_some_and(|c| c.is_wrapline());
            if is_last || !soft {
                out.push_str(current.trim_end());
                current.clear();
                if !is_last {
                    out.push('\n');
                }
            }
        }
        out
    }

    /// The lowest absolute line a soft-wrap buffer walk may reach. On the alt screen
    /// `scrollback` holds the *primary* buffer's history — a separate logical space —
    /// so a walk floors at `scrollback.len()` (the alt grid's first line) and must not
    /// join across it. Mirrors the `search()` (#144) and `viewport_logical_lines`
    /// (#113) floors: justerm's single `[scrollback ++ grid]` buffer reproduces the
    /// primary↔alt isolation xterm gets from separate `Buffer` objects.
    fn abs_floor(&self) -> usize {
        if self.on_alt {
            self.scrollback.len()
        } else {
            0
        }
    }

    /// The cell position before `(line, col)` in the *logical* line — the column
    /// to the left, or the end of the previous row if it soft-wrapped into this
    /// one. `None` at the buffer start or across a hard line-end.
    fn prev_pos(&self, line: usize, col: usize) -> Option<(usize, usize)> {
        if col > 0 {
            return Some((line, col - 1));
        }
        // Only step up while the previous row is still on *this* buffer (>= floor):
        // on alt, row 0 (`line == scrollback.len()`) must not join the primary
        // scrollback row below it, even when that row carries WRAPLINE (#207).
        if line > self.abs_floor() {
            let prev = self.abs_line(line - 1);
            if prev.last().is_some_and(|c| c.is_wrapline()) {
                return Some((line - 1, prev.len() - 1));
            }
        }
        None
    }

    /// The cell position after `(line, col)` in the *logical* line — the column
    /// to the right, or the start of the next row if this row soft-wrapped.
    /// `None` at the buffer end or across a hard line-end.
    fn next_pos(&self, line: usize, col: usize) -> Option<(usize, usize)> {
        let cells = self.abs_line(line);
        if col + 1 < cells.len() {
            return Some((line, col + 1));
        }
        let total = self.scrollback.len() + self.grid.rows();
        // Symmetric floor guard (#207): a row below the floor (primary scrollback on
        // alt) must not soft-wrap-join down into the alt grid. `line >= floor` holds
        // for any position reachable on alt once `prev_pos` is floored; kept explicit
        // so no future caller can cross from a primary row.
        if line >= self.abs_floor()
            && line + 1 < total
            && cells.last().is_some_and(|c| c.is_wrapline())
        {
            return Some((line + 1, 0));
        }
        None
    }

    /// Walk left to the first cell of `p`'s word (a maximal run of non-boundary
    /// chars), following a soft wrap into the previous row.
    fn word_start(&self, p: BufferPoint) -> BufferPoint {
        let cells = self.abs_line(p.line);
        let (mut line, mut col) = (p.line, p.col.min(cells.len().saturating_sub(1)));
        while let Some((pl, pc)) = self.prev_pos(line, col) {
            if is_word_boundary(self.abs_line(pl)[pc].c()) {
                break;
            }
            line = pl;
            col = pc;
        }
        BufferPoint { line, col }
    }

    /// Walk right to the last cell of `p`'s word, following a soft wrap into the
    /// next row.
    fn word_end(&self, p: BufferPoint) -> BufferPoint {
        let cells = self.abs_line(p.line);
        let (mut line, mut col) = (p.line, p.col.min(cells.len().saturating_sub(1)));
        while let Some((nl, nc)) = self.next_pos(line, col) {
            if is_word_boundary(self.abs_line(nl)[nc].c()) {
                break;
            }
            line = nl;
            col = nc;
        }
        BufferPoint { line, col }
    }

    /// Resize the screen to `cols` x `rows`. Rows dropped off the top (on shrink)
    /// enter scrollback. Column reflow of soft-wrapped lines is layered on top
    /// separately (#7). The whole screen is damaged.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        // A terminal is never 0-wide/0-tall; clamp so the math below (rows - 1,
        // chunking by cols) can't underflow or divide by zero.
        let cols = cols.max(1);
        let rows = rows.max(1);
        let old_cols = self.grid.cols();
        let limit = self.scrollback_limit;

        // A reflow moves match coordinates (and can change the match set), so the
        // query-derived highlights are invalidated; the consumer re-searches at
        // the new width. The selection re-anchors below — it is user-authored.
        self.invalidate_search_highlights();

        // Both screens are resized. Scrollback pairs with the PRIMARY screen
        // (whichever is active) — the alt screen has no history of its own.
        let dims = ReflowDims {
            old_cols,
            cols,
            rows,
            limit,
        };
        let scrollback = std::mem::take(&mut self.scrollback);
        if self.on_alt {
            // Active = alt (cursor, no scrollback); inactive = primary. Selection
            // is primary-only and cleared on alt enter, so no anchors to track.
            // Alt markers DO ride this reflow (#187): justerm column-reflows the
            // alt grid, so a marker must follow its content or it drifts off. Their
            // stored line is `base + alt_row` (base = primary scrollback len), so
            // convert to alt-local rows for the pane, and re-anchor on the reflowed
            // base afterward (the primary scrollback below may rewrap its length).
            let old_base = scrollback.len();
            let alt_pts: Vec<(usize, usize)> = self
                .alt_markers
                .iter()
                .map(|m| (m.line - old_base, 0))
                .collect();
            let alt = self.grid.take_lines();
            let r_alt = reflow_pane(alt, VecDeque::new(), self.cursor.point(), &alt_pts, dims);
            self.grid.set_screen(r_alt.screen, cols, rows);
            self.cursor.set_point(r_alt.cursor, rows, cols);

            // Primary is inactive here, but markers anchor *primary* content, so
            // they reflow with it (the selection is already cleared on alt enter).
            let marker_pts: Vec<(usize, usize)> =
                self.normal_markers.iter().map(|m| (m.line, 0)).collect();
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
                m.line = r.extras[i].0;
            }
            let new_base = self.scrollback.len();
            for (i, m) in self.alt_markers.iter_mut().enumerate() {
                m.line = new_base + r_alt.extras[i].0;
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
                sel.anchor.point = BufferPoint {
                    line: r.extras[0].0,
                    col: r.extras[0].1,
                };
                sel.focus.point = BufferPoint {
                    line: r.extras[1].0,
                    col: r.extras[1].1,
                };
            }
            let marker_off = sel_pts.len();
            for (i, m) in self.normal_markers.iter_mut().enumerate() {
                m.line = r.extras[marker_off + i].0;
                m.col = r.extras[marker_off + i].1;
            }

            let alt = self.alt_grid.take_lines();
            let r = reflow_pane(alt, VecDeque::new(), (0, 0), &[], dims);
            self.alt_grid.set_screen(r.screen, cols, rows);
        }

        // Margins reset to the full screen; tab stops reset to the default grid.
        self.cursor.pending_wrap = false;
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
    fn linefeed(&mut self) {
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
                    self.grid.scroll_up_recycle(blank)
                } else {
                    // Top-anchored sub-region: copy row 0, then region-scroll
                    // `[0..=scroll_bottom]` (rows below stay fixed).
                    let evicted = self.grid.row_owned(0);
                    self.grid
                        .scroll_up_region(self.scroll_top, self.scroll_bottom);
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
                self.grid
                    .scroll_up_region(self.scroll_top, self.scroll_bottom);
            }
            self.record_scroll(self.scroll_top, self.scroll_bottom, 1);
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
            self.grid
                .scroll_down_region(self.scroll_top, self.scroll_bottom);
            self.record_scroll(self.scroll_top, self.scroll_bottom, -1);
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
            if self.grid.cell(prev, last).is_wrapline() {
                self.grid
                    .cell_mut(prev, last)
                    .remove_flags(CellFlags::WRAPLINE);
                self.cursor.row = prev;
                self.cursor.col = last;
            }
        }
    }

    /// Auto-wrap at end of line: line-feed then return to column 0.
    fn wrapline(&mut self) {
        self.linefeed();
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

    /// Write one glyph at the cursor, handling deferred wrap and the wide-char
    /// spacer, then advance the cursor (deferring the wrap if it hits the edge).
    fn write_glyph(&mut self, c: char, width: usize) {
        let cols = self.grid.cols();

        // Resolve a deferred last-column wrap before placing the next glyph.
        // The row being left soft-wrapped: mark its last cell so reflow (#7) can
        // tell it from a hard CR/LF line-end.
        if self.cursor.pending_wrap {
            let row = self.cursor.row;
            self.grid
                .cell_mut(row, cols - 1)
                .insert_flags(CellFlags::WRAPLINE);
            self.wrapline();
        }

        // A width-2 glyph that cannot fit in the last column wraps first — unless
        // autowrap is off, in which case it is dropped (xterm.js `continue`), not
        // squeezed or wrapped.
        if width == 2 && self.cursor.col + 1 >= cols {
            if !self.autowrap {
                return;
            }
            // Mark the row soft-wrapped, like the pending-wrap path above: the
            // vacated last column is a continuation, not a hard line-end. Search,
            // logical lines (#113), and reflow (#7) all read WRAPLINE for the join.
            // Also tag it a leading spacer so the text extractors skip the blank
            // (xterm's LEADING_WIDE_CHAR_SPACER) instead of joining "ab한"→"ab 한".
            let vacated = self.grid.cell_mut(self.cursor.row, cols - 1);
            vacated.insert_flags(CellFlags::WRAPLINE);
            vacated.set_leading_spacer();
            self.wrapline();
        }

        // Insert mode (IRM): open a `width`-wide gap at the cursor first, shifting
        // the row's tail right (off-edge cells discarded, wide halves repaired),
        // then write into the gap — mirrors xterm.js's insertCells (#64).
        if self.insert_mode {
            self.insert_chars(width);
        }

        let (row, col) = (self.cursor.row, self.cursor.col);

        // Overwriting one half of an existing wide glyph orphans the other —
        // clear it so no stray lead/spacer is left behind.
        let last = col + width - 1;
        if col > 0 && self.grid.cell(row, col).is_wide_spacer() {
            self.grid.cell_mut(row, col - 1).reset();
        }
        if last + 1 < cols && self.grid.cell(row, last).is_wide() {
            self.grid.cell_mut(row, last + 1).reset();
        }

        let mut cell = self.cursor.pen.cell(c);
        if width == 2 {
            cell.insert_flags(CellFlags::WIDE_CHAR);
        }
        *self.grid.cell_mut(row, col) = cell;
        // Stamp the open hyperlink, if any, into the row's link map (#26/#46).
        if let Some(link) = self.current_link {
            self.grid.row_mut(row).set_link(col, link);
        }

        // The trailing column of a wide glyph carries a distinct spacer marker —
        // and the same link, so a hover/selection over either half agrees.
        if width == 2 && col + 1 < cols {
            let mut spacer = self.cursor.pen.cell(' ');
            spacer.insert_flags(CellFlags::WIDE_CHAR_SPACER);
            *self.grid.cell_mut(row, col + 1) = spacer;
            if let Some(link) = self.current_link {
                self.grid.row_mut(row).set_link(col + 1, link);
            }
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
            self.grid.cell_mut(row, col + 1).reset(); // free the now-unused spacer
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
            self.grid.cell_mut(row, col + 2).reset();
        }
        self.grid
            .cell_mut(row, col)
            .insert_flags(CellFlags::WIDE_CHAR);
        let mut spacer = self.cursor.pen.cell(' ');
        spacer.insert_flags(CellFlags::WIDE_CHAR_SPACER);
        *self.grid.cell_mut(row, col + 1) = spacer;
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
    /// autowrap off, or a 1-column screen (no room for a wide cell anywhere), it stays narrow.
    fn relocate_cluster_wide(&mut self, row: usize, col: usize) {
        let cols = self.grid.cols();
        if cols < 2 || !self.autowrap {
            return; // nowhere to place a wide cell — leave it narrow
        }
        // Capture the base cell (glyph + attrs) and its marks before vacating.
        let base = *self.grid.cell(row, col);
        let marks: Vec<char> = self
            .combining_at(row, col)
            .map(<[char]>::to_vec)
            .unwrap_or_default();
        // Vacate the last column as a soft-wrap leading spacer (mirrors write_glyph 2457-2459).
        // reset() clears the base's combining bit, so its stale marks entry is never read again.
        let vacated = self.grid.cell_mut(row, col);
        vacated.reset();
        vacated.insert_flags(CellFlags::WRAPLINE);
        vacated.set_leading_spacer();
        self.damage_span(row, col, col);
        // Advance to the next line (scrolls if at the bottom); cursor lands at col 0.
        self.wrapline();
        let nr = self.cursor.row;
        // Re-place the base as a wide lead + spacer, re-attaching the marks fresh (drop the combining
        // bit so push_combining starts a clean cluster at the new column).
        let mut lead = base;
        lead.set_combined(false);
        lead.insert_flags(CellFlags::WIDE_CHAR);
        *self.grid.cell_mut(nr, 0) = lead;
        for m in marks {
            self.grid.row_mut(nr).push_combining(0, m);
        }
        let mut spacer = self.cursor.pen.cell(' ');
        spacer.insert_flags(CellFlags::WIDE_CHAR_SPACER);
        *self.grid.cell_mut(nr, 1) = spacer;
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
    fn clear_cells(&mut self, row: usize, from: usize, to: usize) {
        let cols = self.grid.cols();
        // Don't orphan a wide char straddling the erase boundary.
        if from > 0 && self.grid.cell(row, from).is_wide_spacer() {
            self.grid.cell_mut(row, from - 1).reset();
        }
        if to > from && to < cols && self.grid.cell(row, to - 1).is_wide() {
            self.grid.cell_mut(row, to).reset();
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

    /// Erase in display (ED): 0 = cursor→end, 1 = start→cursor, 2 = all.
    fn erase_display(&mut self, mode: u16) {
        let (cols, rows) = (self.grid.cols(), self.grid.rows());
        let (cr, cc) = (self.cursor.row, self.cursor.col);
        match mode {
            0 => {
                self.clear_cells(cr, cc, cols);
                for row in (cr + 1)..rows {
                    self.clear_cells(row, 0, cols);
                }
            }
            1 => {
                for row in 0..cr {
                    self.clear_cells(row, 0, cols);
                }
                self.clear_cells(cr, 0, cc + 1);
            }
            2 => {
                for row in 0..rows {
                    self.clear_cells(row, 0, cols);
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
            0 => self.clear_cells(cr, cc, cols),
            1 => self.clear_cells(cr, 0, cc + 1),
            2 => self.clear_cells(cr, 0, cols),
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
            self.grid.cell_mut(r, col - 1).reset();
        }
        if col + n < cols && self.grid.cell(r, col + n).is_wide_spacer() {
            self.grid.cell_mut(r, col + n).reset();
        }
        // A lead shifted to the last column lost its spacer off the edge.
        if self.grid.cell(r, cols - 1).is_wide() {
            self.grid.cell_mut(r, cols - 1).reset();
        }
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
            self.grid.cell_mut(r, col - 1).reset();
        }
        if self.grid.cell(r, col).is_wide_spacer() {
            self.grid.cell_mut(r, col).reset();
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
            if down {
                self.grid.scroll_down_region(top, bottom);
                self.record_scroll(top, bottom, -1);
            } else {
                self.grid.scroll_up_region(top, bottom);
                self.record_scroll(top, bottom, 1);
            }
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
                // bright foreground/background (aixterm) → palette 8..=15.
                90..=97 => pen.fg = Color::Indexed((code - 90 + 8) as u8),
                100..=107 => pen.bg = Color::Indexed((code - 100 + 8) as u8),
                _ => {}
            }
        }
    }
}

/// Parse `38`/`48` extended colour, in either form:
/// - sub-parameter (colon) form inline in `param`: `38:5:n`, `38:2:r:g:b`
///   (optionally `38:2:cs:r:g:b` with a colorspace id), or
/// - legacy (semicolon) form: pull the following top-level params from `iter`.
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
}

/// The result of reflowing one pane.
struct PaneReflow {
    screen: Vec<Row>,
    scrollback: VecDeque<Row>,
    /// The cursor's new screen-relative position.
    cursor: (usize, usize),
    /// Each tracked extra point's new **absolute** position, index-aligned with
    /// the `extra_abs` argument.
    extras: Vec<(usize, usize)>,
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

    let pts = if dims.cols != dims.old_cols {
        let (reflowed, np) = crate::grid::reflow(all, dims.cols, &pts);
        all = reflowed;
        np
    } else {
        pts
    };

    let split = all.len().saturating_sub(dims.rows);
    let history: Vec<Row> = all.drain(0..split).collect();
    let mut sb: VecDeque<Row> = history.into();
    let mut dropped = 0usize;
    while sb.len() > dims.limit {
        sb.pop_front();
        dropped += 1;
    }

    // The cursor returns to screen-relative (its absolute index minus the
    // history split). The extras stay absolute, shifted down by any lines the
    // cap dropped from the front of history.
    PaneReflow {
        cursor: (pts[0].0.saturating_sub(split), pts[0].1),
        extras: pts[1..]
            .iter()
            .map(|&(l, c)| (l.saturating_sub(dropped), c))
            .collect(),
        screen: all,
        scrollback: sb,
    }
}

/// Whether `c` ends a word for Word (semantic) selection. Whitespace plus a
/// punctuation set mirroring Alacritty's default `semantic_escape_chars`, so
/// path/URL-ish runs (`.`, `/`, `-`) stay one word.
fn is_word_boundary(c: char) -> bool {
    c.is_whitespace() || ",│`|:\"'()[]{}<>".contains(c)
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
