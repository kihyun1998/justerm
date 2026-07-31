//! Issue #6 — binary, reference-based wire format for a damage frame.
//!
//! `encode` a [`Frame`] to bytes, `decode` them back; the round-trip is the
//! contract. Reference-based (colour refs, Unicode scalars — never resolved RGB
//! or atlas ids) so the engine stays theme- and font-agnostic; the consumer's
//! adapter resolves references before handing cells to the renderer. Format spec
//! and rationale: `docs/architecture.md` §Serialization + ADR-0005.

use crate::cell::{Cell, CellFlags};
use crate::color::Color;
use crate::cursor::CursorShape;
use crate::damage::ScrollOp;
use crate::input::MouseEvents;
use crate::selection::SelectionSpan;
use core::num::NonZeroU32;
use std::collections::BTreeMap;

/// Wire magic ("juSTerm") + format version. A new feature bumps `VERSION`.
const MAGIC: [u8; 2] = *b"JT";
const VERSION: u8 = 14; // v14 moves combining clusters and hyperlink refs off the fixed cell record (18 B -> 14 B) into per-span sparse groups, inlining the cluster (no side-table) but keeping the URI table interned, and widens every count/length prefix they use to u32 — the engine could hold a cluster, a URI, or a viewport its own decoder then rejected or, worse, mis-read as Ok (#621); v13 adds a per-span underline-colour group: sparse (col, Color) pairs for cells drawing a coloured underline (SGR 58, #520); v12 adds a fifth overlay group: the consumer-designated active search match's spans (#428); v11 adds a fourth overlay group: every live marker's absolute buffer line for the overview ruler (#120 S3); v10 adds a marker kind discriminant + optional i32 exit to the overlay marker group (#159); v9 adds the alt-screen flag in the header (#149); v8 adds the mouse wanted-events mask in the header (#129/ADR-0016); v7 overlay marker group (#118/ADR-0015); v6 overlay selection + search-match spans (#108/ADR-0014); v5 scroll position (#112/ADR-0013); v4 cursor shape+blink (#81); v3 cursor row/col/visibility (#38)

/// The wire-format version (the gating `VERSION` byte), exposed so a binding can
/// assert at load that its decoder matches the backend encoder (#34/ADR-0008).
pub const WIRE_VERSION: u8 = VERSION;

/// Whether a frame redraws everything or just its spans.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameKind {
    /// Every row is present (resize / alt-screen clear).
    Full,
    /// Only the listed spans changed since the consumer's ack.
    Partial,
}

/// A damaged column run on one line, with its cells.
///
/// `combining` and `links` map a span-relative column to what that cell carries —
/// combining clusters (#45) and hyperlinks (#46) live in per-row maps, so neither
/// rides the cell. Since v14 (#621) both are **sparse wire groups of their own**,
/// not indices in the cell record, which is what removed the `u16` ceilings the
/// engine could legitimately exceed.
///
/// The two are deliberately **not** symmetric, and the asymmetry is measured rather
/// than stylistic:
///
/// - `combining` holds the cluster **inline**. `Term::frame` pushes one entry per
///   combining cell with no interning, so an index bought nothing but a level of
///   indirection and a table to count. Inlining is size-neutral (measured: −0.5% on
///   a combining-heavy frame) and buys the deletion of both.
/// - `links` holds a **1-based index into [`Frame::link_table`]**, because that
///   table *is* interned (`Term::frame`'s `link_remap` ships each referenced URI
///   once). Inlining a URI at every linked cell was measured at +171…403% on
///   link-dense frames, and all three references share one copy across cells —
///   ghostty ref-counts its hyperlink set explicitly *"so that a set of cells can
///   share the same hyperlink without duplicating the data"*, xterm.js keys cells to
///   an `OscLinkService` id, alacritty holds `Arc<HyperlinkInner>`.
///
/// A column is present in either map iff its cell carries the matching bit — and,
/// as with `ucolors` below, that bit does **not** travel on the wire (the record
/// encodes `cell.c()` and `encode_color(bg)`, which drop `C_COMBINED` and
/// `LINK_PRESENT` respectively). `decode` re-arms both from these maps' own entries.
/// A `Span` built by hand for a test owes the same pairing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Span {
    pub line: u16,
    pub left: u16,
    pub right: u16,
    pub cells: Vec<Cell>,
    pub combining: BTreeMap<usize, Vec<char>>,
    pub links: BTreeMap<usize, NonZeroU32>,
    /// Underline colours (SGR 58, #520): span-relative column → the `Color`
    /// reference the cell's coloured underline draws in. Sparse — only cells that
    /// carry a non-default underline colour appear (gated on the `UNDERLINE`
    /// attribute at parse time). Unlike `combining`/`links` this is a colour
    /// reference, not a side-table index, so it ships inline (no `_table` on the
    /// [`Frame`]). Kept off the per-cell record so a plain-text frame pays nothing
    /// (ADR-0020: no inert per-cell payload).
    ///
    /// Like `combining` and `links`, a column here is present iff its cell carries
    /// the matching bit ([`Cell::is_ucolored`]) — but that bit does **not** travel on
    /// the wire (`encode_color` keeps only mode+value, and `CellFlags` holds no
    /// presence bits), so `decode` re-arms it from this map's own entries. A `Span`
    /// built by hand for a test owes the same pairing: an entry here without
    /// [`Cell::set_ucolored`] on the cell is a column the gated readers cannot see.
    /// (#531)
    pub ucolors: BTreeMap<usize, Color>,
}

