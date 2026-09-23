// Shared pixel-reading helpers for the `demo/*.html` proofs (#328). Everything here is in device
// pixels: the drawing buffer, and the cell `cell_width()`/`cell_height()` report.
//
// Deliberate: the cell is always read from the renderer, never re-derived from CSS size or buffer
// width — see docs/map/territory/browser-proof-harness.md.

/** A grid's exact device-pixel cell, `[width, height]`. Per grid since #773: the cell belongs to
 *  the font configuration that grid selects into, and two grids need not share one. */
export const deviceCell = (r, g) => [r.cell_width(g), r.cell_height(g)];

/**
 * Mount the arrangement almost every page here assumes — one terminal filling the whole surface,
 * the buffer exactly `cols * cellWidth` device pixels — the consumer's half of ADR-0021 D3's two
 * tiers (grid dimensions, drawing buffer).
 *
 * Call it again after anything that moves the cell (a font size, a family, either spacing option).
 */
export function fitGrid(r, g, cols, rows) {
  r.resizeGrid(g, cols, rows);
  const [cw, ch] = deviceCell(r, g);
  const [w, h] = [cols * cw, rows * ch];
  r.resizeSurface(w, h);
  r.setViewport(g, 0, 0, w, h);
  return [w, h];
}

/**
 * The device-pixel `readPixels` rect covering `cols` cells starting at grid cell `(col, row)`.
 *
 * The projection puts the grid's origin at the buffer's TOP-left (`orthographic_from_size` uses
 * `top = 0`), while `readPixels` counts from the BOTTOM — hence the flip. A single-row demo must
 * still use it: reading at `y = 0` only happens to work when the buffer height is exactly one cell.
 */
export function cellRect(gl, r, g, col, row = 0, cols = 1) {
  const [cw, ch] = deviceCell(r, g);
  return { x: col * cw, y: gl.drawingBufferHeight - (row + 1) * ch, w: cols * cw, h: ch };
}

/** Read a device-pixel rect back as RGBA bytes. */
export function readRect(gl, { x, y, w, h }) {
  const buf = new Uint8Array(w * h * 4);
  gl.readPixels(x, y, w, h, gl.RGBA, gl.UNSIGNED_BYTE, buf);
  return { buf, w, h };
}

/** Read one grid cell (or `cols` adjacent cells) back as RGBA bytes. */
export const readCells = (gl, r, g, col, row = 0, cols = 1) =>
  readRect(gl, cellRect(gl, r, g, col, row, cols));

/** A pixel is "lit" when its red channel clears the foreground threshold. */
export const LIT_THRESHOLD = 150;

/** Count lit pixels in a rect read by `readRect`. */
export function countLit({ buf }) {
  let n = 0;
  for (let i = 0; i < buf.length; i += 4) if (buf[i] > LIT_THRESHOLD) n++;
  return n;
}

/** Whether the pixel at rect-local `(x, y)` (top-left origin flipped in `cellRect`) is lit. */
export const litAt = ({ buf, w }, x, y) => buf[(y * w + x) * 4] > LIT_THRESHOLD;

/** The default "lit" predicate: a foreground pixel on a dark background clears the red threshold. */
export const litByRed = (r) => r > LIT_THRESHOLD;

/**
 * The fraction of a rect's pixels that count as ink, `0..1`.
 *
 * This is what tells a *filled* glyph from a *hollow* one, and therefore an emoji the font really
 * drew from the browser's missing-glyph box (#334). Tofu is achromatic and has ink, so "achromatic
 * and lit" — how `⬛`/`⚫` prove they took the emoji path — cannot reject it. "Filled" can: measured
 * over the ink box at 24 px, `⬛` covers 0.99 and `⚫` 0.80, while tofu covers 0.23.
 *
 * `isLit(r, g, b, a)` is the caller's, because what counts as ink is the proof's policy, not this
 * file's. The default reads the red channel, which is right for white-on-dark; a page probing a
 * BLACK glyph on a GRAY background (`emoji297.html`) must pass "differs from the background" instead,
 * or every pixel of `⬛` reads as unlit.
 */
export function inkCoverage({ buf, w, h }, isLit = (r) => litByRed(r)) {
  let n = 0;
  for (let i = 0; i < buf.length; i += 4) {
    if (isLit(buf[i], buf[i + 1], buf[i + 2], buf[i + 3])) n++;
  }
  return n / (w * h);
}

/** Min/max of the alpha channel over a rect — the #298 translucency probe. */
export function alphaStats({ buf }) {
  let min = 255, max = 0;
  for (let i = 3; i < buf.length; i += 4) {
    if (buf[i] < min) min = buf[i];
    if (buf[i] > max) max = buf[i];
  }
  return { min, max };
}

/**
 * Whether the runtime has a colour-emoji font (Segoe UI Emoji / Apple Color Emoji / Noto Color Emoji).
 *
 * A precondition, not a result. The emoji proofs assert that the browser draws emoji IN COLOUR, but the
 * glyphs come from the browser's 2D text engine (`OffscreenCanvas.fillText`, via `rasterizer.rs`), not
 * from the renderer. On a font-less host they rasterise monochrome or as tofu, and the proof fails for
 * a reason that has nothing to do with justerm. Probing it lets the failure say so by name (#334).
 */
