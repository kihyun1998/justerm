import { expect, test, type BrowserContext, type Page } from "@playwright/test";

import { DEMO_URL } from "../playwright.config";
import { readAsyncProbe as harvest } from "./probe";

/**
 * Epic #287 S8 (#776) — **two terminals on one canvas, in a real browser.**
 *
 * Every slice of the epic before this one is proven by unit tests against a fake backend and by
 * pixel probes inside the renderer's own crate. This file is the consumer-side proof, and it is not
 * a nicety: measured at the start of the slice, nothing outside `src/` called `TerminalSurface.open`,
 * `JustermRenderer.attach`, `observeViewportRect` or `onDensityChange`, so the adapter's
 * `composedSurface === false` branch had never executed in a browser at all.
 *
 * It drives `demo/shared-surface.html`. `demo/index.html` is untouched by this slice — it is the
 * harness the rest of the suite is written against, and moving it would quietly change what a large
 * number of unrelated assertions mean.
 *
 * **What a pixel here can and cannot say.** `readPixels` reads the drawing buffer *before* the
 * compositor, so it sees what the renderer drew and not what a person sees; the page's checkerboard
 * exists so those two do not diverge (#577). What makes the shared-canvas claim readable at all is
 * that `draw()` clears the whole buffer to `rgba(0,0,0,0)` and each grid then clears only inside its
 * own scissor rect — so canvas that no grid was placed over stays transparent, and a sample there
 * distinguishes "two rects on one buffer" from "one grid spanning both".
 */

/** The probes `demo/shared-surface.ts` installs. */
type AsyncProbe = "__independenceProbe" | "__surfaceLossProbe";

/** This page's typed alias over the shared park-and-harvest helper (#731; extracted in #776). */
const readAsyncProbe = <K extends AsyncProbe>(
  page: Page,
  name: K,
): Promise<Awaited<ReturnType<NonNullable<Window[K]>>>> =>
  harvest<Awaited<ReturnType<NonNullable<Window[K]>>>>(page, name);

const READY = "[data-testid='surface-ready']";

/**
 * The **stage's** CSS box and the density. Deliberately the stage and not the canvas.
 *
 * Every "is the buffer the right size" claim has to stand on something derived from neither the
 * buffer nor the grant, and it took two tries to find one. `cssWidth()` is `bufW / dpr` inside the
 * renderer (`justerm-renderer/src/webgl.rs`), so `bufW === round(cssW * dpr)` is an identity holding
 * for any buffer at all — measured, 7x5 device px short and green. Reading the **canvas** element's
 * box instead does not help and was the second version of this: `resizeSurface` writes that box from
 * `cssWidth()`, so it is the same derived number wearing a DOM shape, and it measured green too. The
 * stage is sized from the page's own intended layout and never from anything the renderer returns,
 * which is what makes it an independent quantity.
 */
const stageBox = (page: Page): Promise<{ w: number; h: number; dpr: number }> =>
  page.evaluate(() => {
    const r = document.querySelector("#stage")!.getBoundingClientRect();
    return { w: r.width, h: r.height, dpr: window.devicePixelRatio };
  });

/** The `#735` warm-up's two explicit budgets, summing to 20s inside the hook's 30s slot. */
const GOTO_BUDGET_MS = 8_000;
const BAR_BUDGET_MS = 12_000;

/**
 * #735 — the cold boot is paid here, where the budget can absorb it.
 *
 * **This is a copy of `demo.spec.ts`'s hook, and it has to be.** `beforeAll` runs once per file per
 * worker, `browser` is worker-scoped, and playwright spreads files across workers — so this file
 * lands in its own worker with its own cold browser process and inherits nothing from the other
 * spec's warm-up. That consequence is stated in the other file's own comment; this is it happening.
 *
 * Fail-soft, for the same reason it is there: the hook asserts nothing, and the only thing it could
 * prove is already proven per test by `beforeEach` with a better message. A throw is logged and the
 * run simply pays the cold boot on test one.
 */
