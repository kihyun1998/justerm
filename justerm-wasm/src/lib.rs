//! WASM binding for justerm's canonical wire-format decoder (#34, ADR-0008).
//!
//! Compiles the engine's `decode` to WASM so a web consumer (PenTerm — first
//! consumer, a Tauri webview) shares *one* decoder with the native backend: the
//! backend `encode`s, the bytes cross IPC, this `decodeFrame`s them. No
//! TypeScript mirror to re-implement the format and drift as the wire `VERSION`
//! bumps, and the consumer inherits the decoder's robustness coverage (ADR-0007)
//! for free.
//!
//! Scope is **decode only** (ADR-0008). The decoder stops at *references*:
//! colour ref -> RGB, codepoint -> atlas glyph-id, and cursor drawing stay the
//! consumer's theme/renderer-specific adapter. WASM is adopted for maintenance +
//! consistency, *not* speed — see ADR-0008's "Non-goal" note.
//!
//! ## Structure
//! [`flatten`] is the pure core (`Frame` -> renderer-friendly flat buffers),
//! testable with plain `cargo test` — no wasm runtime. [`DecodedFrame`] is the
//! thin `#[wasm_bindgen]` layer that exposes [`Flat`]'s buffers to JS as
//! zero-copy typed-array views.

use justerm::{Frame, FrameKind, decode};
use wasm_bindgen::prelude::*;

/// Number of `u32` fields per span in the flat span directory:
/// `line, left, right, cell_offset, cell_count`.
const SPAN_STRIDE: usize = 5;

/// A decoded frame flattened to renderer-friendly buffers — the pure core the
/// `#[wasm_bindgen]` layer exposes as views. Kept separate from the binding so
/// it is testable with plain `cargo test`, no wasm runtime.
#[derive(Debug, Default, PartialEq, Eq)]
struct Flat {
    cols: u16,
    rows: u16,
    /// `0` = Full, `1` = Partial.
    kind: u8,
    /// `(top, bottom, count)` of the frame's scroll op, applied before spans.
    scroll: Option<(u16, u16, i16)>,
    /// Per-cell base codepoint (`cell.c`), span order — the `codepoints` column.
    codepoints: Vec<u32>,
    /// Per-cell foreground/background colour refs as tagged u32s (see
    /// `justerm::encode_color`) — the `fg`/`bg` columns.
    fg: Vec<u32>,
    bg: Vec<u32>,
    /// Per-cell `CellFlags` bits — the `flags` column.
    flags: Vec<u16>,
    /// Per-cell frame-local side-table / hyperlink indices (`0` = none) — the
    /// `extra` / `link` columns.
    extra: Vec<u16>,
    link: Vec<u16>,
    /// Span directory: `SPAN_STRIDE` `u32`s per span — see [`SPAN_STRIDE`].
    /// `cell_offset` is the index of the span's first cell within the cell
    /// columns (`codepoints`/`fg`/…); `cell_count` is its number of cells.
    spans: Vec<u32>,
    /// Grapheme clusters referenced by cells' `extra` index (frame-local).
    side_table: Vec<Vec<char>>,
    /// OSC 8 hyperlink URIs referenced by cells' `link` index (frame-local).
    link_table: Vec<String>,
}