/// A stable handle to a buffer line, handed out by `Engine::add_marker` (#118).
/// Monotonic per engine. The consumer attaches a decoration to the id; the frame
/// reports where the marker currently sits, and `TermEvent::MarkerDisposed`
/// signals when its line has left the buffer.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MarkerId(pub u32);

/// What a marker means (#158). A plain `add_marker` decoration carries no
/// semantics ([`MarkerKind::Plain`]); OSC 133 shell-integration marks carry the
/// command-boundary role (prompt/command/output start, or command finished with
/// its optional exit code). The engine only *parses and anchors* these — the
/// success/failure colour, earcon and prompt-to-prompt navigation are consumer
/// policy (ADR-0017), driven off the kind + exit the wire (#159) carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkerKind {
    /// A `add_marker` decoration anchor (#118) — no OSC-133 semantics.
    Plain,
    /// OSC `133;A` — the shell prompt begins here.
    PromptStart,
    /// OSC `133;B` — the typed command begins here (the prompt ended).
    CommandStart,
    /// OSC `133;C` — the command was submitted; its output begins here.
    OutputStart,
    /// OSC `133;D[;exit]` — the command finished, with its exit code if reported
    /// (absent, empty or non-numeric → `None`).
    CommandFinished(Option<i32>),
}

/// A marker projected onto the viewport (#118): its id, the row it sits on, and
/// its kind (#159). Only markers visible in the current viewport are reported; an
/// off-screen marker is omitted but still alive (death comes via `MarkerDisposed`,
/// not absence — so the consumer can tell "scrolled away" from "gone"). The kind
/// carries the OSC 133 command-boundary role + exit code so the consumer can drive
/// prompt-to-prompt navigation and success/fail signals (#160).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MarkerPosition {
    pub id: MarkerId,
    pub row: usize,
    pub kind: MarkerKind,
}

/// A marker's absolute buffer line (#120 S3, v11). Unlike [`MarkerPosition`],
/// this is reported for EVERY live marker — on-screen or not — so a frame-mode
/// consumer can place overview-ruler marks buffer-relatively (dividing by
/// `scrollback + rows`), the whole point of a ruler being to show off-viewport
/// anchors. The consumer joins `id` with its decoration registry; the ruler mark's
/// colour is the consumer's (theme-agnostic), so no kind/exit rides here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MarkerLine {
    pub id: MarkerId,
    /// Absolute buffer line, in the same `[0, scrollback_len + rows)` frame the
    /// header's `scrollback_len`/`display_offset` use.
    pub line: u32,
}

/// Interaction overlays projected onto the viewport (#108): highlight spans the
/// engine carries on the frame so a frame-mode consumer can paint them without
/// an in-process model query. Positions only — highlight colour is the
/// consumer's (theme-agnostic). Coordinates are viewport rows/cols, re-projected
/// by `frame()` against the scroll offset so the engine stays the single
/// anchoring authority.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Overlay {
    /// The live selection projected onto visible rows (`selection_range`).
    pub selection: Vec<SelectionSpan>,
    /// The search highlights projected onto visible rows. Search matches
    /// are consumer-owned (next/prev navigation holds the `Vec<Match>`), so the
    /// consumer hands the highlight set back via `set_search_highlights` and the
    /// engine projects it here — mirroring how the engine-owned selection rides.
    pub matches: Vec<SelectionSpan>,
    /// Engine-owned markers visible in this viewport (#118): persistent line
    /// anchors for decorations. Unlike the selection (cleared on a screen swap)
    /// and search highlights (invalidated on output), markers re-anchor through
    /// buffer mutation and survive an alt-screen excursion; only their viewport
    /// position rides here.
    pub markers: Vec<MarkerPosition>,
    /// Every live marker's absolute buffer line (#120 S3, v11), on-screen or not —
    /// the overview ruler needs off-viewport anchors, which `markers` (viewport-
    /// only) can't supply. A superset of `markers` by id; different frame of
    /// reference (absolute line, not viewport row).
    pub marker_lines: Vec<MarkerLine>,
    /// The *active* (current) search match's spans (#428, v12): the member of
    /// `matches` the consumer designated via `set_active_search_highlight`
    /// (which match is active is consumer policy — next/prev navigation).
    /// Projected by the same mechanism as `matches`, and *also* present there —
    /// the renderer's highlight ranking resolves the overlap (#424), not
    /// exclusion here. Empty when nothing is designated.
    pub active_match: Vec<SelectionSpan>,
}

