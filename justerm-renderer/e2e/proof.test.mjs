// Unit tests for `demo/proof.js` — the pixel helpers every `demo/*.html` proof reads its evidence
// through. They had none. A bug here does not fail a proof; it silently weakens all nine of them.
//
// Run: `node --test e2e/proof.test.mjs` (node's built-in runner — no dependency).
import test from "node:test";
import assert from "node:assert/strict";

import { alphaStats, cellRect, countLit, gridFit, inkCoverage, isUniform, litAt, tonalSplit } from "../demo/proof.js";

/**
 * Build an RGBA rect from a picture: `#` is a lit pixel (red 255), `.` is background (red 0).
 * The expected counts below are read off the picture by eye, not recomputed from the code.
 */
function rect(rows) {
  const h = rows.length, w = rows[0].length;
  const buf = new Uint8Array(w * h * 4);
  rows.forEach((row, y) =>
    [...row].forEach((ch, x) => {
      const i = (y * w + x) * 4;
      const lit = ch === "#";
      buf[i] = lit ? 255 : 0; // red — the channel the helpers threshold on
      buf[i + 3] = lit ? 255 : 64; // alpha
    }),
  );
  return { buf, w, h };
}

test("a filled block covers its cell; a hollow outline barely does", () => {
  // #334: with no colour-emoji font, `⬛` rasterises as tofu — the browser's missing-glyph outline.
  // Both are achromatic and both have lit pixels, so `achromaticEmojiPath` (lit > 0, no red
  // dominance) and `squareWasAchromatic` (low spread) are satisfied by BOTH, and the emoji-path
  // proof passes vacuously. They differ in one obvious, measurable way: the block is FILLED.
  //
  // Measured in headless Chromium at 24 px, ink over the glyph's own bounding box:
  //   ⬛ 0.988   ⚫ 0.804   🚀 0.523   A 0.359   tofu 0.228
  const block = rect(Array(10).fill("##########"));
  const tofu = rect([
    "##########",
    "#........#",
    "#........#",
    "#........#",
    "#........#",
    "#........#",
    "#........#",
    "#........#",
    "#........#",
    "##########",
  ]);

  assert.equal(inkCoverage(block), 1);
  // 100 pixels; the outline lights 10 + 10 + 8*2 = 36 of them.
  assert.equal(inkCoverage(tofu), 0.36);
  // ...and that is what separates them: the demo's threshold sits between.
  assert.ok(inkCoverage(tofu) < 0.5 && inkCoverage(block) > 0.5);
});

test("the row index is flipped, because readPixels counts from the bottom", () => {
  // The projection puts the grid's origin at the buffer's TOP-left (`orthographic_from_size` uses
  // `top = 0`); `readPixels` counts from the BOTTOM. Get this wrong and every proof reads a
  // different row than it names — the single most load-bearing line in the file.
  const gl = { drawingBufferWidth: 40, drawingBufferHeight: 30 };
  const r = { cell_width: () => 10, cell_height: () => 10 };

  // Row 0 is drawn at the top of a 30-px-tall buffer, so it occupies GL rows 20..29.
  assert.deepEqual(cellRect(gl, r, 0, 0), { x: 0, y: 20, w: 10, h: 10 });
  // The bottom row sits at the GL origin.
  assert.deepEqual(cellRect(gl, r, 0, 2), { x: 0, y: 0, w: 10, h: 10 });
  // Columns advance left-to-right in both spaces, and `cols` widens the rect without moving it.
  assert.deepEqual(cellRect(gl, r, 3, 1), { x: 30, y: 10, w: 10, h: 10 });
  assert.deepEqual(cellRect(gl, r, 1, 1, 2), { x: 10, y: 10, w: 20, h: 10 });
});

