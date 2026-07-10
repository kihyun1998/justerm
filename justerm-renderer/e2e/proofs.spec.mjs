// Headless runner for the `demo/*.html` GL proofs (#328).
//
// Each proof drives the REAL wasm renderer against a REAL WebGL2 context and publishes
// `window.__proof.ok` after reading pixels back. Until now they were a manual ritual: someone
// remembered to open the pages in a browser. That is how #328 went unnoticed — every proof was red
// on any HiDPI machine because they addressed the device-px drawing buffer with CSS-px arithmetic.
//
// So the sweep runs each page at several device pixel ratios. A proof that only holds at dpr 1 is
// not a proof of anything a real user sees.
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test, expect } from "@playwright/test";

const DEMO_DIR = fileURLToPath(new URL("../demo", import.meta.url));
const DEMOS = readdirSync(DEMO_DIR)
  .filter((f) => f.endsWith(".html"))
  .sort();

// 1 is the baseline; 1.5 is a Windows box at 150 % display scaling; 2 is Retina. 1.1 earns its place:
// it is browser zoom at 110 %, and it is the density at which every proof's grid used to overhang its
// drawing buffer (#331). A sweep that skips the awkward ratios proves the easy half of the contract.
const RATIOS = [1, 1.1, 1.5, 2];

/** Load a proof page and return its `window.__proof` once it has published `__done`. */
async function runProof(browser, deviceScaleFactor, demo) {
  const context = await browser.newContext({ deviceScaleFactor });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  try {
    await page.goto(`/demo/${demo}`);
    await page.waitForFunction(() => window.__done === true, null, { timeout: 30_000 });
    return { proof: await page.evaluate(() => window.__proof), errors };
  } finally {
    await context.close();
  }
}

for (const deviceScaleFactor of RATIOS) {
  test.describe(`devicePixelRatio ${deviceScaleFactor}`, () => {
    for (const demo of DEMOS) {
      test(demo, async ({ browser }) => {
        const { proof, errors } = await runProof(browser, deviceScaleFactor, demo);
        expect(errors, `page errors in ${demo}`).toEqual([]);

        // Name the failing checks rather than just asserting `ok` — a bare `false` tells the next
        // reader nothing about which property broke.
        const failing = Object.entries(proof.checks ?? {})
          .filter(([, passed]) => !passed)
          .map(([name]) => name);
        expect(failing, `failing checks in ${demo}`).toEqual([]);
        expect(proof.ok, `${demo} reported not-ok`).toBe(true);

        // #331: the drawing buffer IS the grid. Since resize() derives one from the other this can
        // no longer fail on its own (#353) — it stays as a tripwire against re-deriving the buffer
        // from a CSS box, which is what made every demo's grid overhang its buffer at dpr 1.1.
        if (proof.gridFit) {
          expect(
            proof.gridFit.grid,
            `${demo} @ dpr ${deviceScaleFactor}: grid must equal the drawing buffer (#331)`,
          ).toEqual(proof.gridFit.buffer);
          // #339: this one CAN fail. `canvas.width` is the size we asked for; `drawingBufferWidth`
          // is the size WebGL granted. If they diverge the renderer is drawing a grid the viewport
          // cannot hold, and nothing else in the harness would notice.
          expect(
            proof.gridFit.attr,
            `${demo} @ dpr ${deviceScaleFactor}: the browser clamped the drawing buffer below ` +
              `canvas.width and resize() did not adopt the grid that fits (#339)`,
          ).toEqual(proof.gridFit.buffer);
        }
      });
    }
  });
}

test("the atlas is rasterised in device pixels, not CSS pixels", async ({ browser }) => {
  // #265's property (b), which no single page can falsify: the atlas is baked at `FONT_SIZE * dpr`,
  // so the DEVICE cell grows with the density while the CSS cell stays put. A CSS-px atlas would do
  // the opposite — a fixed device cell and a cell_width() that halves at dpr 2. Needs one run per
  // ratio, hence the runner rather than the page.
  const measured = [];
  for (const deviceScaleFactor of RATIOS) {
    const { proof } = await runProof(browser, deviceScaleFactor, "dpr.html");
    measured.push({
      dpr: proof.dpr,
      cssCell: [proof.cellCssW, proof.cellCssH],
      deviceCell: proof.deviceCell,
    });
  }
  const at1 = measured.find((m) => m.dpr === 1);
  const label = JSON.stringify(measured);

  for (const m of measured) {
    for (const axis of [0, 1]) {
      // The device cell tracks the density. `±1` because the cell is an ink-scan of `█` at the
      // scaled font size, not an exact multiple — e.g. the height is 33, not 32, at dpr 2.
      const expectedDevice = at1.deviceCell[axis] * m.dpr;
      expect(Math.abs(m.deviceCell[axis] - expectedDevice), `device cell @ dpr ${m.dpr}: ${label}`)
        .toBeLessThanOrEqual(1);

      // The CSS cell does NOT track it. It is an unrounded float now (#331), so it still drifts by
      // up to a pixel — the ink-scan is 16 device px at dpr 1 and 33 (not 32) at dpr 2, i.e. 16.5
      // CSS px. What matters is that it does not SCALE with the density, which is what a CSS-px
      // atlas would do.
      expect(Math.abs(m.cssCell[axis] - at1.cssCell[axis]), `css cell @ dpr ${m.dpr}: ${label}`)
        .toBeLessThanOrEqual(1);
    }
  }

  // Guard against the assertions above passing vacuously at a single density.
  const [lo, hi] = [measured[0], measured[measured.length - 1]];
  expect(hi.deviceCell[0]).toBeGreaterThan(lo.deviceCell[0]);
  expect(hi.deviceCell[1]).toBeGreaterThan(lo.deviceCell[1]);
});