/// One serialized damage cycle: the decoded logical form that `encode`/`decode`
/// round-trip. `link_table` holds this frame's OSC 8 hyperlink URIs, each shipped
/// once and referenced by [`Span::links`]. Grapheme clusters have **no** table —
/// since v14 (#621) they are inlined at their column in [`Span::combining`],
/// because nothing interned them and the table only bought an index to overflow.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    pub cols: u16,
    pub rows: u16,
    pub kind: FrameKind,
    /// Cursor row/col in screen coordinates (0-based), and whether the engine
    /// shows it (DECTCEM). Rides in the header because the cursor moves with
    /// almost every frame (#38). *Drawing* the cursor — cell-invert / overlay —
    /// stays the consumer's renderer adapter; the engine only reports state.
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub cursor_visible: bool,
    /// The caret shape (DECSCUSR #89) and whether it blinks (att610 ?12, #81).
    /// Reported for the renderer; drawing/animation stays the consumer's.
    pub cursor_shape: CursorShape,
    pub cursor_blink: bool,
    /// Viewport scroll position (#112 / ADR-0013), for the consumer's scrollbar.
    /// `display_offset` = lines scrolled up from the bottom (0 = following the
    /// live screen); `scrollback_len` = history lines (total = `+ rows`). Ride in
    /// the header like the cursor — per-frame viewport state, not cell content.
    pub display_offset: u32,
    pub scrollback_len: u32,
    /// The mouse tracking mode as a *wanted-events* mask (#129): which mouse
    /// event categories the app asked to receive, so the consumer routes an event
    /// to the app (bit set) or keeps it local. `empty()` = no reporting. Rides the
    /// header like the cursor — per-frame mode state the consumer reads, not cell
    /// content. Positions/encoding never cross; the backend encodes via
    /// `encode_mouse`.
    pub mouse_events: MouseEvents,
    /// Whether the alternate screen (`?1049`/`?47`) is active (#149). Buffer-global
    /// state a frame-mode consumer can't derive from viewport damage — the
    /// accessibility announce policy (#119) gates on it (suppress TUI repaints).
    /// Rides the header like the cursor scalars (ADR-0014).
    pub alt_screen: bool,
    pub scroll: Option<ScrollOp>,
    pub spans: Vec<Span>,
    pub link_table: Vec<String>,
    /// Interaction overlays for this viewport (#108): selection, search
    /// highlights, the active match, and markers — see [`Overlay`].
    pub overlay: Overlay,
}

/// Why a byte buffer could not be decoded into a [`Frame`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecodeError {
    /// Ran out of bytes mid-field.
    Truncated,
    /// First two bytes are not the wire magic.
    BadMagic,
    /// Unsupported format version.
    BadVersion(u8),
    /// A tag/kind byte held a value outside its defined set.
    BadTag,
    /// A part of the frame does not fit the geometry the frame itself declares: a span
    /// whose `left` is past its `right` (which would underflow the cell count), a span
    /// reaching past `cols` or sitting past `rows`, a sparse group entry keyed outside
    /// its own span, or a scroll region whose `bottom` is past the last row (#582).
    ///
    /// One rule, one error: a coordinate describing a cell the frame says does not exist
    /// is malformed input, and the consumer must not be handed it.
    ///
    /// A dedicated `BadScroll` was considered and **not** taken, and the trade is worth
    /// stating honestly rather than as a slogan. Against it: this enum is `pub` and not
    /// `#[non_exhaustive]`, so a new variant is a breaking change for any downstream
    /// exhaustive match — of which there are, measured, **none in this workspace**; the
    /// cost is borne only by an external matcher nobody has seen. For it: this variant is
    /// now the whole diagnostic for six distinct malformations, and the JS side has no
    /// more to work with (`justerm-wasm-decode` formats the variant name into the thrown
    /// value). The distinction is real but belongs to the next release that is breaking
    /// anyway — a version bump spent on a diagnostic label, on a crate published in
    /// lockstep with an npm package, is the more expensive half of this trade today.
    BadSpan,
}