test.beforeAll(async ({ browser }) => {
  let context: BrowserContext | undefined;
  try {
    context = await browser.newContext({ baseURL: DEMO_URL });
    const page = await context.newPage();
    await page.goto("/shared-surface.html", { timeout: GOTO_BUDGET_MS });
    await page.locator(READY).waitFor({ state: "visible", timeout: BAR_BUDGET_MS });
  } catch (e) {
    console.log(`[e2e] warm-up navigation did not complete (#735); test one pays the cold boot: ${e}`);
  } finally {
    await context?.close();
  }
});

/**
 * One navigation per test, and it is this hook's (#733/#653).
 *
 * The gate is a node **the subject emits**: `demo/shared-surface.ts` writes this paragraph as its
 * last statement, after both terminals are mounted and every probe is installed. The other page's
 * gate is its control bar, which is a *proxy* — sound only because the one `await` between the bar
 * and the probe assignments resolves on the microtask queue, a validity condition that spec records.
 * This page needs no such condition, and the one comparable Playwright suite gates the same way
 * (xterm.js waits for `.xterm-rows`, `test/playwright/TestUtils.ts:515`).
 */
test.beforeEach(async ({ page }) => {
  await page.goto("/shared-surface.html");
  await expect(page.locator(READY)).toHaveText(/^ready — A \d+x\d+ · B \d+x\d+/);
});

/**
 * Headless SwiftShader loses contexts on its own — 2 of 6 runs in one session, 0 of 12 in the next,
 * with no test deliberately losing one (#580). Every pixel then reads `0,0,0,0`, which is an
 * environment failure and not a defect. Naming it is the repair; waiting is not, since a lost
 * context never gets its buffer back.
 */
const expectContextAlive = (s: { contextLost: boolean }): void => {
  expect(
    s.contextLost,
    "the WebGL context was lost during the probe — every pixel below reads 0, which is an environment failure and not a defect in the thing under test",
  ).toBe(false);
};

/** `rgba` of the two terminals' `defaultBg`, as `demo/shared-surface.ts` sets them. Written out
 * rather than derived: a colour the consumer chose is one of the few things on this page that IS
 * portable — unlike a cell dimension, which is a font's ink box and differs on CI (#578). */
const BG_A = "27,42,74,255"; // 0x1b2a4a
const BG_B = "18,58,36,255"; // 0x123a24
const UNPAINTED = "0,0,0,0";

test("two grids draw on one canvas, each in its own rect, with the buffer bare between them", async ({
  page,
}) => {
  const s = await page.evaluate(() => window.__surfaceProbe!());
  expectContextAlive(s);

  // One buffer, sized by the HOST in device px — the widget cannot derive it, because a buffer
  // holding N grids in M font configurations has no cell to be a multiple of (ADR-0021 D3).
  //
  // Compared against the STAGE's box — see `stageBox` for why neither `cssWidth()` nor the canvas
  // element's own box can carry this, both of them being the grant wearing a different shape.
  const box = await stageBox(page);
  expect(s.bufW).toBe(Math.round(box.w * box.dpr));
  expect(s.bufH).toBe(Math.round(box.h * box.dpr));
  // The display box is written from what the browser GRANTED rather than from what was asked for
  // (#339), so this pair says the grant matched the request. A real clamp would fail here, which is
  // information: at 900x340 there is nothing near a limit, and a CI that starts clamping should say
  // so rather than quietly drawing a smaller terminal.
  expect(s.cssW).toBe(box.w);
  expect(s.cssH).toBe(box.h);

  // Each pane painted its own background, and the two differ — without the second assertion the
  // first pair could both be satisfied by one grid covering the whole canvas.
  expect(s.a.centre).toBe(BG_A);
  expect(s.b.centre).toBe(BG_B);
  expect(s.a.centre).not.toBe(s.b.centre);

  // THE SHARED-CANVAS CLAIM. The gutter between the panes and the band above pane B are buffer that
  // no grid was placed over, and they come back fully transparent — so the two rects are two
  // placements on one buffer rather than one grid spanning both, and the page's checkerboard is
  // what a person sees through them.
  expect(s.gutter).toBe(UNPAINTED);
  expect(s.band).toBe(UNPAINTED);
  // …and that is not "everything reads transparent": both panes' pixels are opaque.
  expect(s.gutter).not.toBe(s.a.centre);
  expect(s.gutter).not.toBe(s.b.centre);
});

