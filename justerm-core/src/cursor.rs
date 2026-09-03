//! The cursor and its drawing pen.

use crate::cell::{Cell, CellFlags};
use crate::color::Color;

/// The current SGR state — the appearance copied into each printed cell.
///
/// Modelling it as a "template cell" mirrors Alacritty: a later slice can make
/// erase (ED/EL) fill cleared cells with `bg` instead of `Default` and that
/// *is* Background Color Erase (BCE), no structural change. See `term.rs`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pen {
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
    /// The underline colour (SGR 58, #520): what an underline / strikethrough draws
    /// in, independent of `fg`. `Default` means "follow the fg". It is *not* packed
    /// into the printed `Cell` (the 12-byte cell is full); the print path stamps a
    /// non-default value into the row's ucolor map. See `term.rs::write_glyph`.
    pub underline_color: Color,
}

impl Pen {
    /// Reset to default appearance (SGR 0).
    pub fn reset(&mut self) {
        *self = Pen::default();
    }

    /// Build a cell carrying this pen's appearance and the given glyph.
    pub fn cell(&self, c: char) -> Cell {
        Cell::from_parts(c, self.fg, self.bg, self.flags)
    }
}

/// The cursor's drawn shape (DECSCUSR / the renderer's caret glyph). The engine
/// reports it on the frame; the renderer draws it. Default `Block` (#81).
///
/// **Deliberately exhaustive (#843) — and the reason is the wire, not the spec.**
///
/// An earlier draft of that sweep said "DECSCUSR's shape space, closed". **That is
/// false**, and the counter-example is in this repository: `justerm-renderer` has
/// carried a fourth shape, `HollowBlock`, since before the sweep
/// (`justerm-renderer/src/cursor.rs:60`, wire id `3`), and no core frame can ask
/// for it. The space is not closed by the spec; it has already been grown once,
/// one crate over.
///
/// What actually decides it is that **this enum is mapped onto wire values by a
/// `match` outside this crate** — `justerm-wasm-decode/src/lib.rs:198` turns each
/// member into an int for the frame header. Marking it non-exhaustive would force
/// a `_` arm there, converting a future compile error into a silently wrong wire
/// value. That is the same construct that reddened `cargo test --workspace` for
/// [`crate::MarkerKind`], where the rule is stated in full.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Bar,
}