/// Serialize a frame to the binary wire format.
pub fn encode(frame: &Frame) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.push(VERSION);
    out.push(frame.scroll.is_some() as u8);
    out.push(match frame.kind {
        FrameKind::Full => 0,
        FrameKind::Partial => 1,
    });
    out.extend_from_slice(&frame.cols.to_le_bytes());
    out.extend_from_slice(&frame.rows.to_le_bytes());
    out.extend_from_slice(&frame.cursor_row.to_le_bytes());
    out.extend_from_slice(&frame.cursor_col.to_le_bytes());
    out.push(frame.cursor_visible as u8);
    out.push(match frame.cursor_shape {
        CursorShape::Block => 0,
        CursorShape::Underline => 1,
        CursorShape::Bar => 2,
    });
    out.push(frame.cursor_blink as u8);
    out.extend_from_slice(&frame.display_offset.to_le_bytes());
    out.extend_from_slice(&frame.scrollback_len.to_le_bytes());
    // Mouse wanted-events mask (#129): one byte in the header, like the cursor
    // scalars. Off = 0.
    out.push(frame.mouse_events.bits());
    // Alt-screen flag (#149): one byte in the header, like the cursor scalars.
    out.push(frame.alt_screen as u8);
    if let Some(s) = frame.scroll {
        out.extend_from_slice(&(s.top as u16).to_le_bytes());
        out.extend_from_slice(&(s.bottom as u16).to_le_bytes());
        out.extend_from_slice(&(s.count as i16).to_le_bytes());
    }
    out.extend_from_slice(&(frame.spans.len() as u16).to_le_bytes());
    for span in &frame.spans {
        out.extend_from_slice(&span.line.to_le_bytes());
        out.extend_from_slice(&span.left.to_le_bytes());
        out.extend_from_slice(&span.right.to_le_bytes());
        for cell in &span.cells {
            out.extend_from_slice(&encode_cell_record(cell));
        }
    }
    // Hyperlink table (#26), interned: each referenced URI ships once as a
    // length-prefixed UTF-8 run, and `Span::links` points at it. Both the count and
    // the length are u32 since v14 — the old u16 length rejected a URI the engine
    // stores happily, and the old u16 count could not describe one entry per cell of
    // a viewport the header's own `cols`/`rows` (u16 *each*) permit (#621).
    out.extend_from_slice(&(frame.link_table.len() as u32).to_le_bytes());
    for uri in &frame.link_table {
        out.extend_from_slice(&(uri.len() as u32).to_le_bytes());
        out.extend_from_slice(uri.as_bytes());
    }
    // Combining-cluster group (#45, v14): one sparse map per span, in span order, so
    // the column keys need no span index — the same positional convention `ucolors`
    // uses below. Each entry is `(col u16, len u32, len * char u32)`, the cluster
    // inline. There is no side-table and no index: nothing interned them, so the
    // indirection only bought a second count to overflow (#621).
    //
    // **An entry keyed outside its span is dropped, not narrowed (#582).** `Span`'s maps
    // are `pub` and keyed by `usize` while the wire key is a `u16`, so `col as u16` did
    // not lose an out-of-range entry — it *moved* it onto a different, live column
    // (measured: 65539 encoded as 3 and armed the cell 'D'; 65540 in this group armed
    // 'E'). Dropping is the only answer that fails in the harmless direction, the same
    // asymmetry [`Term::damage_span`] records from ghostty — "may have false positives
    // but should never have false negatives".
    //
    // The `debug_assert` is the detector and the drop is the release backstop, again as
    // `damage_span`: `Term::frame` cannot build such a key (it inserts `col - left` for
    // `col` in `left..=right`), so one arriving here is a justerm bug and should name its
    // producer at the site — but justerm is a library, and a panic crosses into the
    // consumer's process.
    //
    // The keys are sorted, so the writable entries are a prefix and the *last* key decides
    // whether any were dropped: when none were — every frame the engine produces — the
    // count is still `len()`, O(1), and only the write loop walks the map. Counting the
    // range unconditionally would have replaced an O(1) read with a walk per span per
    // group on the encode hot path, to describe a case that cannot occur.
    for span in &frame.spans {
        debug_assert!(
            span.combining.keys().all(|&c| c < span.cells.len()),
            "combining key past the end of its {}-cell span",
            span.cells.len()
        );
        let n = if span
            .combining
            .last_key_value()
            .is_none_or(|(&k, _)| k < span.cells.len())
        {
            span.combining.len()
        } else {
            span.combining.range(..span.cells.len()).count()
        };
        out.extend_from_slice(&(n as u32).to_le_bytes());
        for (&col, cluster) in span.combining.range(..span.cells.len()) {
            out.extend_from_slice(&(col as u16).to_le_bytes());
            out.extend_from_slice(&(cluster.len() as u32).to_le_bytes());
            for &ch in cluster {
                out.extend_from_slice(&(ch as u32).to_le_bytes());
            }
        }
    }
    // Hyperlink reference group (#46, v14): same positional shape, but the value is a
    // 1-based index into `link_table` rather than the URI — see `Span`'s doc for why
    // this half stays interned where the one above does not.
    // Out-of-span keys are dropped here for the reason written out at the combining group
    // above; every group answers the question the same way or the rule is not a rule.
    for span in &frame.spans {
        debug_assert!(
            span.links.keys().all(|&c| c < span.cells.len()),
            "link key past the end of its {}-cell span",
            span.cells.len()
        );
        let n = if span
            .links
            .last_key_value()
            .is_none_or(|(&k, _)| k < span.cells.len())
        {
            span.links.len()
        } else {
            span.links.range(..span.cells.len()).count()
        };
        out.extend_from_slice(&(n as u32).to_le_bytes());
        for (&col, &idx) in span.links.range(..span.cells.len()) {
            out.extend_from_slice(&(col as u16).to_le_bytes());
            out.extend_from_slice(&idx.get().to_le_bytes());
        }
    }
    // Underline-colour group (SGR 58, #520, v13): one sparse map per span, in span
    // order, so the column keys need no span index — the decoder reads exactly
    // `span_count` maps and attaches each to its span. Each entry is `(col u16,
    // colour u32)`, the colour packed by the same `encode_color` as fg/bg. A frame
    // with no coloured underlines pays 2 bytes per span (the zero count).
    // Out-of-span keys are dropped here too — see the combining group above for why.
    for span in &frame.spans {
        debug_assert!(
            span.ucolors.keys().all(|&c| c < span.cells.len()),
            "underline-colour key past the end of its {}-cell span",
            span.cells.len()
        );
        let n = if span
            .ucolors
            .last_key_value()
            .is_none_or(|(&k, _)| k < span.cells.len())
        {
            span.ucolors.len()
        } else {
            span.ucolors.range(..span.cells.len()).count()
        };
        out.extend_from_slice(&(n as u16).to_le_bytes());
        for (&col, &color) in span.ucolors.range(..span.cells.len()) {
            out.extend_from_slice(&(col as u16).to_le_bytes());
            out.extend_from_slice(&encode_color(color).to_le_bytes());
        }
    }
    // Overlay section (#108): each group is a u16 count then that many
    // `(row, left, right)` u16 viewport triples. Selection first, then search
    // matches. Append-only, version-gated — a future group (markers, #118) adds
    // a third count here at the next version bump.
    encode_overlay_spans(&mut out, &frame.overlay.selection);
    encode_overlay_spans(&mut out, &frame.overlay.matches);
    // Third overlay group (#118): markers as `(id u32, row u16)` pairs — a
    // different record shape from the span groups (a marker is a line anchor,
    // not a column run). v10 (#159) appends a kind discriminant (u8, like
    // `cursor_shape`), and — only for `CommandFinished` — a presence byte + i32
    // exit code (the presence pattern mirrors the header's `scroll` option).
    out.extend_from_slice(&(frame.overlay.markers.len() as u16).to_le_bytes());
    for m in &frame.overlay.markers {
        out.extend_from_slice(&m.id.0.to_le_bytes());
        out.extend_from_slice(&(m.row as u16).to_le_bytes());
        out.push(match m.kind {
            MarkerKind::Plain => 0,
            MarkerKind::PromptStart => 1,
            MarkerKind::CommandStart => 2,
            MarkerKind::OutputStart => 3,
            MarkerKind::CommandFinished(_) => 4,
        });
        if let MarkerKind::CommandFinished(exit) = m.kind {
            out.push(exit.is_some() as u8);
            out.extend_from_slice(&exit.unwrap_or(0).to_le_bytes());
        }
    }
    // Fourth overlay group (#120 S3, v11): every live marker's absolute buffer
    // line as `(id u32, line u32)` pairs — a superset of the viewport marker group
    // above, for placing overview-ruler marks off-viewport. Count-prefixed like the
    // others.
    out.extend_from_slice(&(frame.overlay.marker_lines.len() as u16).to_le_bytes());
    for m in &frame.overlay.marker_lines {
        out.extend_from_slice(&m.id.0.to_le_bytes());
        out.extend_from_slice(&m.line.to_le_bytes());
    }
    // Fifth overlay group (#428, v12): the active search match's spans, same
    // count + `(row, left, right)` shape as the selection/match groups. Appended
    // at the tail so the section stays append-only.
    encode_overlay_spans(&mut out, &frame.overlay.active_match);
    out
}

