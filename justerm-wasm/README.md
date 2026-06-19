# justerm-wasm

The **canonical web decoder** for [justerm](https://github.com/kihyun1998/justerm)'s binary wire
format — the engine's native `decode` compiled to WASM, so a web consumer shares *one* decoder with
the native backend instead of hand-writing (and re-syncing) a TypeScript mirror.

Typical data path: the native engine `encode`s a damage frame in your backend, the bytes cross your
IPC (e.g. a Tauri Channel), and in the webview this package `decodeFrame`s them into renderer-ready
buffers.

**Scope is decode only.** It stops at *references*: resolving a colour reference → RGB (via your
theme), mapping a codepoint → atlas glyph id, and drawing the cursor stay your adapter's job. See
[ADR-0008](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0008-wasm-decode-binding-separate-crate.md)
for why (theme-/renderer-agnostic boundary).

> Version-locked to the `justerm` crate: this package's version equals the engine version, so pinning
> one `justerm` version gives you a matching encoder (native) + decoder (this). `wireVersion()` lets
> you assert agreement at load.

## Install

```sh
npm install justerm-wasm
```

## Usage

```js
// Bundler target (Vite/webpack) — no init needed:
import { decodeFrame, wireVersion } from "justerm-wasm";

// Web target (no bundler) — await the default init first:
//   import init, { decodeFrame, wireVersion } from "justerm-wasm";
//   await init();

console.assert(wireVersion() === 2); // optional: assert the backend encoder agrees

const frame = decodeFrame(wireBytes); // wireBytes: Uint8Array from your transport

frame.cols; // u16
frame.rows; // u16
frame.kind; // 0 = Full (every row), 1 = Partial (only the listed spans)
if (frame.hasScroll) {
  // apply BEFORE drawing spans: shift rows [scrollTop..=scrollBottom] by scrollCount
  frame.scrollTop, frame.scrollBottom, frame.scrollCount;
}

const cells = frame.cells; // Uint8Array view — fixed-stride 18-byte records (see layout below)
const spans = frame.spans; // Uint32Array — 5 per span: [line, left, right, cellOffset, cellCount]
const view = new DataView(cells.buffer, cells.byteOffset, cells.byteLength);

for (let s = 0; s < spans.length; s += 5) {
  const line = spans[s], left = spans[s + 1], cellOffset = spans[s + 3], cellCount = spans[s + 4];
  for (let k = 0; k < cellCount; k++) {
    const col = left + k;
    const b = (cellOffset + k) * 18; // byte offset of this cell's record
    const codepoint = view.getUint32(b, true);
    const fg = view.getUint32(b + 4, true);
    const bg = view.getUint32(b + 8, true);
    const flags = view.getUint16(b + 12, true);
    const extra = view.getUint16(b + 14, true); // 0 = none, else frame.sideTable[extra - 1]
    const linkId = view.getUint16(b + 16, true); // 0 = none, else frame.linkTable[linkId - 1]
    // your adapter: resolve fg/bg → RGB, codepoint → atlas glyph, apply flags, place at (line, col)
  }
}

frame.free(); // release the WASM-side buffers (or use `using frame = decodeFrame(...)`)
```

### Lifetime of `cells` / `spans`

`cells` and `spans` are **zero-copy views** into WASM linear memory — the bulk data reaches JS with
no per-cell boundary crossing. A view is invalidated when WASM memory grows, which the **next**
`decodeFrame` call can trigger. So: read or copy what you need from one frame before decoding the
next, and `free()` the frame (or scope it with `using`) when done.

## Cell record layout (18 bytes, little-endian)

| Bytes | Field   | Meaning |
|-------|---------|---------|
| 0..4  | `c`     | `u32` Unicode scalar (the base codepoint — **not** an atlas glyph id) |
| 4..8  | `fg`    | `u32` colour reference (see below) |
| 8..12 | `bg`    | `u32` colour reference |
| 12..14| `flags` | `u16` attribute + layout bits (see below) |
| 14..16| `extra` | `u16` 1-based index into `sideTable` for a grapheme cluster, `0` = none |
| 16..18| `link`  | `u16` 1-based index into `linkTable` for an OSC 8 hyperlink, `0` = none |

**Colour reference** (the `u32`): the high byte is a tag, the low 24 bits the payload.

```js
const tag = ref >>> 24;             // 0 = Default, 1 = Indexed, 2 = Rgb
const payload = ref & 0x00ffffff;
// tag 1: 256-colour palette index = payload & 0xff
// tag 2: r = (payload >> 16) & 0xff, g = (payload >> 8) & 0xff, b = payload & 0xff
```

The tag is mandatory so `Default`, `Indexed(0)`, and `Rgb(0,0,0)` stay distinct. Your adapter maps
`Default`/`Indexed` to RGB via your frozen scheme.

**Flags** (`u16` bitset):

| Bit | Flag | | Bit | Flag |
|-----|------|-|-----|------|
| 0 | Bold | | 7 | Strikethrough |
| 1 | Dim | | 8 | Wide char (first cell of a width-2 glyph) |
| 2 | Italic | | 9 | Wide char spacer (trailing cell — skip when drawing) |
| 3 | Underline | | 10 | Wrapline (row soft-wrapped into the next) |
| 4 | Blink | | | |
| 5 | Inverse | | | |
| 6 | Hidden | | | |

A cell's display width derives from the flags: a `Wide char` cell spans two columns, and the next
column is its `Wide char spacer`.

## License

Dual-licensed under [MIT](https://github.com/kihyun1998/justerm/blob/master/LICENSE-MIT) or
[Apache-2.0](https://github.com/kihyun1998/justerm/blob/master/LICENSE-APACHE), at your option —
same as the `justerm` crate.