/// The input position, its pending-wrap state, and the current pen.
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// Deferred last-column wrap (xterm's "wrapnext"). Set when a print fills the
    /// last column: the cursor stays put and the actual line wrap happens on the
    /// *next* print. Eager wrapping here is the classic off-by-one that shifts
    /// lines (see `docs/architecture.md` "Hidden VT state").
    ///
    /// # The lifecycle, and why it is written here (#848)
    ///
    /// **What the flag means:** *the cursor is logically one past the column it
    /// sits on.* That sentence is what every site below is measured against — but
    /// read the next paragraph before treating it as a rule you can derive a new
    /// verb's behaviour from, because you cannot.
    ///
    /// **The clear is per-verb and is not derivable.** The first draft of this
    /// comment said a verb clears iff it *acted*, with `HT` at the last column as
    /// the one exception because it moves nothing. That predicate is false, and
    /// the counter-example is one verb over: `CUF` at the last column also moves
    /// nothing, also destroys the character that was there — and **all four
    /// references clear anyway**, three of them unconditionally and one before it
    /// has even computed the clamp (xterm `cursor.c:243`, alacritty
    /// `term/mod.rs:1241`, ghostty `Terminal.zig:1739` under *"Always resets
    /// pending wrap"*, xterm.js `InputHandler.ts:919` via `_restrictCursor`). So a
    /// derived predicate would instruct the next author to *remove* a clear that
    /// four engines agree on. What separates `HT` is not a property of the verb; it
    /// is that on `HT` the references agree the other way, 3-1 (#848).
    ///
    /// The site-classes, which are what this comment can honestly enumerate:
    ///
    /// - **Armed** by the print path, when a glyph fills the last column and
    ///   `DECAWM` is on — `Term::write_glyph`, `Term::promote_cluster_to_wide`,
    ///   `Term::relocate_cluster_wide`.
    /// - **Consumed** by the wrap machinery, which is not a clear: `Term::wrapline`
    ///   performs the deferred wrap and only then puts the flag down.
    /// - **Translated** by `Term::resize`: where a reflow leaves the cursor off the
    ///   last column the logical position becomes representable, so the flag is
    ///   dropped and `col` takes it instead. Neither an arm nor a clear.
    /// - **Cleared** by the positioning verbs, `HT` excepted — checked verb by verb
    ///   against the references and recorded in
    ///   `docs/agents/reference-facts.md`, not inferred.
    /// - **Restored** by `Term::restore_cursor` and by leaving the alt screen. This
    ///   one is *not* sound: neither saved slot is repaired on a resize, so a
    ///   `DECSC` / resize / `DECRC` round-trip can install the flag at a column that
    ///   is not the last one — a state the sentence at the top forbids. ghostty
    ///   applies its repair to the saved cursor for exactly this reason
    ///   (`Screen.zig:2094`). Pre-existing and out of #848's scope; tracked.
    /// - **Read as a `+1`** by `term::markers`, which adds the flag to `cursor.col`
    ///   to get an exclusive bound. A change to when the flag survives changes that
    ///   bound — measured for `HT` at the right edge and the recorded column does
    ///   move (3 where it was 2 at four columns), but **no public output changed**:
    ///   the extracted command text is identical either way, because the run that
    ///   cleared the flag also let the next print overwrite the last cell, and the
    ///   two shifts cancel. Recorded so the next change here starts from a
    ///   measurement rather than from the assumption that a reader exists but does
    ///   not matter. The column itself is not observable through any public API.
    ///
    /// **What the obvious check does not reach.** Grepping this crate for writes to
    /// `cursor.col` / `cursor.row` finds **20** functions — and it is blind to the
    /// row-shift and erase verbs, which write neither field: `SU`, `SD`, `IL`,
    /// `DL`, `ICH`, `DCH`, `ECH`, `EL`, `ED` all leave the flag exactly as they
    /// found it. That is deliberate for `SU`/`SD`, where ghostty saves and restores
    /// it across the scroll on purpose (`Terminal.zig:2390`), and unsettled for
    /// `IL`/`DL`, where xterm and ghostty both clear and this engine does not.
    ///
    /// The rule is stated at the property because that is where it is true, the
    /// same reason ADR-0025 D2 gives for the wrap link's per-verb table living in
    /// `Term::end_wrap`'s doc-comment.
    pub pending_wrap: bool,
    pub pen: Pen,
    /// Whether the cursor is shown (DEC ?25). The engine only reports it.
    pub visible: bool,
    /// The caret shape (DECSCUSR, #89) — reported on the frame, drawn by the
    /// renderer.
    pub shape: CursorShape,
    /// Whether the caret blinks (att610 ?12, #81). The engine reports the *mode*;
    /// the actual animation is the renderer's.
    pub blink: bool,
}

impl Cursor {
    /// The cursor's `(row, col)` position.
    pub(crate) fn point(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// Set the position, clamped to a `rows` x `cols` screen.
    pub(crate) fn set_point(&mut self, point: (usize, usize), rows: usize, cols: usize) {
        self.row = point.0.min(rows - 1);
        self.col = point.1.min(cols - 1);
    }
}

impl Default for Cursor {
    fn default() -> Self {
        // The cursor starts visible; a manual impl is needed because `bool`'s
        // derived default is `false`.
        Cursor {
            row: 0,
            col: 0,
            pending_wrap: false,
            pen: Pen::default(),
            visible: true,
            shape: CursorShape::Block,
            blink: false,
        }
    }
}
