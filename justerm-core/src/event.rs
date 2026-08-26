//! Consumer event surface (#12): point-in-time notifications the engine
//! accumulates while parsing, for the consumer to drain.
//!
//! Pull, not push — the engine queues events during `feed` and the consumer
//! takes them with `drain_events`, mirroring the rest of the pull cadence
//! (`damage` / `frame` / `reset_damage`). No callback is injected across the
//! boundary, so the engine stays decoupled from the consumer's event loop
//! (unlike alacritty's `EventListener`, whose push model would couple them).
//!
//! OSC 8 hyperlinks are deliberately absent — a hyperlink is per-cell state
//! (which cells are links), not a point-in-time event, so it is modelled like
//! graphemes in its own slice (#26), not here.

use crate::serialize::{MarkerId, MarkerKind};

/// A consumer-facing event emitted while parsing the VT stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEvent {
    /// The window/icon title was set (OSC 0 or OSC 2).
    Title(String),
    /// The terminal bell rang (BEL, `0x07`).
    Bell,
    /// The working directory was reported (OSC 7), e.g. `file://host/path`.
    Cwd(String),
    /// The app requested 80/132-column mode (DECCOLM `?3`). justerm is
    /// dimension-free, so this is a *request* — the consumer may honor it by
    /// calling `resize(cols, rows)`, or ignore it. `cols` is 80 or 132 (#82).
    ColumnMode { cols: usize },
    /// The app queried the light/dark color scheme (DSR `CSI ? 996 n`). justerm
    /// is theme-agnostic, so the consumer (which knows the scheme) answers by
    /// calling `Engine::report_color_scheme` (#85).
    ColorSchemeQuery,
    /// The app set ANSI palette entry `index` to `spec` (OSC 4). One event per
    /// `index ; spec` pair in the sequence. The cell still references
    /// `Indexed(index)` — only the consumer's `palette[index]` changes, so the
    /// engine stays theme-agnostic (#122).
    SetPaletteColor { index: u8, spec: String },
    /// The app set the default foreground colour (OSC 10). Raw spec, forwarded
    /// for the consumer to apply — theme-agnostic, like [`SetBackground`](Self::SetBackground) (#122).
    SetForeground(String),
    /// The app set the default background colour (OSC 11). The engine is
    /// theme-agnostic, so it forwards the raw spec string (`rgb:…`/`#…`) for the
    /// consumer to parse and apply to its palette — it never holds hex (#122).
    SetBackground(String),
    /// The app reset palette entries to the theme default (OSC 104). `None` =
    /// the whole table (no argument); `Some(index)` = one entry, one event per
    /// index given. The consumer restores its palette (#122).
    ResetPaletteColor(Option<u8>),
    /// The app queried ANSI palette entry `index` (OSC 4 with `?` for that pair);
    /// the consumer answers with `report_palette_color` (#122).
    QueryPaletteColor { index: u8 },
    /// The app set the cursor colour (OSC 12, #832). The third slot of the same
    /// dynamic-colour sequence `SetForeground` and `SetBackground` ride, and
    /// theme-agnostic for the same reason: the raw spec is forwarded and the
    /// consumer — which owns the palette *and* the cursor's contrast guard —
    /// applies it.
    SetCursorColor(String),
    /// The app queried the cursor colour (OSC 12 with `?`); the consumer answers
    /// with `report_cursor_color` (#832).
    QueryCursorColor,
    /// The app reset the cursor colour to the theme default (OSC 112, #832). The
    /// third member of the 110/111/112 reset family, and the one real
    /// applications emit most: `nvim` sends it on startup, on every alt-screen
    /// transition and on exit.
    ResetCursorColor,
    /// The app reset the default foreground to the theme default (OSC 110, #122).
    ResetForeground,
    /// The app reset the default background to the theme default (OSC 111, #122).
    ResetBackground,
    /// The app queried the default foreground colour (OSC 10 with `?`); the
    /// consumer answers with `report_foreground` (#122).
    QueryForeground,
    /// The app queried the default background colour (OSC 11 with `?`). The
    /// theme-agnostic engine relays it; the consumer answers with
    /// `report_background` (#122), mirroring `ColorSchemeQuery`.
    QueryBackground,
    /// A decoration marker's line left the buffer — evicted past the scrollback
    /// cap, or scrolled out of an in-screen region (#118). The handle is now
    /// dead; the consumer drops the decoration bound to it. This is the
    /// frame-mode equivalent of xterm's `IMarker.onDispose` — disposal is a
    /// point-in-time fact (a marker absent from a frame may merely be scrolled
    /// off-screen), so it rides the event queue, not the frame overlay.
    MarkerDisposed(MarkerId),
    /// A marker was created (#490) — by `add_marker`, or by the *stream* through an
    /// OSC 133 command mark, which the consumer never called for.
    ///
    /// The mirror of [`TermEvent::MarkerDisposed`], and it exists for the same reason
    /// ADR-0020 R1 gives: an appearance is an occurrence, not state, so it rides this
    /// queue rather than a frame field. Without it a consumer that pulled a marker
    /// index (`Engine::marker_index`) has no way to learn of a marker born after its
    /// pull — the population would only ever shrink.
    ///
    /// `line` is absolute at the moment of creation, and `evicted_total` / `epoch` are the
    /// instant it is absolute at — the same triple [`crate::MarkerIndex`] carries, because
    /// this event is that pull's incremental mirror. The consumer appends the entry with
    /// the basis it arrived on and rebases it exactly like a pulled one.
    ///
    /// **The two are one fact and neither is usable alone (#737).** A single `feed` can
    /// create a marker and then evict, so by the end of the batch the buffer's origin has
    /// moved out from under the line this event already carries. `Frame::evicted_total` is
    /// the basis at the *end* of that batch, so reading `line` against it misplaces the
    /// marker by however much the batch evicted after the birth — measured at three lines,
    /// with the event line, both frame bases, the epoch and `Frame::marker_count` all
    /// identical to the batch that evicted *first* and needs no adjustment at all.
    ///
    /// **And a basis dates only a uniform move (#741).** Eviction shifts every marker by
    /// the same amount, which is what one scalar can say; a reflow or a region rotate
    /// moves them *individually*, which is what `epoch` is for. A birth still queued when
    /// the epoch moves describes a buffer that no longer exists, and carrying only the
    /// basis leaves that indistinguishable from a birth in the current generation —
    /// measured, a mark at absolute 3 reflowed to 5 with the basis unmoved at 0.
    ///
    /// Deliberately not an epoch bump: a bump costs a whole re-pull, and creation is
    /// `O(1)` information.
    MarkerCreated {
        id: MarkerId,
        line: u32,
        kind: MarkerKind,
        /// Lines evicted since RIS at the moment of creation — the basis `line` is
        /// absolute at. Carried rather than inferred so that placement does not depend on
        /// whether the consumer drains this queue before or after it reads the frame,
        /// which nothing in the API specifies.
        ///
        /// It is the same quantity `Frame::evicted_total` reports, so a consumer whose
        /// transport crosses a language boundary owes it the same treatment: the wasm
        /// frame getter hands its `u64` over as an `f64` deliberately (exact to 2^53),
        /// because a `BigInt` on one side of a subtraction and a `number` on the other is
        /// a `TypeError`, not a rounding question.
        evicted_total: u64,
        /// The marker generation this line belongs to — [`crate::MarkerIndex::epoch`] at
        /// the moment of creation (#741). Two lines dated with different epochs are
        /// answers about different buffers and nothing rebases one onto the other, so a
        /// consumer adopts this entry only into the generation it names and lets the
        /// re-pull that the bump already forces supply it otherwise.
        ///
        /// Compare it for **equality**, never for order: the counter is
        /// `wrapping_add`, so `<` is meaningless across a wrap while `==` is exact.
        epoch: u32,
    },
}