/// Encode one overlay group: a u32 span count, then each span as three u16s
/// (`row`, `left`, `right`) in viewport coordinates.
///
/// The count is u32 since v14 (#621), and this is the *same* defect as the cluster
/// and URI fields — found by that issue's own acceptance item ("do not assume those
/// two are the only `as u16` narrowings"), which the first sweep answered for the
/// `(row, left, right)` triples and not for the count above them. A one-character
/// search over a large viewport reaches it: measured at 1000×133, 66 000 highlight
/// spans wrapped the count to 464, and `decode` returned **`Ok`** having also
/// fabricated 928 marker-lines and 3 active-match spans the engine never had — the
/// wrapped count leaves the reader mid-group, and every group after it is read from
/// the wrong offset.
///
/// **This widens the three viewport-projected groups and deliberately not the two
/// marker groups**, which keep their `u16` counts a few lines below. That is not an
/// oversight and not inconsistency: `frame()` clips selection / matches /
/// active-match to the viewport, so their counts are `O(viewport)` — ADR-0020 R3
/// satisfied, and widening entrenches nothing. The marker groups report **every
/// live marker**, on-screen or not; they are unbounded by the viewport and are the
/// one R3 violation ADR-0020 records against itself. Widening those would make that
/// violation cheaper to keep, which is precisely what #490 exists to remove — so
/// they stay narrow and stay #490's.
fn encode_overlay_spans(out: &mut Vec<u8>, spans: &[SelectionSpan]) {
    out.extend_from_slice(&(spans.len() as u32).to_le_bytes());
    for s in spans {
        out.extend_from_slice(&(s.row as u16).to_le_bytes());
        out.extend_from_slice(&(s.left as u16).to_le_bytes());
        out.extend_from_slice(&(s.right as u16).to_le_bytes());
    }
}

/// Length in bytes of one fixed-width wire cell record (see
/// [`encode_cell_record`]).
pub const CELL_RECORD_LEN: usize = 14;

/// Encode one [`Cell`] to its fixed 14-byte little-endian record:
/// `c` u32 (Unicode scalar) · `fg` u32 · `bg` u32 · `flags` u16. Width derives
/// from `flags`.
///
/// **The record carries no grapheme or hyperlink reference (v14, #621).** Both were
/// `u16` fields on every cell, and widening them to hold what the engine can
/// legitimately store would have inflated a record every cell pays — the trade
/// ADR-0008's Axis 4 already rejected in the other direction. They moved to sparse
/// per-[`Span`] groups instead, which is why this record *shrank* by 4 bytes:
/// measured, −20.9% on an ordinary frame that carries neither.
///
/// This is the single definition of the cell record layout — [`encode`] writes
/// it per span cell, and an alternate consumer (the WASM decoder, #34/ADR-0008)
/// reuses it to lay decoded cells out flat without re-implementing the layout,
/// so the two cannot drift.
pub fn encode_cell_record(cell: &Cell) -> [u8; CELL_RECORD_LEN] {
    let mut r = [0u8; CELL_RECORD_LEN];
    r[0..4].copy_from_slice(&(cell.c() as u32).to_le_bytes());
    r[4..8].copy_from_slice(&encode_color(cell.fg()).to_le_bytes());
    r[8..12].copy_from_slice(&encode_color(cell.bg()).to_le_bytes());
    r[12..14].copy_from_slice(&cell.flags().bits().to_le_bytes());
    r
}