test("each GL viewport sits where its DOM overlay does, at a non-zero origin (#775)", async ({
  page,
}) => {
  const s = await page.evaluate(() => window.__surfaceProbe!());
  expectContextAlive(s);

  // Derived from the DOM, independently of anything the page recorded: the contract is that a
  // terminal's GL viewport follows its transparent overlay, so the expected origin comes from
  // measuring the two elements rather than from re-reading the page's own constants.
  const boxes = await page.evaluate(() => {
    const canvas = document.querySelector("#surface")!.getBoundingClientRect();
    const rect = (sel: string): { x: number; y: number } => {
      const r = document.querySelector(sel)!.getBoundingClientRect();
      return { x: r.left - canvas.left, y: r.top - canvas.top };
    };
    return { dpr: window.devicePixelRatio, a: rect("#pane-a"), b: rect("#pane-b") };
  });

  // The arithmetic: what the page computed from the overlay agrees with what this spec computed
  // from the same two elements. This checks `viewportOrigin`, and by itself it is **not** a
  // placement proof — see the pixel reads below.
  expect(s.a.rectX).toBe(Math.round(boxes.a.x * boxes.dpr));
  expect(s.a.rectY).toBe(Math.round(boxes.a.y * boxes.dpr));
  expect(s.b.rectX).toBe(Math.round(boxes.b.x * boxes.dpr));
  expect(s.b.rectY).toBe(Math.round(boxes.b.y * boxes.dpr));

  // Anti-vacuity, and the reason pane B is not placed at the origin: the renderer flips a top-origin
  // rect to GL's bottom-origin y itself, and at `y = 0` a sign error is invisible. Pane A at the
  // origin is the control — it is where a sole tenant sits, so the pair says the placement is
  // computed rather than constant.
  expect(s.b.rectX).toBeGreaterThan(0);
  expect(s.b.rectY).toBeGreaterThan(0);
  expect(s.a.rectX).toBe(0);
  expect(s.a.rectY).toBe(0);

  // THE PLACEMENT PROOF, and it has to be a pixel. **Measured while writing this**: with the one
  // `setViewportRect` call deleted from the page — so both grids draw at the origin and overlap —
  // every assertion above still passed, because they compare two derivations of a number and
  // neither of them is affected by whether the renderer was ever told it. Three other tests went
  // red; this one, the placement test, went green. A pixel read at a DOM-derived point is what
  // closes that: it is only the pane's own background if the grid is actually drawn there.
  //
  // Sampled at the top-RIGHT of each pane's cell extent, three device px in. Row 0 carries each
  // pane's title, which is far shorter than its column count, so that corner is background rather
  // than glyph ink on any font — where a top-left sample would land inside the first character.
  const topRight = (origin: { x: number; y: number }, cols: number, cellW: number): Promise<string> =>
    page.evaluate(
      ([x, y]) => window.__pixelAt!(x!, y!),
      [Math.round(origin.x * boxes.dpr) + cols * cellW - 3, Math.round(origin.y * boxes.dpr) + 3],
    );

  expect(await topRight(boxes.a, s.a.cols, s.a.cellW)).toBe(BG_A);
  expect(await topRight(boxes.b, s.b.cols, s.b.cellW)).toBe(BG_B);
});

