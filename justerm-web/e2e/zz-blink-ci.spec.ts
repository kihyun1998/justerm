// TEMPORARY CI probe (never merged): why a blink probe misses the OFF phase on the runner.
import { test, expect } from "@playwright/test";
import { DEMO_URL } from "../playwright.config";
import { readAsyncProbe } from "./probe";
for (const probe of ["__cursorBlinkProbe", "__blinkIdleProbe", "__composeCaretProbe"]) for (let rep = 0; rep < 6; rep++)
test(`zz blink ${probe} #${rep}`, async ({ page }) => {
  test.setTimeout(90_000);
  const t0 = Date.now();
  await page.goto(DEMO_URL);
  await expect(page.getByRole("button", { name: "Cursor blink: OFF" })).toBeVisible({ timeout: 30_000 });
  const boot = Date.now() - t0;
  await page.locator("#term").dispatchEvent("mousedown");
  await readAsyncProbe(page, probe as any);
  const ev: string[] = await page.evaluate(() => (window as any).__trace.ev);
  // split into poll windows
  const polls: string[][] = []; let cur: string[] | null = null;
  for (const e of ev) { if (e.startsWith("POLL")) { cur = [e]; polls.push(cur); } else if (cur) { cur.push(e); if (e.startsWith("END")) cur = null; } }
  const rows = await page.evaluate(() => document.querySelectorAll("[role='list'] [role='listitem']").length);
  for (const w of polls) {
    const end = w.find((e) => e.startsWith("END")) ?? "END?";
    const ps = w.filter((e) => e.startsWith("P")).length, fs = w.filter((e) => e.startsWith("F")).length, ss = w.filter((e) => e.startsWith("S")).length;
    const gaps = w.filter((e) => e.startsWith("G")).map((e) => e.split(":")[1]);
    console.log(`ZZB ${probe} #${rep} boot=${boot}ms a11yRows=${rows} ${w[0]} ${end} samples=${ss} presents=${ps} frames=${fs} gaps>120ms=[${gaps.join(",")}]`);
    if (end.includes("MISSED")) console.log(`ZZB-TRACE ${w.join(" ")}`);
  }
});
