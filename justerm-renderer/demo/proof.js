// Shared pixel-reading helpers for the `demo/*.html` proofs (#328).
//
// Every proof reads back the GL drawing buffer to check what was actually drawn, and the buffer is
// addressed in **device pixels**. The renderer's cell is measured in device px too — the rasteriser
// ink-scans `█` at `FONT_SIZE * devicePixelRatio`, and that integer becomes the shader's
// `u_cell_size` — and since #331/#335 `cell_width()`/`cell_height()` report exactly that integer.
//
// Never re-derive it. Neither `cssCellWidth() * dpr` nor `drawingBufferWidth / COLS` recovers it,
// and both were in use here before #328, misreading the buffer whenever `devicePixelRatio !== 1`.

/** The renderer's exact device-pixel cell, `[width, height]`. */
export const deviceCell = (r) => [r.cell_width(), r.cell_height()];

/**
 * The device-pixel `readPixels` rect covering `cols` cells starting at grid cell `(col, row)`.
 *
 * The projection puts the grid's origin at the buffer's TOP-left (`orthographic_from_size` uses
 * `top = 0`), while `readPixels` counts from the BOTTOM — hence the flip. A single-row demo must
 * still use it: reading at `y = 0` only happens to work when the buffer height is exactly one cell.
 *
 * Since #331 the buffer is `cols * cell` device px exactly, so a rect for a cell inside the grid the
 * renderer was sized to always lies inside the buffer.
 */
export function cellRect(gl, r, col, row = 0, cols = 1) {
  const [cw, ch] = deviceCell(r);
  return { x: col * cw, y: gl.drawingBufferHeight - (row + 1) * ch, w: cols * cw, h: ch };
}

/** Read a device-pixel rect back as RGBA bytes. */
export function readRect(gl, { x, y, w, h }) {
  const buf = new Uint8Array(w * h * 4);
  gl.readPixels(x, y, w, h, gl.RGBA, gl.UNSIGNED_BYTE, buf);
  return { buf, w, h };
}

/** Read one grid cell (or `cols` adjacent cells) back as RGBA bytes. */
export const readCells = (gl, r, col, row = 0, cols = 1) =>
  readRect(gl, cellRect(gl, r, col, row, cols));

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
 * Whether a `cols × rows` grid of device cells fits inside the drawing buffer.
 *
 * Since #331 `resize(cols, rows)` sizes the buffer to `cols * cell_width()` exactly, `grid` equals
 * `buffer` for any grid the renderer was sized to. On its own that comparison became an identity —
 * both sides are the same product (#353): it is kept as a tripwire against re-deriving the buffer
 * from a CSS box, not as a live check.
 *
 * `attr` is what makes this falsifiable again (#339). `canvas.width` is what we *asked* the browser
 * for; `drawingBufferWidth` is what it *gave* us, and WebGL is free to give less. Measured in
 * Chromium: `canvas.width = 16385` leaves the attribute at 16385 while the buffer comes back at
 * MAX_TEXTURE_SIZE. So `clamped` is the only observable that separates "we sized the grid" from
 * "the browser overruled us", and the caller should pass `r.cols()`/`r.rows()` — the grid actually
 * adopted — rather than the numbers it hoped for.
 */
export function gridFit(gl, r, cols, rows) {
  const [cw, ch] = deviceCell(r);
  const canvas = gl.canvas;
  return {
    grid: [cols * cw, rows * ch],
    buffer: [gl.drawingBufferWidth, gl.drawingBufferHeight],
    attr: [canvas.width, canvas.height],
    fits: cols * cw <= gl.drawingBufferWidth && rows * ch <= gl.drawingBufferHeight,
    clamped: canvas.width !== gl.drawingBufferWidth || canvas.height !== gl.drawingBufferHeight,
  };
}

// --- Composited pixels (#352) ---------------------------------------------------------------
//
// `gl.readPixels` and a screenshot are DIFFERENT MEASUREMENTS. `readPixels` reads the drawing
// buffer — what GL drew. A screenshot reads what the compositor put on the screen, which is the
// buffer after CSS sizing, `image-rendering`, layer promotion and DPR resampling. Every proof in
// this directory reads the buffer; none of them can say the image ever reached the screen.
//
// The trap: **the first document rendered in a headless Chromium process composites garbage** —
// solid white at `devicePixelRatio != 1`, solid black at 1 — while `readPixels` in that same page
// returns the correct frame. Measured: it is independent of canvas size, of the CSS box (integer,
// fractional or unset), of the DPR and of WebGL; it is NOT cured by ten extra
// `requestAnimationFrame`s, a 300 ms sleep, a throwaway screenshot, or
// `--run-all-compositor-stages-before-draw`. It IS cured by ONE prior navigation to a real document,
// anywhere in the process — `about:blank` does not count (observed; no source found that says why).
// Headed Chromium never shows it.
//
// `page.screenshot()` is CDP `Page.captureScreenshot`, which copies from a surface the first real
// navigation has not presented yet. Chromium names that failure in `page_handler.cc` ("capturing a
// surface snapshot will stall because the surface is never presented"), crbug 377715191. Playwright
// already passes `--enable-features=CDPScreenshotNewSurface`, Chromium's remedy for that class; it
// does not cover this first-surface case. The white-vs-black split is consistent with reading a
// default-cleared buffer, but that last step is a hypothesis, not a citation.
//
// So a composited-pixel proof must (a) not be the first document its browser process renders, and
// (b) refuse a uniform region before measuring anything about it. A blur metric reads solid white
// as "perfectly sharp"; a coverage metric reads solid black as "nothing drawn, as expected".
//
// And (c): a tone HISTOGRAM is blind to structure. Shrink the CSS box to 80 % and the surviving
// pixels are still 50/50 white — `isUniform` is happy, and so is any `|composited - source|` tone
// comparison. Whatever the proof claims about *where* the image landed, it must check per-cell, and
// it must pin the composited region against the drawing buffer's own dimensions.

/**
 * Split an RGBA buffer into white / black / intermediate fractions by luminance.
 *
 * Takes **raw RGBA bytes**, unlike `countLit`/`litAt`/`inkCoverage`/`alphaStats`, which take the
 * `{buf, w, h}` rect that `readCells` returns. Handing it that rect used to iterate
 * `undefined.length` zero times and answer `{NaN, NaN, NaN}` — so it throws instead.
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
 * before any metric that would happily describe a blank rectangle (see the note above).
 *
 * A degenerate split is uniform. `NaN >= 0.9` is `false`, so a naive comparison called the emptiest
 * possible region "not uniform" — the guard was most permissive exactly where it had to be strictest.
 */
export function isUniform(split, threshold = 0.9) {
  if (!Number.isFinite(split.white) || !Number.isFinite(split.black)) return true;
  return split.white >= threshold || split.black >= threshold;
}