test("two font sizes give the two grids two cell geometries (#772 per-config tier)", async ({
  page,
}) => {
  const s = await page.evaluate(() => window.__surfaceProbe!());
  expectContextAlive(s);

  // The relation, never the value: a cell is the ink box of the font's `█` (ADR-0022), so CI's
  // fonts give different numbers from this machine's — #578's dpr-2 test pinned `cellW === 18` and
  // met 20. Both cells positive first, so a `0` from an unmeasured font cannot satisfy the `>`.
  expect(s.a.cellW).toBeGreaterThan(0);
  expect(s.b.cellW).toBeGreaterThan(0);
  expect(s.a.cellW).toBeGreaterThan(s.b.cellW);
  expect(s.a.cellH).toBeGreaterThan(s.b.cellH);

  // The consequence a single cell measurement cannot show: pane A's box is WIDER than pane B's and
  // it still fits fewer columns, because the two grids are laid out through different atlases.
  expect(s.a.cols).toBeLessThan(s.b.cols);

  // **What this test does not read: a pixel.** Everything above is what the API reports, and the
  // title said "in one frame" until the completeness pass measured that this was the *only* test of
  // the six still green with both grids collapsed onto the origin and painting over each other. That
  // the two cells are actually *drawn* differently is pinned one layer down, per grid, by a lit-run
  // measurement (`justerm-renderer/demo/per-config-atlas.html`); here it is bounded from the side by
  // the placement test's corner samples, which land inside each pane's own extent.
});

test("feeding one terminal leaves its sibling byte-identical (#773 per-grid state)", async ({
  page,
}) => {
  const { before, after } = await readAsyncProbe(page, "__independenceProbe");
  expectContextAlive(before);
  expectContextAlive(after);

  // The control, in the same run: the fed pane's rect actually moved. Without it "B is unchanged"
  // is satisfied by a page that drew nothing at all.
  expect(after.a.step).toBe(before.a.step + 1);
  expect(after.a.hash).not.toBe(before.a.hash);

  // THE CLAIM. `present()` redraws the WHOLE canvas — one call, every registered grid — so pane B
  // was re-rendered here; it comes back identical because it was re-rendered from its own retained
  // grid. A whole-rect digest rather than samples, so "unchanged" covers every pixel.
  expect(after.b.step).toBe(before.b.step);
  expect(after.b.hash).toBe(before.b.hash);
  // Not a degenerate match — and the first line of this is what the pass had to add. Comparing A's
  // hash to B's proves only that the two rects DIFFER, which an all-transparent B satisfies
  // trivially while A is painted: measured, with pane B's viewport never placed, every B-side
  // assertion in this test held and only the A-side control failed. What settles it is a pixel that
  // says B's rect holds B's grid.
  expect(before.b.centre).toBe(BG_B);
  expect(before.a.hash).not.toBe(before.b.hash);
});

test("one context loss and one restore bring back BOTH terminals, with no frame re-fed (#774)", async ({
  page,
}) => {
  const { before, raceWindow, afterRestoreNoPresent, after, framesPushed } = await readAsyncProbe(
    page,
    "__surfaceLossProbe",
  );
  expectContextAlive(before);

  // ASSERT THE WINDOW EXISTS BEFORE ASSERTING BEHAVIOUR INSIDE IT (#639). A browser destroys a
  // context synchronously and only *queues* `webglcontextlost`, so in this instant the driver says
  // lost and the widget's report — event-driven by design (ADR-0027 D4) — does not. If a browser
  // ever dispatches synchronously, this section stops describing a real loss and says so here
  // instead of passing vacuously.
  expect(raceWindow.glSaysLost, "the context did not actually go down").toBe(true);
  expect(raceWindow.widgetSaysLost, "the report is `was I told`, not `is the GPU usable`").toBe(false);

  // Both panes had real content before the loss — a lost context reads `0,0,0,0` everywhere, so
  // without this the equality below could be two reads of a dead buffer.
  expect(before.a.centre).toBe(BG_A);
  expect(before.b.centre).toBe(BG_B);

  // THE SURFACE'S OWN HALF, read inside the `webglcontextrestored` turn with nothing presented by
  // this suite. Without it the rest of this test is satisfied by the renderer alone: `snapshot()`
  // presents before it reads, and presenting is what runs the renderer's deferred rebuild — measured
  // here, deleting the surface's restore listener outright left all five tests green.
  expect(afterRestoreNoPresent.a).toBe(BG_A);
  expect(afterRestoreNoPresent.b).toBe(BG_B);
  expect(afterRestoreNoPresent.gutter).toBe(UNPAINTED);

  // **What this does NOT reach, recorded so the coverage is not overstated.** The surface's restore
  // handler is `present() -> reapplyAll() -> present()`, and only the presents are observable here:
  // cutting `reapplyAll` to a single lease leaves every assertion in this file green (measured). That
  // is not a hole in the handler — `reapplyAll` exists for a restore that adopts a density which
  // moved while the context was dead, where the cell changes under every grid, and this page holds
  // the density still. Reaching it needs a CDP density override, which is the other suite's #325
  // test; a two-terminal version of that is not in this slice's acceptance.

  // THE CLAIM: one loss, one restore, BOTH grids back — byte for byte, and with nothing re-fed. The
  // last part is what makes it a recovery rather than a repaint: a frame-mode consumer holds only
  // the current frame (ADR-0020 R3), so it has no retained state to be asked again for, and the
  // renderer has to repaint from the grids it kept.
  expect(framesPushed).toBe(0);
  expect(after.contextLost).toBe(false);
  expect(after.a.hash).toBe(before.a.hash);
  expect(after.b.hash).toBe(before.b.hash);
  expect(after.a.centre).toBe(BG_A);
  expect(after.b.centre).toBe(BG_B);
  // The gutter survives too: a restore that re-created the buffer without re-placing the viewports
  // would leave one grid's clear covering the whole canvas.
  expect(after.gutter).toBe(UNPAINTED);
  expect(after.band).toBe(UNPAINTED);
});

