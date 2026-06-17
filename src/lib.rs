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
mod grid;
mod term;

pub use cell::{Cell, CellFlags};
pub use color::Color;
pub use cursor::{Cursor, Pen};
pub use damage::{LineDamage, ScrollOp, TermDamage};
pub use grid::{Grid, Row};
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

    /// Number of lines currently held in scrollback history.
    pub fn scrollback_len(&self) -> usize {
        self.term.scrollback_len()
    }

    /// What changed since the last [`Engine::reset_damage`] — line ranges each
    /// with a changed column span (see ADR-0003).
    pub fn damage(&self) -> TermDamage {
        self.term.damage()
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
}
