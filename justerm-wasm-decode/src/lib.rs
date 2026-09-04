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
//! `flatten` is the pure core (`Frame` -> renderer-friendly flat buffers),
//! testable with plain `cargo test` — no wasm runtime. [`DecodedFrame`] is the
//! thin `#[wasm_bindgen]` layer that exposes `Flat`'s buffers to JS as
//! zero-copy typed-array views.

use justerm_core::{Frame, FrameKind, MarkerKind, decode};
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
    /// Cursor row/col (screen coords, 0-based) + DECTCEM visibility (#38). The
    /// consumer reads these to draw the caret (cell-invert / overlay); justerm
    /// only reports state.
    cursor_row: u16,
    cursor_col: u16,
    cursor_visible: bool,
    /// Caret shape (`0` = Block, `1` = Underline, `2` = Bar) + blink (#81).
    cursor_shape: u8,
    cursor_blink: bool,
    /// Viewport scroll position for the consumer's scrollbar (#112 / ADR-0013):
    /// `display_offset` lines scrolled up from the bottom (0 = following), and
    /// `scrollback_len` history lines (total height = `+ rows`).
    display_offset: u32,
    scrollback_len: u32,
    /// Marker-index basis (#490): lines evicted since RIS, and the epoch that says a
    /// pulled marker index went stale for a reason the eviction delta cannot express.
    evicted_total: u64,
    marker_epoch: u32,
    /// How many markers are live in the active buffer (#490, v16) — the drift check
    /// for a consumer maintaining a pulled index.
    marker_count: u32,
    /// Mouse wanted-events mask (#129) — the routing bits the active tracking
    /// mode reports (DOWN/UP/WHEEL/DRAG/MOVE). `0` = no reporting.
    mouse_events: u8,
    /// Whether the alternate screen is active (#149) — gates the a11y announce
    /// policy (#119), which the frame-mode consumer can't derive from damage.
    alt_screen: bool,
    /// `(top, bottom, count)` of the frame's scroll op, applied before spans.
    scroll: Option<(u16, u16, i16)>,
    /// Per-cell base codepoint (`cell.c`), span order — the `codepoints` column.
    codepoints: Vec<u32>,
    /// Per-cell foreground/background colour refs as tagged u32s (see
    /// `justerm_core::encode_color`) — the `fg`/`bg` columns.
    fg: Vec<u32>,
    bg: Vec<u32>,
    /// Per-cell underline colour ref (SGR 58, #520) as a tagged u32, `0` = Default
    /// (follow the fg) — the `underlineColor` column. Densified from the wire's
    /// sparse per-span group: a cell with no coloured underline reads `0`.
    underline_color: Vec<u32>,
    /// Per-cell `CellFlags` bits — the `flags` column.
    flags: Vec<u16>,
    /// Per-cell frame-local side-table / hyperlink indices (`0` = none) — the
    /// `extra` / `link` columns.
    ///
    /// `u32`, not `u16`, since v14 (#621). Both index tables whose size is bounded by
    /// the *viewport*, and the frame header stores `cols` and `rows` as `u16` **each**
    /// — so a legal viewport holds far more cells than a `u16` index can number.
    /// Keeping these narrow after the wire widened would have relocated the very
    /// defect #621 removed, one layer downstream, where nothing would have caught it.
    /// `types.ts` declares both as `ArrayLike<number>`, so the JS contract is unchanged.
    extra: Vec<u32>,
    link: Vec<u32>,
    /// Span directory: `SPAN_STRIDE` `u32`s per span — see [`SPAN_STRIDE`].
    /// `cell_offset` is the index of the span's first cell within the cell
    /// columns (`codepoints`/`fg`/…); `cell_count` is its number of cells.
    spans: Vec<u32>,
    /// Grapheme clusters referenced by cells' `extra` index (frame-local).
    side_table: Vec<Vec<char>>,
    /// OSC 8 hyperlink URIs referenced by cells' `link` index (frame-local).
    link_table: Vec<String>,
    /// Overlay highlight spans (#108), `OVERLAY_STRIDE` u32s per span
    /// (`row`, `left`, `right`) in viewport coords — `selection` is the live
    /// selection, `matches` the search highlights. Positions only; the
    /// consumer picks the highlight colour (theme-agnostic).
    selection_spans: Vec<u32>,
    match_spans: Vec<u32>,
    /// The consumer-designated ACTIVE search match's spans (#428, v12), same
    /// `OVERLAY_STRIDE` layout as `match_spans` — a separate directory so the
    /// renderer's active channel reads it directly. The active member is also
    /// present in `match_spans` (ranking, not exclusion, resolves the overlap).
    active_match_spans: Vec<u32>,
    /// Marker positions (#118/#159), `MARKER_STRIDE` u32s per marker
    /// (`id`, `row`, `kind`, `exitPresent`, `exitBits`).
    marker_positions: Vec<u32>,
}

/// u32s per overlay span in the `selection_spans` / `match_spans` directories:
/// `row`, `left`, `right` (viewport coordinates, inclusive columns).
pub const OVERLAY_STRIDE: usize = 3;

/// u32s per marker in the `marker_positions` directory: `id`, `row`, `kind`
/// (0 Plain, 1 PromptStart, 2 CommandStart, 3 OutputStart, 4 CommandFinished),
/// `exitPresent` (1 if the finished command reported an exit code), `exitBits`
/// (the exit as raw u32 — reinterpret as i32 on the JS side, `bits | 0`). Non-
/// `CommandFinished` markers carry `exitPresent = 0` (#159).
pub const MARKER_STRIDE: usize = 5;