export function hasColourEmojiFont() {
  const c = new OffscreenCanvas(64, 64);
  const x = c.getContext("2d", { willReadFrequently: true });
  x.font = "24px monospace";
  x.textBaseline = "alphabetic";
  x.fillStyle = "white"; // a monochrome font keeps this; a colour font overrides it
  x.fillText("\u{1F680}", 8, 40);
  const d = x.getImageData(0, 0, 64, 64).data;
  for (let i = 0; i < d.length; i += 4) {
    if (d[i + 3] < 32) continue;
    if (Math.max(d[i], d[i + 1], d[i + 2]) - Math.min(d[i], d[i + 1], d[i + 2]) > 24) return true;
  }
  return false;
}

/**
 * Whether a `cols × rows` grid of device cells fits inside the drawing buffer — the #331/#339
 * identities `proofs.spec.mjs` asserts (docs/map/territory/browser-proof-harness.md).
 *
 * `attr` is what the page asked for (`canvas.width`), `buffer` what WebGL granted, and `clamped` the
 * only observable that separates the two. A clamp shows on the SURFACE, never in `r.cols(g)`, which
 * reports what `resizeGrid` was told.
 */
export function gridFit(gl, r, g, cols, rows) {
  const [cw, ch] = deviceCell(r, g);
  const canvas = gl.canvas;
  return {
    grid: [cols * cw, rows * ch],
    buffer: [gl.drawingBufferWidth, gl.drawingBufferHeight],
    attr: [canvas.width, canvas.height],
    fits: cols * cw <= gl.drawingBufferWidth && rows * ch <= gl.drawingBufferHeight,
    clamped: canvas.width !== gl.drawingBufferWidth || canvas.height !== gl.drawingBufferHeight,
  };
}

/**
 * The CSS `letterSpacing` that makes a bar cursor's thickness clear the cell HEIGHT by `factor`x.
 *
 * A bar's thickness is `round(0.15 * cellWidth)` device px (alacritty `display/cursor.rs:25`,
 * `cursor.rs` `THICKNESS`), and its width is clamped by the cell it sits in — `thickness.min(cell.0)`
 * (`cursor.rs:133`) — never by the cell height. To PROVE that clamp, `cursor.html` needs a cell wide
 * enough that the thickness exceeds the height (else a height-clamp bug is invisible) yet stays under
 * the width (else the width clamp masks it). Sized from the MEASURED cell (#374): the thickness lands
 * at `factor x baseCh`.
 *
 * `baseCw`/`baseCh` are the device cell at `letterSpacing 0`; `dpr` maps CSS spacing to device px the
 * way the renderer does (`round(css * dpr)`, `metrics.rs:75`). Returns CSS px for `setLetterSpacing`.
 */
export function spacingForThickBar(baseCw, baseCh, dpr, factor = 2, thicknessFrac = 0.15) {
  // round(frac * cellW) >= factor * baseCh  is implied by  cellW >= (factor * baseCh) / frac.
  const targetCellW = Math.ceil((factor * baseCh) / thicknessFrac);
  // cellW = baseCw + round(css * dpr); ceil so the rounded device width never lands short.
  return Math.ceil((targetCellW - baseCw) / dpr);
}

// --- Composited pixels (#352) ---------------------------------------------------------------
//
// Helpers for the proofs that read a SCREENSHOT — what the compositor put on screen — rather than
// the drawing buffer `readPixels` reads. A composited proof must not be its browser process's first
// document, must refuse a uniform region before measuring it, and must check per cell:
// docs/map/territory/browser-proof-harness.md says why.

/**
 * Split an RGBA buffer into white / black / intermediate fractions by luminance.
 *
 * Takes **raw RGBA bytes**, unlike `countLit`/`litAt`/`inkCoverage`/`alphaStats`, which take the
 * `{buf, w, h}` rect that `readCells` returns — and throws on that rect rather than answering `NaN`.
 *
 * Reads the **green** channel, where the other helpers read red. Both are luminance for the
 * grayscale patterns these proofs draw (R=G=B); a coloured composited pattern must not use this.
 * `lo`/`hi` deliberately leave a wide intermediate band for antialiasing, and are unrelated to
 * `LIT_THRESHOLD`, which is a binary ink/no-ink cut.
 */
export function tonalSplit(data, { lo = 20, hi = 235 } = {}) {
  if (!ArrayBuffer.isView(data)) {
    throw new TypeError("tonalSplit takes raw RGBA bytes, not a {buf,w,h} rect");
  }
  if (data.length < 4) throw new RangeError("tonalSplit: no pixels — an empty region proves nothing");
  let white = 0, black = 0, mid = 0;
  for (let i = 0; i < data.length; i += 4) {
    const l = data[i + 1];
    if (l >= hi) white++;
    else if (l <= lo) black++;
    else mid++;
  }
  const total = data.length / 4;
  return { white: white / total, black: black / total, mid: mid / total };
}

/**
 * Is this region too uniform to be evidence of anything? A composited proof calls this FIRST,
 * before any metric that would happily describe a blank rectangle. A degenerate (`NaN`) split is
 * uniform.
 */
export function isUniform(split, threshold = 0.9) {
  if (!Number.isFinite(split.white) || !Number.isFinite(split.black)) return true;
  return split.white >= threshold || split.black >= threshold;
}