test("a density change re-places every pane, not just the buffer (ADR-0021 D3)", async ({ page }) => {
  const boot = await page.evaluate(() => window.__surfaceProbe!());
  expectContextAlive(boot);

  // Adopt twice the current ratio. Driven through the surface's own setter rather than through CDP:
  // Chromium's density override dispatches no `change` event, so the watcher cannot see it and only
  // the *adoption* half is reachable from a test — which is the half a shared surface gets wrong.
  const { before, after } = await page.evaluate(
    (d) => window.__densityProbe!(d),
    boot.dpr * 2,
  );

  // The buffer, which the HOST owes: nothing below this layer can size it, because a buffer holding
  // N grids in M font configurations has no cell to be a multiple of.
  expect(after.bufW).toBe(before.bufW * 2);
  expect(after.bufH).toBe(before.bufH * 2);

  // THE RECTS, which the host also owes and which nothing warns it about. `observeViewportRect` does
  // not fire here — a `ResizeObserver` on the default box reports CSS px and a density change moves
  // no CSS box — so a host that re-supplies only the buffer leaves pane B at half its offset, over
  // its left sibling, with no error. Measured red before the page's handler was fixed to re-place.
  expect(after.b.rectX).toBe(before.b.rectX * 2);
  expect(after.b.rectY).toBe(before.b.rectY * 2);
  // The control that keeps the assertion above from being "everything doubled": pane A is at the
  // origin, which no ratio moves, and it must stay there.
  expect(before.a.rectX).toBe(0);
  expect(after.a.rectX).toBe(0);
  expect(after.a.rectY).toBe(0);
  // Anti-vacuity: pane B's origin is non-zero, so doubling it is observable at all.
  expect(before.b.rectX).toBeGreaterThan(0);
  expect(before.b.rectY).toBeGreaterThan(0);

  // …and both panes are still painting after the move, rather than merely holding tidy numbers.
  expect(after.a.centre).toBe(BG_A);
  expect(after.b.centre).toBe(BG_B);
});

