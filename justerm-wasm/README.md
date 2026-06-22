# justerm-wasm

The **canonical web decoder** for [justerm](https://github.com/kihyun1998/justerm)'s binary wire
format — the engine's native `decode` compiled to WASM, so a web consumer shares *one* decoder with
the native backend instead of hand-writing (and re-syncing) a TypeScript mirror.

Typical data path: the native engine `encode`s a damage frame in your backend, the bytes cross your
IPC (e.g. a Tauri Channel), and in the webview this package decodes them into renderer-ready columns.

The decoder owns the **fixed formats and standards** (the wire records, the colour-ref encoding, the
flag bit positions, the xterm 16–255 colour formula). *Theme values* (your 16 ANSI colours + default
fg/bg) and *render policy* (inverse/dim/bold→bright, the font atlas, the cursor) stay yours. See
[ADR-0008](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0008-wasm-decode-binding-separate-crate.md).

> Version-locked to the `justerm` crate: this package's version equals the engine version, so pinning
> one `justerm` version gives you a matching encoder (native) + decoder (this). `wireVersion()` lets
> you assert agreement at load.

## Install

```sh
npm install justerm-wasm
```

## Usage

```js
import { decodeFrame, buildPalette, flags, wireVersion } from "justerm-wasm";
import { resolveRgb, decodeColorRef, FG, BG } from "justerm-wasm/colors.js";

// Bundler target (Vite/webpack): the above imports work directly.
// Web target (no bundler): `import init, { ... } from "justerm-wasm"; await init();` first.

console.assert(wireVersion() === 2); // optional: assert the backend encoder agrees

// --- once at startup / on theme change ---
// buildPalette fills 0..15 from your scheme's ANSI colours and 16..255 from the
// fixed xterm cube/grayscale. Keep your default fg/bg alongside (they are the
// `Default` colour ref, resolved by role — not part of the 256).
const palette = {
  colors: buildPalette(Uint32Array.from(scheme.ansi16)), // 16 × 0xRRGGBB
  defaultFg: scheme.defaultFg, // 0xRRGGBB
  defaultBg: scheme.defaultBg,
};
const F = flags(); // bit constants, read once

// --- per frame (e.g. an IPC message) ---
const frame = decodeFrame(wireBytes); // throws on a malformed buffer
// Structure-of-arrays columns (zero-copy views) + the span directory.
const { codepoints, fg, bg, extra, link, spans } = frame;
const flagBits = frame.flags; // note: the column; `flags()` above is the constants

for (let s = 0; s < spans.length; s += 5) {
  const line = spans[s], left = spans[s + 1], offset = spans[s + 3], count = spans[s + 4];
  for (let k = 0; k < count; k++) {
    const i = offset + k;
    const col = left + k;

    const fgRgb = resolveRgb(fg[i], palette, FG); // 0xRRGGBB
    const bgRgb = resolveRgb(bg[i], palette, BG);
    const bold = (flagBits[i] & F.bold) !== 0;
    if (flagBits[i] & F.wide_char_spacer) continue; // trailing half of a wide glyph

    // your adapter: map codepoints[i] -> atlas glyph, apply bold/inverse/dim,
    // resolve extra[i]/link[i] via sideTable/linkTable, place at (line, col).
  }
}

frame.free(); // release the column views — or scope with `using frame = decodeFrame(...)`
```

### Lifetime of the columns

`codepoints` / `fg` / `bg` / `flags` / `extra` / `link` / `spans` are **zero-copy views** into WASM
linear memory — the bulk data reaches JS with no per-cell boundary crossing. A view is invalidated
when WASM memory grows, which the **next** `decodeFrame` call can trigger. Read or copy what you need
from one frame before decoding the next, and `free()` the frame (or scope it with `using`) when done.
The `palette` from `buildPalette` is an owned copy, so it is safe to keep across frames.

## What the columns hold

One entry per cell, in span order. `spans` is a flat directory: 5 `u32`s per span —
`line, left, right, cell_offset, cell_count` — where cell `k` of a span is column index
`cell_offset + k`.

| Column | Type | Meaning |
|--------|------|---------|
| `codepoints` | `Uint32Array` | base Unicode codepoint (not an atlas glyph id) |
| `fg` / `bg` | `Uint32Array` | colour references — pass to `resolveRgb` |
| `flags` | `Uint16Array` | attribute + layout bits — test with `flags()` constants |
| `extra` | `Uint16Array` | 1-based `sideTable` index for a grapheme cluster (`0` = none) |
| `link` | `Uint16Array` | 1-based `linkTable` index for an OSC 8 hyperlink (`0` = none) |

`frame.sideTable` (`string[]`) and `frame.linkTable` (`string[]`) carry the referenced clusters/URIs.

## Colour helpers

- **`resolveRgb(ref, palette, role) → 0xRRGGBB`** — resolves a `fg[i]`/`bg[i]` ref: `Default` → the
  role's default (`FG`/`BG`), `Indexed` → `palette.colors[i]`, `Rgb` → passthrough. Alloc-free; call
  it per cell. It does **not** apply inverse/dim/hidden/bold→bright — that is your render policy.
- **`buildPalette(ansi16) → Uint32Array(256)`** — the 256-colour table (0..15 your ANSI, 16..255 the
  fixed xterm standard). Build once per scheme.
- **`decodeColorRef(ref)`** — `{ kind: "default" } | { kind: "indexed", index } | { kind: "rgb", r,
  g, b }`. For inspection; allocates, so prefer `resolveRgb` in the hot loop.

## Flag constants (`flags()`)

`bold`, `dim`, `italic`, `underline`, `blink`, `inverse`, `hidden`, `strikethrough`, `wide_char`,
`wide_char_spacer`, `wrapline` — each the bit to AND against a `flags[i]` value. How to act on them
(skip the spacer, bold→bright, dim) is your render policy. `wrapline` is engine reflow/copy metadata,
usually ignored by a renderer.

## License

Dual-licensed under [MIT](https://github.com/kihyun1998/justerm/blob/master/LICENSE-MIT) or
[Apache-2.0](https://github.com/kihyun1998/justerm/blob/master/LICENSE-APACHE), at your option —
same as the `justerm` crate.