/// Flatten a decoded [`Frame`] into renderer-friendly buffers ([`Flat`]).
///
/// Cells are de-interleaved into one column per field (structure-of-arrays), so a
/// consumer reads `frame.fg[i]` etc. with no byte-offset knowledge (#35). Colour
/// refs reuse `justerm::encode_color` — the single definition of the tagged-u32
/// encoding, no drift. The span directory records where each span's cells sit so
/// JS walks the *directory*, never per cell.
fn flatten(frame: &Frame) -> Flat {
    let cell_count: usize = frame.spans.iter().map(|s| s.cells.len()).sum();
    let mut codepoints = Vec::with_capacity(cell_count);
    let mut fg = Vec::with_capacity(cell_count);
    let mut bg = Vec::with_capacity(cell_count);
    let mut flags = Vec::with_capacity(cell_count);
    let mut extra = Vec::with_capacity(cell_count);
    let mut link = Vec::with_capacity(cell_count);
    let mut spans = Vec::with_capacity(frame.spans.len() * SPAN_STRIDE);
    let mut cell_offset: u32 = 0;
    for span in &frame.spans {
        let count = span.cells.len() as u32;
        spans.extend_from_slice(&[
            span.line as u32,
            span.left as u32,
            span.right as u32,
            cell_offset,
            count,
        ]);
        cell_offset += count;
        for cell in &span.cells {
            codepoints.push(cell.c as u32);
            fg.push(justerm::encode_color(cell.fg));
            bg.push(justerm::encode_color(cell.bg));
            flags.push(cell.flags.bits());
            extra.push(cell.extra.map_or(0, |n| n.get() as u16));
            link.push(cell.link.map_or(0, |n| n.get() as u16));
        }
    }

    Flat {
        cols: frame.cols,
        rows: frame.rows,
        kind: match frame.kind {
            FrameKind::Full => 0,
            FrameKind::Partial => 1,
        },
        scroll: frame
            .scroll
            .map(|s| (s.top as u16, s.bottom as u16, s.count as i16)),
        codepoints,
        fg,
        bg,
        flags,
        extra,
        link,
        spans,
        side_table: frame.side_table.clone(),
        link_table: frame.link_table.clone(),
    }
}

/// A decoded damage frame, presented for a web renderer.
///
/// Scalars come via getters; cells are exposed as **structure-of-arrays** — one
/// zero-copy typed-array column per field (`codepoints`/`fg`/`bg`/`flags`/
/// `extra`/`link`) plus the `spans` directory — so a consumer reads `frame.fg[i]`
/// with no byte-offset knowledge and no per-cell boundary crossing (#34/#35).
#[wasm_bindgen]
pub struct DecodedFrame {
    flat: Flat,
}

#[wasm_bindgen]
impl DecodedFrame {
    #[wasm_bindgen(getter)]
    pub fn cols(&self) -> u16 {
        self.flat.cols
    }

    #[wasm_bindgen(getter)]
    pub fn rows(&self) -> u16 {
        self.flat.rows
    }

    /// `0` = Full (every row present), `1` = Partial (only the listed spans).
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> u8 {
        self.flat.kind
    }

    #[wasm_bindgen(getter, js_name = hasScroll)]
    pub fn has_scroll(&self) -> bool {
        self.flat.scroll.is_some()
    }

    #[wasm_bindgen(getter, js_name = scrollTop)]
    pub fn scroll_top(&self) -> u16 {
        self.flat.scroll.map_or(0, |s| s.0)
    }

    #[wasm_bindgen(getter, js_name = scrollBottom)]
    pub fn scroll_bottom(&self) -> u16 {
        self.flat.scroll.map_or(0, |s| s.1)
    }

    #[wasm_bindgen(getter, js_name = scrollCount)]
    pub fn scroll_count(&self) -> i16 {
        self.flat.scroll.map_or(0, |s| s.2)
    }

    /// Per-cell base codepoints (`cell.c` as `u32`), in span order — one of the
    /// structure-of-arrays cell columns (#35). Zero-copy view into WASM memory;
    /// the bulk data reaches JS with no per-cell boundary crossing (#34 AC3).
    ///
    /// # Lifetime (applies to every column + `spans`)
    /// The returned array views WASM memory directly; it is invalidated if that
    /// memory grows (e.g. the next `decodeFrame` call allocates). Read it before
    /// the next decode.
    #[wasm_bindgen(getter)]
    pub fn codepoints(&self) -> js_sys::Uint32Array {
        // SAFETY: the view borrows `self`-owned memory; consume before the next
        // WASM allocation. (Same for every column getter below.)
        unsafe { js_sys::Uint32Array::view(&self.flat.codepoints) }
    }

    /// Per-cell foreground colour references as tagged `u32`s (high byte = tag
    /// `Default|Indexed|Rgb`, low 24 = payload). Resolve with `resolveRgb`.
    #[wasm_bindgen(getter)]
    pub fn fg(&self) -> js_sys::Uint32Array {
        unsafe { js_sys::Uint32Array::view(&self.flat.fg) }
    }

    /// Per-cell background colour references (tagged `u32`s, as [`DecodedFrame::fg`]).
    #[wasm_bindgen(getter)]
    pub fn bg(&self) -> js_sys::Uint32Array {
        unsafe { js_sys::Uint32Array::view(&self.flat.bg) }
    }