test("ending one terminal releases only its grid, and the survivor still recovers (#775)", async ({
  page,
}) => {
  const before = await page.evaluate(() => window.__surfaceProbe!());
  expectContextAlive(before);
  expect(before.a.centre).toBe(BG_A);
  expect(before.b.centre).toBe(BG_B);

  await page.getByTestId("end-a").click();

  const after = await page.evaluate(() => window.__surfaceProbe!());
  expect(after.a.ended).toBe(true);
  // Pane A's area goes back to showing the page: its grid left the registry, so the draw loop skips
  // it and nothing re-clears its rect after the full-canvas transparent clear.
  expect(after.a.centre).toBe(UNPAINTED);
  // …and the survivor is untouched, byte for byte. `dispose` releases the leaver's VAO, its instance
  // buffer and — only if it was the last grid on its font configuration — that configuration's
  // atlas; pane B is on its own configuration and keeps everything.
  expect(after.b.centre).toBe(BG_B);
  expect(after.b.hash).toBe(before.b.hash);

  // **The claim #775 exists for, and it is the one with no browser witness until now.** Its own
  // rationale for moving the density watcher and the loss channel onto the surface is that while
  // they sat on the terminal, *"disposing one terminal stopped density tracking for every
  // sibling"*. So the test is not that the survivor is alive — it is that the survivor still
  // RECOVERS, after the departure, through a channel the leaver used to own.
  const loss = await readAsyncProbe(page, "__surfaceLossProbe");
  expect(loss.raceWindow.glSaysLost, "the context did not actually go down").toBe(true);
  expect(loss.framesPushed).toBe(0);
  expect(loss.afterRestoreNoPresent.b).toBe(BG_B);
  expect(loss.after.b.hash).toBe(loss.before.b.hash);
  // The departed pane stays departed across the restore — a recovery that resurrected it would mean
  // the renderer had kept a grid the lease gave back.
  expect(loss.after.a.centre).toBe(UNPAINTED);
});

/**
 * The two-phase teardown (#775, #776).
 *
 * Two tests rather than one, because the claim is a **difference**: the surface's own answers are
 * identical in both orders — `gridCount` reaches `0` and `addGrid` throws either way — so a test of
 * the good order alone would pass just as happily against the bad one.
 *
 * They exist because this slice found the README describing the wrong one, and the honest reason to
 * write them is narrower than "coverage": a doc claim nothing can falsify is exactly what rotted
 * here twice already. The second test is therefore a **stale-doc detector** as much as a behaviour
 * test — if `surface.dispose()` ever does start ending widgets, it fails, and the README paragraph
 * it guards is the thing to change.
 */
test("tearing down terminals first leaves nothing behind (#775)", async ({ page }) => {
  const r = await page.evaluate(() => window.__teardownProbe!("terminals-first"));

  expect(r.textareasBefore, "one hidden IME textarea per mounted Terminal").toBe(2);
  // Every tenant handed its own grid back BEFORE the surface was asked, which is the state ghostty's
  // root asserts rather than assumes (`src/App.zig:115`) and the order the class doc keeps.
  expect(r.gridCountBeforeSurface).toBe(0);
  expect(r.surfaceDisposeThrew).toBeUndefined();
  // Nothing left mounted, and a frame arriving late is swallowed by a widget that has unsubscribed.
  expect(r.textareasAfter).toBe(0);
  expect(r.lateFrameThrew).toBeUndefined();
  expect(r.addGridThrew).toMatch(/disposed/);
});

test("disposing only the surface leaves every widget mounted, and the next frame throws (#776)", async ({
  page,
}) => {
  const r = await page.evaluate(() => window.__teardownProbe!("surface-only"));

  // The surface ends properly — this is the half that makes the trap invisible.
  expect(r.gridCountAfter).toBe(0);
  expect(r.surfaceDisposeThrew).toBeUndefined();
  expect(r.addGridThrew).toMatch(/disposed/);
  // …and it had to do it for the tenants, none of which had ended.
  expect(r.gridCountBeforeSurface).toBe(2);

  // THE TRAP, and the reason the README paragraph exists. The surface holds grids, not widgets: it
  // never saw the `Terminal` the host constructed, so every one stays mounted with its hidden
  // textarea in the DOM and still subscribed to the host's frame source — and the next frame the
  // host's backend pushes reaches a renderer whose grid is gone.
  expect(r.textareasAfter).toBe(r.textareasBefore);
  expect(r.textareasAfter).toBe(2);
  expect(r.lateFrameThrew).toMatch(/no grid with id/);
});