/// A colour reference as a tagged u32: high byte = tag
/// (0 = Default, 1 = Indexed, 2 = Rgb), low 24 bits = payload. The tag is
/// mandatory so `Default`, `Indexed(0)`, and `Rgb(0,0,0)` stay distinct.
///
/// Public so an alternate consumer (the WASM decoder's structure-of-arrays
/// `fg`/`bg` columns, #35) reuses this single definition of the colour-ref
/// encoding instead of re-implementing the tag packing — no drift.
pub fn encode_color(c: Color) -> u32 {
    match c {
        Color::Default => 0,
        Color::Indexed(i) => (1 << 24) | i as u32,
        Color::Rgb(r, g, b) => (2 << 24) | (r as u32) << 16 | (g as u32) << 8 | b as u32,
    }
}

/// Deserialize the binary wire format back into a [`Frame`].
pub fn decode(bytes: &[u8]) -> Result<Frame, DecodeError> {
    let mut r = Reader::new(bytes);
    if r.take(2)? != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let version = r.u8()?;
    if version != VERSION {
        return Err(DecodeError::BadVersion(version));
    }
    let has_scroll = r.u8()? != 0;
    let kind = match r.u8()? {
        0 => FrameKind::Full,
        1 => FrameKind::Partial,
        _ => return Err(DecodeError::BadTag),
    };
    let cols = r.u16()?;
    let rows = r.u16()?;
    let cursor_row = r.u16()?;
    let cursor_col = r.u16()?;
    let cursor_visible = r.u8()? != 0;
    let cursor_shape = match r.u8()? {
        0 => CursorShape::Block,
        1 => CursorShape::Underline,
        2 => CursorShape::Bar,
        _ => return Err(DecodeError::BadTag),
    };
    let cursor_blink = r.u8()? != 0;
    let display_offset = r.u32()?;
    let scrollback_len = r.u32()?;
    let mouse_events = MouseEvents::from_bits_retain(r.u8()?);
    let alt_screen = r.u8()? != 0;
    let scroll = if has_scroll {
        let top = r.u16()? as usize;
        let bottom = r.u16()? as usize;
        let count = (r.u16()? as i16) as isize;
        // A scroll region is a write index in a consumer, not an annotation (#582):
        // `justerm-web`'s `cell-mirror.ts` assigns `cells[y * cols + x]` for every `y` in
        // `top..=bottom` with nothing bounding it against `rows`. The renderer already
        // rejects the same value (`FrameGrid::validate` → `ScrollOutsideGrid`) after a
        // `line == rows` off-by-one trapped the wasm module and left it poisoned (#355).
        //
        // **This is deliberately one notch stricter than the renderer, and the difference
        // is not an oversight.** `FrameGrid::validate` gates its check on `kind != Full`,
        // because a Full frame repaints everything and its own `shift_region` is skipped.
        // The web mirror does not have that exemption: it blanks the grid on a Full frame
        // and *then* still runs `shiftRegion` if the frame carries a scroll op. So the two
        // consumers disagree about whether a Full frame's scroll op is live, and the wire
        // is the wrong place to encode either answer — it rejects a region that cannot be
        // applied to the frame it rides on, whatever the consumer then does with it.
        //
        // `top > bottom` is an empty region, not an error — no consumer iterates it, and
        // the renderer says so explicitly. Rejecting it would be new strictness with no
        // failure behind it.
        //
        // Not checked here: `count`. It is `isize` in memory and `i16` on the wire, and
        // ordinary output overflows it — measured, 40 000 line feeds at the bottom of a
        // 3-row screen with no intervening `reset_damage` encode to **−25 536**, turning an
        // up-scroll into a down-scroll on a `Partial` frame. That is an *encode*-side
        // capacity defect in `Term::record_scroll`'s unbounded accumulation, not a
        // validation gap: rejecting it here would reject a frame this engine produces
        // today, which is exactly what this change promises not to do.
        if top <= bottom && bottom >= rows as usize {
            return Err(DecodeError::BadSpan);
        }
        Some(ScrollOp { top, bottom, count })
    } else {
        None
    };
    let span_count = r.u16()?;
    let mut spans = Vec::with_capacity(span_count as usize);
    for _ in 0..span_count {
        let line = r.u16()?;
        let left = r.u16()?;
        let right = r.u16()?;
        // A span is read against the frame's own header, not just against itself (#582).
        // Before this, a frame could declare a 4×2 grid and carry a span claiming column 8
        // of line 99 and still decode `Ok` — and the consumer that writes those cells does
        // not fail on it either: `cell-mirror.ts` keeps the viewport as one flat array, so
        // a column past `cols` lands in the *next row's* slot and silently overwrites it.
        // A screen reader then announces, and a copy produces, characters that are not on
        // that line. `decode`'s own input is attacker-influenced (`tests/robustness.rs`),
        // and rejecting malformed input rather than repairing it is what ADR-0008 makes
        // this boundary for.
        if right < left || right >= cols || line >= rows {
            return Err(DecodeError::BadSpan);
        }
        // Widen before the arithmetic: `right - left + 1` in `u16` overflows when
        // `right == u16::MAX` (e.g. left=0, right=65535), panicking under overflow checks
        // (#33, found by `cargo fuzz`). `right >= left` is enforced just above, so the
        // subtraction in `usize` cannot underflow.
        //
        // Since #582 this can no longer be *reached*: `right < cols` and `cols` is a u16,
        // so `right <= 65534` and the sum fits. Kept anyway, and deliberately — it is the
        // cheaper of the two guarantees and it does not depend on the check above keeping
        // its position. Deleting it would make a reordering of this function silently
        // reintroduce a panic that a fuzz run had to find once already.
        let n = right as usize - left as usize + 1;
        let mut cells = Vec::with_capacity(n);
        for _ in 0..n {
            cells.push(decode_cell(&mut r)?);
        }
        spans.push(Span {
            line,
            left,
            right,
            cells,
            // Filled from their own groups below (v14): the record no longer carries
            // either reference, so neither the maps nor the cells' presence bits can
            // be built here.
            combining: BTreeMap::new(),
            links: BTreeMap::new(),
            ucolors: BTreeMap::new(),
        });
    }
    // Counts and lengths are u32 since v14 — see `encode`. Note the deliberate loss of
    // `Vec::with_capacity`: these counts are attacker-influenced (`tests/robustness.rs`
    // drives `decode` from arbitrary bytes) and a u32 one can now declare 4 billion
    // entries, so reserving up front would turn a 12-byte buffer into an OOM. Growing
    // as the entries actually arrive is bounded by the input's own length.
    let link_count = r.u32()?;
    let mut link_table = Vec::new();
    for _ in 0..link_count {
        let len = r.u32()? as usize;
        let bytes = r.take(len)?;
        link_table.push(String::from_utf8_lossy(bytes).into_owned());
    }
    // Combining group (v14, #621): inverse of the encode above — one sparse map per
    // span, in span order, attached to `spans[i]` positionally.
    //
    // Re-arming `C_COMBINED` here is not a nicety, it is the only place left that can.
    // The bit never travels *as a bit*: the record encodes `cell.c()`, the char, so
    // the content word's marker is dropped. Until v14 `decode_cell` could reconstruct
    // it inline from `extra != 0` because the index rode the record; now that it does
    // not, this loop inherits that duty — the same reconstruction `ucolors` has always
    // done, and the defect #531 was filed for when it was missing.
    //
    // A key outside its span is rejected, not tolerated (#582, answering the question this
    // comment used to defer): `col` is attacker-influenced and nothing on the wire bounds
    // it against the span's width, and an entry addressing a cell the span does not have
    // describes nothing the frame contains. It used to arm no cell and stay in the map,
    // which handed the consumer a coordinate it is free to index with.
    for span in &mut spans {
        let count = r.u32()?;
        for _ in 0..count {
            let col = r.u16()? as usize;
            let len = r.u32()?;
            let mut cluster = Vec::new();
            for _ in 0..len {
                cluster.push(char::from_u32(r.u32()?).ok_or(DecodeError::BadTag)?);
            }
            let Some(cell) = span.cells.get_mut(col) else {
                return Err(DecodeError::BadSpan);
            };
            cell.set_combined(true);
            span.combining.insert(col, cluster);
        }
    }
    // Hyperlink reference group (v14, #621): same shape, and `LINK_PRESENT` is re-armed
    // for the same reason — it lives in the bg word, which `encode_color` drops.
    for span in &mut spans {
        let count = r.u32()?;
        for _ in 0..count {
            let col = r.u16()? as usize;
            let idx = NonZeroU32::new(r.u32()?).ok_or(DecodeError::BadTag)?;
            let Some(cell) = span.cells.get_mut(col) else {
                return Err(DecodeError::BadSpan);
            };
            cell.set_linked(true);
            span.links.insert(col, idx);
        }
    }
    // Underline-colour group (v13, #520): inverse of the encode above — one sparse
    // map per span, in span order, so each attaches to `spans[i]` positionally.
    for span in &mut spans {
        let count = r.u16()?;
        for _ in 0..count {
            let col = r.u16()? as usize;
            let color = decode_color(r.u32()?)?;
            // Re-arm `UCOLOR_PRESENT` from the group (#531). On this wire a presence
            // bit never travels *as a bit*: it lives in the bg word (`BG_UCOLOR`),
            // which `encode_color` drops when it keeps only mode+value, and
            // `CellFlags` carries no presence bits. So every one of them is
            // *reconstructed* from whether its group carries an entry — the same
            // derivation `decode_cell` performs for `combined`/`linked`, which can do
            // it inline only because `extra`/`link` ride the cell record while a
            // colour reference rides a separate group. `Row::ucolor_at` gates the map
            // read on this bit, so without the re-arm `Cell::is_ucolored()` returns
            // false on a decoded cell whose column *does* carry a colour.
            //
            // `col` is attacker-influenced (see `tests/robustness.rs`) and nothing on
            // the wire bounds it against the span's width, so an unchecked
            // `span.cells[col]` would panic where ADR-0008 owes a typed error. #531
            // bought the safety with a gate that kept the entry; #582 answers the
            // question that gate deferred — the entry is rejected, because a colour for
            // a cell this span does not have is not a colour the frame carries.
            let Some(cell) = span.cells.get_mut(col) else {
                return Err(DecodeError::BadSpan);
            };
            cell.set_ucolored(true);
            span.ucolors.insert(col, color);
        }
    }
    // Overlay section (#108): selection group then match group, each a count +
    // `(row, left, right)` triples (inverse of `encode_overlay_spans`).
    let selection = decode_overlay_spans(&mut r)?;
    let matches = decode_overlay_spans(&mut r)?;
    // Third group (#118): marker `(id u32, row u16)` records, each followed by a
    // kind discriminant (v10, #159) and — for `CommandFinished` — a presence byte
    // + i32 exit (inverse of the marker encode loop).
    let marker_count = r.u16()?;
    let mut markers = Vec::with_capacity(marker_count as usize);
    for _ in 0..marker_count {
        let id = MarkerId(r.u32()?);
        let row = r.u16()? as usize;
        let kind = match r.u8()? {
            0 => MarkerKind::Plain,
            1 => MarkerKind::PromptStart,
            2 => MarkerKind::CommandStart,
            3 => MarkerKind::OutputStart,
            4 => {
                // Always read presence + i32 (the encoder writes both); a 0
                // presence means the exit bytes are padding to discard.
                let present = r.u8()? != 0;
                let exit = r.u32()? as i32;
                MarkerKind::CommandFinished(present.then_some(exit))
            }
            _ => return Err(DecodeError::BadTag),
        };
        markers.push(MarkerPosition { id, row, kind });
    }
    // Fourth group (#120 S3, v11): every live marker's `(id u32, line u32)` — the
    // absolute-line superset for the overview ruler (inverse of the encode loop).
    let marker_line_count = r.u16()?;
    let mut marker_lines = Vec::with_capacity(marker_line_count as usize);
    for _ in 0..marker_line_count {
        let id = MarkerId(r.u32()?);
        let line = r.u32()?;
        marker_lines.push(MarkerLine { id, line });
    }
    // Fifth group (#428, v12): the active search match's spans (inverse of the
    // tail `encode_overlay_spans` call).
    let active_match = decode_overlay_spans(&mut r)?;
    let overlay = Overlay {
        selection,
        matches,
        markers,
        marker_lines,
        active_match,
    };
    Ok(Frame {
        cols,
        rows,
        kind,
        cursor_row,
        cursor_col,
        cursor_visible,
        cursor_shape,
        cursor_blink,
        display_offset,
        scrollback_len,
        mouse_events,
        alt_screen,
        scroll,
        spans,
        link_table,
        overlay,
    })
}