test("a pixel is lit by its red channel, strictly above the threshold", () => {
  // 150 is the threshold; the proofs draw white-on-dark, so a foreground pixel clears it and an
  // antialiased edge below it does not.
  const strip = rect(["#.#."]);
  assert.equal(countLit(strip), 2);
  assert.ok(litAt(strip, 0, 0));
  assert.ok(!litAt(strip, 1, 0));
  assert.ok(litAt(strip, 2, 0));

  // The boundary itself: `> 150`, not `>= 150`. Nothing else in the suite would notice the flip.
  const boundary = { buf: Uint8Array.from([150, 0, 0, 255, 151, 0, 0, 255]), w: 2, h: 1 };
  assert.equal(countLit(boundary), 1);
  assert.ok(!litAt(boundary, 0, 0));
  assert.ok(litAt(boundary, 1, 0));
});

test("alpha stats span the rect, so a translucent cell is distinguishable from an opaque one", () => {
  // #298: a default-background cell drops to ~half alpha while its glyph strokes stay opaque, so
  // the proof needs BOTH ends of the range, not an average.
  const mixed = rect(["#.", ".#"]); // lit pixels are alpha 255, unlit are alpha 64
  assert.deepEqual(alphaStats(mixed), { min: 64, max: 255 });

  const opaque = rect(["##", "##"]);
  assert.deepEqual(alphaStats(opaque), { min: 255, max: 255 });
});

test("what counts as ink is the caller's, not the helper's", () => {
  // The default predicate reads the RED channel, which is right for the white-on-dark proofs. But
  // `emoji297.html` draws a BLACK square on a GRAY background to prove the achromatic-emoji path:
  // its red channel is 0 everywhere, so the default would report zero coverage for a fully filled
  // cell — the exact glyph the check exists to measure. (Observed in the browser, not imagined.)
  const blackOnGray = { buf: new Uint8Array(4 * 4 * 4), w: 4, h: 4 };
  for (let i = 0; i < blackOnGray.buf.length; i += 4) {
    blackOnGray.buf[i] = blackOnGray.buf[i + 1] = blackOnGray.buf[i + 2] = 0; // black ink
    blackOnGray.buf[i + 3] = 255;
  }

  assert.equal(inkCoverage(blackOnGray), 0); // the default predicate sees nothing
  const differsFromGray = (r, g, b) =>
    Math.abs(r - 128) > 12 || Math.abs(g - 128) > 12 || Math.abs(b - 128) > 12;
  assert.equal(inkCoverage(blackOnGray, differsFromGray), 1); // the page's predicate sees it all
});

// A `gl` stand-in: `gridFit` reads only these four numbers plus `gl.canvas`.
const fakeGl = (bufferW, bufferH, attrW = bufferW, attrH = bufferH) => ({
  drawingBufferWidth: bufferW,
  drawingBufferHeight: bufferH,
  canvas: { width: attrW, height: attrH },
});
const fakeRenderer = (cw, ch) => ({ cell_width: () => cw, cell_height: () => ch });

test("gridFit reports no clamp when WebGL granted the buffer that was asked for", () => {
  // #339: the ordinary case. `canvas.width` and `drawingBufferWidth` agree, so the grid the renderer
  // adopted is the grid it drew.
  const fit = gridFit(fakeGl(360, 144), fakeRenderer(9, 18), 40, 8);
  assert.deepEqual(fit.grid, [360, 144]);
  assert.deepEqual(fit.buffer, [360, 144]);
  assert.deepEqual(fit.attr, [360, 144]);
  assert.equal(fit.fits, true);
  assert.equal(fit.clamped, false);
});

test("gridFit sees the clamp that `grid === buffer` cannot", () => {
  // Measured in Chromium: `canvas.width = 16385` leaves the ATTRIBUTE at 16385 while the drawing
  // buffer comes back at MAX_TEXTURE_SIZE. Pre-#339 the renderer kept the oversized grid, so
  // `grid === buffer` compared 16385*1 against... 16385, and reported a clean fit for a viewport
  // that could not hold it. Only `attr` vs `buffer` can tell.
  const fit = gridFit(fakeGl(16384, 144, 16385, 144), fakeRenderer(1, 18), 16384, 8);
  assert.equal(fit.clamped, true, "a buffer smaller than the canvas attribute is a clamp");
  assert.deepEqual(fit.buffer, [16384, 144]);
  assert.deepEqual(fit.attr, [16385, 144]);
});

