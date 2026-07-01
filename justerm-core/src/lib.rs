//! justerm-core — the pure terminal engine of the `justerm` family.
//!
//! Feed VT bytes in; read terminal state out. The engine does no I/O, no IPC,
//! no rendering, and is theme-agnostic (it stores colour *references*, never
//! hex). See `CLAUDE.md` for the boundary invariants and `docs/architecture.md`
//! for the full contract.
//!
//! ```
//! use justerm_core::{Color, Engine};
//!
//! let mut term = Engine::new(80, 24);
//! term.feed(b"\x1b[31mhi\x1b[0m");
//! assert_eq!(term.grid().cell(0, 0).c(), 'h');
//! assert_eq!(term.grid().cell(0, 0).fg(), Color::Indexed(1));
//! ```

mod cell;
mod color;
mod cursor;
mod damage;
mod event;
mod grid;
mod input;
mod logical;
mod search;
mod selection;
mod serialize;
mod term;

pub use cell::{Cell, CellFlags};
pub use color::Color;
pub use cursor::{Cursor, CursorShape, Pen};
pub use damage::{LineDamage, ScrollOp, TermDamage};
pub use event::TermEvent;
pub use grid::{Grid, Row};
pub use input::{
    Key, KeyAction, KeyEvent, KeypadKey, Modifiers, MouseAction, MouseButton, MouseEvent,
    MouseEvents,
};
pub use logical::LogicalLine;
pub use search::Match;
pub use selection::{SelectionSpan, SelectionType, Side};
pub use serialize::{
    CELL_RECORD_LEN, DecodeError, Frame, FrameKind, MarkerId, MarkerKind, MarkerPosition, Overlay,
    Span, WIRE_VERSION, decode, encode, encode_cell_record, encode_color,
};

pub use term::Term;

use vte::Parser;

/// The terminal engine: pairs the `vte` parser with our state model.
///
/// `Parser` and `Term` are kept as separate fields because `Parser::advance`
/// borrows both the parser and the performer mutably at once — a single struct
/// owning both could not satisfy the borrow checker.
pub struct Engine {
    parser: Parser,
    term: Term,
}

impl Engine {
    /// A blank engine with a `cols` × `rows` screen and a default scrollback cap.
    pub fn new(cols: usize, rows: usize) -> Self {
        Engine {
            parser: Parser::new(),
            term: Term::new(cols, rows),
        }
    }

    /// Like [`Engine::new`] but with an explicit scrollback line limit.
    pub fn with_scrollback(cols: usize, rows: usize, scrollback_limit: usize) -> Self {
        Engine {
            parser: Parser::new(),
            term: Term::with_scrollback(cols, rows, scrollback_limit),
        }
    }