    /// Per-cell `CellFlags` bits. Test with the constants from `flags()`.
    #[wasm_bindgen(getter)]
    pub fn flags(&self) -> js_sys::Uint16Array {
        unsafe { js_sys::Uint16Array::view(&self.flat.flags) }
    }

    /// Per-cell frame-local grapheme side-table index (`0` = none; else
    /// `sideTable[extra - 1]`).
    #[wasm_bindgen(getter)]
    pub fn extra(&self) -> js_sys::Uint16Array {
        unsafe { js_sys::Uint16Array::view(&self.flat.extra) }
    }

    /// Per-cell frame-local hyperlink index (`0` = none; else `linkTable[link - 1]`).
    #[wasm_bindgen(getter)]
    pub fn link(&self) -> js_sys::Uint16Array {
        unsafe { js_sys::Uint16Array::view(&self.flat.link) }
    }

    /// Span directory: 5 `u32`s per span — `line, left, right, cell_offset,
    /// cell_count` — where `cell_offset` indexes the cell columns (cell k of a
    /// span is column index `cell_offset + k`). JS walks this directory, never per
    /// cell (#34 AC3). Same zero-copy view lifetime as the columns.
    #[wasm_bindgen(getter)]
    pub fn spans(&self) -> js_sys::Uint32Array {
        unsafe { js_sys::Uint32Array::view(&self.flat.spans) }
    }

    /// This frame's grapheme clusters, each joined into a string, indexed by a
    /// cell's `extra` field (1-based; index 0 means none). Small and rare, so
    /// copied to a JS array rather than viewed.
    #[wasm_bindgen(getter, js_name = sideTable)]
    pub fn side_table(&self) -> Vec<String> {
        self.flat
            .side_table
            .iter()
            .map(|cluster| cluster.iter().collect())
            .collect()
    }

    /// This frame's OSC 8 hyperlink URIs, indexed by a cell's `link` field
    /// (1-based; index 0 means none). Small and rare, so copied to a JS array.
    #[wasm_bindgen(getter, js_name = linkTable)]
    pub fn link_table(&self) -> Vec<String> {
        self.flat.link_table.clone()
    }
}

/// The wire-format version this decoder understands (the `VERSION` byte gating
/// ADR-0005). A consumer can read it at load time to assert the WASM decoder and
/// the backend encoder agree before any frame flows; `decodeFrame` also returns a
/// `BadVersion` error on mismatch, so a stale artifact fails loudly.
#[wasm_bindgen(js_name = wireVersion)]
pub fn wire_version() -> u8 {
    justerm::WIRE_VERSION
}

/// The `CellFlags` bit positions, exported so a consumer tests `flags[i] & F.bold`
/// without hard-coding bit values (#36). The values come straight from Rust
/// `CellFlags`, so there is no JS mirror to drift. Read once and cache (e.g.
/// destructure the result): the bits never change within a build.
#[wasm_bindgen]
pub struct Flags {
    pub bold: u16,
    pub dim: u16,
    pub italic: u16,
    pub underline: u16,
    pub blink: u16,
    pub inverse: u16,
    pub hidden: u16,
    pub strikethrough: u16,
    pub wide_char: u16,
    pub wide_char_spacer: u16,
    pub wrapline: u16,
}

/// The `CellFlags` bit constants (see [`Flags`]).
#[wasm_bindgen(js_name = flags)]
pub fn flags() -> Flags {
    use justerm::CellFlags as F;
    Flags {
        bold: F::BOLD.bits(),
        dim: F::DIM.bits(),
        italic: F::ITALIC.bits(),
        underline: F::UNDERLINE.bits(),
        blink: F::BLINK.bits(),
        inverse: F::INVERSE.bits(),
        hidden: F::HIDDEN.bits(),
        strikethrough: F::STRIKETHROUGH.bits(),
        wide_char: F::WIDE_CHAR.bits(),
        wide_char_spacer: F::WIDE_CHAR_SPACER.bits(),
        wrapline: F::WRAPLINE.bits(),
    }
}

