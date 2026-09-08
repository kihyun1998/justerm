//! The cursor and its drawing pen.

use crate::cell::{Cell, CellFlags};
use crate::color::Color;

/// The current SGR state — the appearance copied into each printed cell.
///
/// Modelling it as a "template cell" mirrors Alacritty: a later slice can make
/// erase (ED/EL) fill cleared cells with `bg` instead of `Default` and that
/// *is* Background Color Erase (BCE), no structural change. See `term.rs`.
///
/// **No `#[non_exhaustive]` (#844): nothing outside this crate has a reason to build one.** No
/// public function accepts it — the engine hands it out — and there are zero out-of-crate literal
/// sites, so the attribute would bind nothing it does not already bind.
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
///
/// **No `#[non_exhaustive]` (#844): nothing outside this crate has a reason to build one.** No
/// public function accepts it — the engine hands it out — and there are zero out-of-crate literal
/// sites, so the attribute would bind nothing it does not already bind.
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
    /// - **Armed** by the print path, when a glyph fills the last column —
    ///   `Term::write_glyph`, `Term::promote_cluster_to_wide`,
    ///   `Term::relocate_cluster_wide`. **Unconditionally, since #869**: `DECAWM` is
    ///   tested where the park is *consumed*, not where it is armed, which is what the
    ///   three references that arm this state all do. Folding the mode into the arm
    ///   made the sentence at the top false for a whole mode — under `?7l` the cursor
    ///   was pinned with the flag clear — and cost two readers a correct answer
    ///   (#865, #869) before it was found.
    /// - **Consumed**, which is not a clear — the flag is *spent* on work it owed.
    ///   Two sites, and they spend it in opposite directions: `Term::wrapline` performs
    ///   the deferred wrap and only then puts the flag down, and `Term::step_back`
    ///   under `?45` takes the park as the first unit of the move and therefore does
    ///   **not** decrement the column (#80). **`Term::step_back` is reached by two verbs
    ///   since #873** — `BS` and `CSI D`, the second n times per sequence — so a change
    ///   to that spend now moves cursor-left as well; that is the whole point of the
    ///   step being shared, and it is xterm's shape (one `CursorBack` from `CASE_BS`
    ///   and `CASE_CUB`). A consume site that cleared instead of
    ///   spending would be indistinguishable from a clear on the flag alone — the
    ///   difference shows up only in where the cursor lands, which is why both are
    ///   pinned against an unparked control at the same coordinate.
    /// - **Translated** by `Term::resize`: where a reflow leaves the cursor off the
    ///   last column the logical position becomes representable, so the flag is
    ///   dropped and `col` takes it instead. Neither an arm nor a clear.
    /// - **Cleared** by the positioning verbs, `HT` excepted — checked verb by verb
    ///   against the references and recorded in
    ///   `docs/agents/reference-facts.md`, not inferred.
    /// - **Restored** by `Term::restore_cursor` and by leaving the alt screen, each
    ///   of which then calls `Term::settle_restored_wrap`: a restored park that is
    ///   no longer at the last column becomes a column, the same translation
    ///   `Term::resize` applies to the live cursor. Without it a `DECSC` / resize /
    ///   `DECRC` round-trip installed a state the sentence at the top forbids.
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
    /// row-shift and erase verbs, which write neither field. `IL` and `DL` now clear
    /// (3-1); `SU` and `SD` deliberately do not, because ghostty saves and restores
    /// the flag across those two on purpose (`Terminal.zig:2388`); and `ICH`, `DCH`,
    /// `ECH`, `EL`, `ED` **were unmeasured until #869 and are now measured**: xterm
    /// clears in every one of them. `ResetWrap` (`ptyx.h:3253`) puts down `do_wrap`
    /// *and* `char_was_written` together, and `util.c` calls it from exactly seven
    /// sites — `InsertLine` `:1295`, `DeleteLine` `:1388`, `InsertChar` `:1497`,
    /// `DeleteChar` `:1582`, `ClearInLine2` `:1787`, `ClearRight` `:1873`,
    /// `ClearScreen` `:1926`. This engine keeps the park across all seven, and #869
    /// widened that divergence's reach from one mode to both. Not a defect on any
    /// measurement so far, but no longer an unknown. alacritty alone additionally makes
    /// `EL 0` a no-op while parked (`term/mod.rs:1643`). A grep on the cursor fields
    /// will not tell you any of that.
    ///
    /// One more site the field-grep misses: the print path itself reads
    /// `self.autowrap` before consuming, because `DECAWM` can be turned off after
    /// the flag is armed and the park must then be spent rather than wrapped.
    ///
    /// **What this flag is again a general answer to, and what it cost to get there
    /// (#865, #869).** It now answers *is the cursor parked on the glyph it just
    /// wrote* in every mode, which is simply the sentence at the top being true. It
    /// was not, for as long as the arm folded `DECAWM` in: under `?7l` a print that
    /// filled the last column pinned the cursor and armed nothing, so a pin and a bare
    /// move onto that column were identical in every field of this struct. Two readers
    /// paid for that — `Term::cursor_cluster_col`, which grew a workaround in #865 and
    /// lost it again in #869, and `term::markers`'s `+1` above, whose bound was one
    /// short under `?7l` until the arm was fixed.
    ///
    /// **So a new reader may ask this flag *which cell did the last print land in*,
    /// and the three arm sites owe that answer.** They are not free to re-introduce a
    /// condition on the arm without repairing those readers; that is the obligation
    /// the mode-gated arm carried invisibly for two releases.
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