    /// Push a slice of VT bytes. The caller owns the PTY/SSH/socket I/O — the
    /// engine only consumes the bytes it is handed.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// Resize the screen to `cols` x `rows`. Rows that scroll off the top enter
    /// scrollback; the whole screen is damaged. (Soft-wrap reflow lands in #7.)
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.term.resize(cols, rows);
    }

    /// The current screen grid.
    pub fn grid(&self) -> &Grid {
        self.term.grid()
    }

    /// The current cursor (position, pending-wrap, pen).
    pub fn cursor(&self) -> &Cursor {
        self.term.cursor()
    }

    /// Whether bracketed-paste mode (DEC ?2004) is enabled. A consumer's input
    /// encoder reads this to decide whether to wrap pasted text in markers.
    pub fn bracketed_paste(&self) -> bool {
        self.term.bracketed_paste()
    }

    /// Encode a key event to the bytes an application expects, honouring the
    /// engine's cursor-key mode (DECCKM). The inverse of [`Engine::feed`] — the
    /// consumer hands a decoded key event and writes the bytes to its PTY.
    /// Returns `None` for a key with no defined encoding.
    pub fn encode_key(&self, ev: KeyEvent) -> Option<Vec<u8>> {
        self.term.encode_key(ev)
    }

    /// Encode a mouse event using the engine's active tracking mode + encoding.
    /// Returns `None` when mouse reporting is off, or when the event is filtered
    /// out by the mode (e.g. a bare move while only ?1000 is set).
    pub fn encode_mouse(&self, ev: MouseEvent) -> Option<Vec<u8>> {
        self.term.encode_mouse(ev)
    }

    /// Encode pasted text — wrapped in bracketed-paste markers when ?2004 is on,
    /// raw otherwise.
    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        self.term.encode_paste(text)
    }

    /// Encode a focus change (`CSI I` on focus-in, `CSI O` on focus-out), or
    /// `None` when focus reporting (?1004) is off.
    pub fn encode_focus(&self, focused: bool) -> Option<Vec<u8>> {
        self.term.encode_focus(focused)
    }

    /// Take the consumer events accumulated since the last drain (title / bell /
    /// cwd — see [`TermEvent`]), emptying the queue. The pull counterpart to a
    /// callback: poll this alongside [`Engine::frame`].
    pub fn drain_events(&mut self) -> Vec<TermEvent> {
        self.term.drain_events()
    }

    /// Take the reply bytes the engine produced for app queries (DA / DSR /
    /// DECRQM) since the last drain — the consumer writes them straight back to
    /// the PTY. The inbound-query counterpart to [`Engine::drain_events`].
    pub fn drain_replies(&mut self) -> Vec<u8> {
        self.term.drain_replies()
    }

    /// The OSC 8 hyperlink index at **screen** `(row, col)` — the live grid, same
    /// coordinates as [`Engine::grid`]'s `cell(row, col)` — or `None`. Combining
    /// and links no longer ride on the [`Cell`](crate::Cell) (#45/#46); read the
    /// index here, then resolve it with [`Engine::hyperlink`].
    pub fn link_at(&self, row: usize, col: usize) -> Option<core::num::NonZeroU32> {
        self.term.screen_link_at(row, col)
    }

    /// The OSC 8 hyperlink index at **viewport** `(row, col)` — the visible
    /// window including scrollback at the current scroll, same coordinates as
    /// [`Engine::viewport_line`] — or `None`.
    pub fn viewport_link_at(&self, row: usize, col: usize) -> Option<core::num::NonZeroU32> {
        self.term.viewport_link_at(row, col)
    }

    /// Resolve a hyperlink index (from [`Engine::link_at`] /
    /// [`Engine::viewport_link_at`], or a decoded `Span`'s `links`) to its URI,
    /// to make a cell clickable.
    pub fn hyperlink(&self, link: core::num::NonZeroU32) -> Option<&str> {
        self.term.hyperlink(link)
    }

    /// Number of lines currently held in scrollback history.
    pub fn scrollback_len(&self) -> usize {
        self.term.scrollback_len()
    }

    /// Whether the app has an open **synchronized-output** block (DEC `?2026`):
    /// it has asked that the next frame of output be painted atomically. The
    /// engine only *reports* this — **the consumer owns the paint-hold and the
    /// spec-mandated timeout** (a buggy app that never closes the block must not
    /// freeze the screen forever, and the engine has no clock). Poll this after
    /// `feed`; while it is `true`, defer applying frames, and apply once it
    /// clears (or your own timeout fires). (#73)
    pub fn synchronized_output(&self) -> bool {
        self.term.synchronized_output()
    }

    /// Whether the app enabled color-scheme-update notifications (DEC `?2031`).
    /// The engine is theme-agnostic — it never knows the scheme. The consumer
    /// answers a [`TermEvent::ColorSchemeQuery`] (from `?996`) and, when its
    /// scheme changes *and* this is `true`, sends an unsolicited notification, in
    /// both cases by calling [`Engine::report_color_scheme`] (#85).
    pub fn color_scheme_updates(&self) -> bool {
        self.term.color_scheme_updates()
    }

    /// Report the current light/dark color scheme to the app as `CSI ? 997 ; 1 n`
    /// (dark) / `; 2 n` (light), drained via [`Engine::drain_replies`]. Call this
    /// to answer a [`TermEvent::ColorSchemeQuery`], or — guarded by
    /// [`Engine::color_scheme_updates`] — when the scheme changes. The engine only
    /// formats the bit you pass; it stores no scheme (#85).
    pub fn report_color_scheme(&mut self, dark: bool) {
        self.term.report_color_scheme(dark);
    }

    /// Answer an OSC 11 `QueryBackground` event (#122): the consumer hands back
    /// the current background spec (it owns the palette) and the engine queues
    /// the OSC 11 reply for `drain_replies`. Theme-agnostic — the engine never
    /// knows the colour, only formats the envelope.
    pub fn report_background(&mut self, spec: &str) {
        self.term.report_background(spec);
    }

    /// Answer an OSC 10 `QueryForeground` event (#122): queue the OSC 10 reply
    /// from the consumer-supplied spec. Theme-agnostic envelope-only.
    pub fn report_foreground(&mut self, spec: &str) {
        self.term.report_foreground(spec);
    }

    /// Answer an OSC 4 `QueryPaletteColor` event (#122): queue the OSC 4 reply for
    /// `index` from the consumer-supplied spec. Theme-agnostic envelope-only.
    pub fn report_palette_color(&mut self, index: u8, spec: &str) {
        self.term.report_palette_color(index, spec);
    }

    /// Whether the app enabled **win32-input-mode** (DEC `?9001`): it asked for
    /// keys as raw Windows key-records. The engine only tracks the flag — encoding
    /// the records (`CSI Vk;Sc;Uc;Kd;Cs;Rc _`) is a non-goal (raw passthrough, no
    /// semantic conversion), so [`Engine::encode_key`] is unchanged. A ConPTY
    /// consumer reads this to decide whether to emit the records itself (#86).
    pub fn win32_input_mode(&self) -> bool {
        self.term.win32_input_mode()
    }

    /// What changed since the last [`Engine::reset_damage`] — line ranges each
    /// with a changed column span (see ADR-0003).
    pub fn damage(&self) -> TermDamage {
        self.term.damage()
    }

    /// Build a serializable [`Frame`] of the current diff — the damaged spans
    /// (or every row, when `Full`), the recorded scroll op, and a frame-local
    /// grapheme side-table. Pass it to [`encode`] for the wire (see #6). Reading
    /// a frame does not clear damage; call [`Engine::reset_damage`] on ack.
    pub fn frame(&self) -> Frame {
        self.term.frame()
    }

    /// Clear accumulated damage after a frame is applied (the consumer's ack).
    pub fn reset_damage(&mut self) {
        self.term.reset_damage();
    }

    /// Force the next [`Engine::frame`] to be a `Full` frame (every row), even if
    /// little changed. The use case is **reattach / late subscribe**: a renderer
    /// that connects after output has already been parsed needs the whole current
    /// viewport once, then incremental diffs. Marks the screen fully damaged; the
    /// next `frame()` reports `FrameKind::Full`.
    pub fn mark_fully_damaged(&mut self) {
        self.term.mark_fully_damaged();
    }

    /// The first-class scroll recorded since the last [`Engine::reset_damage`],
    /// if any — lets the renderer shift rows instead of redrawing them.
    pub fn scroll_delta(&self) -> Option<ScrollOp> {
        self.term.scroll_delta()
    }

    /// The cells of visible row `i` (0..rows) at the current scroll position.
    pub fn viewport_line(&self, i: usize) -> &[Cell] {
        self.term.viewport_line(i)
    }

    /// Scroll the viewport up by `n` lines into scrollback history.
    pub fn scroll_up(&mut self, n: usize) {
        self.term.scroll_up(n);
    }

    /// Scroll the viewport down by `n` lines toward the live screen.
    pub fn scroll_down(&mut self, n: usize) {
        self.term.scroll_down(n);
    }

    /// Jump the viewport back to the live screen (follow the bottom).
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_to_bottom();
    }

    /// Begin a selection of `ty` at viewport cell `(row, col)`, on `side` of the
    /// cell. Coordinates are viewport-relative (what a mouse event carries).
    pub fn selection_begin(&mut self, row: usize, col: usize, side: Side, ty: SelectionType) {
        self.term.selection_begin(row, col, side, ty);
    }

    /// Extend the live selection to viewport cell `(row, col)`, on `side`.
    pub fn selection_extend(&mut self, row: usize, col: usize, side: Side) {
        self.term.selection_extend(row, col, side);
    }

    /// Clear the selection.
    pub fn selection_clear(&mut self) {
        self.term.selection_clear();
    }

    /// The selection projected onto the viewport: one inclusive-column span per
    /// visible row, for the renderer to highlight. Empty when nothing is
    /// selected or the selection is fully scrolled off-screen.
    pub fn selection_range(&self) -> Vec<SelectionSpan> {
        self.term.selection_range()
    }

    /// The selected text for copy (respects scrollback), or `None` if no
    /// selection.
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_text()
    }

    /// Literal search over the grid + scrollback, returning every match in
    /// absolute buffer coordinates (top-to-bottom). Smart-case: a query with no
    /// uppercase matches case-insensitively. The consumer drives next/prev by
    /// walking the returned `Vec` and calling [`Engine::scroll_to_match`].
    pub fn search(&self, query: &str) -> Vec<Match> {
        self.term.search(query)
    }

    /// The viewport's logical lines (#113/ADR-0017): each soft-wrap-joined line's
    /// text plus a per-char map to its viewport `(row, col)`. The buffer-wide
    /// mechanism for consumer-side URL detection — the consumer runs its own
    /// regex / `new URL()` over the text and maps matches back through `cells`.
    /// Also serves the a11y mirror (#119).
    pub fn viewport_logical_lines(&self) -> Vec<LogicalLine> {
        self.term.viewport_logical_lines()
    }

    /// The whole buffer (scrollback + screen) as one text document for a
    /// screen-reader accessible view (#150) — soft-wrap-joined, wide-spacers
    /// skipped, trailing blanks trimmed at the logical end, `\n` between logical
    /// lines. A query seam the consumer summons (frame mode: over IPC, like
    /// [`selection_text`](Self::selection_text)); no wire-format change. On the
    /// alt screen only the alt buffer is shown.
    pub fn accessible_text(&self) -> String {
        self.term.accessible_text()
    }

    /// Scroll the viewport so `m` is visible (next/prev navigation: the consumer
    /// picks the match, the engine scrolls to it).
    pub fn scroll_to_match(&mut self, m: &Match) {
        self.term.search_scroll_to(m);
    }

    /// The match projected onto the viewport as inclusive-column spans per
    /// visible row, for the renderer to highlight.
    pub fn match_spans(&self, m: &Match) -> Vec<SelectionSpan> {
        self.term.match_spans(m)
    }

    /// Set the active search highlights the frame should carry (#108). The
    /// consumer owns match navigation, so it hands the set to highlight back
    /// here; [`Engine::frame`] then projects them onto the viewport overlay
    /// alongside the selection. An empty vec clears the highlights.
    pub fn set_search_highlights(&mut self, matches: Vec<Match>) {
        self.term.set_search_highlights(matches);
    }

    /// Register a decoration marker at viewport `row`, returning its stable id
    /// (#118). The marker anchors the content currently on that row and tracks
    /// it through scroll/eviction/reflow; [`Engine::frame`] reports its viewport
    /// position while visible. Use the id to remove it or to match the
    /// `TermEvent::MarkerDisposed` fired when its line leaves the buffer.
    pub fn add_marker(&mut self, row: usize) -> MarkerId {
        self.term.add_marker(row)
    }

    /// Remove a marker by id (#118), firing `TermEvent::MarkerDisposed`. A no-op
    /// for an unknown or already-disposed id.
    pub fn remove_marker(&mut self, id: MarkerId) {
        self.term.remove_marker(id);
    }

    /// The OSC 133 shell-integration command marks in buffer order — `(id,
    /// absolute line, kind)` (#158). Excludes plain `add_marker` decorations.
    /// The consumer pairs prompt/command/finished marks to drive prompt-to-prompt
    /// navigation and command/exit announcements (#160); the engine only parses
    /// the `133;A/B/C/D` sequences and anchors the marks.
    pub fn command_marks(&self) -> Vec<(MarkerId, usize, MarkerKind)> {
        self.term.command_marks()
    }
}
