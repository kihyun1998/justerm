// TEMPORARY CI measurement (renderer 0.23 vs 0.24 on the GitHub runner). Never merged.
import { test } from "@playwright/test";
import { DEMO_URL } from "../playwright.config";
test("zz ci probe", async ({ browser }) => {
  test.setTimeout(240_000);
  const lines: string[] = [];
  const say = (s: string) => { lines.push(s); console.log(`CIPROBE ${s}`); };
  for (const fam of ["monospace", "DejaVu Sans Mono"]) {
    const page = await browser.newPage();
    const fit: string[] = [];
    page.on("console", (m) => { if (m.text().includes("[fit]")) fit.push(m.text()); });
    // boot
    const boots: number[] = [];
    for (let i = 0; i < 3; i++) {
      const t0 = Date.now();
      await page.goto(DEMO_URL + (fam === "monospace" ? "" : "?probeFont=1"));
      await page.getByRole("button", { name: /Finish command/ }).waitFor({ timeout: 60_000 });
      await page.evaluate(() => 1);
      boots.push(Date.now() - t0);
    }
    if (fam !== "monospace") await page.evaluate((f) => (window as any).__probeSetFont?.(f), fam);
    // page latency while the demo's 300ms live output runs (the state every e2e test is in)
    await page.waitForTimeout(1500);
    const lat: number[] = [];
    for (let i = 0; i < 25; i++) { const t0 = Date.now(); await page.evaluate(() => 1); lat.push(Date.now() - t0); await page.waitForTimeout(97); }
    const busy = await page.evaluate(() => new Promise<number>((res) => { let n = 0, last = performance.now(), gap = 0; const t0 = last; const f = () => { const now = performance.now(); gap += Math.max(0, now - last - 20); last = now; if (now - t0 < 3000) { n++; setTimeout(f, 10); } else res(Math.round((gap / (now - t0)) * 100)); }; setTimeout(f, 10); }));
    lat.sort((a, b) => a - b);
    say(`font=${fam} LIVE latency median=${lat[12]}ms p90=${lat[22]}ms max=${lat[24]}ms mainThreadBlocked~${busy}%`);
    await page.evaluate(() => window.__output!(false));
    await page.waitForTimeout(800);
    const info = await page.evaluate(() => {
      const gl = document.createElement("canvas").getContext("webgl2")!;
      const e = gl.getExtension("WEBGL_debug_renderer_info");
      return { gl: e ? gl.getParameter(e.UNMASKED_RENDERER_WEBGL) : "?", cores: navigator.hardwareConcurrency, dpr: devicePixelRatio,
        canvas: (() => { const c = document.querySelector("#term") as HTMLCanvasElement; return `${c.width}x${c.height}`; })() };
    });
    // per present
    const pp: number[] = [];
    for (let r = 0; r < 3; r++) {
      const t0 = Date.now();
      await page.evaluate(() => { for (let i = 0; i < 20; i++) window.__seedRows!(1); });
      await page.evaluate(() => 1);
      pp.push(Math.round((Date.now() - t0) / 20));
    }
    // re-bake (dpr change forces a new atlas + fit), then back
    const rb: number[] = [];
    for (const d of [1.5, 1, 1.5, 1]) {
      const t0 = Date.now();
      await page.evaluate((x) => window.__setDpr!(x), d);
      await page.evaluate(() => 1);
      rb.push(Date.now() - t0);
    }
    say(`font=${fam} gl=${info.gl} cores=${info.cores} dpr=${info.dpr} canvas=${info.canvas} fit=${fit.at(-1) ?? "?"} boot=${boots.join("/")}ms present=${pp.join("/")}ms rebake=${rb.join("/")}ms`);
    await page.close();
  }
});