test("gridFit's `fits` is about the grid, `clamped` is about the browser — they are independent", () => {
  // A grid that overhangs a buffer the browser granted in full: `fits` false, `clamped` false.
  const overhang = gridFit(fakeGl(360, 144), fakeRenderer(9, 18), 41, 8);
  assert.equal(overhang.fits, false);
  assert.equal(overhang.clamped, false);
  // And a grid that fits a buffer the browser shrank: `fits` true, `clamped` true.
  const shrunk = gridFit(fakeGl(360, 144, 400, 144), fakeRenderer(9, 18), 40, 8);
  assert.equal(shrunk.fits, true);
  assert.equal(shrunk.clamped, true);
});

// --- #352: a composited screenshot needs a guard `readPixels` never did ---

/** RGBA bytes from a luminance picture: `#` = 255, `.` = 0, `~` = 128 (an edge/AA pixel). */
const bytes = (rows) => {
  const h = rows.length, w = rows[0].length;
  const buf = new Uint8Array(w * h * 4);
  rows.forEach((row, y) =>
    [...row].forEach((ch, x) => {
      const l = ch === "#" ? 255 : ch === "~" ? 128 : 0;
      const i = (y * w + x) * 4;
      buf[i] = buf[i + 1] = buf[i + 2] = l;
      buf[i + 3] = 255;
    }),
  );
  return buf;
};

test("tonalSplit reads the picture, not the code that drew it", () => {
  // Counted by eye off the picture: 8 white, 6 black, 2 intermediate, of 16.
  const split = tonalSplit(bytes(["####....", "####~~.."]));
  assert.equal(split.white, 8 / 16);
  assert.equal(split.black, 6 / 16);
  assert.equal(split.mid, 2 / 16);
});

test("isUniform rejects the frame headless hands back before its first real paint", () => {
  // #352: the FIRST document rendered in a headless Chromium process composites garbage — solid
  // white at dpr != 1, solid black at dpr 1 — while `gl.readPixels` in that same page returns the
  // correct frame. A blur/coverage metric reads solid white as "perfectly sharp", so a composited
  // proof MUST refuse a uniform region before measuring anything about it.
  assert.equal(isUniform(tonalSplit(bytes(["########", "########"]))), true, "all white");
  assert.equal(isUniform(tonalSplit(bytes(["........", "........"]))), true, "all black");
  // The real pattern: alternating block/space, ~50/50. Evidence, not garbage.
  assert.equal(isUniform(tonalSplit(bytes(["####....", "####...."]))), false);
  // A nearly-but-not-quite uniform region is still refused — the default threshold is 0.9.
  assert.equal(isUniform(tonalSplit(bytes(["#########.", "##########"]))), true, "95% white");
  assert.equal(isUniform(tonalSplit(bytes(["#####.....", "#########."]))), false, "70% white");
});

test("a region with no pixels is not evidence — the guard must not answer NaN", () => {
  // #352, found by the sibling lens. `tonalSplit` divided by `total = 0`, so every fraction was NaN,
  // and `NaN >= 0.9` is false — `isUniform` called the emptiest possible region "not uniform", i.e.
  // valid evidence. A `display: none` canvas (rect.width === 0) landed exactly there, and only the
  // tone-delta check caught it, by the accident of `|NaN - 0.5| < tol` also being false.
  assert.throws(() => tonalSplit(new Uint8Array(0)), /no pixels/);
  // And the wrong argument shape — `{buf, w, h}`, which every OTHER helper in this file takes —
  // used to iterate `undefined.length` zero times and return the same NaN triple.
  assert.throws(() => tonalSplit({ buf: new Uint8Array(16), w: 2, h: 2 }), /RGBA bytes/);
  // Belt and braces: a split that somehow arrives degenerate is uniform, never evidence.
  assert.equal(isUniform({ white: NaN, black: NaN, mid: NaN }), true);
});
