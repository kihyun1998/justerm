// The proofs that read the SCREEN, not the drawing buffer (#352): every `demo/screen-*.html` page,
// which publishes `__composited(pngBase64)` instead of `__proof`. The generic runner skips exactly
// that prefix, so a new screen proof is picked up here.
//
// Deliberate: this spec launches its own browser and burns one navigation before measuring — the
// first document a headless process renders composites garbage. Why, what was ruled out and what
// upstream says: docs/map/territory/browser-proof-harness.md.
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

        // Start `__composited`, park its outcome, harvest it — never await its promise across CDP
        // (#731; docs/map/invariant/an-awaited-in-page-promise-needs-an-anchor.md).
        await page.evaluate((b64) => {
          delete window.__compositedSettled;
          void window.__composited(b64).then(
            (value) => {
              window.__compositedSettled = { ok: true, value };
            },
            (error) => {
              window.__compositedSettled = { ok: false, error: String(error) };
            },
          );
        }, shot.toString("base64"));
        await page.waitForFunction(() => window.__compositedSettled, null, { timeout: 30_000 });
        const settled = await page.evaluate(() => window.__compositedSettled);
        // Rejections are parked too, or a throwing decode would time out above naming nothing.
        if (!settled.ok) throw new Error(`__composited rejected: ${settled.error}`);
        const out = settled.value;
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
