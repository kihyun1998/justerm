// Unit tests for `demo/proof.js` — the pixel helpers every `demo/*.html` proof reads its evidence
// through. They had none. A bug here does not fail a proof; it silently weakens all nine of them.
//
// Run: `node --test e2e/proof.test.mjs` (node's built-in runner — no dependency).
import test from "node:test";
import assert from "node:assert/strict";

import { alphaStats, cellRect, countLit, inkCoverage, litAt } from "../demo/proof.js";

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