/// Flatten a decoded [`Frame`] into renderer-friendly buffers ([`Flat`]).
///
/// Cells are de-interleaved into one column per field (structure-of-arrays), so a
/// consumer reads `frame.fg[i]` etc. with no byte-offset knowledge (#35). Colour
/// refs reuse `justerm_core::encode_color` — the single definition of the tagged-u32
/// encoding, no drift. The span directory records where each span's cells sit so
/// JS walks the *directory*, never per cell.
fn flatten(frame: &Frame) -> Flat {
    let cell_count: usize = frame.spans.iter().map(|s| s.cells.len()).sum();
    let mut codepoints = Vec::with_capacity(cell_count);
    let mut fg = Vec::with_capacity(cell_count);
    let mut bg = Vec::with_capacity(cell_count);
    let mut underline_color = Vec::with_capacity(cell_count);
    let mut flags = Vec::with_capacity(cell_count);
    let mut extra = Vec::with_capacity(cell_count);
    let mut link = Vec::with_capacity(cell_count);
    // Rebuilt here rather than read off the wire (v14, #621) — see the push site.
    let mut side_table: Vec<Vec<char>> = Vec::new();
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
        for (col, cell) in span.cells.iter().enumerate() {
            codepoints.push(cell.c() as u32);
            fg.push(justerm_core::encode_color(cell.fg()));
            bg.push(justerm_core::encode_color(cell.bg()));
            // Underline colour rides the span's sparse ucolor map (per column, #520),
            // like combining/link; densify to `0` (Default) where absent.
            underline_color.push(
                span.ucolors
                    .get(&col)
                    .map_or(0, |&c| justerm_core::encode_color(c)),
            );
            flags.push(cell.flags().bits());
            // Combining/link references ride on the span (per column), not the cell,
            // since slices #45/#46 moved them into per-row maps — and since v14 (#621)
            // they are sparse wire groups rather than fields on the cell record.
            //
            // The JS-facing shape is unchanged: a dense `extra` column of 1-based
            // indices into a `sideTable`. The wire no longer carries that table, so it
            // is **built here**, which is precisely this crate's job (ADR-0008: flatten
            // the wire's logical form into the renderer's structure-of-arrays). One
            // entry per combining cell, in cell order — the same thing `Term::frame`
            // used to ship, reconstructed on the consumer's side of the boundary where
            // it costs no wire bytes.
            extra.push(match span.combining.get(&col) {
                Some(cluster) => {
                    side_table.push(cluster.clone());
                    side_table.len() as u32
                }
                None => 0,
            });
            link.push(span.links.get(&col).map_or(0, |n| n.get()));
        }
    }

    Flat {
        cols: frame.cols,
        rows: frame.rows,
        kind: match frame.kind {
            FrameKind::Full => 0,
            FrameKind::Partial => 1,
        },
        cursor_row: frame.cursor_row,
        cursor_col: frame.cursor_col,
        cursor_visible: frame.cursor_visible,
        cursor_shape: match frame.cursor_shape {
            justerm_core::CursorShape::Block => 0,
            justerm_core::CursorShape::Underline => 1,
            justerm_core::CursorShape::Bar => 2,
        },
        cursor_blink: frame.cursor_blink,
        display_offset: frame.display_offset,
        scrollback_len: frame.scrollback_len,
        evicted_total: frame.evicted_total,
        marker_epoch: frame.marker_epoch,
        marker_count: frame.marker_count,
        mouse_events: frame.mouse_events.bits(),
        alt_screen: frame.alt_screen,
        scroll: frame
            .scroll
            .map(|s| (s.top as u16, s.bottom as u16, s.count as i16)),
        codepoints,
        fg,
        bg,
        underline_color,
        flags,
        extra,
        link,
        spans,
        side_table,
        link_table: frame.link_table.clone(),
        selection_spans: flatten_overlay_spans(&frame.overlay.selection),
        match_spans: flatten_overlay_spans(&frame.overlay.matches),
        active_match_spans: flatten_overlay_spans(&frame.overlay.active_match),
        marker_positions: frame
            .overlay
            .markers
            .iter()
            .flat_map(|m| {
                let (kind, present, exit) = match m.kind {
                    MarkerKind::Plain => (0, 0, 0),
                    MarkerKind::PromptStart => (1, 0, 0),
                    MarkerKind::CommandStart => (2, 0, 0),
                    MarkerKind::OutputStart => (3, 0, 0),
                    MarkerKind::CommandFinished(e) => {
                        (4, e.is_some() as u32, e.unwrap_or(0) as u32)
                    }
                };
                [m.id.0, m.row as u32, kind, present, exit]
            })
            .collect(),
    }
}

/// Flatten an overlay group into `OVERLAY_STRIDE` u32s per span (`row`, `left`,
/// `right`), so JS reads the directory as one zero-copy typed array — the same
/// structure-of-arrays treatment the cell spans get.
fn flatten_overlay_spans(spans: &[justerm_core::SelectionSpan]) -> Vec<u32> {
    let mut out = Vec::with_capacity(spans.len() * OVERLAY_STRIDE);
    for s in spans {
        out.extend_from_slice(&[s.row as u32, s.left as u32, s.right as u32]);
    }
    out
}

