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

use justerm::{FrameKind, decode, encode_cell_record};
use wasm_bindgen::prelude::*;

/// The wire-format version this decoder understands (the `VERSION` byte gating
/// ADR-0005). A consumer can read it at load time to assert the WASM decoder and
/// the backend encoder agree before any frame flows; `decodeFrame` also returns a
/// `BadVersion` error on mismatch, so a stale artifact fails loudly.
#[wasm_bindgen(js_name = wireVersion)]
pub fn wire_version() -> u8 {
    justerm::WIRE_VERSION
}

/// A decoded damage frame, presented for a web renderer.
///
/// Scalars come via getters; the bulk cell data is exposed as a zero-copy
/// [`js_sys::Uint8Array`] view over WASM linear memory (see [`DecodedFrame::cells`]).
/// The span directory mapping cells back to grid rows lands in S2.
#[wasm_bindgen]
pub struct DecodedFrame {
    cols: u16,
    rows: u16,
    kind: u8,
    has_scroll: bool,
    scroll_top: u16,
    scroll_bottom: u16,
    scroll_count: i16,
    cells: Vec<u8>,
}

#[wasm_bindgen]
impl DecodedFrame {
    #[wasm_bindgen(getter)]
    pub fn cols(&self) -> u16 {
        self.cols
    }

    #[wasm_bindgen(getter)]
    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// `0` = Full (every row present), `1` = Partial (only the listed spans).
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> u8 {
        self.kind
    }

    #[wasm_bindgen(getter, js_name = hasScroll)]
    pub fn has_scroll(&self) -> bool {
        self.has_scroll
    }

    #[wasm_bindgen(getter, js_name = scrollTop)]
    pub fn scroll_top(&self) -> u16 {
        self.scroll_top
    }

    #[wasm_bindgen(getter, js_name = scrollBottom)]
    pub fn scroll_bottom(&self) -> u16 {
        self.scroll_bottom
    }

    #[wasm_bindgen(getter, js_name = scrollCount)]
    pub fn scroll_count(&self) -> i16 {
        self.scroll_count
    }

    /// Every cell across the frame's spans, concatenated as fixed-stride 18-byte
    /// records (`justerm::CELL_RECORD_LEN`), exposed as a zero-copy view into
    /// WASM linear memory — the bulk data reaches JS with no per-cell boundary
    /// crossing (#34 AC3).
    ///
    /// # Lifetime
    /// The returned array views WASM memory directly; it is invalidated if that
    /// memory grows (e.g. the next `decodeFrame` call allocates). Read it before
    /// the next decode.
    #[wasm_bindgen(getter)]
    pub fn cells(&self) -> js_sys::Uint8Array {
        // SAFETY: the view borrows `self.cells`, which `self` keeps alive. The
        // caller must consume it before triggering another WASM allocation.
        unsafe { js_sys::Uint8Array::view(&self.cells) }
    }
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

    let (has_scroll, scroll_top, scroll_bottom, scroll_count) = match frame.scroll {
        Some(s) => (true, s.top as u16, s.bottom as u16, s.count as i16),
        None => (false, 0, 0, 0),
    };

    let cell_count: usize = frame.spans.iter().map(|s| s.cells.len()).sum();
    let mut cells = Vec::with_capacity(cell_count * justerm::CELL_RECORD_LEN);
    for span in &frame.spans {
        for cell in &span.cells {
            cells.extend_from_slice(&encode_cell_record(cell));
        }
    }

    Ok(DecodedFrame {
        cols: frame.cols,
        rows: frame.rows,
        kind: match frame.kind {
            FrameKind::Full => 0,
            FrameKind::Partial => 1,
        },
        has_scroll,
        scroll_top,
        scroll_bottom,
        scroll_count,
        cells,
    })
}
