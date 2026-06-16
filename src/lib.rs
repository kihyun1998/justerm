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
mod grid;
mod term;

pub use cell::{Cell, CellFlags};
pub use color::Color;
pub use cursor::{Cursor, Pen};
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
    /// A blank engine with a `cols` × `rows` screen.
    pub fn new(cols: usize, rows: usize) -> Self {
        Engine {
            parser: Parser::new(),
            term: Term::new(cols, rows),
        }
    }

    /// Push a slice of VT bytes. The caller owns the PTY/SSH/socket I/O — the
    /// engine only consumes the bytes it is handed.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// The current screen grid.
    pub fn grid(&self) -> &Grid {
        self.term.grid()
    }

    /// The current cursor (position, pending-wrap, pen).
    pub fn cursor(&self) -> &Cursor {
        self.term.cursor()
    }
}