/// Decode one overlay group: a u16 span count, then that many `(row, left,
/// right)` u16 triples back into viewport [`SelectionSpan`]s (inverse of
/// [`encode_overlay_spans`]).
fn decode_overlay_spans(r: &mut Reader) -> Result<Vec<SelectionSpan>, DecodeError> {
    let count = r.u32()?;
    // No `with_capacity`: the count is attacker-influenced and now u32 — see `decode`.
    let mut spans = Vec::new();
    for _ in 0..count {
        let row = r.u16()? as usize;
        let left = r.u16()? as usize;
        let right = r.u16()? as usize;
        spans.push(SelectionSpan { row, left, right });
    }
    Ok(spans)
}

/// Decode one 14-byte cell record (inverse of [`encode_cell_record`]).
///
/// **No presence bit is re-armed here — since v14 (#621), none of the three can be.**
/// They are reconstructed rather than transmitted (they live in the packed content
/// and colour words, which [`encode_color`] and `Cell::c` strip), and every one of
/// them now has its evidence in a sparse group read further down the buffer:
/// `C_COMBINED` and `LINK_PRESENT` from the combining and link groups, and
/// `UCOLOR_PRESENT` from the underline-colour group as it always was (#531).
///
/// This doc used to say *two of the three are re-armed here*, and that exception —
/// "mine rides the record, so it needs nothing" — is the reading #531 was filed for.
/// There is no longer a member of the set it could apply to.
fn decode_cell(r: &mut Reader) -> Result<Cell, DecodeError> {
    let c = char::from_u32(r.u32()?).ok_or(DecodeError::BadTag)?;
    let fg = decode_color(r.u32()?)?;
    let bg = decode_color(r.u32()?)?;
    let flags = CellFlags::from_bits_retain(r.u16()?);
    // `C_COMBINED` and `LINK_PRESENT` are deliberately left off here and re-armed by
    // `decode` from the per-span groups (v14, #621). This function used to set them
    // from the record's `extra`/`link`; with those gone it has nothing to read, and
    // guessing `false` is correct precisely because the groups are the authority.
    Ok(Cell::from_parts(c, fg, bg, flags))
}

/// Decode a tagged-u32 colour reference (inverse of [`encode_color`]).
fn decode_color(v: u32) -> Result<Color, DecodeError> {
    let payload = v & 0x00FF_FFFF;
    match v >> 24 {
        0 => Ok(Color::Default),
        1 => Ok(Color::Indexed(payload as u8)),
        2 => Ok(Color::Rgb(
            (payload >> 16) as u8,
            (payload >> 8) as u8,
            payload as u8,
        )),
        _ => Err(DecodeError::BadTag),
    }
}

/// A little-endian cursor over the wire bytes.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Truncated)?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(DecodeError::Truncated)?;
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}
