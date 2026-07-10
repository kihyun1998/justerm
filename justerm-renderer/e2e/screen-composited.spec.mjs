// The proofs that read the SCREEN, not the drawing buffer (#352).
//
// Every page under `proofs.spec.mjs` asserts on `gl.readPixels` — what GL drew. None of them can say
// the image ever reached the compositor; a canvas with `display: none` satisfies all ten. A page
// named `screen-*.html` publishes `__composited(pngBase64)` instead of `__proof`, and this spec
// drives every one of them. The generic runner skips exactly that prefix, so a new screen proof is
// picked up here automatically rather than falling between the two runners.
//
// ## Why this spec owns its browser, and warms it up first
//
// The first document a headless Chromium **process** renders composites garbage: the canvas region
// comes back solid white at `deviceScaleFactor != 1` and solid black at 1, while `readPixels` in
// that same page returns the correct frame. Measured (the full grid is in `demo/proof.js`): it is
// independent of canvas size, of the CSS box (integer, fractional, or unset), of the DPR and of
// WebGL. It is NOT cured by ten extra `requestAnimationFrame`s, a 300 ms sleep, a throwaway
// `page.screenshot()`, or `--run-all-compositor-stages-before-draw`. It IS cured by one prior
// navigation to a real document, anywhere in the process — `about:blank` does not count (observed;
// we found no source that says why). Headed Chromium never shows it.
//
// So: launch our own browser, burn one navigation, then measure. Sharing the runner's browser would
// hide the whole thing — by then another proof has warmed the process, and deleting the warm-up
// would change nothing. Verified both ways: with our own browser, deleting `warmUp()` reddens the
// first density; and warming up per-context (which an earlier draft did) is redundant, because one
// navigation warms the whole process.
//
// ## What upstream says
//
// `page.screenshot()` is CDP `Page.captureScreenshot` (Playwright's `screenshotter.ts` / `crPage.ts`
// — no frame wait, no BeginFrame), which lands in `GetSnapshotFromBrowser(from_surface: true)` and
// copies from a surface the first real navigation has not presented yet. Chromium names this failure
// in `page_handler.cc` ("capturing a surface snapshot will stall because the surface is never
// presented") and tracks it as crbug 377715191 / Playwright #33330. Playwright 1.61 ALREADY passes
// `--enable-features=CDPScreenshotNewSurface` (`chromiumSwitches.ts`) — Chromium's own remedy for
// that class — and it does not cover this first-surface case. Do not "discover" it as the fix.
//
// `toHaveScreenshot`'s stability loop is not the fix either: it retries until two consecutive
// screenshots agree, and the garbage frame is *stable*. Two identical blank frames agree. That is
// precisely the gap `isUniform()` fills — refuse a uniform region before measuring anything.
import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { chromium, expect, test } from "@playwright/test";

import config from "../playwright.config.mjs";

const DEMO_DIR = fileURLToPath(new URL("../demo", import.meta.url));
const SCREEN_PROOFS = readdirSync(DEMO_DIR)
  .filter((f) => f.startsWith("screen-") && f.endsWith(".html"))
  .sort();

const RATIOS = [1, 1.1, 1.5, 2];
const BASE_URL = config.use.baseURL; // one source of truth, shared with the generic runner

/** Burn the process's first navigation, whose composited copy is garbage. Any real document does. */
async function warmUp(browser, demo) {
  const context = await browser.newContext({ baseURL: BASE_URL });
  const page = await context.newPage();
  await page.goto(`/demo/${demo}`);
  await page.waitForFunction(() => window.__done === true, null, { timeout: 30_000 });
  await context.close();
}

for (const demo of SCREEN_PROOFS) {
  test(demo, async () => {
    const browser = await chromium.launch();
    try {
      await warmUp(browser, demo);
      for (const deviceScaleFactor of RATIOS) {
        const context = await browser.newContext({
          deviceScaleFactor,
          viewport: { width: 600, height: 300 },
          baseURL: BASE_URL,
        });
        const page = await context.newPage();
        const errors = [];
        page.on("pageerror", (e) => errors.push(String(e)));

        await page.goto(`/demo/${demo}`);
        await page.waitForFunction(() => window.__done === true, null, { timeout: 30_000 });

        const shot = await page.screenshot({ scale: "device" });
        const out = await page.evaluate((b64) => window.__composited(b64), shot.toString("base64"));
        const meta = await page.evaluate(() => window.__meta);

        expect(errors, `${demo} @ dpr ${deviceScaleFactor}`).toEqual([]);
        const failing = Object.entries(out.checks)
          .filter(([, passed]) => !passed)
          .map(([name]) => name);
        expect(
          failing,
          `${demo} @ dpr ${deviceScaleFactor}: buffer ${meta.buffer.join("x")} cell ` +
            `${meta.cell.join("x")} rect ${JSON.stringify(meta.rect)} source ` +
            `${JSON.stringify(meta.source)} composited ${JSON.stringify(out.composited)} ` +
            `columns ${JSON.stringify(out.columns)}`,
        ).toEqual([]);

        await context.close();
      }
    } finally {
      await browser.close();
    }
  });
}
