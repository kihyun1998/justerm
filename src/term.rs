//! The terminal state model: a `vte::Perform` that maps parsed VT actions onto
//! the grid, cursor, and pen. This is where the "hidden VT state" lives —
//! pending-wrap, the wide-char spacer, and the pen (BCE seam).

use unicode_width::UnicodeWidthChar;
use vte::{Params, Perform};

use crate::cell::CellFlags;
use crate::color::Color;
use crate::cursor::Cursor;
use crate::grid::Grid;

/// Owns the authoritative screen state and applies VT actions to it.
pub struct Term {
    grid: Grid,
    cursor: Cursor,
}

impl Term {
    pub fn new(cols: usize, rows: usize) -> Self {
        Term {
            grid: Grid::new(cols, rows),
            cursor: Cursor::default(),
        }
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    // ---- cursor / scroll primitives ------------------------------------------

    /// Move down one line, scrolling the screen if already at the bottom. Column
    /// is unchanged (raw LF; CR is what returns to column 0).
    fn linefeed(&mut self) {
        if self.cursor.row + 1 >= self.grid.rows() {
            self.grid.scroll_up_one();
        } else {
            self.cursor.row += 1;
        }
    }

    fn carriage_return(&mut self) {
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
    }

    /// Auto-wrap at end of line: line-feed then return to column 0.
    fn wrapline(&mut self) {
        self.linefeed();
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
    }

    // ---- printing ------------------------------------------------------------

    /// Write one glyph at the cursor, handling deferred wrap and the wide-char
    /// spacer, then advance the cursor (deferring the wrap if it hits the edge).
    fn write_glyph(&mut self, c: char, width: usize) {
        let cols = self.grid.cols();

        // Resolve a deferred last-column wrap before placing the next glyph.
        if self.cursor.pending_wrap {
            self.wrapline();
        }

        // A width-2 glyph that cannot fit in the last column wraps first.
        // TODO: xterm leaves a LEADING_WIDE_CHAR_SPACER in the vacated column;
        // common-90% just wraps and leaves it blank.
        if width == 2 && self.cursor.col + 1 >= cols {
            self.wrapline();
        }

        let (row, col) = (self.cursor.row, self.cursor.col);

        let mut cell = self.cursor.pen.cell(c);
        if width == 2 {
            cell.flags.insert(CellFlags::WIDE_CHAR);
        }
        *self.grid.cell_mut(row, col) = cell;

        // The trailing column of a wide glyph carries a distinct spacer marker.
        if width == 2 && col + 1 < cols {
            let mut spacer = self.cursor.pen.cell(' ');
            spacer.flags.insert(CellFlags::WIDE_CHAR_SPACER);
            *self.grid.cell_mut(row, col + 1) = spacer;
        }

        // Advance. Reaching/passing the last column sets pending-wrap instead of
        // wrapping eagerly — the cursor parks on the last column.
        let new_col = col + width;
        if new_col >= cols {
            self.cursor.col = cols - 1;
            self.cursor.pending_wrap = true;
        } else {
            self.cursor.col = new_col;
        }
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
        self.cursor.row = row.min(self.grid.rows() - 1);
        self.cursor.col = col.min(self.grid.cols() - 1);
        self.cursor.pending_wrap = false;
    }

    // ---- erase (CSI J / K) ---------------------------------------------------

    /// Clear cells `from..to` on `row`.
    ///
    /// TODO(#7): Background Color Erase — fill with the pen's bg, not Default.
    /// The pen is already the template; swap `reset()` for a pen-bg fill there.
    fn clear_cells(&mut self, row: usize, from: usize, to: usize) {
        for col in from..to {
            self.grid.cell_mut(row, col).reset();
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

/// First sub-parameter of CSI param `idx`, or `default` when absent or zero
/// (a zero/omitted numeric param means "1" for cursor movement and "0" for
/// erase — callers pass the right default).
fn param_or(params: &Params, idx: usize, default: u16) -> u16 {
    match params.iter().nth(idx).and_then(|p| p.first().copied()) {
        Some(v) if v != 0 => v,
        _ => default,
    }
}

impl Perform for Term {
    fn print(&mut self, c: char) {
        match c.width() {
            // Zero-width (combining marks): the grapheme-cluster side-table is a
            // later slice; drop for now rather than mis-place it as its own cell.
            Some(0) | None => {}
            Some(width) => self.write_glyph(c, width),
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            // LF, VT, FF all line-feed.
            b'\n' | 0x0b | 0x0c => self.linefeed(),
            b'\r' => self.carriage_return(),
            0x08 => {
                // Backspace.
                self.cursor.col = self.cursor.col.saturating_sub(1);
                self.cursor.pending_wrap = false;
            }
            b'\t' => {
                // Horizontal tab to the next 8-column stop. Real, settable tab
                // stops are a later slice.
                let next = ((self.cursor.col / 8) + 1) * 8;
                self.cursor.col = next.min(self.grid.cols() - 1);
                self.cursor.pending_wrap = false;
            }
            // BEL (0x07) and others: an event/notification surface is a later
            // slice; ignore for now.
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        // Private-mode sequences (intermediates such as '?', '>') — DEC modes —
        // are a later slice; ignore them rather than misinterpret.
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
            'm' => self.sgr(params),
            _ => {}
        }
    }

    // OSC, DCS (hook/put/unhook), and ESC dispatch are later slices; the default
    // no-op `Perform` impls cover them.
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}
