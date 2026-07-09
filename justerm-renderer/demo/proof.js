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
 * Whether a `cols × rows` grid of device cells fits inside the drawing buffer.
 *
 * Since #331 `resize(cols, rows)` sizes the buffer to `cols * cell_width()` exactly, so this is now
 * an INVARIANT rather than a warning: `fits` is true and `grid` equals `buffer` for any grid the
 * renderer was sized to. Kept, and asserted by the runner, because it is the property that used to
 * fail — at dpr 1.1 every proof's grid overhung its buffer by 1–2 device px.
 */
export function gridFit(gl, r, cols, rows) {
  const [cw, ch] = deviceCell(r);
  return {
    grid: [cols * cw, rows * ch],
    buffer: [gl.drawingBufferWidth, gl.drawingBufferHeight],
    fits: cols * cw <= gl.drawingBufferWidth && rows * ch <= gl.drawingBufferHeight,
  };
}
