// The crate-level docs and the compiled usage example both live in README.md,
// pulled in here so the published crates.io front page and this doctest are one
// source: the `cargo test` doc pass compiles the README's `rust` block, so the
// published usage snippet cannot drift from the real API (#483, and the #473
// rule that a shipped usage snippet must compile against the real types).
#![doc = include_str!("../README.md")]

mod base64;
mod cell;
mod color;
mod cursor;
mod damage;
mod event;
mod grapheme;
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
pub use event::{ClipboardTarget, TermEvent};
pub use grid::{Grid, Row};
pub use input::{
    Key, KeyAction, KeyEvent, KeypadKey, Modifiers, MouseAction, MouseButton, MouseEvent,
    MouseEvents,
};
pub use logical::LogicalLine;
pub use search::{Match, SearchOptions, is_valid_regex};
pub use selection::{SelectionSpan, SelectionType, Side};
pub use serialize::{
    CELL_RECORD_LEN, DecodeError, Frame, FrameKind, MarkerId, MarkerKind, MarkerPosition, Overlay,
    Span, WIRE_VERSION, decode, encode, encode_cell_record, encode_color,
};

pub use term::{
    CommandLine, DEFAULT_WORD_SEPARATORS, Hyperlink, MAX_COLUMNS, MAX_COMMAND_TEXT, MAX_MARKERS,
    MAX_ROWS, MIN_COLUMNS, MarkerEntry, MarkerIndex, Term, TrackedId,
};

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
    ///
    /// `cols` is widened to [`MIN_COLUMNS`] — a narrower screen cannot represent a
    /// width-2 glyph, so the engine clamps rather than accepting a size it would
    /// have three different answers for (#547).
    pub fn new(cols: usize, rows: usize) -> Self {
        Engine {
            parser: Parser::new(),
            term: Term::new(cols, rows),
        }
    }

    /// Like [`Engine::new`] but with an explicit scrollback line limit. `cols` is
    /// clamped to [`MIN_COLUMNS`] the same way.
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
    /// scrollback; the whole screen is damaged.
    ///
    /// **The primary screen reflows; the alternate screen does not (#567).** On the
    /// primary, soft-wrapped logical lines are re-split at the new width — scrollback
    /// included, since it is one buffer with the screen — so a long line keeps its tail
    /// instead of being truncated. Reflow is *not* gated on DECAWM: the wrap flag records
    /// that a row continues into the next one, which stays true after a re-split, and
    /// re-reading a momentary mode at resize time would decide the fate of history written
    /// under the opposite setting. The alt screen is re-fit only — rows are dropped or added
    /// to reach the new size and nothing re-wraps, because a full-screen application places
    /// its own lines and re-wrapping them would change what it drew.
    ///
    /// **What a consumer must redo afterwards.** Query-derived state is *invalidated* and
    /// user-authored state is *re-anchored*: search highlights are dropped (re-run the
    /// search at the new width — a reflow moves match coordinates and can change the match
    /// set), while the selection is carried to its new coordinates for you.
    ///
    /// `cols` is widened to [`MIN_COLUMNS`] **silently**: a `resize(1, rows)` during
    /// a pane drag yields a two-column screen with no error. Read the resulting
    /// width back from [`Engine::grid`] or the frame header rather than assuming the
    /// value passed here, and size the PTY from that same width (#547).
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

    /// The OSC 8 hyperlink **URI** at **screen** `(row, col)` — the live grid, same
    /// coordinates as [`Engine::grid`]'s `cell(row, col)` — or `None` if that cell
    /// carries no declared link.
    ///
    /// **One call, not two, since #628.** This returned a `NonZeroU32` index that a
    /// second method resolved against a buffer-wide pool; the pool is gone (it was never
    /// reclaimed, and nothing interned across opens that a shared `Arc` does not), so
    /// there is no index left to hand out.
    ///
    /// **Owned, not borrowed** — a `&str` into the row's map would be tied to `&Engine`,
    /// so a hover handler could not keep it across the next [`Engine::feed`]. Measured:
    /// the borrow reads at 0.75 ns but cannot be held at all, and the caller's workaround
    /// (copying the string) costs 62.6 ns against this handle's 17.9 ns. See
    /// [`Hyperlink`].
    ///
    /// Do **not** confuse this with a decoded `Span`'s `links`, which is a *frame-local*
    /// index into that frame's `link_table` and belongs to the wire, not to the engine.
    /// The old two-call form invited exactly that mix-up and its doc-comment recommended
    /// it: the two index spaces coincide only when a frame carries a single link.
    pub fn link_at(&self, row: usize, col: usize) -> Option<Hyperlink> {
        self.term.screen_link_at(row, col)
    }

    /// The underline colour (SGR 58, #520) at **screen** `(row, col)` — same
    /// coordinates as [`Engine::grid`]'s `cell(row, col)`. A theme-agnostic
    /// [`Color`] reference; [`Color::Default`] means the underline follows the
    /// glyph's foreground (the common case, and what a cell with no SGR 58 returns).
    /// Like the hyperlink, the colour rides a per-row side table, not the 12-byte
    /// [`Cell`] (#520).
    pub fn underline_color_at(&self, row: usize, col: usize) -> Color {
        self.term.screen_underline_color_at(row, col)
    }

    /// The OSC 8 hyperlink **URI** at **viewport** `(row, col)` — the visible window
    /// including scrollback at the current scroll, same coordinates as
    /// [`Engine::viewport_line`] — or `None`. Mirror of [`Engine::link_at`], including
    /// its #628 note about the vanished index.
    pub fn viewport_link_at(&self, row: usize, col: usize) -> Option<Hyperlink> {
        self.term.viewport_link_at(row, col)
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

    /// Answer an OSC 12 `QueryCursorColor` event (#832): queue the OSC 12 reply
    /// from the consumer-supplied spec. Theme-agnostic envelope-only, like its
    /// foreground and background siblings.
    pub fn report_cursor_color(&mut self, spec: &str) {
        self.term.report_cursor_color(spec);
    }

    /// Answer an OSC 4 `QueryPaletteColor` event (#122): queue the OSC 4 reply for
    /// `index` from the consumer-supplied spec. Theme-agnostic envelope-only.
    pub fn report_palette_color(&mut self, index: u8, spec: &str) {
        self.term.report_palette_color(index, spec);
    }

    /// Answer an OSC 52 [`TermEvent::QueryClipboard`] event (#828): base64-encode
    /// the consumer's clipboard text into the OSC 52 reply envelope for
    /// [`Engine::drain_replies`].
    ///
    /// The engine holds no clipboard — the text comes from the consumer, which
    /// owns it along with every policy about it. **Not calling this is how a read
    /// is refused**, independently of whether stores are honoured, and nothing is
    /// queued until you do.
    pub fn report_clipboard(&mut self, target: ClipboardTarget, text: &str) {
        self.term.report_clipboard(target, text);
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
    ///
    /// **`count` is capped at the scroll region's own height (#661).** Repeated
    /// scrolls of one region accumulate into a single op between acks, and a flood
    /// accumulates far past the region: 32 KB of newlines in one [`Engine::feed`] is
    /// enough. Shifting a region by more than its height already moves every source
    /// row outside it, so the surplus names nothing a consumer can act on — while it
    /// did overflow the `i16` this value rides on the wire and arrive as a scroll in
    /// the *opposite* direction. Suppressed entirely while the viewport is scrolled
    /// up, since a content scroll must not shift a frozen view.
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

    /// Replace the characters that end a word for [`SelectionType::Word`] — consumer
    /// policy injected into a core mechanism (ADR-0017). Defaults to
    /// [`DEFAULT_WORD_SEPARATORS`]. `' '` is forced in; see [`Term::set_word_separators`]
    /// for why that floor is load-bearing rather than defensive.
    pub fn set_word_separators(&mut self, separators: &str) {
        self.term.set_word_separators(separators);
    }

    /// The word-boundary set currently in force (including the forced `' '`).
    pub fn word_separators(&self) -> &str {
        self.term.word_separators()
    }

    /// Clear the selection.
    pub fn selection_clear(&mut self) {
        self.term.selection_clear();
    }

    /// The selection projected onto the viewport: one inclusive-column span per
    /// visible row, for the renderer to highlight. Empty when nothing is
    /// selected or the selection is fully scrolled off-screen.
    ///
    /// **A span never ends inside a wide glyph** (#454). An endpoint landing on
    /// half of a width-2 pair takes the whole pair, so a highlight cannot split
    /// a CJK glyph down the middle — which also means a span may be one column
    /// wider than the columns the caller's gesture named. On a `Block`
    /// selection that widening is per row, so the rectangle's rows can differ
    /// in width. [`selection_text`](Self::selection_text) widens identically;
    /// the two never disagree.
    pub fn selection_range(&self) -> Vec<SelectionSpan> {
        self.term.selection_range()
    }

    /// The selected text for copy (respects scrollback), or `None` if no
    /// selection.
    ///
    /// Widened onto whole wide-glyph pairs exactly as
    /// [`selection_range`](Self::selection_range) is (#454) — a spacer extracts
    /// as nothing, so a range ending inside a pair would copy text the
    /// highlight does not show.
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

    /// Search with explicit [`SearchOptions`] — regex, whole-word, and a case-sensitivity override
    /// beyond the literal + smart-case [`search`](Self::search) (#314).
    pub fn search_with(&self, query: &str, opts: SearchOptions) -> Vec<Match> {
        self.term.search_with(query, opts)
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
    ///
    /// **This is the document [`CommandLine::line`] indexes, which makes that last
    /// sentence a pairing obligation rather than a detail (#743):** ask both in the
    /// same breath and keep them together, because a document line is meaningless
    /// against a document sampled at another instant — and while the alt screen is up
    /// the two queries are about different buffers entirely. See
    /// [`Engine::command_lines`].
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
    ///
    /// **A span never ends inside a wide glyph** (#454), the same widening
    /// [`selection_range`](Self::selection_range) applies. It matters more here,
    /// because a `Match` may be one the *caller* assembled: an out-of-range
    /// column is bounded onto the row's last cell, which is a trailing spacer
    /// whenever the row ends in a wide glyph — so without the widening a
    /// highlight could be that glyph's right half alone.
    pub fn match_spans(&self, m: &Match) -> Vec<SelectionSpan> {
        self.term.match_spans(m)
    }

    /// Set the search highlights the frame should carry (#108). The
    /// consumer owns match navigation, so it hands the set to highlight back
    /// here; [`Engine::frame`] then projects them onto the viewport overlay
    /// alongside the selection. An empty vec clears the highlights.
    pub fn set_search_highlights(&mut self, matches: Vec<Match>) {
        self.term.set_search_highlights(matches);
    }

    /// Designate which member of the held highlight set is the *active* match
    /// (#428) — the one next/prev navigation currently points at (that choice is
    /// the consumer's policy). [`Engine::frame`] projects it into the overlay's
    /// `active_match` group; it also stays in `matches`, and the renderer's
    /// highlight ranking resolves the overlap (#424). `None` or an out-of-range
    /// index projects nothing. Passing a new set to
    /// [`set_search_highlights`](Self::set_search_highlights) resets the
    /// designation, so re-designate after every hand-over.
    pub fn set_active_search_highlight(&mut self, index: Option<usize>) {
        self.term.set_active_search_highlight(index);
    }

    /// Designate the *active* match by its absolute span (#436), independent of
    /// the held highlight set — the past-cap path. A backend that caps its
    /// hand-over (the documented 1000, xterm's `highlightLimit`) can still give
    /// the current match its active emphasis: xterm builds its active
    /// decoration from the found result *outside* the capped list, and this is
    /// that model. The span projects through the same wrap-aware viewport math
    /// as any match; past the cap it paints the ACTIVE colour only (no plain
    /// highlight underneath — honest about the cap). `None` clears. Same
    /// lifecycle as the index form: reset on every
    /// [`set_search_highlights`](Self::set_search_highlights) hand-over and on
    /// any coordinate-shifting invalidation (eviction, region scroll, reflow,
    /// alt-screen swaps), so re-designate after each hand-over.
    pub fn set_active_search_match(&mut self, m: Option<Match>) {
        self.term.set_active_search_match(m);
    }

    /// Register a decoration marker at viewport `row`, returning its stable id
    /// (#118). The marker anchors the content currently on that row and tracks
    /// it through scroll/eviction/reflow; [`Engine::frame`] reports its viewport
    /// position while visible. Use the id to remove it or to match the
    /// `TermEvent::MarkerDisposed` fired when its line leaves the buffer.
    ///
    /// A buffer holds at most [`MAX_MARKERS`] live markers (#721) — the population is
    /// also grown by the *stream*, through OSC 133 command marks, so it is bounded.
    /// Past the cap the **oldest** marker is retired and announced through the same
    /// `MarkerDisposed` event, so a consumer that already handles disposal needs no new
    /// handling; a consumer that ignores it can leave a decoration bound to a dead id.
    pub fn add_marker(&mut self, row: usize) -> MarkerId {
        self.term.add_marker(row)
    }

    /// Remove a marker by id (#118), firing `TermEvent::MarkerDisposed`. A no-op
    /// for an unknown or already-disposed id.
    pub fn remove_marker(&mut self, id: MarkerId) {
        self.term.remove_marker(id);
    }

    /// Track absolute buffer `(line, col)`, returning a stable id (#691): the
    /// engine keeps the position on the content that is there now, through
    /// scrollback eviction, region scrolls and reflow.
    ///
    /// This is what an absolute coordinate held *outside* the engine needs to stay
    /// meaningful — a search anchor carrying an emphasis across a re-search is the
    /// case it exists for. The engine renumbers this space (evicting the oldest
    /// history line shifts every index down by one), and it renumbers it in the
    /// consumer's absence, so a remembered `Match` silently comes to name
    /// different text.
    ///
    /// Mechanism only: which position is worth remembering, and what to do once it
    /// is gone, stay with the consumer (ADR-0017). Release it with
    /// [`Engine::untrack_point`] — the engine cannot know when you are done.
    ///
    /// **The line is maintained; the column is carried, not tracked.** In-row edits
    /// (ICH / DCH) shift cells past a tracked column without moving it, so a point
    /// on text that was pushed sideways names the wrong cell in that row. No
    /// reference maintains a column here either — xterm's markers carry none at
    /// all, and ghostty's pins are untouched by its `insertChars`/`deleteChars` —
    /// so this is the convergent behaviour rather than an omission.
    pub fn track_point(&mut self, line: usize, col: usize) -> TrackedId {
        self.term.track_point(line, col)
    }

    /// Where the point registered as `id` sits now, in the **active** screen's
    /// coordinates — or `None` (#691).
    ///
    /// `None` covers three cases, and a caller does not need to tell them apart:
    /// the content has left the buffer, the id is unknown or released, or the point
    /// belongs to *the other screen*. The last one is not a limitation but the only
    /// honest answer: the primary grid and the alt grid occupy the **same** absolute
    /// indices, so a number alone cannot say which screen it means. All three say
    /// *do not move anything on account of this point*.
    ///
    /// An out-of-range coordinate is clamped rather than rejected, at both ends
    /// (ADR-0026 D2/D3): the line into the buffer's range, the column to the grid
    /// width. That bound is applied here, at the read; a coordinate that was never
    /// in range to begin with is also **resolved by a reflow** (it maps to the top
    /// of the buffer), so "bounded once" holds for the site, not for the value.
    pub fn tracked_point(&self, id: TrackedId) -> Option<(usize, usize)> {
        self.term.tracked_point(id)
    }

    /// Release a tracked point (#691). A no-op for an unknown or already-released
    /// id.
    pub fn untrack_point(&mut self, id: TrackedId) {
        self.term.untrack_point(id);
    }

    /// The OSC 133 shell-integration command marks in buffer order — `(id,
    /// absolute line, kind)` (#158). Excludes plain `add_marker` decorations.
    /// The consumer pairs prompt/command/finished marks to drive prompt-to-prompt
    /// navigation and command/exit announcements (#160); the engine only parses
    /// the `133;A/B/C/D` sequences and anchors the marks.
    ///
    /// **The answer is instantaneous — it describes the buffer it was asked of, and
    /// nothing on it dates it (#742). Re-ask; never keep it and never rebase it.**
    /// The lines move on *both* of the axes [`MarkerIndex`] carries a scalar for:
    /// scrollback eviction shifts every mark by the same amount, and a top-anchored
    /// `DECSTBM` region shifts the marks below its margin once per output line — the
    /// second inside a single [`Engine::feed`], with no resize anywhere.
    ///
    /// **Why this is not shaped like its sibling.** [`Engine::marker_index`] carries a
    /// basis and an epoch because a consumer *must* hold its answer: it feeds an
    /// overview ruler that has to be current in every frame, and re-pulling per frame
    /// is the `O(M)`-per-frame payload ADR-0020 R3 exists to forbid. This query is
    /// consumed when a user acts, so re-asking **is** the natural act — and here a
    /// re-ask always answers, because this population's frame of reference never
    /// changes. That is the property the sibling lacks, and the reason it needed the
    /// epoch rather than a reason this one does: an alt switch is one of the four
    /// moves that epoch announces.
    ///
    /// **The lines are `[scrollback ++ primary]`, always — including while the alt
    /// screen is up.** They do not name the *active* buffer. The two buffers occupy the
    /// same absolute indices, so one integer from here and the same integer from
    /// [`Engine::marker_index`] name different content, and neither tuple nor struct
    /// says which. [`Engine::tracked_point`] meets that ambiguity and answers `None`
    /// rather than a number (ADR-0026 D2/D3); it can, because it is asked about *one*
    /// point of unknown origin. This query enumerates a population whose screen is
    /// fixed by definition, so it answers — and states the screen here instead.
    ///
    /// Consequently an empty answer means every mark was disposed and can mean nothing
    /// else, where `marker_index`'s silence is ambiguous between that and *"you are on
    /// the other screen"*.
    ///
    /// **A mark also dies when a whole row is blanked where it stands (#750).** Until
    /// then the only deaths were the buffer *moving* — eviction, a region rotate, a
    /// reflow — and a `clear` left every mark on the screen alive over blank rows. `ED`
    /// now retires the marks on each whole row it blanks, through the same
    /// `TermEvent::MarkerDisposed` a consumer already handles, so this query going empty
    /// after a `clear` is the ordinary meaning above and not a new one. **`EL` and `ECH`
    /// deliberately do not**, whatever they blank: a line editor redraws its input line
    /// with `\r ESC[K` on every keystroke, and the `CommandStart` of the command being
    /// typed is on that row.
    pub fn command_marks(&self) -> Vec<(MarkerId, usize, MarkerKind)> {
        self.term.command_marks()
    }

    /// The executed shell commands recovered from OSC-133 marks, in buffer order
    /// (#166) — the query behind screen-reader command navigation. Each
    /// [`CommandLine`] carries the typed command text (prompt/output excluded via
    /// the captured columns), its jump line (CommandStart), and the exit code.
    /// This is a full-buffer query, wired to the frame-mode consumer over IPC like
    /// [`Engine::accessible_text`]; the web side has no scrollback cells to derive
    /// it (ADR-0017 — buffer-wide text is core's).
    ///
    /// **The text and the exit are frozen when the stream reveals them; only the line
    /// is derived (#750).** [`CommandLine::command`] is captured at the `133;C` that
    /// closes the command — the instant it is complete and on screen — and
    /// [`CommandLine::exit`] is written down when `133;D` is parsed. Neither is
    /// recoverable afterwards: re-reading the text through the recorded columns names
    /// whatever *now* occupies those cells, which a plain overwrite, `ICH`, `DCH` and an
    /// erase all arrange, and an exit code is in no cell at any time. A capture is
    /// bounded at [`MAX_COMMAND_TEXT`] `char`s, truncated at a `char` boundary, for the
    /// reason [`MAX_MARKERS`] exists: the stream chooses the distance between `B` and
    /// `C`. [`CommandLine::line`] stays derived, because it is the half the anchor
    /// fixups already maintain.
    ///
    /// **The answer is instantaneous — it describes the buffer it was asked of, and
    /// nothing on it dates it (#743). Re-ask; never keep it past the document it
    /// indexes, and never rebase it.** Same discharge as [`Engine::command_marks`] and
    /// for the same two reasons (ADR-0029 D3): the clock is a user action, so the ask
    /// *is* the act; and this population's frame of reference never flips, so a re-ask
    /// always answers. Absence means the command is gone **or** that its output has not
    /// started yet — both of which the next ask resolves. What absence never means is
    /// *"you are on the other screen"*, which is the meaning no re-ask could undo.
    ///
    /// **Do not rebase by [`MarkerIndex::evicted_total`].** That dates the *absolute*
    /// space. [`CommandLine::line`] is a **document** line, and the two spaces move
    /// apart in both directions: an eviction that pops a soft-wrap continuation row
    /// moves the absolute lines and not this one, and flipping a row's wrap bit — which
    /// ordinary output does — moves this one while the absolute lines and both of
    /// `MarkerIndex`'s scalars stay put. They agree most of the time, which is what
    /// makes rebasing look correct right up until it silently is not.
    ///
    /// **Ask [`Engine::accessible_text`] in the same breath, and only on the primary
    /// screen.** The lines index that document; while the alt screen is up it returns
    /// the *alt* document instead, and these lines are indices into the primary one. If
    /// the alt screen is taller than the held index — a full-screen TUI, which is the
    /// normal case — the index still **resolves**, onto unrelated content, so a bounds
    /// check does not save a caller here. The query keeps answering on the alt screen
    /// deliberately: emptying it would give absence the one meaning a re-ask cannot
    /// recover from, which is what the discharge above rests on. Pairing the two is the
    /// caller's, and this is where it is said.
    pub fn command_lines(&self) -> Vec<CommandLine> {
        self.term.command_lines()
    }

    /// Every live marker of the active buffer with its **absolute** buffer line, plus
    /// the basis that says how long the answer stays usable (#490).
    ///
    /// The pull half of the marker surface. It shares [`Engine::command_lines`]'s
    /// *shape* — the consumer asks once and keeps the answer, rather than being handed
    /// every live marker inside every frame, which is `O(M)` payload per frame for a
    /// quantity unrelated to what changed (ADR-0020 R3). It does **not** share its
    /// coordinate: only the lines *here* are buffer-absolute and rebasable by the
    /// `evicted_total` delta. [`CommandLine::line`] is a **document** line over
    /// [`Engine::accessible_text`], where soft-wrapped rows collapse — eviction moves it
    /// by an amount no scalar on this surface expresses, so it is an answer to keep only
    /// as long as the buffer it was asked of.
    ///
    /// Ask again when [`MarkerIndex::epoch`] differs from the one you hold. Drop an
    /// entry when its `TermEvent::MarkerDisposed` arrives, and append one when
    /// `TermEvent::MarkerCreated` does — neither deliberately moves the epoch, so
    /// neither costs a re-pull. **Append it on the instant the event carries, not on the
    /// newest frame's**: a `feed` can create a marker and then evict, and those are two
    /// different origins (#737).
    ///
    /// **Adopt a birth only into the generation it names (#741).** The event carries this
    /// pull's whole triple — line, basis, [`MarkerIndex::epoch`] — because the basis dates
    /// only a *uniform* move. A reflow or a region rotate moves markers individually, so a
    /// line dated to the generation before one is not stale by a delta; it is an answer
    /// about a different buffer, and the re-pull the epoch already forces is what supplies
    /// the marker instead. Compare generations for **equality**: the counter wraps.
    ///
    /// **Draining before you read the frame is then a cost preference, not a correctness
    /// one.** Reading the frame first leaves `marker_count` one ahead of an index that has
    /// not been told yet, so a consumer comparing the two spends an `O(M)` re-pull
    /// reconciling a fact this event delivered at `O(1)`. Placement does not depend on the
    /// order, on either axis.
    pub fn marker_index(&self) -> MarkerIndex {
        self.term.marker_index()
    }
}