/// Resolve a 16-colour ANSI scheme into the full xterm 256-colour table (#36).
///
/// Slots `0..16` are the supplied ANSI colours (the theme's values); `16..256`
/// are the fixed xterm 6×6×6 cube + grayscale ramp, computed here so a consumer
/// never re-implements that standard. Returns an **owned** copy (built per scheme;
/// it outlives many `decodeFrame` calls). `ansi` is expected to have 16 entries
/// (extras ignored, missing treated as `0`). The default fg/bg are *not* part of
/// the 256 — the consumer keeps them and passes them to `resolveRgb`.
#[wasm_bindgen(js_name = buildPalette)]
pub fn build_palette(ansi: &[u32]) -> Vec<u32> {
    let mut colors = vec![0u32; 256];
    for (slot, &c) in colors.iter_mut().zip(ansi.iter()).take(16) {
        *slot = c;
    }
    // 6×6×6 cube, indices 16..=231: each component picks one of six fixed levels.
    const LEVELS: [u32; 6] = [0, 95, 135, 175, 215, 255];
    for n in 0..216u32 {
        let r = LEVELS[(n / 36) as usize];
        let g = LEVELS[((n / 6) % 6) as usize];
        let b = LEVELS[(n % 6) as usize];
        colors[16 + n as usize] = (r << 16) | (g << 8) | b;
    }
    // Grayscale ramp, indices 232..=255: value = 8 + 10·i (i = 0..24), 8..=238.
    for i in 0..24u32 {
        let v = 8 + 10 * i;
        colors[232 + i as usize] = (v << 16) | (v << 8) | v;
    }
    colors
}

