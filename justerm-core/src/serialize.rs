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
const VERSION: u8 = 8; // v8 adds the mouse wanted-events mask in the header (#129/ADR-0016); v7 overlay marker group (#118/ADR-0015); v6 overlay selection + search-match spans (#108/ADR-0014); v5 scroll position (#112/ADR-0013); v4 cursor shape+blink (#81); v3 cursor row/col/visibility (#38)

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
/// `combining` and `links` map a span-relative column to its frame-local index
/// (1-based) — `combining` into [`Frame::side_table`], `links` into
/// [`Frame::link_table`]. These are the per-cell `extra`/`link` references lifted
/// out of the cell now that combining clusters (#45) and hyperlinks (#46) live in
/// per-row maps. A column is present iff its cell carries the matching bit; on the
/// wire they are the cell record's `extra`/`link` fields, so the bytes are
/// unchanged.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Span {
    pub line: u16,
    pub left: u16,
    pub right: u16,
    pub cells: Vec<Cell>,
    pub combining: BTreeMap<usize, NonZeroU32>,
    pub links: BTreeMap<usize, NonZeroU32>,
}

/// A stable handle to a buffer line, handed out by `Engine::add_marker` (#118).
/// Monotonic per engine. The consumer attaches a decoration to the id; the frame
/// reports where the marker currently sits, and `TermEvent::MarkerDisposed`
/// signals when its line has left the buffer.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MarkerId(pub u32);

/// A marker projected onto the viewport (#118): its id and the row it sits on.
/// Only markers visible in the current viewport are reported; an off-screen
/// marker is omitted but still alive (death comes via `MarkerDisposed`, not
/// absence — so the consumer can tell "scrolled away" from "gone").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MarkerPosition {
    pub id: MarkerId,
    pub row: usize,
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
    /// The active search highlights projected onto visible rows. Search matches
    /// are consumer-owned (next/prev navigation holds the `Vec<Match>`), so the
    /// consumer hands the active set back via `set_search_highlights` and the
    /// engine projects it here — mirroring how the engine-owned selection rides.
    pub matches: Vec<SelectionSpan>,
    /// Engine-owned markers visible in this viewport (#118): persistent line
    /// anchors for decorations. Unlike the selection (cleared on a screen swap)
    /// and search highlights (invalidated on output), markers re-anchor through
    /// buffer mutation and survive an alt-screen excursion; only their viewport
    /// position rides here.
    pub markers: Vec<MarkerPosition>,
}

/// One serialized damage cycle: the decoded logical form that `encode`/`decode`
/// round-trip. `side_table` holds this frame's grapheme clusters (referenced by
/// each cell's frame-local `extra`); `link_table` holds its OSC 8 hyperlink URIs
/// (referenced by each cell's frame-local `link`).
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
    pub scroll: Option<ScrollOp>,
    pub spans: Vec<Span>,
    pub side_table: Vec<Vec<char>>,
    pub link_table: Vec<String>,
    /// Interaction overlays (selection/search highlights) for this viewport (#108).
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
    /// A span's `left` was past its `right` (would underflow the cell count).
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
        for (col, cell) in span.cells.iter().enumerate() {
            // The grapheme and hyperlink indices now ride on the span (per
            // column), not the cell.
            let extra = span.combining.get(&col).map_or(0, |n| n.get() as u16);
            let link = span.links.get(&col).map_or(0, |n| n.get() as u16);
            out.extend_from_slice(&encode_cell_record(cell, extra, link));
        }
    }
    out.extend_from_slice(&(frame.side_table.len() as u16).to_le_bytes());
    for cluster in &frame.side_table {
        out.extend_from_slice(&(cluster.len() as u16).to_le_bytes());
        for &ch in cluster {
            out.extend_from_slice(&(ch as u32).to_le_bytes());
        }
    }
    // Hyperlink side-table: each URI as a length-prefixed UTF-8 byte run (#26).
    out.extend_from_slice(&(frame.link_table.len() as u16).to_le_bytes());
    for uri in &frame.link_table {
        out.extend_from_slice(&(uri.len() as u16).to_le_bytes());
        out.extend_from_slice(uri.as_bytes());
    }
    // Overlay section (#108): each group is a u16 count then that many
    // `(row, left, right)` u16 viewport triples. Selection first, then search
    // matches. Append-only, version-gated — a future group (markers, #118) adds
    // a third count here at the next version bump.
    encode_overlay_spans(&mut out, &frame.overlay.selection);
    encode_overlay_spans(&mut out, &frame.overlay.matches);
    // Third overlay group (#118): markers as `(id u32, row u16)` pairs — a
    // different record shape from the span groups (a marker is a line anchor,
    // not a column run).
    out.extend_from_slice(&(frame.overlay.markers.len() as u16).to_le_bytes());
    for m in &frame.overlay.markers {
        out.extend_from_slice(&m.id.0.to_le_bytes());
        out.extend_from_slice(&(m.row as u16).to_le_bytes());
    }
    out
}

/// Encode one overlay group: a u16 span count, then each span as three u16s
/// (`row`, `left`, `right`) in viewport coordinates.
fn encode_overlay_spans(out: &mut Vec<u8>, spans: &[SelectionSpan]) {
    out.extend_from_slice(&(spans.len() as u16).to_le_bytes());
    for s in spans {
        out.extend_from_slice(&(s.row as u16).to_le_bytes());
        out.extend_from_slice(&(s.left as u16).to_le_bytes());
        out.extend_from_slice(&(s.right as u16).to_le_bytes());
    }
}

