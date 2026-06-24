//! Issue #6 — binary, reference-based wire format for a damage frame.
//!
//! `encode` a [`Frame`] to bytes, `decode` them back; the round-trip is the
//! contract. Reference-based (colour refs, Unicode scalars — never resolved RGB
//! or atlas ids) so the engine stays theme- and font-agnostic; the consumer's
//! adapter resolves references before handing cells to the renderer. Format spec
//! and rationale: `docs/architecture.md` §Serialization + ADR-0005.

use crate::cell::{Cell, CellFlags};
use crate::color::Color;
use crate::damage::ScrollOp;
use core::num::NonZeroU32;
use std::collections::BTreeMap;

/// Wire magic ("juSTerm") + format version. A new feature bumps `VERSION`.
const MAGIC: [u8; 2] = *b"JT";
const VERSION: u8 = 3; // v3 adds the per-frame cursor row/col + visibility (#38)

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
/// `combining` maps a span-relative column to its frame-local grapheme index
/// (1-based, into [`Frame::side_table`]) — the per-cell `extra` reference lifted
/// out of the cell now that combining clusters live in a per-row map (#45). A
/// column is present here iff its cell carries the combining bit; on the wire it
/// is the cell record's `extra` field, so the bytes are unchanged.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Span {
    pub line: u16,
    pub left: u16,
    pub right: u16,
    pub cells: Vec<Cell>,
    pub combining: BTreeMap<usize, NonZeroU32>,
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
    pub scroll: Option<ScrollOp>,
    pub spans: Vec<Span>,
    pub side_table: Vec<Vec<char>>,
    pub link_table: Vec<String>,
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
            // The grapheme index now rides on the span (per column), not the cell.
            let extra = span.combining.get(&col).map_or(0, |n| n.get() as u16);
            out.extend_from_slice(&encode_cell_record(cell, extra));
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
    out
}

/// Length in bytes of one fixed-width wire cell record (see
/// [`encode_cell_record`]).
pub const CELL_RECORD_LEN: usize = 18;

/// Encode one [`Cell`] to its fixed 18-byte little-endian record:
/// `c` u32 (Unicode scalar) · `fg` u32 · `bg` u32 · `flags` u16 · `extra` u16
/// (frame-local grapheme index, 0 = none) · `link` u16 (frame-local hyperlink
/// index, 0 = none). Width derives from `flags`.
///
/// `extra` is passed in rather than read from the cell: combining clusters now
/// live in a per-row map, so the grapheme index rides on the [`Span`], not the
/// cell (#45). The wire bytes are unchanged.
///
/// This is the single definition of the cell record layout — [`encode`] writes
/// it per span cell, and an alternate consumer (the WASM decoder, #34/ADR-0008)
/// reuses it to lay decoded cells out flat without re-implementing the layout,
/// so the two cannot drift.
pub fn encode_cell_record(cell: &Cell, extra: u16) -> [u8; CELL_RECORD_LEN] {
    let mut r = [0u8; CELL_RECORD_LEN];
    r[0..4].copy_from_slice(&(cell.c() as u32).to_le_bytes());
    r[4..8].copy_from_slice(&encode_color(cell.fg()).to_le_bytes());
    r[8..12].copy_from_slice(&encode_color(cell.bg()).to_le_bytes());
    r[12..14].copy_from_slice(&cell.flags().bits().to_le_bytes());
    r[14..16].copy_from_slice(&extra.to_le_bytes());
    r[16..18].copy_from_slice(&cell.link().map_or(0, |n| n.get() as u16).to_le_bytes());
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
        for col in 0..n {
            let (cell, extra) = decode_cell(&mut r)?;
            if let Some(idx) = NonZeroU32::new(extra as u32) {
                combining.insert(col, idx);
            }
            cells.push(cell);
        }
        spans.push(Span {
            line,
            left,
            right,
            cells,
            combining,
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
    Ok(Frame {
        cols,
        rows,
        kind,
        cursor_row,
        cursor_col,
        cursor_visible,
        scroll,
        spans,
        side_table,
        link_table,
    })
}

/// Decode one 18-byte cell record (inverse of [`encode_cell_record`]), returning
/// the cell and its raw `extra` grapheme index (0 = none). A non-zero `extra`
/// sets the cell's combining bit; the caller records the index on the span.
fn decode_cell(r: &mut Reader) -> Result<(Cell, u16), DecodeError> {
    let c = char::from_u32(r.u32()?).ok_or(DecodeError::BadTag)?;
    let fg = decode_color(r.u32()?)?;
    let bg = decode_color(r.u32()?)?;
    let flags = CellFlags::from_bits_retain(r.u16()?);
    let extra = r.u16()?;
    let link = NonZeroU32::new(r.u16()? as u32);
    let mut cell = Cell::from_parts(c, fg, bg, flags, link);
    cell.set_combined(extra != 0);
    Ok((cell, extra))
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
