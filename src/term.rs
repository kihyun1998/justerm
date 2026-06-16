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
    /// Scroll region top/bottom margins (DECSTBM), 0-based inclusive. A
    /// line-feed at `scroll_bottom` scrolls only rows `[scroll_top..=scroll_bottom]`.
    /// Default = the full screen.
    scroll_top: usize,
    scroll_bottom: usize,
}

impl Term {
    pub fn new(cols: usize, rows: usize) -> Self {
        Term {
            grid: Grid::new(cols, rows),
            alt_grid: Grid::new(cols, rows),
            cursor: Cursor::default(),
            saved_cursor: Cursor::default(),
            on_alt: false,
            tabs: default_tabs(cols),
            scroll_top: 0,
            scroll_bottom: rows - 1,
        }
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    // ---- cursor / scroll primitives ------------------------------------------

    /// Move down one line. At the bottom margin, scroll the region instead;
    /// below the region, just descend (no scroll). Column is unchanged (raw LF;
    /// CR is what returns to column 0).
    fn linefeed(&mut self) {
        if self.cursor.row == self.scroll_bottom {
            self.grid.scroll_up_region(self.scroll_top, self.scroll_bottom);
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
    fn enter_alt_screen(&mut self) {
        if self.on_alt {
            return;
        }
        self.saved_cursor = self.cursor;
        std::mem::swap(&mut self.grid, &mut self.alt_grid);
        self.grid.clear();
        self.on_alt = true;
    }

    /// Leave the alternate screen: swap the primary grid back in and restore the
    /// saved cursor.
    fn leave_alt_screen(&mut self) {
        if !self.on_alt {
            return;
        }
        std::mem::swap(&mut self.grid, &mut self.alt_grid);
        self.cursor = self.saved_cursor;
        self.on_alt = false;
    }

    /// RI (ESC M): move up one line. At the top margin, scroll the region down
    /// instead.
    fn reverse_index(&mut self) {
        if self.cursor.row == self.scroll_top {
            self.grid.scroll_down_region(self.scroll_top, self.scroll_bottom);
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
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
    /// Background Color Erase (BCE): erased cells carry the current SGR
    /// background only — fg and text attributes reset to default (matches
    /// xterm/alacritty, where the fill is `cursor.template.bg.into()`).
    fn clear_cells(&mut self, row: usize, from: usize, to: usize) {
        let bg = self.cursor.pen.bg;
        for col in from..to {
            let cell = self.grid.cell_mut(row, col);
            cell.reset();
            cell.bg = bg;
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
            b'\t' => self.put_tab(),
            // BEL (0x07) and others: an event/notification surface is a later
            // slice; ignore for now.
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        // DEC private modes arrive with a '?' intermediate.
        if intermediates.first() == Some(&b'?') {
            let mode = param_or(params, 0, 0);
            match (action, mode) {
                ('h', 1049) => self.enter_alt_screen(),
                ('l', 1049) => self.leave_alt_screen(),
                _ => {} // other DEC modes are later slices
            }
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
            'g' => self.clear_tab_stop(param_or(params, 0, 0)),
            'r' => {
                let rows = self.grid.rows() as u16;
                let top = param_or(params, 0, 1) as usize;
                let bottom = param_or(params, 1, rows) as usize;
                self.set_scroll_region(top, bottom);
            }
            'm' => self.sgr(params),
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        if !intermediates.is_empty() {
            return;
        }
        match byte {
            b'D' => self.linefeed(),       // IND (line-feed without CR)
            b'H' => self.set_tab_stop(),   // HTS
            b'M' => self.reverse_index(),  // RI
            _ => {}
        }
    }
}