/// Decode a justerm wire buffer (ADR-0005) into a [`DecodedFrame`].
///
/// On a malformed buffer this throws a JS `Error` carrying the `DecodeError`
/// variant name — the validation a hand-written TS decoder would otherwise have
/// to re-implement (and fuzz). Identical bytes yield a frame identical to the
/// native `justerm::decode` (the build-parity test, #34 AC2).
#[wasm_bindgen(js_name = decodeFrame)]
pub fn decode_frame(bytes: &[u8]) -> Result<DecodedFrame, JsValue> {
    let frame = decode(bytes).map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
    Ok(DecodedFrame {
        flat: flatten(&frame),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use justerm::{Cell, CellFlags, Color, Span};

    /// Build a plain ASCII span of `s` on `line` starting at column `left`.
    fn ascii_span(line: u16, left: u16, s: &str) -> Span {
        let cells: Vec<Cell> = s
            .chars()
            .map(|c| Cell {
                c,
                ..Cell::default()
            })
            .collect();
        Span {
            line,
            left,
            right: left + cells.len() as u16 - 1,
            cells,
        }
    }

    fn partial(cols: u16, rows: u16, spans: Vec<Span>) -> Frame {
        Frame {
            cols,
            rows,
            kind: FrameKind::Partial,
            scroll: None,
            spans,
            side_table: vec![],
            link_table: vec![],
        }
    }

    // --- #36: build_palette (xterm 256-colour table) ---

    /// The 16 base ANSI colours a consumer would pass (values are arbitrary here;
    /// `build_palette` must echo them into slots 0..15 verbatim).
    const ANSI16: [u32; 16] = [
        0x000000, 0x800000, 0x008000, 0x808000, 0x000080, 0x800080, 0x008080, 0xc0c0c0, 0x808080,
        0xff0000, 0x00ff00, 0xffff00, 0x0000ff, 0xff00ff, 0x00ffff, 0xffffff,
    ];

    #[test]
    fn build_palette_passes_through_the_16_ansi_colours() {
        let colors = build_palette(&ANSI16);
        assert_eq!(colors.len(), 256);
        assert_eq!(&colors[..16], &ANSI16[..]);
    }

    #[test]
    fn build_palette_fills_the_6x6x6_cube() {
        let colors = build_palette(&ANSI16);
        // Verified against published xterm values (ditig 256-colours cheat sheet).
        assert_eq!(colors[16], 0x000000);
        assert_eq!(colors[21], 0x0000ff);
        assert_eq!(colors[88], 0x870000);
        assert_eq!(colors[196], 0xff0000);
        assert_eq!(colors[226], 0xffff00);
        assert_eq!(colors[231], 0xffffff);
    }

    #[test]
    fn build_palette_fills_the_grayscale_ramp() {
        let colors = build_palette(&ANSI16);
        // Verified against published xterm values (ditig 256-colours cheat sheet).
        assert_eq!(colors[232], 0x080808);
        assert_eq!(colors[244], 0x808080);
        assert_eq!(colors[255], 0xeeeeee);
    }

    #[test]
    fn flags_constants_match_cell_flags_bits() {
        let f = flags();
        assert_eq!(f.bold, CellFlags::BOLD.bits());
        assert_eq!(f.dim, CellFlags::DIM.bits());
        assert_eq!(f.italic, CellFlags::ITALIC.bits());
        assert_eq!(f.underline, CellFlags::UNDERLINE.bits());
        assert_eq!(f.blink, CellFlags::BLINK.bits());
        assert_eq!(f.inverse, CellFlags::INVERSE.bits());
        assert_eq!(f.hidden, CellFlags::HIDDEN.bits());
        assert_eq!(f.strikethrough, CellFlags::STRIKETHROUGH.bits());
        assert_eq!(f.wide_char, CellFlags::WIDE_CHAR.bits());
        assert_eq!(f.wide_char_spacer, CellFlags::WIDE_CHAR_SPACER.bits());
        assert_eq!(f.wrapline, CellFlags::WRAPLINE.bits());
    }

    // --- #35: structure-of-arrays cell columns ---

    #[test]
    fn flatten_exposes_codepoints_column() {
        let frame = partial(
            80,
            24,
            vec![ascii_span(0, 0, "hi"), ascii_span(1, 5, "abc")],
        );
        let flat = flatten(&frame);
        assert_eq!(
            flat.codepoints,
            vec!['h' as u32, 'i' as u32, 'a' as u32, 'b' as u32, 'c' as u32]
        );
    }

    #[test]
    fn flatten_exposes_fg_bg_columns_as_tagged_refs() {
        let cells = vec![
            Cell {
                c: 'A',
                fg: Color::Indexed(9),
                bg: Color::Rgb(1, 2, 3),
                ..Cell::default()
            },
            Cell {
                c: 'B',
                ..Cell::default() // Default fg/bg
            },
        ];
        let frame = partial(
            80,
            24,
            vec![Span {
                line: 0,
                left: 0,
                right: 1,
                cells,
            }],
        );
        let flat = flatten(&frame);
        // tagged u32: high byte = tag (0 Default, 1 Indexed, 2 Rgb), low 24 = payload.
        assert_eq!(flat.fg, vec![(1 << 24) | 9, 0]);
        assert_eq!(flat.bg, vec![(2 << 24) | (1 << 16) | (2 << 8) | 3, 0]);
    }

    #[test]
    fn flatten_exposes_flags_extra_link_columns() {
        use core::num::NonZeroU32;
        let cells = vec![
            Cell {
                c: 'A',
                flags: CellFlags::BOLD | CellFlags::ITALIC,
                extra: NonZeroU32::new(3),
                link: NonZeroU32::new(7),
                ..Cell::default()
            },
            Cell {
                c: 'B',
                ..Cell::default() // no flags / extra / link
            },
        ];
        let frame = partial(
            80,
            24,
            vec![Span {
                line: 0,
                left: 0,
                right: 1,
                cells,
            }],
        );
        let flat = flatten(&frame);
        assert_eq!(
            flat.flags,
            vec![(CellFlags::BOLD | CellFlags::ITALIC).bits(), 0]
        );
        assert_eq!(flat.extra, vec![3, 0]); // 1-based index, 0 = none
        assert_eq!(flat.link, vec![7, 0]);
    }

    #[test]
    fn flatten_carries_scalars_and_scroll() {
        let mut frame = partial(120, 40, vec![]);
        frame.kind = FrameKind::Full;
        frame.scroll = Some(justerm::ScrollOp {
            top: 2,
            bottom: 39,
            count: -3,
        });
        let flat = flatten(&frame);
        assert_eq!((flat.cols, flat.rows, flat.kind), (120, 40, 0));
        assert_eq!(flat.scroll, Some((2, 39, -3)));
    }

    // --- S2: span directory ---

    #[test]
    fn flatten_builds_span_directory_with_record_offsets() {
        let frame = partial(
            80,
            24,
            vec![ascii_span(0, 0, "hi"), ascii_span(1, 5, "abc")],
        );
        let flat = flatten(&frame);
        // [line, left, right, cell_offset(records), cell_count] per span.
        assert_eq!(
            flat.spans,
            vec![
                0, 0, 1, 0, 2, // "hi" at row 0, cols 0..=1, first 2 records
                1, 5, 7, 2, 3, // "abc" at row 1, cols 5..=7, next 3 records
            ]
        );
    }

    // --- S2: side-table + link-table carried through ---

    #[test]
    fn flatten_carries_side_and_link_tables() {
        let mut frame = partial(80, 24, vec![ascii_span(0, 0, "x")]);
        frame.side_table = vec![vec!['e', '\u{301}'], vec!['a', '\u{308}']];
        frame.link_table = vec!["https://example.com".to_string()];
        let flat = flatten(&frame);
        assert_eq!(
            flat.side_table,
            vec![vec!['e', '\u{301}'], vec!['a', '\u{308}']]
        );
        assert_eq!(flat.link_table, vec!["https://example.com".to_string()]);
    }

    // --- S3/AC2: flatten faithfully represents the native-decoded frame ---

    #[test]
    fn flatten_matches_native_decode_for_a_rich_frame() {
        use core::num::NonZeroU32;
        // A frame exercising wide chars, colours, a grapheme ref, a link ref,
        // scroll, and multiple spans — then round-tripped through the real wire.
        let wide = Cell {
            c: '한',
            flags: CellFlags::WIDE_CHAR,
            ..Cell::default()
        };
        let spacer = Cell {
            c: ' ',
            flags: CellFlags::WIDE_CHAR_SPACER,
            ..Cell::default()
        };
        let coloured = Cell {
            c: 'A',
            fg: Color::Indexed(9),
            bg: Color::Rgb(1, 2, 3),
            flags: CellFlags::BOLD,
            extra: NonZeroU32::new(1),
            link: NonZeroU32::new(1),
        };
        let frame = Frame {
            cols: 80,
            rows: 24,
            kind: FrameKind::Full,
            scroll: Some(justerm::ScrollOp {
                top: 0,
                bottom: 23,
                count: 5,
            }),
            spans: vec![
                Span {
                    line: 0,
                    left: 0,
                    right: 2,
                    cells: vec![wide, spacer, coloured],
                },
                ascii_span(3, 10, "hi"),
            ],
            side_table: vec![vec!['e', '\u{301}']],
            link_table: vec!["https://x.example".to_string()],
        };

        let bytes = justerm::encode(&frame);
        let native = justerm::decode(&bytes).expect("native decode");
        let flat = flatten(&native);

        // Scalars + scroll + tables match the native frame.
        assert_eq!((flat.cols, flat.rows, flat.kind), (80, 24, 0));
        assert_eq!(flat.scroll, Some((0, 23, 5)));
        assert_eq!(flat.side_table, native.side_table);
        assert_eq!(flat.link_table, native.link_table);

        // SoA columns: each equals the corresponding field of every native cell,
        // in span order (decode -> flatten preserves every cell, no drop/reorder).
        let mut exp_codepoints = Vec::new();
        let mut exp_fg = Vec::new();
        let mut exp_bg = Vec::new();
        let mut exp_flags = Vec::new();
        let mut exp_extra = Vec::new();
        let mut exp_link = Vec::new();
        let mut expected_spans = Vec::new();
        let mut off: u32 = 0;
        for span in &native.spans {
            let n = span.cells.len() as u32;
            expected_spans.extend_from_slice(&[
                span.line as u32,
                span.left as u32,
                span.right as u32,
                off,
                n,
            ]);
            off += n;
            for cell in &span.cells {
                exp_codepoints.push(cell.c as u32);
                exp_fg.push(justerm::encode_color(cell.fg));
                exp_bg.push(justerm::encode_color(cell.bg));
                exp_flags.push(cell.flags.bits());
                exp_extra.push(cell.extra.map_or(0, |x| x.get() as u16));
                exp_link.push(cell.link.map_or(0, |x| x.get() as u16));
            }
        }
        assert_eq!(flat.codepoints, exp_codepoints);
        assert_eq!(flat.fg, exp_fg);
        assert_eq!(flat.bg, exp_bg);
        assert_eq!(flat.flags, exp_flags);
        assert_eq!(flat.extra, exp_extra);
        assert_eq!(flat.link, exp_link);
        assert_eq!(flat.spans, expected_spans);
    }
}
