//! justerm — a pure terminal engine.
//!
//! Feed VT bytes in; read terminal state out. The engine does no I/O, no IPC,
//! no rendering, and is theme-agnostic (it stores colour *references*, never
//! hex). See `CLAUDE.md` for the boundary invariants and `docs/architecture.md`
//! for the full contract.
//!
//! ```
//! use justerm::{Color, Engine};
//!
//! let mut term = Engine::new(80, 24);
//! term.feed(b"\x1b[31mhi\x1b[0m");
//! assert_eq!(term.grid().cell(0, 0).c, 'h');
//! assert_eq!(term.grid().cell(0, 0).fg, Color::Indexed(1));
//! ```

mod cell;
mod color;
mod cursor;
mod damage;
mod event;
mod grid;
mod input;
mod search;
mod selection;
mod serialize;
mod term;

pub use cell::{Cell, CellFlags};
pub use color::Color;
pub use cursor::{Cursor, Pen};
pub use damage::{LineDamage, ScrollOp, TermDamage};
pub use event::TermEvent;
pub use grid::{Grid, Row};
pub use input::{Key, KeyEvent, Modifiers, MouseAction, MouseButton, MouseEvent};
pub use search::Match;
pub use selection::{SelectionSpan, SelectionType, Side};
pub use serialize::{DecodeError, Frame, FrameKind, Span, decode, encode};

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

    /// Number of lines currently held in scrollback history.
    pub fn scrollback_len(&self) -> usize {
        self.term.scrollback_len()
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
}