/// A decoded damage frame, presented for a web renderer.
///
/// Scalars come via getters; cells are exposed as **structure-of-arrays** — one
/// zero-copy typed-array column per field (`codepoints`/`fg`/`bg`/`underlineColor`/
/// `flags`/`extra`/`link`) plus the `spans` directory — so a consumer reads
/// `frame.fg[i]` with no byte-offset knowledge and no per-cell boundary crossing
/// (#34/#35).
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

    /// Cursor row (screen coords, 0-based). The consumer draws the caret here by
    /// cell-invert / overlay — justerm only reports where it is (#38).
    #[wasm_bindgen(getter, js_name = cursorRow)]
    pub fn cursor_row(&self) -> u16 {
        self.flat.cursor_row
    }

    /// Cursor column (screen coords, 0-based).
    #[wasm_bindgen(getter, js_name = cursorCol)]
    pub fn cursor_col(&self) -> u16 {
        self.flat.cursor_col
    }

    /// Whether the engine shows the cursor (DECTCEM `?25`). When `false` the
    /// consumer stops drawing the caret.
    #[wasm_bindgen(getter, js_name = cursorVisible)]
    pub fn cursor_visible(&self) -> bool {
        self.flat.cursor_visible
    }

    /// Caret shape: `0` = Block, `1` = Underline, `2` = Bar (DECSCUSR #89). The
    /// consumer draws the shape; the engine only reports it (#81).
    #[wasm_bindgen(getter, js_name = cursorShape)]
    pub fn cursor_shape(&self) -> u8 {
        self.flat.cursor_shape
    }

    /// Whether the caret blinks (att610 `?12`). The engine reports the mode; the
    /// renderer does the animation (#81).
    #[wasm_bindgen(getter, js_name = cursorBlink)]
    pub fn cursor_blink(&self) -> bool {
        self.flat.cursor_blink
    }

    /// Lines the viewport is scrolled up from the bottom (`0` = following the
    /// live screen). With [`scrollback_len`](Self::scrollback_len), sizes the
    /// consumer's scrollbar thumb (#112 / ADR-0013).
    #[wasm_bindgen(getter, js_name = displayOffset)]
    pub fn display_offset(&self) -> u32 {
        self.flat.display_offset
    }

    /// History lines in scrollback; total content height is `scrollbackLen + rows`.
    #[wasm_bindgen(getter, js_name = scrollbackLen)]
    pub fn scrollback_len(&self) -> u32 {
        self.flat.scrollback_len
    }

    /// Lines evicted from the front of scrollback since RIS (#490). Rebase a marker
    /// line pulled earlier by the delta against the value you pulled it at.
    ///
    /// `f64` rather than `u64`: the field is 64-bit on the wire so it cannot wrap in
    /// a long session, and JS numbers hold it exactly up to 2^53 — which is four
    /// orders of magnitude past any reachable eviction count, and avoids handing the
    /// consumer a `BigInt` it would have to convert at every frame.
    #[wasm_bindgen(getter, js_name = evictedTotal)]
    pub fn evicted_total(&self) -> f64 {
        self.flat.evicted_total as f64
    }

    /// Bumped when a pulled marker index went stale for a reason the eviction delta
    /// cannot express — a reflow, a region scroll that moved a marker, an alt-screen
    /// switch (#490). Re-pull when it differs from the epoch you pulled at.
    #[wasm_bindgen(getter, js_name = markerEpoch)]
    pub fn marker_epoch(&self) -> u32 {
        self.flat.marker_epoch
    }

    /// How many markers are live in the active buffer (#490, v16).
    ///
    /// Compare it against the size of a pulled index: a mismatch means the index has
    /// drifted — most likely because the create/dispose events are not being forwarded —
    /// and the answer is to pull again. It cannot see a create and a dispose inside one
    /// frame, so it is a net under the events, not a replacement for them.
    #[wasm_bindgen(getter, js_name = markerCount)]
    pub fn marker_count(&self) -> u32 {
        self.flat.marker_count
    }

    #[wasm_bindgen(getter, js_name = hasScroll)]
    pub fn has_scroll(&self) -> bool {
        self.flat.scroll.is_some()
    }

    /// The mouse wanted-events mask (#129): which event categories the active
    /// tracking mode reports (bit 0 DOWN, 1 UP, 2 WHEEL, 3 DRAG, 4 MOVE). `0` =
    /// no reporting. The consumer routes a mouse/wheel event to the app when its
    /// bit is set, else keeps it local (selection / scrollback). Encoding the
    /// report bytes stays the backend's (`encode_mouse`); only this routing mask
    /// crosses.
    #[wasm_bindgen(getter, js_name = mouseWantedEvents)]
    pub fn mouse_wanted_events(&self) -> u8 {
        self.flat.mouse_events
    }

    /// Whether the alternate screen (`?1049`/`?47`) is active (#149). The a11y
    /// announce policy (#119) suppresses output reads here — a full-screen TUI
    /// repaint isn't "new output". Buffer-global state the consumer can't derive
    /// from viewport damage.
    #[wasm_bindgen(getter, js_name = altScreen)]
    pub fn alt_screen(&self) -> bool {
        self.flat.alt_screen
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

    /// Per-cell underline colour references (SGR 58, #520) as tagged `u32`s (as
    /// [`DecodedFrame::fg`]). `0` = `Default` — the underline follows the fg. Only
    /// cells drawing a coloured underline carry a non-zero value; resolve with
    /// `resolveRgb`, the same as `fg`/`bg`.
    #[wasm_bindgen(getter, js_name = underlineColor)]
    pub fn underline_color(&self) -> js_sys::Uint32Array {
        unsafe { js_sys::Uint32Array::view(&self.flat.underline_color) }
    }

    /// Per-cell `CellFlags` bits. Test with the constants from `flags()`.
    #[wasm_bindgen(getter)]
    pub fn flags(&self) -> js_sys::Uint16Array {
        unsafe { js_sys::Uint16Array::view(&self.flat.flags) }
    }

    /// Per-cell frame-local grapheme side-table index (`0` = none; else
    /// `sideTable[extra - 1]`).
    #[wasm_bindgen(getter)]
    pub fn extra(&self) -> js_sys::Uint32Array {
        unsafe { js_sys::Uint32Array::view(&self.flat.extra) }
    }

    /// Per-cell frame-local hyperlink index (`0` = none; else `linkTable[link - 1]`).
    #[wasm_bindgen(getter)]
    pub fn link(&self) -> js_sys::Uint32Array {
        unsafe { js_sys::Uint32Array::view(&self.flat.link) }
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

    /// The live selection projected onto the viewport (#108), `OVERLAY_STRIDE`
    /// u32s per span (`row`, `left`, `right`, inclusive cols). The consumer
    /// paints the highlight; the colour is the consumer's (theme-agnostic). Same
    /// zero-copy view lifetime as the cell columns.
    #[wasm_bindgen(getter, js_name = selectionSpans)]
    pub fn selection_spans(&self) -> js_sys::Uint32Array {
        unsafe { js_sys::Uint32Array::view(&self.flat.selection_spans) }
    }

    /// The search highlights projected onto the viewport (#108), same
    /// `(row, left, right)` triple layout as [`DecodedFrame::selection_spans`].
    /// Set on the backend via `Engine::set_search_highlights`.
    #[wasm_bindgen(getter, js_name = matchSpans)]
    pub fn match_spans(&self) -> js_sys::Uint32Array {
        unsafe { js_sys::Uint32Array::view(&self.flat.match_spans) }
    }

    /// The *active* (current) search match's spans (#428, v12), same
    /// `(row, left, right)` triple layout as [`DecodedFrame::match_spans`].
    /// Designated on the backend via `Engine::set_active_search_highlight`
    /// (which match is active is the consumer's next/prev policy); also present
    /// in [`DecodedFrame::match_spans`] — the renderer's highlight ranking
    /// resolves the overlap (#424), not exclusion here. Empty when nothing is
    /// designated.
    #[wasm_bindgen(getter, js_name = activeMatchSpans)]
    pub fn active_match_spans(&self) -> js_sys::Uint32Array {
        unsafe { js_sys::Uint32Array::view(&self.flat.active_match_spans) }
    }

    /// Decoration markers visible in this viewport (#118/#159), `MARKER_STRIDE`
    /// u32s per marker (`id`, `row`, `kind`, `exitPresent`, `exitBits` — see
    /// [`MARKER_STRIDE`]). An off-screen marker is absent (still alive); disposal
    /// arrives out-of-band via the backend's `MarkerDisposed` event, so absence
    /// here is "scrolled away", not "gone".
    #[wasm_bindgen(getter, js_name = markerPositions)]
    pub fn marker_positions(&self) -> js_sys::Uint32Array {
        unsafe { js_sys::Uint32Array::view(&self.flat.marker_positions) }
    }
}

/// The wire-format version this decoder understands (the `VERSION` byte gating
/// ADR-0005). A consumer can read it at load time to assert the WASM decoder and
/// the backend encoder agree before any frame flows; `decodeFrame` also returns a
/// `BadVersion` error on mismatch, so a stale artifact fails loudly.
#[wasm_bindgen(js_name = wireVersion)]
pub fn wire_version() -> u8 {
    justerm_core::WIRE_VERSION
}

/// Whether `pattern` is a regex the engine's `search_with(regex)` can run — the
/// **same `regex` dialect**, so the web validates a regex-mode query as-you-type
/// instead of guessing with JS `RegExp` (whose grammar differs and would misjudge,
/// reproducing the silent bad-pattern-vs-no-match gap, #316 D2). Thin delegate to
/// `justerm_core::is_valid_regex` — no JS mirror to drift.
#[wasm_bindgen(js_name = isValidRegex)]
pub fn is_valid_regex(pattern: &str) -> bool {
    justerm_core::is_valid_regex(pattern)
}

/// The `CellFlags` bit positions, exported so a consumer tests `flags[i] & F.bold`
/// without hard-coding bit values (#36). The values come straight from Rust
/// `CellFlags`, so there is no JS mirror to drift. Read once and cache (e.g.
/// destructure the result): the bits never change within a build.
///
/// **This covers the flags and nothing else.** A `flags[i]` word also carries the underline
/// *style* — a 3-bit field, not a flag — and no mask here can answer "which of six", so that half
/// is [`underline_style`] (#831). The two together are the whole word a consumer needs to name.
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
    use justerm_core::CellFlags as F;
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

/// How a cell's underline is drawn — the value of `SGR 4 : Ps` (#831).
///
/// **A field, not a flag, and that is why it needs its own export.** [`Flags`] hands out one bit
/// per attribute because each of those questions is yes-or-no; this one is "which of six", so a
/// twelfth mask could not have answered it and a consumer given one would still be shifting by
/// hand — the exact thing [`Flags`] exists to prevent.
///
/// [`None`](Self::None) is a **member** of the style, not the absence of one: a cell that is not
/// underlined reads as `None`, so a consumer never infers "no style" from a zero it was not
/// promised. `flags[i] & F.underline` and a non-`None` style are the same question asked twice —
/// the engine derives the flag from this field and normalises a styleless underline to
/// [`Single`](Self::Single), so the two cannot disagree on a word this decoder produced.
///
/// Mirrors `justerm_core::UnderlineStyle`, and [`underline_style`] is the only producer. The
/// conversion there is an **exhaustive `match`** on the core enum, so a style added upstream is a
/// compile error here rather than a value arriving unnamed.
#[wasm_bindgen]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnderlineStyle {
    /// Not underlined.
    None = 0,
    /// `SGR 4` or `4:1` — one straight line.
    Single = 1,
    /// `4:2`, and the legacy `SGR 21`.
    Double = 2,
    /// `4:3` — a curl.
    Curly = 3,
    /// `4:4` — a dotted line.
    Dotted = 4,
    /// `4:5` — a dashed line.
    Dashed = 5,
}