/// Length in bytes of one fixed-width wire cell record (see
/// [`encode_cell_record`]).
pub const CELL_RECORD_LEN: usize = 18;

/// Encode one [`Cell`] to its fixed 18-byte little-endian record:
/// `c` u32 (Unicode scalar) · `fg` u32 · `bg` u32 · `flags` u16 · `extra` u16
/// (frame-local grapheme index, 0 = none) · `link` u16 (frame-local hyperlink
/// index, 0 = none). Width derives from `flags`.
///
/// `extra` and `link` are passed in rather than read from the cell: combining
/// clusters (#45) and hyperlinks (#46) now live in per-row maps, so both indices
/// ride on the [`Span`], not the cell. The wire bytes are unchanged.
///
/// This is the single definition of the cell record layout — [`encode`] writes
/// it per span cell, and an alternate consumer (the WASM decoder, #34/ADR-0008)
/// reuses it to lay decoded cells out flat without re-implementing the layout,
/// so the two cannot drift.
pub fn encode_cell_record(cell: &Cell, extra: u16, link: u16) -> [u8; CELL_RECORD_LEN] {
    let mut r = [0u8; CELL_RECORD_LEN];
    r[0..4].copy_from_slice(&(cell.c() as u32).to_le_bytes());
    r[4..8].copy_from_slice(&encode_color(cell.fg()).to_le_bytes());
    r[8..12].copy_from_slice(&encode_color(cell.bg()).to_le_bytes());
    r[12..14].copy_from_slice(&cell.flags().bits().to_le_bytes());
    r[14..16].copy_from_slice(&extra.to_le_bytes());
    r[16..18].copy_from_slice(&link.to_le_bytes());
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
    let scroll = if has_scroll {
        let top = r.u16()? as usize;
        let bottom = r.u16()? as usize;
        let count = (r.u16()? as i16) as isize;
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
        if right < left {
            return Err(DecodeError::BadSpan);
        }
        // Widen before the arithmetic: `right - left + 1` in `u16` overflows
        // when `right == u16::MAX` (e.g. left=0, right=65535), panicking under
        // overflow checks. `right >= left` is enforced just above, so the
        // subtraction in `usize` cannot underflow.
        let n = right as usize - left as usize + 1;
        let mut cells = Vec::with_capacity(n);
        let mut combining = BTreeMap::new();
        let mut links = BTreeMap::new();
        for col in 0..n {
            let (cell, extra, link) = decode_cell(&mut r)?;
            if let Some(idx) = NonZeroU32::new(extra as u32) {
                combining.insert(col, idx);
            }
            if let Some(idx) = NonZeroU32::new(link as u32) {
                links.insert(col, idx);
            }
            cells.push(cell);
        }
        spans.push(Span {
            line,
            left,
            right,
            cells,
            combining,
            links,
        });
    }
    let side_table_count = r.u16()?;
    let mut side_table = Vec::with_capacity(side_table_count as usize);
    for _ in 0..side_table_count {
        let len = r.u16()?;
        let mut cluster = Vec::with_capacity(len as usize);
        for _ in 0..len {
            cluster.push(char::from_u32(r.u32()?).ok_or(DecodeError::BadTag)?);
        }
        side_table.push(cluster);
    }
    let link_count = r.u16()?;
    let mut link_table = Vec::with_capacity(link_count as usize);
    for _ in 0..link_count {
        let len = r.u16()? as usize;
        let bytes = r.take(len)?;
        link_table.push(String::from_utf8_lossy(bytes).into_owned());
    }
    // Overlay section (#108): selection group then match group, each a count +
    // `(row, left, right)` triples (inverse of `encode_overlay_spans`).
    let selection = decode_overlay_spans(&mut r)?;
    let matches = decode_overlay_spans(&mut r)?;
    // Third group (#118): marker `(id u32, row u16)` pairs.
    let marker_count = r.u16()?;
    let mut markers = Vec::with_capacity(marker_count as usize);
    for _ in 0..marker_count {
        let id = MarkerId(r.u32()?);
        let row = r.u16()? as usize;
        markers.push(MarkerPosition { id, row });
    }
    let overlay = Overlay {
        selection,
        matches,
        markers,
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
        scroll,
        spans,
        side_table,
        link_table,
        overlay,
    })
}

/// Decode one overlay group: a u16 span count, then that many `(row, left,
/// right)` u16 triples back into viewport [`SelectionSpan`]s (inverse of
/// [`encode_overlay_spans`]).
fn decode_overlay_spans(r: &mut Reader) -> Result<Vec<SelectionSpan>, DecodeError> {
    let count = r.u16()?;
    let mut spans = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let row = r.u16()? as usize;
        let left = r.u16()? as usize;
        let right = r.u16()? as usize;
        spans.push(SelectionSpan { row, left, right });
    }
    Ok(spans)
}

/// Decode one 18-byte cell record (inverse of [`encode_cell_record`]), returning
/// the cell and its raw `extra` grapheme index and `link` index (0 = none). A
/// non-zero index sets the corresponding presence bit; the caller records the
/// indices on the span.
fn decode_cell(r: &mut Reader) -> Result<(Cell, u16, u16), DecodeError> {
    let c = char::from_u32(r.u32()?).ok_or(DecodeError::BadTag)?;
    let fg = decode_color(r.u32()?)?;
    let bg = decode_color(r.u32()?)?;
    let flags = CellFlags::from_bits_retain(r.u16()?);
    let extra = r.u16()?;
    let link = r.u16()?;
    let mut cell = Cell::from_parts(c, fg, bg, flags);
    cell.set_combined(extra != 0);
    cell.set_linked(link != 0);
    Ok((cell, extra, link))
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
