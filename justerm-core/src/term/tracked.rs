//! Tracked points: absolute buffer positions the engine keeps on their **content**
//! for a holder that lives outside it, and the fixups that do the keeping.
//!
//! The forcing case is a search anchor (#691). A consumer that carries an emphasis
//! across a re-search has to remember *where* the user was, and the only name it can
//! use is an absolute `[scrollback ++ screen]` coordinate — which the write path
//! renumbers, at four separate sites and with two different signs. The selection and
//! the markers each carry a fixup per site for exactly that reason; a coordinate held
//! across the boundary carried none, so it named different text after every eviction.
//!
//! **This is mechanism, not policy** (ADR-0017). The engine keeps a position valid; it
//! does not know or decide what the position *means* — which occurrence is current,
//! and what to do once the point is gone, stay with the consumer. That split is why the
//! answer is a point registry rather than the engine owning the search anchor itself.
//!
//! Two things it deliberately is **not**:
//!
//! - **Not a marker.** The shape is the same one `markers.rs` implements (stable id,
//!   line kept by the write path, death observable), but every live marker rides two
//!   frame groups, so registering one to remember a position would paint it on the
//!   overview ruler. Nothing here reaches a frame.
//! - **Not an announcement.** A marker's death is a `TermEvent` because a decoration
//!   holder is push-driven and would otherwise never learn. A tracked point is *asked*
//!   — [`Term::tracked_point`] answers `None` — and the one caller shape that exists
//!   asks on every re-search anyway, so an event would be a second channel carrying
//!   what the first already says.

use crate::term::{Term, TrackedId, TrackedPoint};

impl Term {
    /// The active buffer's tracked points — alt while the alt screen is up, else
    /// primary. Mirrors `markers`/`markers_mut`, and for the same reason: an
    /// absolute index means a different thing on each screen.
    fn tracked_mut(&mut self) -> &mut Vec<TrackedPoint> {
        if self.on_alt {
            &mut self.alt_tracked
        } else {
            &mut self.normal_tracked
        }
    }

    /// Track absolute buffer `(line, col)`, returning a stable id (#691). The
    /// engine keeps the position on the content that is there now, through
    /// eviction, region scrolls and reflow, for as long as that content is in the
    /// buffer; [`Self::tracked_point`] reads it back and answers `None` once it is
    /// gone.
    ///
    /// The coordinate is **absolute**, not a viewport row, because the positions
    /// worth tracking are off-screen ones — a search match in scrollback is the
    /// case this exists for, and `add_marker`'s viewport intake structurally
    /// cannot name it.
    ///
    /// Out of range is bounded, not rejected, and bounded at the **read** rather
    /// than here: the engine owns no producer for this coordinate — it is the
    /// consumer's, like a `Match` — which is the second branch of ADR-0026 D2, the
    /// same one `match_spans` takes.
    pub fn track_point(&mut self, line: usize, col: usize) -> TrackedId {
        let id = TrackedId(self.next_tracked_id);
        self.next_tracked_id += 1;
        self.tracked_mut().push(TrackedPoint { id, line, col });
        id
    }

    /// Where the point registered as `id` sits now, or `None` if it has left the
    /// buffer (or the id was never issued / already released).
    ///
    /// Bounded here, both ends, per ADR-0026 D2/D3: the line to the buffer's own
    /// range — floored by `abs_floor` so an alt-screen point cannot name primary
    /// history — and the column to the grid width rather than the line's text
    /// (D4). The column's domain is `[0, cols]` like a marker's: one past the last
    /// cell is a legal *bound*, which is what a caller pairing this with text
    /// extraction needs.
    pub fn tracked_point(&self, id: TrackedId) -> Option<(usize, usize)> {
        let p = self
            .normal_tracked
            .iter()
            .chain(&self.alt_tracked)
            .find(|p| p.id == id)?;
        let floor = self.abs_floor();
        let last = (self.scrollback.len() + self.grid.rows()).saturating_sub(1);
        Some((
            p.line.clamp(floor, last.max(floor)),
            p.col.min(self.grid.cols()),
        ))
    }

    /// Release `id`. A no-op for an unknown or already-released id.
    ///
    /// Not optional housekeeping: the engine cannot know when a holder is done
    /// with a position, so without this the registry only ever grows.
    pub fn untrack_point(&mut self, id: TrackedId) {
        // Id-based and buffer-agnostic, like `remove_marker`: ids are unique
        // across both lists, so a point is released whichever screen it is on.
        self.normal_tracked.retain(|p| p.id != id);
        self.alt_tracked.retain(|p| p.id != id);
    }

    /// Shift tracked points up one absolute line from `from` down, after a
    /// top-anchored sub-region scroll grew scrollback while the rows below the
    /// bottom margin stayed put on screen (#449). Primary only, because the
    /// accrual branch that needs it is. The tracked-point analogue of
    /// `selection_shift_below_margin` / `markers_shift_below_margin`.
    pub(super) fn tracked_shift_below_margin(&mut self, from: usize) {
        for p in &mut self.normal_tracked {
            if p.line >= from {
                p.line += 1;
            }
        }
    }

    /// Rotate tracked points within an in-screen region scroll of absolute lines
    /// `[top, bottom]` (`up` = a line dropped at `top`, else at `bottom`). A point
    /// on the dropped edge has left the buffer and is released. The analogue of
    /// `selection_rotate_region` / `markers_rotate_region`.
    ///
    /// It follows the *marker* policy, not the selection's: a marker on the edge
    /// is disposed, while a selection clamps to keep the part of a range still in
    /// the buffer. A tracked point is one position, not a range — there is no
    /// surviving part to keep, and clamping would hand back a coordinate naming
    /// content the caller never asked about.
    pub(super) fn tracked_rotate_region(&mut self, top: usize, bottom: usize, up: bool) {
        self.tracked_mut().retain_mut(|p| {
            if p.line < top || p.line > bottom {
                return true; // outside the region — unchanged
            }
            let dropped_edge = if up { top } else { bottom };
            if p.line == dropped_edge {
                false
            } else {
                p.line = if up { p.line - 1 } else { p.line + 1 };
                true
            }
        });
    }

    /// Shift tracked points down one absolute line after the oldest history line
    /// is evicted past the scrollback cap; a point *on* that line has left the
    /// buffer and is dropped. The tracked-point analogue of
    /// `selection_evict_oldest` / `markers_evict_oldest`, and the site this whole
    /// module was filed for (#691).
    pub(super) fn tracked_evict_oldest(&mut self) {
        // Scrollback eviction is primary-only (the alt screen has none).
        self.normal_tracked.retain_mut(|p| {
            if p.line == 0 {
                false
            } else {
                p.line -= 1;
                true
            }
        });
    }
}