/// The underline style carried by one `flags[i]` word (#831).
///
/// Pass the word straight from [`DecodedFrame::flags`]; the style lives in bits this API does not
/// make you know, which is the point — it delegates to `justerm_core`'s
/// `CellFlags::underline_style`, so no consumer of *this* package writes the shift or the width.
///
/// The family's own renderer is the exception and is not a counter-example: `justerm-renderer`
/// does not depend on `justerm-core`, so it re-declares the field position (`attrs.rs`,
/// `USTYLE_SHIFT`) and forwards the raw bits to a shader without naming a single value. That
/// duplication is the recorded one in `docs/map/territory/colour-policy.md`; it is what this
/// export exists so that nobody *else* has to repeat.
///
/// **Total.** The 3 bits have eight representable values and six meanings, so anything outside the
/// enum reads as [`Single`](UnderlineStyle::Single) — the same normalisation the engine applies,
/// not a second one invented here. Bits outside the field are ignored, so a whole `flags[i]` word
/// is the intended argument rather than something the caller pre-masks.
#[wasm_bindgen(js_name = underlineStyle)]
pub fn underline_style(flags: u16) -> UnderlineStyle {
    // `from_bits_retain`, not `from_bits`: the style bits are *not* declared members of
    // `CellFlags`, so the checked constructor would reject every underlined cell.
    match justerm_core::CellFlags::from_bits_retain(flags).underline_style() {
        justerm_core::UnderlineStyle::None => UnderlineStyle::None,
        justerm_core::UnderlineStyle::Single => UnderlineStyle::Single,
        justerm_core::UnderlineStyle::Double => UnderlineStyle::Double,
        justerm_core::UnderlineStyle::Curly => UnderlineStyle::Curly,
        justerm_core::UnderlineStyle::Dotted => UnderlineStyle::Dotted,
        justerm_core::UnderlineStyle::Dashed => UnderlineStyle::Dashed,
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
/// On a malformed buffer this throws a JS `Error` whose `message` is the
/// `DecodeError` variant name (ADR-0008) — the validation a hand-written TS
/// decoder would otherwise have to re-implement (and fuzz). A real `Error`, not
/// a string primitive: a consumer's `catch (e) { e.message }` reads the variant
/// rather than `undefined`, and the throw carries a stack (#662). Identical
/// bytes yield a frame identical to the native `justerm_core::decode` (the
/// build-parity test, #34 AC2).
#[wasm_bindgen(js_name = decodeFrame)]
pub fn decode_frame(bytes: &[u8]) -> Result<DecodedFrame, JsValue> {
    // `JsValue::from_str` would throw the *string* — `wasm_bindgen` throws an
    // `Err` payload verbatim, so a string primitive arrives with no `.message`,
    // no `.stack`, and `instanceof Error === false` (#662, measured).
    let frame = decode(bytes).map_err(|e| js_sys::Error::new(&format!("{e:?}")))?;
    Ok(DecodedFrame {
        flat: flatten(&frame),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::num::NonZeroU32;
    use justerm_core::{Cell, CellFlags, Color, Span, UnderlineStyle as CoreStyle};
    use std::collections::BTreeMap;

    /// Build a plain ASCII span of `s` on `line` starting at column `left`.
    fn ascii_span(line: u16, left: u16, s: &str) -> Span {
        let cells: Vec<Cell> = s
            .chars()
            .map(|c| Cell::from_parts(c, Color::Default, Color::Default, CellFlags::empty()))
            .collect();
        Span {
            line,
            left,
            right: left + cells.len() as u16 - 1,
            cells,
            combining: BTreeMap::new(),
            links: BTreeMap::new(),
            ucolors: BTreeMap::new(),
        }
    }

    fn partial(cols: u16, rows: u16, spans: Vec<Span>) -> Frame {
        Frame {
            cols,
            rows,
            kind: FrameKind::Partial,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            cursor_shape: justerm_core::CursorShape::Block,
            cursor_blink: false,
            display_offset: 0,
            scrollback_len: 0,
            evicted_total: 0,
            marker_epoch: 0,
            marker_count: 0,
            mouse_events: Default::default(),
            alt_screen: false,
            scroll: None,
            spans,
            link_table: vec![],
            overlay: Default::default(),
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

    /// The map is *complete*, which the test above cannot say (#831).
    ///
    /// `flags_constants_match_cell_flags_bits` enumerates the same eleven names `flags()` fills
    /// in, so the two lists were written by one hand and agree with each other forever: a flag
    /// added to `CellFlags` and forgotten here is invisible to it. This asserts against
    /// `CellFlags::all()` instead — **derived** from the bitflags declaration rather than
    /// restated — so the omission is what reddens, not the value.
    ///
    /// The underline style is deliberately outside this: bits 11..=13 are not declared members of
    /// `CellFlags`, so they are absent from `all()` by construction, and the field has its own
    /// guard in `underline_style_names_every_style_the_engine_can_store` — a mask cannot cover a
    /// field, and one check pretending to do both would be loose at whichever end it stretched to.
    ///
    /// **What it still cannot see**, stated so it is not read as more coverage than it is.
    ///
    /// *A twelfth constant duplicating a bit already named.* OR-ing a duplicate changes neither
    /// the union nor its population count, so no assertion over this word can notice one. A
    /// popcount check was written for exactly that and removed — it could not fire alone, and it
    /// would have been a hand-written `11` to update every time `CellFlags` grew.
    ///
    /// *Anything outside `CellFlags::all()`, which is `0x07ff` — five of the sixteen bits the
    /// published word actually carries.* Bits 11..=13 are the style, covered by the accessor's own
    /// tests. **Bits 14..=15 are reserved for a hyperlink id** (`justerm-core/src/cell.rs`) and are
    /// covered by nothing: if that lands the way the style did — a second undeclared field handled
    /// in `flag_words` — this check stays green while the published word gains another value no
    /// consumer can name. That is the same class as the omission this test was widened for, and it
    /// is recorded here rather than guessed at later.
    #[test]
    fn flags_map_covers_every_declared_cell_flag() {
        let f = flags();
        let exported = f.bold
            | f.dim
            | f.italic
            | f.underline
            | f.blink
            | f.inverse
            | f.hidden
            | f.strikethrough
            | f.wide_char
            | f.wide_char_spacer
            | f.wrapline;
        assert_eq!(
            exported,
            CellFlags::all().bits(),
            "`flags()` does not name every declared `CellFlags` member — missing {:#06x}. Add the \
             constant to `Flags` and to `flags()`; a JS consumer has no other way to name it.",
            CellFlags::all().bits() & !exported
        );
    }

    // --- #831: the underline style is a field a consumer can name ---

    /// A span whose cells each carry one underline style, in the order given.
    fn styled_span(line: u16, left: u16, styles: &[CoreStyle]) -> Span {
        let cells: Vec<Cell> = styles
            .iter()
            .map(|&s| {
                let mut f = CellFlags::empty();
                f.set_underline_style(s);
                Cell::from_parts('x', Color::Default, Color::Default, f)
            })
            .collect();
        Span {
            line,
            left,
            right: left + cells.len() as u16 - 1,
            cells,
            combining: BTreeMap::new(),
            links: BTreeMap::new(),
            ucolors: BTreeMap::new(),
        }
    }

    /// The six styles the engine can store, read back **through the wire** by name.
    ///
    /// The round trip is the point: `underline_style` is asserted against a `flags[i]` word that a
    /// real `encode`/`decode` produced, not against one this test built, so it covers the field's
    /// position surviving serialisation as well as the accessor reading it. Testing the accessor
    /// on a hand-made word would have been a fixture asserting itself.
    #[test]
    fn underline_style_names_every_style_the_engine_can_store() {
        const ALL: [CoreStyle; 6] = [
            CoreStyle::None,
            CoreStyle::Single,
            CoreStyle::Double,
            CoreStyle::Curly,
            CoreStyle::Dotted,
            CoreStyle::Dashed,
        ];
        let frame = partial(80, 24, vec![styled_span(0, 0, &ALL)]);
        let native = justerm_core::decode(&justerm_core::encode(&frame)).expect("decode");
        let flat = flatten(&native);

        let read: Vec<UnderlineStyle> = flat.flags.iter().map(|&w| underline_style(w)).collect();
        assert_eq!(
            read,
            vec![
                UnderlineStyle::None,
                UnderlineStyle::Single,
                UnderlineStyle::Double,
                UnderlineStyle::Curly,
                UnderlineStyle::Dotted,
                UnderlineStyle::Dashed,
            ]
        );
        // Six inputs must give six *distinct* answers — the acceptance criterion is that a
        // consumer can tell them apart, and a binding that collapsed two of them onto one name
        // would still satisfy an element-wise check written from the same list.
        let mut distinct = read.clone();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            6,
            "two styles read as the same name: {read:?}"
        );
    }

    /// **The same word carries flags and the field, and neither eats the other.**
    ///
    /// Every other test here varies the style under one input shape — a row of nothing but styled
    /// cells. That shape cannot see a style that survives alone and is lost beside a flag, or a
    /// flag clipped by the field's mask, because it never puts the two in one word. #830 shipped
    /// with fifteen green checks that shared exactly that blind spot, so the shape is varied here
    /// deliberately rather than the value.
    #[test]
    fn a_style_and_ordinary_flags_ride_the_same_word() {
        let mut f = CellFlags::BOLD | CellFlags::ITALIC | CellFlags::STRIKETHROUGH;
        f.set_underline_style(CoreStyle::Dotted);
        let span = Span {
            line: 7,
            left: 13, // not column 0: a span offset must not shift which bits are read
            right: 13,
            cells: vec![Cell::from_parts(
                'q',
                Color::Indexed(9),
                Color::Rgb(1, 2, 3),
                f,
            )],
            combining: BTreeMap::new(),
            links: BTreeMap::new(),
            ucolors: BTreeMap::new(),
        };
        let frame = partial(80, 24, vec![ascii_span(0, 0, "unstyled"), span]);
        let native = justerm_core::decode(&justerm_core::encode(&frame)).expect("decode");
        let flat = flatten(&native);

        let w = *flat.flags.last().expect("the styled cell is the last one");
        assert_eq!(underline_style(w), UnderlineStyle::Dotted);
        let bits = flags();
        for (name, mask) in [
            ("bold", bits.bold),
            ("italic", bits.italic),
            ("strikethrough", bits.strikethrough),
            ("underline", bits.underline),
        ] {
            assert_ne!(w & mask, 0, "{name} was lost from a word carrying a style");
        }
        // The unstyled neighbours still read `None`. This duplicates
        // `a_cell_with_no_underline_reads_as_none` rather than covering a hazard of its own: the
        // neighbours are separate 14-byte records with their own `u16`, so no over-wide write
        // within one word could have reached them. Kept because it is free and reads as intent.
        for &other in &flat.flags[..flat.flags.len() - 1] {
            assert_eq!(underline_style(other), UnderlineStyle::None);
        }
    }

    /// A cell with no style reads as `None` — the documented answer, not an inferred zero (#831).
    #[test]
    fn a_cell_with_no_underline_reads_as_none() {
        let frame = partial(80, 24, vec![ascii_span(0, 0, "hi")]);
        let native = justerm_core::decode(&justerm_core::encode(&frame)).expect("decode");
        let flat = flatten(&native);
        assert!(flat.flags.iter().all(|&w| w & flags().underline == 0));
        for &w in &flat.flags {
            assert_eq!(underline_style(w), UnderlineStyle::None);
        }
    }

    /// The accessor reads the *field* and nothing else: the same style with every other flag bit
    /// set gives the same answer, and the two representable values outside the enum normalise the
    /// way the engine normalises them rather than a second way invented in the binding.
    #[test]
    fn underline_style_reads_only_its_own_field() {
        let curly = 3u16 << 11;
        assert_eq!(underline_style(curly), UnderlineStyle::Curly);
        assert_eq!(
            underline_style(curly | CellFlags::all().bits()),
            UnderlineStyle::Curly,
            "a flag set beside the style changed the style"
        );
        for spare in [6u16, 7] {
            assert_eq!(underline_style(spare << 11), UnderlineStyle::Single);
        }
    }

    /// **The skew a new decoder sees: a frame written before the style existed.**
    ///
    /// A pre-#829 encoder wrote a bare `UNDERLINE` with bits 11..=13 zero. That word is not
    /// constructible through `Cell` — `flag_words` normalises it on the way in — so the record is
    /// forged in the encoded bytes, which is the only way to get the real historical word through
    /// `decode`. It must not error and must read as a plain single underline.
    #[test]
    fn a_frame_written_before_the_style_existed_reads_as_a_single_underline() {
        let mut f = CellFlags::empty();
        f.set_underline_style(CoreStyle::Single);
        let frame = partial(80, 24, vec![styled_span(0, 0, &[CoreStyle::Single])]);
        let mut bytes = justerm_core::encode(&frame);

        // Locate the record by its own encoding, and locate the flags field **inside** it the same
        // way — by differencing two records that differ in nothing else. A hand-written `+12`
        // was the first version and a refuting pass broke it: reorder `encode_cell_record` and the
        // test clobbered two zero bytes of the colour word, forged no historical word at all, and
        // still went green off the ordinary `Single` cell it had built. The record anchor moved
        // with the layout; the offset did not.
        let cell_of = |flags| {
            justerm_core::encode_cell_record(&Cell::from_parts(
                'x',
                Color::Default,
                Color::Default,
                flags,
            ))
        };
        let record = cell_of(f);
        // **Two probes, because one is not enough and the first draft of this proved it.** The
        // style lives entirely in the word's high byte, so varying only the style moved one byte
        // and the check demanding two fired on its own test rather than on a defect. Varying a
        // flag as well reaches the low byte; the union is the field.
        let mut styled_differently = CellFlags::empty();
        styled_differently.set_underline_style(CoreStyle::Double);
        let diff = |other| -> Vec<usize> {
            record
                .iter()
                .zip(cell_of(other).iter())
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, _)| i)
                .collect()
        };
        let mut differing = diff(styled_differently);
        differing.extend(diff(f | CellFlags::BOLD));
        differing.sort_unstable();
        differing.dedup();
        assert_eq!(
            differing.len(),
            2,
            "records differing only in their flag word must differ in exactly that u16; got {differing:?}"
        );
        assert_eq!(
            differing[1],
            differing[0] + 1,
            "the flags field is not contiguous: {differing:?}"
        );
        let field = differing[0];

        let hits: Vec<usize> = bytes
            .windows(justerm_core::CELL_RECORD_LEN)
            .enumerate()
            .filter(|(_, w)| *w == record.as_slice())
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            hits.len(),
            1,
            "cell record not found exactly once in the frame"
        );
        // A styleless underline: bit 3 set, the style field cleared. Assert what is being
        // overwritten, so a patch that lands anywhere but on the live flags word is a failure
        // rather than a silent no-op.
        let at = hits[0] + field;
        assert_eq!(
            u16::from_le_bytes([bytes[at], bytes[at + 1]]),
            f.bits(),
            "the bytes about to be forged are not the styled cell's flags word"
        );
        bytes[at..at + 2].copy_from_slice(&CellFlags::UNDERLINE.bits().to_le_bytes());

        let native = justerm_core::decode(&bytes).expect("a pre-style frame must still decode");
        let flat = flatten(&native);
        assert_eq!(underline_style(flat.flags[0]), UnderlineStyle::Single);
        assert_ne!(flat.flags[0] & flags().underline, 0);
    }

    /// **The skew an old consumer sees: it ignores bits it has no name for.**
    ///
    /// The claim the no-bump decision rests on is that such a reader still draws *something* — and
    /// that holds only because the engine derives `UNDERLINE` from the style for every non-`None`
    /// value. Asserted per style rather than once, since the derivation could plausibly be lost
    /// for the values #830 added while surviving for the ones #829 shipped.
    #[test]
    fn a_reader_that_cannot_name_the_style_still_sees_an_underline() {
        const LIT: [CoreStyle; 5] = [
            CoreStyle::Single,
            CoreStyle::Double,
            CoreStyle::Curly,
            CoreStyle::Dotted,
            CoreStyle::Dashed,
        ];
        let frame = partial(80, 24, vec![styled_span(0, 0, &LIT)]);
        let native = justerm_core::decode(&justerm_core::encode(&frame)).expect("decode");
        let flat = flatten(&native);
        assert_eq!(flat.flags.len(), LIT.len());
        for (&w, style) in flat.flags.iter().zip(LIT) {
            assert_ne!(
                w & flags().underline,
                0,
                "{style:?} reaches an old consumer with no underline at all"
            );
        }
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
            Cell::from_parts(
                'A',
                Color::Indexed(9),
                Color::Rgb(1, 2, 3),
                CellFlags::empty(),
            ),
            Cell::from_parts('B', Color::Default, Color::Default, CellFlags::empty()),
        ];
        let frame = partial(
            80,
            24,
            vec![Span {
                line: 0,
                left: 0,
                right: 1,
                cells,
                combining: BTreeMap::new(),
                links: BTreeMap::new(),
                ucolors: BTreeMap::new(),
            }],
        );
        let flat = flatten(&frame);
        // tagged u32: high byte = tag (0 Default, 1 Indexed, 2 Rgb), low 24 = payload.
        assert_eq!(flat.fg, vec![(1 << 24) | 9, 0]);
        assert_eq!(flat.bg, vec![(2 << 24) | (1 << 16) | (2 << 8) | 3, 0]);
    }

    #[test]
    fn flatten_exposes_the_underline_colour_column() {
        // #520 slice 3: the underline colour rides the span's sparse ucolor map;
        // flatten densifies it to a per-cell tagged-u32 column, `0` where absent —
        // exactly as fg/bg. Col 0 draws a red underline; col 1 has none.
        let cells = vec![
            Cell::from_parts('A', Color::Default, Color::Default, CellFlags::UNDERLINE),
            Cell::from_parts('B', Color::Default, Color::Default, CellFlags::empty()),
        ];
        let frame = partial(
            80,
            24,
            vec![Span {
                line: 0,
                left: 0,
                right: 1,
                cells,
                combining: BTreeMap::new(),
                links: BTreeMap::new(),
                ucolors: BTreeMap::from([(0, Color::Rgb(255, 0, 0))]),
            }],
        );
        let flat = flatten(&frame);
        assert_eq!(
            flat.underline_color,
            vec![(2 << 24) | (255 << 16), 0],
            "col 0 = Rgb(255,0,0) tagged; col 1 = 0 (Default, follow fg)"
        );
    }

    #[test]
    fn flatten_exposes_flags_extra_link_columns() {
        let cells = vec![
            Cell::from_parts(
                'A',
                Color::Default,
                Color::Default,
                CellFlags::BOLD | CellFlags::ITALIC,
            ),
            Cell::from_parts('B', Color::Default, Color::Default, CellFlags::empty()),
        ];
        let frame = partial(
            80,
            24,
            vec![Span {
                line: 0,
                left: 0,
                right: 1,
                cells,
                // Both ride the span (per column), not the cell — and since v14 (#621)
                // the cluster is inline while the link is still a table index.
                combining: BTreeMap::from([(0, vec!['e', '\u{301}'])]),
                links: BTreeMap::from([(0, NonZeroU32::new(7).unwrap())]),
                ucolors: BTreeMap::new(),
            }],
        );
        let flat = flatten(&frame);
        assert_eq!(
            flat.flags,
            vec![(CellFlags::BOLD | CellFlags::ITALIC).bits(), 0]
        );
        // 1-based index, 0 = none — and since v14 (#621) *assigned here* rather than
        // passed through. The old fixture handed `flatten` an index of 3 into a table
        // nothing had built, and the column echoed it back; now the first cluster in
        // cell order is entry 1 by construction, so an arbitrary number can no longer
        // survive the trip.
        assert_eq!(flat.extra, vec![1, 0]);
        assert_eq!(
            flat.side_table,
            vec![vec!['e', '\u{301}']],
            "and `extra[0]` indexes it — a column with no table behind it is the bug \
             this pairing exists to catch",
        );
        assert_eq!(flat.link, vec![7, 0]);
    }

    #[test]
    fn flatten_carries_scalars_and_scroll() {
        let mut frame = partial(120, 40, vec![]);
        frame.kind = FrameKind::Full;
        frame.scroll = Some(justerm_core::ScrollOp {
            top: 2,
            bottom: 39,
            count: -3,
        });
        let flat = flatten(&frame);
        assert_eq!((flat.cols, flat.rows, flat.kind), (120, 40, 0));
        assert_eq!(flat.scroll, Some((2, 39, -3)));
    }

    #[test]
    fn flatten_carries_cursor() {
        let mut frame = partial(80, 24, vec![]);
        frame.cursor_row = 9;
        frame.cursor_col = 19;
        frame.cursor_visible = false;
        let flat = flatten(&frame);
        assert_eq!(
            (flat.cursor_row, flat.cursor_col, flat.cursor_visible),
            (9, 19, false)
        );
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
    fn flatten_rebuilds_the_side_table_and_carries_the_link_table() {
        // Since v14 (#621) the wire has no side table: clusters ride their column. The
        // JS-facing shape is unchanged (`extra` indexes a `sideTable`), so `flatten`
        // *builds* that table — which makes this a test of the rebuild, not of a copy.
        // The link table is still carried through, because that one is still interned.
        let mut span = ascii_span(0, 0, "ea");
        span.combining.insert(0, vec!['\u{301}']);
        span.combining.insert(1, vec!['\u{308}']);
        let mut frame = partial(80, 24, vec![span]);
        frame.link_table = vec!["https://example.com".to_string()];

        let flat = flatten(&frame);
        assert_eq!(
            flat.side_table,
            vec![vec!['\u{301}'], vec!['\u{308}']],
            "one entry per combining cell, in cell order",
        );
        assert_eq!(
            flat.extra,
            vec![1, 2],
            "1-based indices into the table just built — 0 would mean 'no cluster'",
        );
        assert_eq!(flat.link_table, vec!["https://example.com".to_string()]);
    }

    #[test]
    fn flatten_side_table_from_a_real_feed_holds_marks_only_not_the_base_294() {
        // #294 real-core convention lock (via a REAL feed, not a hand-built frame): justerm-core
        // stores a combining cluster's BASE in the codepoint column and only the trailing width-0
        // MARKS in the side-table. This is the exact DecodedFrame contract justerm-web's cell-mirror
        // consumes, so the mirror must PREPEND the base to the marks — never render the mark alone.
        let mut e = justerm_core::Engine::new(4, 1);
        e.feed("e\u{0301}".as_bytes()); // 'e' + combining acute
        let flat = flatten(&e.frame());
        assert_eq!(
            flat.codepoints[0], 'e' as u32,
            "base 'e' lives in the codepoint column"
        );
        assert_ne!(flat.extra[0], 0, "the cell indexes into the side-table");
        let marks = &flat.side_table[(flat.extra[0] - 1) as usize];
        assert_eq!(
            marks,
            &vec!['\u{0301}'],
            "side-table holds ONLY the combining mark, NOT the base 'e'"
        );
    }

    // --- #108: overlay highlight spans carried through (selection + matches) ---

    #[test]
    fn flatten_carries_overlay_spans_through_the_wire() {
        use justerm_core::{Overlay, SelectionSpan};
        let mut frame = partial(80, 24, vec![ascii_span(0, 0, "x")]);
        frame.overlay = Overlay {
            selection: vec![SelectionSpan {
                row: 0,
                left: 2,
                right: 7,
            }],
            matches: vec![
                SelectionSpan {
                    row: 1,
                    left: 0,
                    right: 3,
                },
                SelectionSpan {
                    row: 4,
                    left: 9,
                    right: 9,
                },
            ],
            markers: vec![],
            active_match: vec![],
        };
        // Through the real wire (encode→decode), then flattened — proves the
        // overlay survives the byte boundary the WASM decoder reads.
        let native = justerm_core::decode(&justerm_core::encode(&frame)).expect("decode");
        let flat = flatten(&native);
        assert_eq!(flat.selection_spans, vec![0, 2, 7]); // one (row,left,right)
        assert_eq!(flat.match_spans, vec![1, 0, 3, 4, 9, 9]); // two triples
    }

    // --- #428 (v12): the active-match group carried through ---

    #[test]
    fn flatten_carries_active_match_spans_through_the_wire() {
        use justerm_core::SelectionSpan;
        let mut frame = partial(80, 24, vec![ascii_span(0, 0, "x")]);
        // A wrapped active match (two rows); it also stays in the match group —
        // ranking, not exclusion, resolves the overlap downstream (#424).
        let active = vec![
            SelectionSpan {
                row: 1,
                left: 6,
                right: 9,
            },
            SelectionSpan {
                row: 2,
                left: 0,
                right: 2,
            },
        ];
        frame.overlay.matches = active.clone();
        frame.overlay.active_match = active;
        let native = justerm_core::decode(&justerm_core::encode(&frame)).expect("decode");
        let flat = flatten(&native);
        // Same (row, left, right) stride as matchSpans, a separate directory.
        assert_eq!(flat.active_match_spans, vec![1, 6, 9, 2, 0, 2]);
        assert_eq!(flat.match_spans, vec![1, 6, 9, 2, 0, 2]); // not excluded
    }

    #[test]
    fn flatten_carries_marker_positions_through_the_wire() {
        use justerm_core::{MarkerId, MarkerKind, MarkerPosition};
        let mut frame = partial(80, 24, vec![ascii_span(0, 0, "x")]);
        frame.overlay.markers = vec![
            MarkerPosition {
                id: MarkerId(5),
                row: 3,
                kind: MarkerKind::PromptStart,
            },
            MarkerPosition {
                id: MarkerId(99),
                row: 0,
                kind: MarkerKind::CommandFinished(Some(-1)),
            },
        ];
        let native = justerm_core::decode(&justerm_core::encode(&frame)).expect("decode");
        let flat = flatten(&native);
        // Stride 5 per marker: (id, row, kind, exitPresent, exitBits). The second
        // marker exercises the signed exit i32→u32 bit-cast (-1 → 0xFFFFFFFF).
        assert_eq!(
            flat.marker_positions,
            vec![
                5,
                3,
                1,
                0,
                0, // PromptStart (kind 1), no exit
                99,
                0,
                4,
                1,
                (-1i32) as u32, // CommandFinished(Some(-1)) (kind 4)
            ]
        );
    }

    #[test]
    fn flatten_carries_mouse_events_mask_through_the_wire() {
        use justerm_core::MouseEvents;
        let mut frame = partial(80, 24, vec![ascii_span(0, 0, "x")]);
        frame.mouse_events = MouseEvents::DOWN | MouseEvents::UP | MouseEvents::WHEEL;
        let native = justerm_core::decode(&justerm_core::encode(&frame)).expect("decode");
        let flat = flatten(&native);
        assert_eq!(flat.mouse_events, frame.mouse_events.bits());
    }

    // --- S3/AC2: flatten faithfully represents the native-decoded frame ---

    #[test]
    fn flatten_matches_native_decode_for_a_rich_frame() {
        // A frame exercising wide chars, colours, a grapheme ref, a link ref,
        // scroll, and multiple spans — then round-tripped through the real wire.
        let wide = Cell::from_parts('한', Color::Default, Color::Default, CellFlags::WIDE_CHAR);
        let spacer = Cell::from_parts(
            ' ',
            Color::Default,
            Color::Default,
            CellFlags::WIDE_CHAR_SPACER,
        );
        let coloured =
            Cell::from_parts('A', Color::Indexed(9), Color::Rgb(1, 2, 3), CellFlags::BOLD);
        let frame = Frame {
            cols: 80,
            rows: 24,
            kind: FrameKind::Full,
            cursor_row: 7,
            cursor_col: 13,
            cursor_visible: false,
            cursor_shape: justerm_core::CursorShape::Block,
            cursor_blink: false,
            display_offset: 0,
            scrollback_len: 0,
            evicted_total: 0,
            marker_epoch: 0,
            marker_count: 0,
            mouse_events: Default::default(),
            alt_screen: false,
            scroll: Some(justerm_core::ScrollOp {
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
                    // the `coloured` cell (column 2) carries a cluster + link 1.
                    combining: BTreeMap::from([(2, vec!['\u{301}'])]),
                    links: BTreeMap::from([(2, NonZeroU32::new(1).unwrap())]),
                    ucolors: BTreeMap::new(),
                },
                ascii_span(3, 10, "hi"),
            ],
            link_table: vec!["https://x.example".to_string()],
            overlay: Default::default(),
        };

        let bytes = justerm_core::encode(&frame);
        let native = justerm_core::decode(&bytes).expect("native decode");
        let flat = flatten(&native);

        // Scalars + cursor + scroll + tables match the native frame (AC3: the
        // WASM path yields cursor fields identical to the native engine state).
        assert_eq!((flat.cols, flat.rows, flat.kind), (80, 24, 0));
        assert_eq!(
            (flat.cursor_row, flat.cursor_col, flat.cursor_visible),
            (native.cursor_row, native.cursor_col, native.cursor_visible)
        );
        assert_eq!(
            (flat.cursor_row, flat.cursor_col, flat.cursor_visible),
            (7, 13, false)
        );
        assert_eq!(flat.scroll, Some((0, 23, 5)));
        // There is no `native.side_table` to compare against since v14 (#621) — the wire
        // carries no such table, so `flatten` builds one. Assert what it built against
        // the clusters the native frame actually carries; a copy check would have
        // nothing left to copy and would quietly assert nothing.
        let native_clusters: Vec<Vec<char>> = native
            .spans
            .iter()
            .flat_map(|s| s.combining.values().cloned())
            .collect();
        assert_eq!(flat.side_table, native_clusters);
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
        // The 1-based side-table index `flatten` assigns, recomputed independently here
        // rather than read back from `flat` — otherwise the assertion below compares the
        // implementation with itself.
        let mut next_cluster: u32 = 0;
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
            for (col, cell) in span.cells.iter().enumerate() {
                exp_codepoints.push(cell.c() as u32);
                exp_fg.push(justerm_core::encode_color(cell.fg()));
                exp_bg.push(justerm_core::encode_color(cell.bg()));
                exp_flags.push(cell.flags().bits());
                exp_extra.push(match span.combining.get(&col) {
                    Some(_) => {
                        next_cluster += 1;
                        next_cluster
                    }
                    None => 0,
                });
                exp_link.push(span.links.get(&col).map_or(0, |x| x.get()));
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

    #[test]
    fn real_core_emits_marks_only_side_table_for_a_combining_sequence() {
        // Cross-crate contract lock for justerm-renderer #285: the renderer assembles a grapheme
        // as base (from the codepoint column) + marks (from side_table). That is only correct
        // because justerm-core stores ONLY the trailing width-0 combining marks in side_table,
        // leaving the base glyph in the cell's codepoint. Verify that against the REAL engine —
        // not a hand-built fixture (the other flatten tests set side_table by hand, so none of
        // them would catch core changing this convention). Feed "e" + U+0301 (combining acute).
        let mut eng = justerm_core::Engine::new(4, 1);
        eng.feed("e\u{0301}".as_bytes());
        let flat = flatten(&eng.frame());

        assert_eq!(
            flat.codepoints[0], 0x65,
            "the base 'e' stays in the codepoint column"
        );
        assert_eq!(flat.extra[0], 1, "cell 0 references side_table[0]");
        assert_eq!(
            flat.side_table,
            vec![vec!['\u{0301}']],
            "side_table holds ONLY the combining mark, never the base 'e'"
        );
    }
}
