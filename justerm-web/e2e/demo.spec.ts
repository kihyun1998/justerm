import { test as base, expect, type BrowserContext, type Page } from "@playwright/test";

import { DEMO_URL } from "../playwright.config";
import { readAsyncProbe as harvest } from "./probe";

/**
 * The #735 warm-up's two explicit budgets. They sum to 20s so the hook stays inside its 30s slot
 * with room for the three awaits that have no budget of their own — see the comment on the hook.
 */
const GOTO_BUDGET_MS = 8_000;
const BAR_BUDGET_MS = 12_000;

/**
 * End-to-end verification of the demo's a11y features in a REAL headless browser
 * — the automated form of the F-key/HITL smoke. We can't hear the WebAudio earcon
 * or run a screen reader, but we assert the exact things an SR consumes: the
 * aria-live region's text (#160 announce) and the signal path (via its console
 * log), plus the #161 gate that suppresses both. The real wasm decoder + the real
 * controllers run behind the demo's stub backend.
 */

const live = "[data-testid='command-live']";

/**
 * #733 — **one navigation per test, and it is `beforeEach`'s.** Every test body starts on a LIVE,
 * fully mounted demo; navigating again would leave the first document running underneath — its
 * `ResizeObserver`, its debounced `[fit] resize` (100ms), its timers — while the second loads, and a
 * `page.on("console")` listener does not reset across navigations, so a log belonging to the page a
 * test is leaving can answer a poll about the page it is entering (#653, measured again on
 * 2026-08-05). A test that needs a different *boot* — a query string, a viewport, a device scale —
 * asks for it through `test.use()`, which applies to that one navigation.
 *
 * The query-string half of that sentence is why `bootUrl` is an **option** rather than the hook
 * hard-coding `"/"`. A `baseURL` cannot carry it: `new URL("/", "http://host/?bgAlpha=0.6")` is
 * `http://host/`, so as long as the hook navigates to a literal `"/"` a query-string boot has no way
 * to *be* the single navigation, and the rule above would have had three standing exceptions
 * (`?bgAlpha=0.6`, `?bgAlpha=foo`, `?letterSpacing=…`) with nothing but a comment holding them.
 *
 * The consequence that outlives the second `goto` is the **boot window**: `beforeEach` returns as
 * soon as the control bar is visible, which is *before* the mount fit's 100ms debounce is guaranteed
 * to have fired, so a body-attached listener may or may not catch it — measured BOTH ways on this
 * machine, idle (no `[fit] resize` yet at body entry) and under load (already there). Which one you
 * get is machine speed, which is exactly what made #653 read as flaky for three CI runs while every
 * local run passed. Do not read either observation as a property.
 *
 * So a test that watches the boot takes `consoleLines` below instead of attaching its own listener.
 * It is an `auto` fixture on purpose, and that is the whole mechanism: a fixture the test merely
 * *declares* is set up **after** `beforeEach` (measured — `["auto", "beforeEach", "declared-by-test",
 * "test"]`), which would leave the listener attached exactly as late as one written by hand. Only
 * `{ auto: true }` runs in front of the hook.
 */
const test = base.extend<{ consoleLines: string[]; bootUrl: string }>({
  /** What `beforeEach` navigates to. Override per describe with `test.use({ bootUrl })`. */
  bootUrl: ["/", { option: true }],
  consoleLines: [
    async ({ page }, use) => {
      const lines: string[] = [];
      page.on("console", (m) => lines.push(m.text()));
      await use(lines);
    },
    { auto: true },
  ],
});

/** Every `[fit] resize CxR` the demo has logged so far, in order. */
const fitsIn = (consoleLines: string[]): string[] =>
  consoleLines.filter((l) => l.includes("[fit] resize"));

/**
 * The names of the demo probes that return a **promise**. Kept explicit rather than derived from
 * `Window`, because a structural `[K in keyof Window]` filter matches unrelated DOM methods too;
 * what keeps this list honest is `test/e2e-async-probe-shape.test.ts`, which reads *both* files and
 * fails if `demo/main.ts` declares an async probe this file then reads directly.
 */
type AsyncProbe =
  | "__aboveTopProbe"
  | "__bgAlphaProbe"
  | "__blinkIdleProbe"
  | "__composeCaretProbe"
  | "__contextLossProbe"
  | "__cursorBlinkProbe"
  | "__disposeProbe"
  | "__rulerAnchorProbe"
  | "__rulerLayerProbe"
  | "__searchRulerProbe"
  | "__textBlinkProbe";

/**
 * Read one of this page's promise-returning probes, through the park-and-harvest shape #731
 * established. **The mechanism, and the reasoning behind it, moved to `e2e/probe.ts` in #776** when
 * a second spec file needed it; this is the two-line alias that recovers this page's return types
 * from its own {@link AsyncProbe} union, which is where the `Window` declarations for these hooks
 * live.
 */
const readAsyncProbe = <K extends AsyncProbe>(
  page: Page,
  name: K,
): Promise<Awaited<ReturnType<NonNullable<Window[K]>>>> =>
  harvest<Awaited<ReturnType<NonNullable<Window[K]>>>>(page, name);

/**
 * #735 — **the cold boot is paid here, where the budget can absorb it.**
 *
 * `beforeEach` below waits for the control bar under playwright's default **5s `expect` timeout**,
 * and `retries: 0`. The first navigation of a browser process costs far more than every later one,
 * and the excess lands entirely on the *first test of the run*, since the browser process is shared
 * across contexts within one worker.
 *
 * **Measured at this gate**, on a 28-core host under heavy CPU contention, running only the first
 * test with this hook toggled off and on inside one session: **off** → 10084ms (passed) and 15277ms
 * (**failed**, `expect(locator).toBeVisible() … Timeout: 5000ms` — the exact shape of #653);
 * **on** → 2928ms and 3338ms, both passed. The issue's own sweep put the cold boot at 4024ms against
 * the 5000ms budget, 79% of it, with warm boots at ~490-909ms.
 *
 * **Where the cost lives, and where the measurement stops.** Holding the dev server warm in *both*
 * arms — so vite's on-demand transform cache is out of the comparison — and launching a fresh
 * chromium per arm, the in-process warm-up still won 8 of 10 paired reps, median **4865ms → 2191ms**.
 * So the expensive state is **per-browser-process**, which is the whole reason this hook takes the
 * `browser` fixture: warming a *different* browser process, or only the server, would not cover it.
 * What is **not** established is *which* process-local cache. V8's compile/code cache is the obvious
 * candidate, but the instrument's own wasm attribution did not separate the arms (median 167ms cold
 * vs 186ms warm), so it stays a candidate. The repair does not depend on the answer.
 *
 * The instrument, so the numbers are re-measurable: `addInitScript` wrapping
 * `WebAssembly.{instantiate,compile}{,Streaming}` before any page script, `getEntriesByType(
 * "resource")` for the resource wall, and this same locator for the bar. Two designs, and the second
 * is the one that isolates: (a) one arm per invocation against a fresh vite *and* a fresh chromium,
 * load applied after the server is up because `webServer`'s health check gates vite's dependency
 * optimization before test one; (b) one pre-warmed vite for the whole run, arms interleaved, a fresh
 * chromium each. Design (a) cannot tell the two caches apart — its warm-up warms both.
 *
 * **A separate context is enough, and that is the counter-intuitive part** — playwright contexts are
 * isolated and do not share an HTTP cache, so the obvious reasoning says this cannot work. It works
 * anyway, because what is shared is the process, not the context. It is *not* `retries: 1` (a retry
 * runs against an already-warm process, so it would always pass — disabling the detector rather than
 * fixing the boot), and it is not a bigger `expect` timeout (which would stop the gate reporting the
 * thing it exists to report).
 *
 * **Budget, and why the arithmetic below is the load-bearing part rather than the `catch`.** A
 * `beforeAll` hook gets a fresh slot worth the **test timeout** — 30s, playwright's default, which
 * this config does not override — against `beforeEach`'s 5s `expect` timeout. Six times the
 * headroom, at exactly the operation that needs it. But the slot is enforced *outside* the hook
 * body: `Promise.race([cb(), running.timeoutPromise])`
 * (`playwright/lib/worker/workerProcessEntry.js:425-428`, 1.61.1), so **the `try/catch` below cannot
 * intercept a slot timeout** — only a thrown failure. And a failed `beforeAll` is not one red test,
 * it skips the rest of the file (`:1795`, `_skipRemainingTestsInSuite`).
 *
 * So every await here carries an explicit budget and they must sum under 30s. They are not optional:
 * a context built by hand off `browser` inherits **none** of the config's defaults — not `baseURL`
 * (above), and equally not `navigationTimeout`, so an unbudgeted `page.goto` would take playwright's
 * own 30s and blow the slot by itself. `GOTO_BUDGET_MS + BAR_BUDGET_MS` = 20s leaves ~10s for
 * `newContext` / `newPage` / `close`, which are the three awaits with no budget of their own.
 *
 * **Fail-soft on purpose.** This hook asserts nothing: it is an optimization, and the only thing it
 * could prove is already proven by `beforeEach`, per test, with a better message. So a throw is
 * swallowed and logged, and the run simply pays the cold boot on test one — the behaviour that
 * existed before this hook. On a host so slow that the budgets above are not enough, that is the
 * right outcome: a warm-up which cannot land inside 20s was not going to rescue the first test
 * either.
 *
 * Cost when nothing is contended: one extra navigation, ~200ms. **It covers this spec file only.**
 * `beforeAll` runs once per file per worker, `browser` is worker-scoped, and `workers` is unset —
 * playwright defaults it to 50% of the logical cores (`playwright/lib/common/index.js:595`), and
 * `fullyParallel: false` serialises tests *within* a file while still spreading files across
 * workers. So a second spec file lands in its own worker with its own cold browser process and
 * needs its own copy of this hook; it does not inherit this one.
 */
test.beforeAll(async ({ browser }) => {
  let context: BrowserContext | undefined;
  try {
    context = await browser.newContext({ baseURL: DEMO_URL });
    const page = await context.newPage();
    await page.goto("/", { timeout: GOTO_BUDGET_MS });
    await page
      .getByRole("button", { name: /Finish command/ })
      .waitFor({ state: "visible", timeout: BAR_BUDGET_MS });
  } catch (e) {
    console.log(`[e2e] warm-up navigation did not complete (#735); test one pays the cold boot: ${e}`);
  } finally {
    await context?.close();
  }
});

test.beforeEach(async ({ page, bootUrl }) => {
  await page.goto(bootUrl);
  // The control bar mounts synchronously; wait for it to prove the app booted.
  //
  // It is a PROXY, and what makes it a sound one is worth stating because nothing enforces it: the
  // bar mounts at `demo/main.ts:859`, ~350 lines before the `window.__*Probe` assignments every
  // test below reaches for, and there is a top-level `await import("justerm-wasm-decode")` between
  // them. That await resolves on the MICROTASK queue — the module is already in flight from
  // `JustermRenderer.create` — and microtasks drain before Chromium can service a CDP `evaluate`,
  // so no test can observe the gap. Put one genuinely task-yielding `await` (a `fetch`, a
  // `setTimeout`, an `img.decode()`) after the bar mounts and every probe-reading test in this file
  // starts failing with `window.__xProbe is not a function`. Then this gate must move to something
  // the probes themselves emit.
  //
  // **The timeout is explicit, and larger than the default, because this gate asserts THAT the app
  // booted and not how fast.** Every other `expect` in this file keeps the 5s default; those are
  // claims about behaviour, and a slow one is worth seeing. This one is a proxy, so the only thing a
  // timeout here can report is machine speed — which is the failure this paragraph already warns
  // about two lines up, and which `retries: 0` turns into a red master.
  //
  // Measured on master `dc85158` (2026-08-25): this gate timed out once, in `web-e2e`, on a commit
  // that changed **one markdown file**. It took 5.9s against the 5s default while its three
  // parameterised-boot siblings in the same run finished in 1.3s, 1.4s and 2.1s *including* their
  // assertions — so the run was not broadly slow, one boot stalled. Locally the same URL boots in
  // 274ms median / 544ms worst of 8, a 9x margin, which is why this is not tuned closer.
  //
  // `retries` is deliberately NOT the fix. A retry hides a genuine flake as readily as an
  // environmental one, and this repo's discipline is that a green you never saw fail is not evidence.
  await expect(page.getByRole("button", { name: /Finish command/ })).toBeVisible({
    timeout: 30_000,
  });
});

test("control bar shows the action buttons", async ({ page }) => {
  await expect(page.getByRole("button", { name: /Accessible view/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Alt screen: (ON|OFF)/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Finish command/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Announce: (TERSE|VERBOSE)/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Screen reader: (ON|OFF)/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Prev command" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Next command" })).toBeVisible();
});

test("alt screen button toggles its label", async ({ page }) => {
  await page.getByRole("button", { name: "Alt screen: OFF" }).click();
  await expect(page.getByRole("button", { name: "Alt screen: ON" })).toBeVisible();
  await page.getByRole("button", { name: "Alt screen: ON" }).click();
  await expect(page.getByRole("button", { name: "Alt screen: OFF" })).toBeVisible();
});

// #189: an alt-scoped decoration (created on the alt screen) is DISPOSED on
// alt-leave — core fires MarkerDisposed on ?1049l (per-buffer clearAllMarkers), which
// the demo forwards to `decorations.onMarkerDisposed`. The green highlight is a
// renderer canvas paint (not DOM; this test asserts the handle's lifecycle rather than
// the pixel — headless CAN read the drawing buffer via the readPixels probes, as the
// #420 theme and #457 decoration tests do), but the disposal
// is observable via the Decorate toggle returning to OFF (the handle is gone, not
// merely off-screen) plus the demo's dispose log. A primary decoration, by contrast,
// survives an alt round-trip (only alt-scoped markers dispose) — locking "no
// cross-buffer teardown". This complements the live-screenshot proof so the DOM-
// observable half of the lifecycle is a regression gate, not a one-time eyeball.
test("alt-scoped decoration disposes on alt-leave; a primary decoration survives (#189)", async ({
  page,
}) => {
  const disposeLogs: string[] = [];
  page.on("console", (msg) => {
    if (msg.text().includes("alt-leave disposed the alt-scoped decoration")) {
      disposeLogs.push(msg.text());
    }
  });

  // Alt-scoped: decorate on the alt screen, then leave → the toggle flips back to OFF.
  await page.getByRole("button", { name: "Alt screen: OFF" }).click(); // enter alt
  await page.getByRole("button", { name: "Decorate line: OFF" }).click(); // decorate (alt-scoped)
  await expect(page.getByRole("button", { name: "Decorate line: ON" })).toBeVisible();

  await page.getByRole("button", { name: "Alt screen: ON" }).click(); // leave alt → dispose
  await expect(page.getByRole("button", { name: "Decorate line: OFF" })).toBeVisible();
  expect(disposeLogs).toHaveLength(1);

  // Primary: decorate on the primary screen, round-trip through alt → still ON, and no
  // further dispose (the alt-leave teardown is alt-scoped only — primary untouched).
  await page.getByRole("button", { name: "Decorate line: OFF" }).click(); // decorate (primary)
  await expect(page.getByRole("button", { name: "Decorate line: ON" })).toBeVisible();
  await page.getByRole("button", { name: "Alt screen: OFF" }).click(); // enter alt
  await page.getByRole("button", { name: "Alt screen: ON" }).click(); // leave alt
  await expect(page.getByRole("button", { name: "Decorate line: ON" })).toBeVisible();
  expect(disposeLogs).toHaveLength(1);
});

test("finish command announces success then failure to the live region", async ({ page }) => {
  const signals: string[] = [];
  page.on("console", (msg) => {
    const t = msg.text();
    if (t.includes("[demo] signal:")) signals.push(t);
  });

  // First finish → exit 0 → success announce + success signal.
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).toHaveText("Command succeeded");

  // Second finish → exit 1 → failure announce (with the code) + failure signal.
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).toHaveText("Command failed, exit 1");

  expect(signals.some((s) => s.includes("succeeded"))).toBe(true);
  expect(signals.some((s) => s.includes("failed"))).toBe(true);
});

test("terse announce drops the exit code on failure (#179)", async ({ page }) => {
  // Flip the announce text to terse (VSCode parity — the exit code is not spoken).
  await page.getByRole("button", { name: "Announce: VERBOSE" }).click();
  await expect(page.getByRole("button", { name: "Announce: TERSE" })).toBeVisible();

  // First finish → exit 0 → success text is identical in either mode.
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).toHaveText("Command succeeded");

  // Second finish → exit 1 → terse omits the code ("Command failed", NOT
  // "Command failed, exit 1"). Proves the injected preset flows through the real
  // controller + aria-live path end-to-end, not just the unit fake.
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).toHaveText("Command failed");
});

test("screen-reader-off suppresses the announce; back on resumes it (#161)", async ({ page }) => {
  // Turn SR off — the host telling justerm no screen reader is present.
  await page.getByRole("button", { name: "Screen reader: ON" }).click();
  await expect(page.getByRole("button", { name: "Screen reader: OFF" })).toBeVisible();

  // A finished command must NOT reach the live region while SR is inactive.
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).toHaveText("");

  // Turn SR back on — announces resume.
  await page.getByRole("button", { name: "Screen reader: OFF" }).click();
  await page.getByRole("button", { name: /Finish command/ }).click();
  await expect(page.locator(live)).not.toHaveText("");
});

test("command nav walks history: announces the command + fires its signal (#166)", async ({
  page,
}) => {
  const signals: string[] = [];
  page.on("console", (msg) => {
    const t = msg.text();
    if (t.includes("[demo] signal:")) signals.push(t);
  });

  // The 0-based index of the focused line within the accessible-view document —
  // the DOM side-effect of reveal() (announce/signal alone would NOT prove the
  // reading cursor moved). The demo's stub commands sit at document lines 0/2/4.
  const focusedLine = () =>
    page.evaluate(() => {
      const doc = document.querySelector("[role='document']");
      return doc ? Array.prototype.indexOf.call(doc.children, document.activeElement) : -1;
    });

  // Open the accessible view so nav loads the command list (cursor at the end).
  await page.getByRole("button", { name: /Accessible view/ }).click();
  await expect(page.locator("[role='document']")).toBeVisible();

  // Prev from the end → last preset command ("ls -la", exit 0): announced on the
  // polite region, a success signal, AND focus revealed on its document line (4).
  // This is the real CommandNavController + DomAccessibleView + wasm.
  await page.getByRole("button", { name: "Prev command" }).click();
  await expect(page.locator(live)).toHaveText("ls -la");
  expect(await focusedLine()).toBe(4); // reveal() moved focus to the command line

  // Prev again → the failing command ("false", exit 1): announce + fail signal +
  // focus revealed on line 2.
  await page.getByRole("button", { name: "Prev command" }).click();
  await expect(page.locator(live)).toHaveText("false");
  expect(await focusedLine()).toBe(2);

  // Next → forward to "ls -la" again (VSCode Next = line > cursor, nearest).
  await page.getByRole("button", { name: "Next command" }).click();
  await expect(page.locator(live)).toHaveText("ls -la");
  expect(await focusedLine()).toBe(4);

  expect(signals.some((s) => s.includes("succeeded"))).toBe(true);
  expect(signals.some((s) => s.includes("failed"))).toBe(true);

  // #743 — the list is only meaningful against the document it indexes. Close the
  // view and that document is gone, so a jump must do nothing at all. Discriminating
  // by construction: the reading cursor is on line 4, so the pre-#743 behaviour
  // announces "false" here (the next command up) while nothing moves on screen.
  // Anything other than the text already in the region means the nav navigated a
  // document that is not there.
  await page.keyboard.press("Escape");
  await expect(page.locator("[role='document']")).toBeHidden();
  await page.getByRole("button", { name: "Prev command" }).click();
  await expect(page.locator(live)).toHaveText("ls -la");
});

test("row-tree churn is skipped while SR inactive, re-syncs on reactivation (#169)", async ({
  page,
}) => {
  // The hidden review row-tree (role=list) mirrors the viewport. Its concatenated
  // row text is the DOM-state proxy for "did the tree churn this frame".
  const treeText = () =>
    page.evaluate(() => {
      const list = document.querySelector("[role='list']");
      return list ? Array.from(list.children, (c) => c.textContent).join("|") : null;
    });

  // SR ON (default): as output appends every 300ms the tree tracks the changing
  // viewport — so its text differs after a few frames.
  const before = await treeText();
  await page.waitForTimeout(900);
  expect(await treeText()).not.toBe(before);

  // Turn SR OFF → the per-frame setRow churn is skipped: the tree FREEZES even
  // though output keeps flowing (the win — no DOM work nobody reads).
  await page.getByRole("button", { name: "Screen reader: ON" }).click();
  const frozen = await treeText();
  await page.waitForTimeout(900); // several frames append while inactive
  expect(await treeText()).toBe(frozen); // no churn — unchanged

  // Turn SR ON → syncTree re-renders from the cached latest frame at once (no
  // cold rebuild, no waiting for the next frame): the tree is current again.
  await page.getByRole("button", { name: "Screen reader: OFF" }).click();
  expect(await treeText()).not.toBe(frozen);
});

test("accessible view opens as a document overlay and Escape closes it", async ({ page }) => {
  const doc = page.locator("[role='document']");
  await expect(doc).toBeHidden();

  await page.getByRole("button", { name: /Accessible view/ }).click();
  await expect(doc).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(doc).toBeHidden();
});

// #217: a native Select-All puts the selection anchor/focus OUTSIDE the hidden row tree
// (on document.body, an ancestor spanning it). The bridge must CLAMP those endpoints to
// the tree instead of no-oping — begin at row 0, col 0 and extend to a later row. Proven
// in a real browser: the demo's a11y `selectionPort` logs `[a11y-sel] begin/extend`, so a
// clamp that fired (vs a silent no-op) is observable via the console signal. This exercises
// the real DOM glue (`compareDocumentPosition` classification + range intersection), which
// the DOM-less unit tests can't.
test("Select-All clamps the out-of-tree selection to the row tree (#217)", async ({ page }) => {
  const selLog: string[] = [];
  page.on("console", (m) => {
    const t = m.text();
    if (t.includes("[a11y-sel]")) selLog.push(t);
  });

  // The row tree mirrors the viewport; wait for it to hold content rows.
  await expect(page.locator("[role='listitem']").first()).toBeAttached();

  // Native Select-All: select everything under <body>, spanning the whole tree.
  await page.evaluate(() => {
    const s = window.getSelection();
    if (!s) throw new Error("no selection");
    s.removeAllRanges();
    s.selectAllChildren(document.body);
  });

  // The clamp fired: begin at the tree start (row 0, col 0) and an extend to a later row —
  // NOT a no-op (pre-#217 this whole selection was dropped because both endpoints were
  // outside the tree).
  await expect.poll(() => selLog.join("\n")).toContain("[a11y-sel] begin 0,0");
  expect(selLog.some((l) => l.includes("[a11y-sel] extend"))).toBe(true);
});

// #217 (Lens-1 edge): an ASYMMETRIC selection — one endpoint resolved inside a row, the
// other a spanning ancestor (e.g. `documentElement`, how some ATs report a "select to
// end"). The out-of-tree end classifies as null (an ancestor contains the whole tree), so
// the rescue must fire on EITHER endpoint being null — not just both — else the whole
// selection is silently dropped. Proven live: the clamp must still emit a begin/extend.
test("asymmetric spanning selection (row → documentElement) still clamps (#217)", async ({
  page,
}) => {
  const selLog: string[] = [];
  page.on("console", (m) => {
    const t = m.text();
    if (t.includes("[a11y-sel]")) selLog.push(t);
  });

  await expect(page.locator("[role='listitem']").first()).toBeAttached();

  await page.evaluate(() => {
    const firstRow = document.querySelector("[role='list'] [role='listitem']");
    const textNode = firstRow?.firstChild;
    if (!textNode) throw new Error("no row text node");
    const r = document.createRange();
    r.setStart(textNode, 0); // anchor INSIDE row 0
    r.setEnd(document.documentElement, document.documentElement.childNodes.length); // focus on a spanning ancestor
    const s = window.getSelection();
    if (!s) throw new Error("no selection");
    s.removeAllRanges();
    s.addRange(r);
  });

  // Not dropped: the spanning-ancestor end clamped, so a real selection was driven.
  await expect.poll(() => selLog.some((l) => l.includes("[a11y-sel] begin"))).toBe(true);
  expect(selLog.some((l) => l.includes("[a11y-sel] extend"))).toBe(true);
});

// #133 (S16): the widget wires input + wheel + focus. Headless can't see the beamterm
// caret paint, but every routing DECISION has a DOM/console proxy the demo exposes: the
// input sink logs intents (`[input] …`), the local scroll logs `[wheel] scroll → …`, the
// scrollbar thumb `top` is the scroll DOM-state, and `document.activeElement` is the focus
// DOM-state. These lock the live-MCP proof as regression gates (the DECISIONS are also
// unit-tested; this is the real DOM glue the node suite can't run). A wheel is dispatched
// as a LINE-mode WheelEvent (one physical notch) for determinism.
test.describe("S16 input + wheel + focus wiring (#133)", () => {
  const wheelNotch = (page: import("@playwright/test").Page, deltaY: number) =>
    page.evaluate((dy) => {
      const c = document.querySelector("#term") as HTMLElement;
      const r = c.getBoundingClientRect();
      c.dispatchEvent(
        new WheelEvent("wheel", {
          deltaY: dy,
          deltaMode: 1, // LINE
          bubbles: true,
          cancelable: true,
          clientX: r.left + 50,
          clientY: r.top + 50,
        }),
      );
    }, deltaY);
  // The scrollbar thumb's `top` (%) is the scroll DOM-state; the track is a body-level
  // absolute div with a right edge and a thumb child.
  const thumbTop = (page: import("@playwright/test").Page) =>
    page.evaluate(() => {
      const track = [...document.querySelectorAll("div")].find(
        (d) =>
          d.style.position === "absolute" &&
          d.style.right === "0px" &&
          d.style.height === "100%" &&
          d.querySelector("div"),
      );
      const t = track?.querySelector("div") as HTMLElement | undefined;
      return t ? parseFloat(t.style.top) : null;
    });

  test("clicking the terminal focuses its hidden IME textarea (#116)", async ({ page }) => {
    // The real input target is a hidden textarea (a canvas can't receive composition
    // events); a pointer-down on the canvas focuses it via the container.
    expect(await page.evaluate(() => document.activeElement?.tagName)).not.toBe("TEXTAREA");
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    expect(await page.evaluate(() => document.activeElement?.tagName)).toBe("TEXTAREA");
  });

  test("keystrokes and paste reach the input sink", async ({ page }) => {
    const intents: string[] = [];
    page.on("console", (m) => {
      if (m.text().includes("[input]")) intents.push(m.text());
    });
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    await page.keyboard.press("a");
    expect(intents.some((l) => l.includes('[input] key {"type":"char","char":"a"}'))).toBe(true);
  });

  test("wheel scrolls scrollback (normal buffer): thumb moves up, offset climbs", async ({
    page,
  }) => {
    const scrolls: number[] = [];
    page.on("console", (m) => {
      const n = m.text().match(/\[wheel\] scroll → displayOffset (\d+)/);
      if (n) scrolls.push(Number(n[1]));
    });
    // Measure `before` while the view is still FOLLOWING the bottom, so `displayOffset` is 0 and
    // `thumbTopRatio = scrollbackLen / (scrollbackLen + rows)` sits at its maximum.
    //
    // Wheeling first, as this test used to, can pin the thumb at the very top. There `before` is 0,
    // an up-notch cannot lower it, and the demo's 300ms line append RAISES `after` (scrollbackLen
    // grows while displayOffset does not), so `after <= before` fails for a reason that has nothing
    // to do with the wheel. Only a loaded machine gets there, which is exactly what CI is (#341).
    //
    // A line appends every 300ms, so scrollback grows on its own; `thumbTop >= 50` means it has
    // reached the viewport height, and the 12 lines wheeled below cannot clamp against the top.
    await expect
      .poll(async () => (await thumbTop(page)) ?? 0, { timeout: 25_000, intervals: [400] })
      .toBeGreaterThanOrEqual(50);

    // DOM state: two up-notches lower the thumb `top` toward the track top (older content). The
    // wheel moves it by 12/total; a line appended in the same window moves it back by only 1/total,
    // so the drop dominates and this can be a STRICT comparison rather than the old `<=`.
    const before = (await thumbTop(page))!;
    await wheelNotch(page, -6);
    await wheelNotch(page, -6);
    const after = (await thumbTop(page))!;
    expect(after).toBeLessThan(before); // the thumb rose toward older content
    expect(scrolls.at(-1)!).toBeGreaterThan(0); // and the engine really scrolled into history
  });

  test("App mouse ON routes the wheel to the app, not scrollback", async ({ page }) => {
    const intents: string[] = [];
    const scrolls: string[] = [];
    page.on("console", (m) => {
      if (m.text().includes("[input] mouse")) intents.push(m.text());
      if (m.text().includes("[wheel] scroll")) scrolls.push(m.text());
    });
    await expect.poll(() => thumbTop(page), { timeout: 15_000 }).toBeLessThan(90);
    await page.getByRole("button", { name: "App mouse: OFF" }).click();
    const before = (await thumbTop(page))!;
    await wheelNotch(page, -3);
    expect(intents.some((l) => l.includes("wheelUp"))).toBe(true); // reported to the app
    expect(scrolls).toHaveLength(0); // did NOT scroll scrollback
    expect(await thumbTop(page)).toBe(before); // thumb unmoved
  });

  test("alt-screen wheel (no scrollback) becomes cursor keys, not a scroll", async ({ page }) => {
    const intents: string[] = [];
    const scrolls: string[] = [];
    page.on("console", (m) => {
      if (m.text().includes("[input] key")) intents.push(m.text());
      if (m.text().includes("[wheel] scroll")) scrolls.push(m.text());
    });
    await page.getByRole("button", { name: "Alt screen: OFF" }).click();
    await wheelNotch(page, -3); // up
    await wheelNotch(page, 3); // down
    expect(intents.some((l) => l.includes('{"type":"up"}'))).toBe(true);
    expect(intents.some((l) => l.includes('{"type":"down"}'))).toBe(true);
    expect(scrolls).toHaveLength(0); // no scrollback scroll on the alt screen
  });

  test("a shift-wheel produces no report and lets native scroll through", async ({ page }) => {
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    // Capture only wheel-derived intents/scrolls (a focus intent from the click above is
    // expected and unrelated); a shift-wheel must yield none of these.
    const signals: string[] = [];
    page.on("console", (m) => {
      const t = m.text();
      if (t.includes("[input] mouse") || t.includes("[input] key") || t.includes("[wheel]")) {
        signals.push(t);
      }
    });
    const prevented = await page.evaluate(() => {
      const c = document.querySelector("#term") as HTMLElement;
      const r = c.getBoundingClientRect();
      const ev = new WheelEvent("wheel", {
        deltaY: -4,
        deltaMode: 1,
        shiftKey: true,
        bubbles: true,
        cancelable: true,
        clientX: r.left + 50,
        clientY: r.top + 50,
      });
      return !c.dispatchEvent(ev); // true iff preventDefault was called
    });
    expect(prevented).toBe(false); // native scroll not suppressed (WheelScroller bailed)
    expect(signals).toHaveLength(0); // no spurious app report / scroll
  });

  test("Alt+wheel fast-scrolls 5× a normal notch (#246)", async ({ page }) => {
    const scrolls: number[] = [];
    page.on("console", (m) => {
      const n = m.text().match(/\[wheel\] scroll → displayOffset (\d+)/);
      if (n) scrolls.push(Number(n[1]));
    });
    const notch = (alt: boolean) =>
      page.evaluate((alt) => {
        const c = document.querySelector("#term") as HTMLElement;
        const r = c.getBoundingClientRect();
        c.dispatchEvent(
          new WheelEvent("wheel", {
            deltaY: -1, // one line up
            deltaMode: 1, // LINE — deterministic (no trackpad accumulation)
            altKey: alt,
            bubbles: true,
            cancelable: true,
            clientX: r.left + 50,
            clientY: r.top + 50,
          }),
        );
      }, alt);
    // Build ample scrollback (a line appends every 300ms) so the Alt +5 jump has room
    // and doesn't clamp at the history top; then measure from the bottom (following).
    await page.waitForTimeout(16_000);
    await notch(false);
    const a = scrolls.at(-1)!; // offset 1
    await notch(false);
    const b = scrolls.at(-1)!; // offset 2
    await notch(true);
    const c = scrolls.at(-1)!; // offset 7 (Alt = 5×)
    expect(b - a).toBe(1); // a normal notch = 1 line
    expect(c - b).toBe(5); // Alt = fastScrollSensitivity (5) lines
  });
});

// #116 (S7): IME composition through the hidden textarea. Headless can't run a real IME,
// but the demo dispatches the same composition/keydown events a Korean IME fires — the
// real CompositionController + Terminal wiring run, and the committed `text` intent is the
// DOM-observable proof (the demo logs `[input] text "…"`). The committed value comes from
// the textarea, never the (misleading) event data — the whole point of the mechanism.
test.describe("S7 IME composition (#116)", () => {
  // Focus the textarea (via a canvas click) and drive a composition that commits `committed`
  // while the last update `data` lies — returns the `[input] text` payloads that were logged.
  const compose = (page: import("@playwright/test").Page, data: string, committed: string) =>
    page.evaluate(
      ({ data, committed }) => {
        const ta = document.querySelector("textarea")!;
        (document.querySelector("#term") as HTMLElement).dispatchEvent(
          new MouseEvent("mousedown", { bubbles: true }),
        ); // focus the textarea
        ta.dispatchEvent(new CompositionEvent("compositionstart"));
        ta.dispatchEvent(new CompositionEvent("compositionupdate", { data }));
        ta.value = committed;
        ta.selectionStart = committed.length;
        ta.selectionEnd = committed.length;
        ta.dispatchEvent(new CompositionEvent("compositionend", { data }));
      },
      { data, committed },
    );

  test("commits the textarea value as a text intent, ignoring the event data", async ({
    page,
  }) => {
    const texts: string[] = [];
    page.on("console", (m) => {
      const captured = m.text().match(/\[input\] text "(.+)"/)?.[1];
      if (captured !== undefined) texts.push(captured);
    });
    // The last update data ("니") lies (jongseong migrated); the textarea holds "아니".
    await compose(page, "니", "아니");
    await expect.poll(() => texts).toContain("아니");
    expect(texts).not.toContain("니"); // never the event data
  });

  test("Enter finalizes an in-progress composition before sending the key", async ({ page }) => {
    const intents: string[] = [];
    page.on("console", (m) => {
      const t = m.text();
      if (t.includes("[input] text") || t.includes("[input] key")) intents.push(t);
    });
    await page.evaluate(() => {
      const ta = document.querySelector("textarea")!;
      (document.querySelector("#term") as HTMLElement).dispatchEvent(
        new MouseEvent("mousedown", { bubbles: true }),
      );
      ta.dispatchEvent(new CompositionEvent("compositionstart"));
      ta.dispatchEvent(new CompositionEvent("compositionupdate", { data: "가" }));
      ta.value = "가";
      ta.selectionStart = 1;
      ta.selectionEnd = 1;
    });
    // Not a settle (#710): what keeps this green is the round-trip between the two `evaluate`
    // calls, which gives `compositionUpdate`'s deferred write its turn. Enter's finalize is
    // SYNCHRONOUS and reads `substring(start, end)`, so that write must have moved `end` off 0 —
    // collapse this into ONE evaluate and the commit is empty. Green without the wait at 20x CPU
    // throttling, so it is an explicit margin for that ordering: do not grow it.
    await page.waitForTimeout(20);
    await page.evaluate(() => {
      document
        .querySelector("textarea")!
        .dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    });
    await expect.poll(() => intents.filter((t) => t.includes('text "가"'))).toHaveLength(1);
    // The commit precedes the Enter key in the intent stream (composition sent first).
    const commitIdx = intents.findIndex((t) => t.includes('text "가"'));
    const enterIdx = intents.findIndex((t) => t.includes('"type":"enter"'));
    expect(commitIdx).toBeGreaterThanOrEqual(0);
    expect(enterIdx).toBeGreaterThan(commitIdx);
  });

  test("the hidden textarea is cleared after a commit (no unbounded growth)", async ({ page }) => {
    await compose(page, "한", "한");
    await expect.poll(() => page.evaluate(() => document.querySelector("textarea")?.value)).toBe("");
  });

  test("the input textarea is a labeled accessible input, not aria-hidden (#248)", async ({
    page,
  }) => {
    // It's programmatically focused to type; focusing an aria-hidden element is a WCAG
    // 4.1.2 violation. It must instead be a named, visually-hidden input (xterm's helper
    // textarea) — the #119 row-tree stays the separate review/announce surface.
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    const ta = page.locator("textarea");
    await expect(ta).toBeFocused();
    await expect(ta).not.toHaveAttribute("aria-hidden", "true"); // not hidden while focused
    await expect(ta).toHaveAttribute("aria-label", /\S/); // has an accessible name
    await expect(ta).toHaveAttribute("aria-multiline", "false"); // a single-line prompt (xterm)
  });

  test("focus returns to the input textarea after the accessible view closes", async ({ page }) => {
    // The input target moved to the hidden textarea; focus-restore paths must target it,
    // not the (now inert) canvas — else typing/IME is dead after the overlay closes.
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    await page.getByRole("button", { name: /Accessible view/ }).click();
    await expect(page.locator("[role='document']")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator("[role='document']")).toBeHidden();
    expect(await page.evaluate(() => document.activeElement?.tagName)).toBe("TEXTAREA");
  });
});

// #117 (S13): consumer event surface. The demo pushes title/bell/cwd through the source's
// event channel (a real backend drains them from core); the widget routes each to the
// consumer handlers. onTitle drives the real document title (DOM-observable); onBell/onCwd
// are proven via their console signal (fire-and-forget, no DOM effect of their own).
test.describe("S13 consumer events (#117)", () => {
  test("Set title drives the document title (onTitle → xterm onTitleChange parity)", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Set title" }).click();
    await expect(page).toHaveTitle("justerm — tab 1");
    await page.getByRole("button", { name: "Set title" }).click();
    await expect(page).toHaveTitle("justerm — tab 2"); // a second event re-fires the handler
  });

  test("Bell and Set cwd fire their handlers", async ({ page }) => {
    const events: string[] = [];
    page.on("console", (m) => {
      const t = m.text();
      if (t.includes("[event]")) events.push(t);
    });
    await page.getByRole("button", { name: "Bell" }).click();
    await page.getByRole("button", { name: "Set cwd" }).click();
    await expect.poll(() => events.some((e) => e === "[event] bell")).toBe(true);
    await expect
      .poll(() => events.some((e) => e.startsWith("[event] cwd") && e.includes("file://")))
      .toBe(true);
  });
});

// #114: on container resize the demo auto-fits — computes cols/rows from the CSS box +
// cell size and drives a debounced resize intent (the demo logs `[fit] resize CxR`). Proven
// live: the ResizeObserver + FitController + proposeDimensions path runs in real Chromium,
// which the DOM-less unit tests can't exercise. Shrinking the viewport yields fewer cols.
//
// #733: the mount fit is a BOOT log, so it comes from `consoleLines` rather than a listener this
// body attaches. A hand-attached one is live only from here, and the file header's note named this
// test as the one riding that race — it would have failed as a poll timeout, never as a wrong number.
test("container resize drives a debounced fit intent with a smaller grid (#114)", async ({
  page,
  consoleLines,
}) => {
  const fits = (): string[] => fitsIn(consoleLines);
  const colsOf = (line: string): number => Number(line.match(/resize (\d+)x/)?.[1]);

  // The observer fires once on mount with the initial (large) viewport.
  await expect.poll(() => fits().length).toBeGreaterThan(0);
  const first = fits()[0];
  if (!first) throw new Error("unreachable: the poll above proved fits is non-empty");
  const firstCols = colsOf(first);

  // Shrink the viewport → smaller box → a new, smaller grid (debounced ~100ms).
  await page.setViewportSize({ width: 360, height: 300 });
  await expect.poll(() => fits().length).toBeGreaterThan(1);

  const last = fits().at(-1);
  if (!last) throw new Error("unreachable: the poll above proved fits has >1 entry");
  const lastCols = colsOf(last);
  expect(lastCols).toBeGreaterThanOrEqual(2); // MINIMUM_COLS
  expect(lastCols).toBeLessThan(firstCols); // the fit tracked the smaller box
});

// #252: the demo's fit() must pass CSS px to beamterm's resize() (which applies
// devicePixelRatio itself) — NOT pre-multiply by dpr. Pre-multiplying made the backing
// buffer css × dpr² (an over-large atlas). A HiDPI context (deviceScaleFactor 2) makes
// the two distinguishable: the correct backing is css × 2, the bug's was css × 4.
test.describe("HiDPI fit sizes the backing buffer to dpr, not dpr² (#252)", () => {
  test.use({ deviceScaleFactor: 2 });

  test("canvas backing = CSS box × devicePixelRatio", async ({ page }) => {
    const r = await page.evaluate(() => {
      const c = document.querySelector("#term") as HTMLCanvasElement;
      const box = c.getBoundingClientRect();
      return { widthRatio: c.width / box.width, heightRatio: c.height / box.height, dpr: window.devicePixelRatio };
    });
    expect(r.dpr).toBe(2);
    expect(Math.abs(r.widthRatio - 2)).toBeLessThan(0.05); // dpr (2), not dpr² (4)
    expect(Math.abs(r.heightRatio - 2)).toBeLessThan(0.05);
  });
});

test.describe("regex validation runs in core's dialect, not JS RegExp (#316 D2, wired by #346)", () => {
  // `SearchController` takes `isValidRegex` as an injected seam, and its own doc says the flag is
  // "always false when no validator is injected". The unit tests inject a fake. So this is the ONLY
  // place that can prove the demo actually got the REAL validator out of `justerm-wasm-decode` —
  // which it did not, while the published package lagged the repo (#346).
  //
  // Three patterns, each ruling out a different wrong implementation. Measured against the real core
  // (`regex` 1.12.4, pinned in Cargo.lock) and against node, rather than assumed:
  //
  //     pattern       core   JS      rules out
  //     (?=x)         false  valid   a JS `RegExp` stand-in (it calls lookaround valid)
  //     (?i)abc       true   throws  a JS stand-in from the other side (it calls inline flags invalid)
  //     (?<name>x)    true   valid   a stub that answers "invalid" to everything
  //
  // `regex` rejects lookaround by design — it is a finite automaton and guarantees linear time
  // (`regex-syntax` `parse_group` -> `UnsupportedLookAround`). It accepts inline flags, which JS has no
  // syntax for. Only a validator that is actually core's dialect answers all three correctly.
  // (`a(b` would prove nothing: both dialects reject it.)
  //
  // The `(?<name>x)` control needs `regex` >= 1.7.0, which first accepted the non-`?P` named-group
  // form. `justerm-core`'s manifest says `regex = "1"` (a floor); the guarantee lives in `Cargo.lock`,
  // which pins 1.12.4. A resolution below 1.7.0 would make that control demand `(?P<name>x)` instead.
  const openSearch = async (page: import("@playwright/test").Page, query: string) => {
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    await page.keyboard.press("Control+f");
    await page.locator("#search-regex").check();
    await page.locator('input[placeholder="search"]').fill(query);
  };

  test("a lookahead is rejected — only core's dialect says so", async ({ page }) => {
    await openSearch(page, "(?=x)");
    await expect(page.locator("#search-count")).toHaveText("invalid");
    // …and the box red-flags it, which is the user-visible half of the contract.
    await expect(page.locator('input[placeholder="search"]')).toHaveCSS(
      "border-color",
      "rgb(243, 139, 168)", // #f38ba8
    );
  });

  test("an inline flag group is accepted — only core's dialect says so", async ({ page }) => {
    // The mirror image of the test above. `new RegExp("(?i)abc")` throws in JS ("Invalid group"), so a
    // JS stand-in would red-flag this. core accepts it. Between the two tests, a JS validator fails
    // whichever way it answers.
    await openSearch(page, "(?i)abc");
    await expect(page.locator("#search-count")).not.toHaveText("invalid");
  });

  test("a named group is accepted — the flag is not simply always on", async ({ page }) => {
    // Valid in BOTH dialects, so this one discriminates nothing about the backend — it exists to reject
    // a stub that answers "invalid" to everything, which the two tests above would otherwise accept.
    await openSearch(page, "(?<name>x)");
    await expect(page.locator("#search-count")).not.toHaveText("invalid");
  });
});

// #417: the runtime font-size button drives the wired renderer setFontSize (#406) through the real
// published justerm-renderer wasm. A bigger font makes a bigger cell, so the SAME viewport fits
// strictly fewer columns — that grid shrink is the observable proof the size reached the atlas and
// re-baked (a no-op / unwired setter would leave the grid unchanged). The demo logs the new grid.
test("the font-size button re-bakes the atlas and re-fits to fewer columns (#417)", async ({
  page,
}) => {
  const grids: { size: number; cols: number; rows: number }[] = [];
  page.on("console", (m) => {
    const g = m.text().match(/font size (\d+)px → grid (\d+)x(\d+)/);
    if (g) grids.push({ size: Number(g[1]), cols: Number(g[2]), rows: Number(g[3]) });
  });
  await expect(page.getByRole("button", { name: "Font: 16px" })).toBeVisible();

  // 16 → 20 (bigger cell, fewer columns), then 20 → 16 (back).
  await page.getByRole("button", { name: "Font: 16px" }).click();
  await expect(page.getByRole("button", { name: "Font: 20px" })).toBeVisible();
  await page.getByRole("button", { name: "Font: 20px" }).click();
  await expect(page.getByRole("button", { name: "Font: 16px" })).toBeVisible();

  const at20 = grids.find((g) => g.size === 20);
  const at16 = grids.find((g) => g.size === 16);
  expect(at20, "a 20px grid was logged").toBeTruthy();
  expect(at16, "a 16px grid was logged").toBeTruthy();
  // The discriminating assertion: the re-bake actually enlarged the cell, so 20px fits fewer columns.
  expect(at20!.cols).toBeLessThan(at16!.cols);
});

// #429: search navigation drives the ACTIVE-match channel, decoupled from the selection. The
// renderer ranking (#427) and the wire group (#428) are proven in their own layers; this locks the
// WEB wiring end-to-end through the real published wasm. The canvas paint has no DOM proxy, so the
// demo's `__searchProbe` samples the drawing buffer (the #420 readPixels pattern; a composited
// screenshot is unreliable, #352) at the active match's first cell and at a plain match cell.
// Colours are the adapter DEFAULTS (nothing theme-injected), so this also proves the default flow:
// active 0x995200 → rgb(153,82,0), match 0x6e5c00 → rgb(110,92,0).
test.describe("active search match rides its own channel, not the selection (#429)", () => {
  const ACTIVE = "rgb(153,82,0)";
  const MATCH = "rgb(110,92,0)";
  const probe = (page: import("@playwright/test").Page) =>
    page.evaluate(() => window.__searchProbe!());

  test("navigation paints the active match distinctly; a user selection coexists and survives clear", async ({
    page,
  }) => {
    // Every demo row contains "select" ('s' has no ascender — the corner-inset
    // pixel the probe samples is guaranteed bg), so matches are plentiful.
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    await page.keyboard.press("Control+f");
    await page.locator('input[placeholder="search"]').fill("select");
    await expect(page.locator("#search-count")).toHaveText(/^1\/\d+/);

    // Landing on match 0 designated it active: its cell paints the ACTIVE
    // colour, a different match paints the plain match colour, and — the
    // decoupling itself — NO selection was created (a11y: the current match is
    // no longer surfaced to AT as a text selection).
    let p = await probe(page);
    expect(p.active).toBe(ACTIVE);
    expect(p.other).toBe(MATCH);
    expect(p.selectionSpans).toEqual([]);
    // The active span is the FIRST on-screen match triple (both from one
    // snapshot, so the demo's row drift can't fake this).
    expect(p.activeSpan).toEqual(p.matchSpans.slice(0, 3));

    // Enter → next match: the channel tracked navigation (now the SECOND triple).
    await page.keyboard.press("Enter");
    await expect(page.locator("#search-count")).toHaveText(/^2\/\d+/);
    p = await probe(page);
    expect(p.active).toBe(ACTIVE);
    expect(p.activeSpan).toEqual(p.matchSpans.slice(3, 6));

    // A manual drag-selection COEXISTS with the search overlays — pre-#429
    // showMatch owned the selection channel, so this was impossible — and the
    // active match keeps its own colour (ranked above the selection).
    const box = (await page.locator("#term").boundingBox())!;
    await page.mouse.move(box.x + 30, box.y + 40);
    await page.mouse.down();
    await page.mouse.move(box.x + 200, box.y + 40, { steps: 4 });
    await page.mouse.up();
    p = await probe(page);
    expect(p.selectionSpans.length).toBeGreaterThan(0);
    expect(p.active).toBe(ACTIVE);

    // Terminal blur (focus moves into the search box) flips the selection to
    // its inactive tint — the ACTIVE channel must survive the flip, not drop.
    await page.locator('input[placeholder="search"]').click();
    p = await probe(page);
    expect(p.active).toBe(ACTIVE);

    // A live theme swap (#420) re-pushes the active colour with the rest of the
    // overlay; the demo themes carry no activeMatchBg, so the default survives
    // the round-trip.
    await page.getByRole("button", { name: "Theme: dark" }).click();
    p = await probe(page);
    expect(p.active).toBe(ACTIVE);
    await page.getByRole("button", { name: "Theme: light" }).click();

    // Escape clears the SEARCH state only — the user's selection survives
    // (pre-#429 the search owned the selection and clear dropped it).
    await page.locator('input[placeholder="search"]').click();
    await page.keyboard.press("Escape");
    await expect(page.locator("#search-count")).toHaveText("0/0");
    p = await probe(page);
    expect(p.activeSpan).toEqual([]);
    expect(p.active).toBeNull();
    expect(p.selectionSpans.length).toBeGreaterThan(0);
  });

  // #441: as-you-type, the emphasis stays on the occurrence the user is on — it
  // does NOT re-land on match 0 and scroll there on every keystroke. Both
  // references that designate while typing anchor it (xterm expands the current
  // selection at its position, `SearchEngine.ts:108-116`; alacritty re-runs from
  // a stored origin, `event.rs:1523` via `:1566`), and the third designates
  // nothing at all while typing. The count label is the DOM proxy and it is discriminating
  // by construction: the old code set the index to 0 unconditionally, so a
  // `current` other than 1 is unreachable without the anchor.
  test("extending the query keeps the emphasis where the user was (#441)", async ({ page }) => {
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    await page.keyboard.press("Control+f");
    // "e" is frequent (several per demo row); "el" occurs once per row, inside
    // "select" — so the extension redistributes the set and the anchored
    // occurrence lands on a DIFFERENT ordinal than the old always-0.
    await page.locator('input[placeholder="search"]').fill("e");
    await expect(page.locator("#search-count")).toHaveText(/^1\/\d+/);
    for (let i = 0; i < 8; i++) await page.keyboard.press("Enter");
    await expect(page.locator("#search-count")).toHaveText(/^9\/\d+/);

    // One real keystroke, extending "e" → "el".
    await page.locator('input[placeholder="search"]').pressSequentially("l");

    await expect(page.locator("#search-count")).not.toHaveText(/^1\/\d+/);
    // …and it is a real match, not a stuck label: the active channel still paints.
    expect((await probe(page)).active).toBe(ACTIVE);
  });

  // #687: the same anchor, through the one path that used to end the session on
  // ordinary typing. In regex mode a group is typed one character at a time, so
  // the query is INVALID for exactly as long as it takes to type the closing
  // paren — and the #316 D2 path has to drop the engine paint meanwhile, or the
  // box says "invalid" over a screen still painting the previous query. Dropping
  // it through `clear()` took the anchor too, so the `)` re-landed on match 0:
  // #441's symptom, reached without ever leaving the search box.
  //
  // `e` → `e(` → `e()` is chosen so the match SET is identical before and after
  // (an empty group matches the empty string), leaving the ordinal as the only
  // thing that can move. Discriminating by construction: the pre-#687 code
  // reached this line at `1/N`.
  test("a regex typed through its invalid intermediate keeps the emphasis (#687)", async ({
    page,
  }) => {
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    await page.keyboard.press("Control+f");
    await page.locator("#search-regex").check();
    await page.locator('input[placeholder="search"]').fill("e");
    await expect(page.locator("#search-count")).toHaveText(/^1\/\d+/);
    for (let i = 0; i < 8; i++) await page.keyboard.press("Enter");
    await expect(page.locator("#search-count")).toHaveText(/^9\/\d+/);

    // Typing an open paren — the state every group passes through.
    await page.locator('input[placeholder="search"]').pressSequentially("(");
    await expect(page.locator("#search-count")).toHaveText("invalid");

    // …and closing it. Same matches, so the emphasis must still be the ninth.
    await page.locator('input[placeholder="search"]').pressSequentially(")");
    await expect(page.locator("#search-count")).toHaveText(/^9\/\d+/);
    expect((await probe(page)).active).toBe(ACTIVE);
  });

  // The other side of the split, in the browser: Escape really does end the
  // session, so a fresh query starts at its first match instead of resuming near
  // an abandoned search. Without this, "keep the anchor" could be satisfied by
  // never dropping it.
  test("Escape ends the session, so the next query starts at its first match (#687)", async ({
    page,
  }) => {
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    await page.keyboard.press("Control+f");
    await page.locator('input[placeholder="search"]').fill("e");
    for (let i = 0; i < 8; i++) await page.keyboard.press("Enter");
    await expect(page.locator("#search-count")).toHaveText(/^9\/\d+/);

    await page.keyboard.press("Escape");
    await page.keyboard.press("Control+f");
    await page.locator('input[placeholder="search"]').fill("e");

    await expect(page.locator("#search-count")).toHaveText(/^1\/\d+/);
  });
});

// #439: search navigation announces "x of y" — VS Code's SimpleFindWidget wording verbatim
// (`status()` polite channel: "{x} of {y} found for '{query}'", "No results found for
// '{query}'"), on a DEDICATED polite region (sharing #119's output or #160's command region
// would let a flush clobber it — the #160 precedent), gated by the SR-active state (#161):
// with SR off the count label still updates visually but nothing is announced. Post-#429 the
// current match is no longer a selection, so this region is the ONLY AT-perceivable side
// effect of search navigation.
test.describe("search navigation announces x of y (#439)", () => {
  const searchLive = "[data-testid='search-live']";
  const openSearch = async (page: import("@playwright/test").Page, query: string) => {
    await page.locator("#term").click({ position: { x: 50, y: 50 } });
    await page.keyboard.press("Control+f");
    await page.locator('input[placeholder="search"]').fill(query);
  };

  test("typing and navigating announce the count; no matches announces 'No results'", async ({
    page,
  }) => {
    await openSearch(page, "select");
    await expect(page.locator(searchLive)).toHaveText(/^1 of \d+ found for 'select'$/);

    await page.keyboard.press("Enter"); // next match — the announce tracks navigation
    await expect(page.locator(searchLive)).toHaveText(/^2 of \d+ found for 'select'$/);

    await page.locator('input[placeholder="search"]').fill("zzzqq");
    await expect(page.locator(searchLive)).toHaveText("No results found for 'zzzqq'");

    // Escape closes the box: the count resets with the query text still in the
    // input — the announce must NOT fire ("No results found for 'select'" here
    // would be the bug), so the region keeps its last spoken text.
    //
    // The ordinal here is **2**, not 1, and that is #441/#437, not drift: the
    // emphasis was on match 2 before "zzzqq", and a query that transiently
    // matches nothing no longer forgets where the user was (alacritty keeps its
    // origin across exactly this; xterm loses it with its selection). This
    // assertion pinned the old jump-to-the-first-match behaviour as expected —
    // it is adjudicated here rather than flipped: the subject of the test is the
    // SILENCE after Escape below, which is unchanged.
    await page.locator('input[placeholder="search"]').fill("select");
    await expect(page.locator(searchLive)).toHaveText(/^2 of \d+ found for 'select'$/);
    await page.keyboard.press("Escape");
    await expect(page.locator("#search-count")).toHaveText("0/0"); // the reset happened…
    await expect(page.locator(searchLive)).toHaveText(/^2 of \d+ found for 'select'$/); // …silently
  });

  test("with the screen reader off, search stays silent while the label still updates (#161)", async ({
    page,
  }) => {
    await page.getByRole("button", { name: "Screen reader: ON" }).click(); // SR off
    await openSearch(page, "select");
    await expect(page.locator("#search-count")).toHaveText(/^1\//); // visual count unaffected
    expect(await page.locator(searchLive).textContent()).toBe(""); // …but no announce
  });
});

// #420: the runtime theme button drives the wired setTheme (renderer setPalette #405) through the
// real published justerm-renderer wasm. Two schemes with opposite defaults, so the palette swap
// recolours the canvas — the demo samples the drawing buffer's centre after each swap (readPixels
// there is reliable, unlike a composited screenshot) and logs the colour; the two must differ.
test("the theme button swaps the palette and recolours the canvas (#420)", async ({ page }) => {
  const samples: { theme: string; r: number; g: number; b: number }[] = [];
  page.on("console", (m) => {
    const s = m.text().match(/theme=(\w+) centre=rgb\((\d+),(\d+),(\d+)\)/);
    if (s) samples.push({ theme: s[1]!, r: Number(s[2]), g: Number(s[3]), b: Number(s[4]) });
  });
  await expect(page.getByRole("button", { name: "Theme: dark" })).toBeVisible();

  await page.getByRole("button", { name: "Theme: dark" }).click(); // → light
  await expect(page.getByRole("button", { name: "Theme: light" })).toBeVisible();
  await page.getByRole("button", { name: "Theme: light" }).click(); // → dark
  await expect(page.getByRole("button", { name: "Theme: dark" })).toBeVisible();

  const light = samples.find((s) => s.theme === "light");
  const dark = samples.find((s) => s.theme === "dark");
  expect(light, "a light-theme sample was logged").toBeTruthy();
  expect(dark, "a dark-theme sample was logged").toBeTruthy();
  // Discriminating: the centre pixel differs between the opposite-default schemes — the palette
  // actually swapped in the renderer (an unwired setTheme would leave the canvas unchanged).
  const differs = light!.r !== dark!.r || light!.g !== dark!.g || light!.b !== dark!.b;
  expect(differs, `light ${JSON.stringify(light)} vs dark ${JSON.stringify(dark)}`).toBe(true);
});

// #457: a right-anchored decoration wider than the viewport overflows the LEFT edge.
// The projection unit test can only see the emitted span — it cannot see whether
// anything reached the screen. This drives the REAL wasm renderer and reads the real
// drawing buffer: before the viewport clip the negative `left` wrapped in the u32 wire
// and the renderer matched no column, so the decoration was invisible everywhere.
test("a right-anchored decoration overflowing the left edge still paints (#457)", async ({
  page,
}) => {
  await expect(page.getByRole("button", { name: "Decorate line: OFF" })).toBeVisible();

  const p = await page.evaluate(() => window.__decorationProbe!());

  // Discriminating: the decorated samples must DIFFER from the undecorated baseline.
  // With the pre-#457 wrap both pairs were identical (nothing painted at all).
  expect(p.overflowLeft, `left: baseline ${p.baselineLeft} vs decorated`).not.toBe(p.baselineLeft);
  expect(p.overflowRight, `right: baseline ${p.baselineRight} vs decorated`).not.toBe(
    p.baselineRight,
  );
  // …and both ends carry the SAME decoration colour: the span really spans the row,
  // rather than a partial paint that happens to differ at one end.
  expect(p.overflowLeft).toBe(p.overflowRight);
});

// #461: a multi-row decoration whose marker scrolled ABOVE the viewport top must paint the
// rows of it that are still visible. Core drops an above-top marker from `markerPositions`,
// so before the fix the whole decoration vanished — a projection test cannot see that, since
// it cannot tell "emitted no rect" from "emitted a rect nothing drew". This drives the real
// wasm renderer: the marker sits 2 rows above the top with height 5, so viewport rows 0-2 are
// its visible tail and row 3 is past it.
test("a decoration anchored above the viewport top still paints its visible rows (#461)", async ({
  page,
}) => {
  // This was the FIRST site to drop its second `goto` (#461), before the file-wide rule in the
  // header existed. What made it bite here rather than anywhere else is worth keeping: the probe
  // is async (#490), so the awaited `evaluate` spans the reload and its context is destroyed.
  await expect(page.getByRole("button", { name: "Decorate line: OFF" })).toBeVisible();

  const p = await readAsyncProbe(page, "__aboveTopProbe");

  expect(p.rows[0], `row 0: baseline ${p.baseline}`).not.toBe(p.baseline);
  expect(p.rows[1]).not.toBe(p.baseline);
  expect(p.rows[2]).not.toBe(p.baseline);
  // All three carry the same decoration colour — a contiguous visible tail, not a stray row.
  expect(p.rows[0]).toBe(p.rows[1]);
  expect(p.rows[1]).toBe(p.rows[2]);
  // …and the span really ends: row 3 is past `height`, so it is undecorated.
  expect(p.rows[3], "row 3 is past the 5-row span").toBe(p.baseline);
});

// #480: a decoration anchors to a BUFFER line, not a viewport row. So scrolling moves its viewport
// row (and the cell highlight with it) but must NOT move its overview-ruler mark, which stays at the
// buffer position. Before the fix the demo derived the absolute line FROM the viewport row, so the
// derived line — and the ruler mark — slid as you scrolled. Driven for real: the probe forces
// scrollback, decorates, and reads the frame the demo emits at two scroll offsets.
test("a decoration's ruler mark stays anchored to its buffer line across scroll (#480)", async ({
  page,
}) => {
  // The second site to drop its second `goto` (#480, in #730) — and the one that proves the
  // header's rule is not the whole story. The two-pages hazard is *not* what produced the CI
  // failure this test had on 2026-08-05 (run 30979831545): that reproduced with a single
  // navigation and with zero `Page`/`Runtime` events on the wire (measured), so nothing navigated.
  // Playwright's "Execution context was destroyed" is a catch-all rewrite — the protocol error
  // underneath was `Promise was collected`. A single navigation does not make an awaited probe
  // safe, which is why this read goes through `readAsyncProbe` and no longer awaits one (#731).
  await expect(page.getByRole("button", { name: "Decorate line: OFF" })).toBeVisible();

  const p = await readAsyncProbe(page, "__rulerAnchorProbe");

  // **First, that the probe measured anything at all.** `lineOf` answers `undefined` until the
  // pull it triggered has landed, and the probe reports that as `NaN` — against which the
  // invariant below is vacuously true, because `expect(NaN).toBe(NaN)` passes (`Object.is`).
  // This test spent its whole life green on exactly that, so the non-vacuity is asserted first.
  expect(Number.isFinite(p.line0), `the index must answer a line, got ${p.line0}`).toBe(true);
  // The absolute buffer line (→ the ruler mark) is invariant under scroll — the #480 fix. Before
  // it, `markerLines` was `viewTop + DECO_ROW`, so this differed by `scrolledBy`.
  expect(p.lineScrolled, `absolute line must not move with scroll (was ${p.line0})`).toBe(p.line0);
  // …while the DERIVED viewport row tracks the scroll by exactly the scrolled distance, so the cell
  // highlight follows its content (a real buffer anchor, not a fixed viewport row).
  expect(p.rowScrolled - p.row0, "the viewport row tracks scroll").toBe(p.scrolledBy);
});

// #458: two decorations on DIFFERENT markers covering the same cell — the LAST REGISTERED wins,
// not the one whose marker core emits later. The projection unit test can only see the emitted
// rect order; this drives the real wasm renderer and reads the real drawing buffer, so it proves
// the whole chain — registration order → wire order → the renderer's per-property last-wins (#452).
test("cross-marker decoration precedence follows registration order at the pixel (#458)", async ({
  page,
}) => {
  await expect(page.getByRole("button", { name: "Decorate line: OFF" })).toBeVisible();

  const p = await page.evaluate(() => window.__precedenceProbe!());

  // Both single-decoration scenarios must paint, and paint DIFFERENTLY from each other — without
  // that, the two-decoration assertions below could pass on a cell nothing ever decorated.
  expect(p.firstMarkerOnly, `baseline ${p.baseline}`).not.toBe(p.baseline);
  expect(p.secondMarkerOnly).not.toBe(p.baseline);
  expect(p.firstMarkerOnly).not.toBe(p.secondMarkerOnly);

  // The discriminating pair: the SAME two decorations, swapped only in registration order, must
  // swap which colour reaches the pixel. Core's marker order is identical in both runs, so it
  // cannot be what decides. Pre-#458 both runs returned `secondMarkerOnly`.
  expect(p.bothFirstMarkerRegisteredLast).toBe(p.firstMarkerOnly);
  expect(p.bothSecondMarkerRegisteredLast).toBe(p.secondMarkerOnly);
});

// #498: a `full`-width overview-ruler mark paints ABOVE the gutter ones, whatever the registration
// order (xterm's rule — it renders every non-`full` zone, then every `full` one). The unit test can
// only see the emitted array; this reads the DOM the scrollbar actually built, which is the link
// between "emitted last" and "painted last" — and vitest's `node` environment cannot host it.
test("a full-width ruler mark is layered above a gutter mark in the DOM (#498)", async ({ page }) => {
  await expect(page.getByRole("button", { name: "Decorate line: OFF" })).toBeVisible();

  const p = await readAsyncProbe(page, "__rulerLayerProbe");

  // Both marks reached the scrollbar…
  expect(p.marks).toHaveLength(2);
  const [first, second] = p.marks;
  // …and the gutter one is first in DOM order, so the full-width one (registered FIRST) paints over
  // it. Registration order alone would have produced the opposite.
  expect(first!.background).toBe("rgb(0, 170, 0)"); // gutter, position: "left"
  expect(second!.background).toBe("rgb(170, 0, 0)"); // full — last = on top

  // Order only means "paints above" if the two actually overlap. Assert it geometrically rather
  // than trusting `rulerMarkX`: the full-width mark's box must contain the gutter mark's band, so a
  // regression to a zero-width or offset box cannot leave this test vacuously green.
  expect(second!.left).toBeLessThanOrEqual(first!.left);
  expect(second!.right).toBeGreaterThanOrEqual(first!.right);
  expect(second!.right - second!.left).toBeGreaterThan(first!.right - first!.left);

  // This pair used to read `second.top === first.top` / `second.bottom === first.bottom`, which
  // #500 §2 breaks by construction — a gutter mark is now taller than a full one. The equality was
  // never the claim: the comment above it says "Order only means 'paints above' if the two actually
  // overlap". Vertical OVERLAP is the claim, and with both marks centred on the same line the
  // thinner full mark's band is strictly inside the fat gutter one's — a stronger statement than
  // the equality was, and one a zero-height or displaced box still cannot satisfy vacuously.
  expect(second!.top).toBeGreaterThan(first!.top);
  expect(second!.bottom).toBeLessThan(first!.bottom);
});

// #500 §3: a mark is CENTRED on its line, not hung below it. This is the whole behavioural change
// and it has no unit-level home — `rulerMarkHeightPx` is unit-tested, but "where the box ends up in
// the track" is CSS the browser resolves (`top: X%` + `translateY(-50%)`), and vitest's `node`
// environment has no layout.
//
// It is also the discriminating shape rather than an incidental one: the two marks have DIFFERENT
// heights, so top-alignment and centring cannot both be true. Top-aligned, their tops coincide and
// their centres differ; centred, their centres coincide and their tops differ. The previous
// assertion pins the second half; this pins the first, against the ratio the projection was given
// rather than against the elements themselves.
// #440: the overview ruler now has TWO mark sources, and the rule that orders them across the join
// (`composeRulerMarks`) is exactly the kind no unit test can observe — array order becomes paint
// order only in a browser, and vitest's `node` environment has no layout. The sibling probe above
// proves the decoration source; this one runs a real query through the controller and the port, so
// it proves the source that did not exist before and the join that now carries both.
//
// The discriminating shape: the decorations are registered BEFORE the search hands over, and the
// `full` one is registered FIRST. Appending source-by-source would put the full mark at the front;
// appending in registration order would put it in the middle. Only re-partitioning by class puts it
// last, which is what ADR-0024 R3 requires and what the assertion pins.
test("a search mark joins the ruler above the gutter decorations and below the full one (#440)", async ({
  page,
}) => {
  await expect(page.getByRole("button", { name: "Decorate line: OFF" })).toBeVisible();

  const p = await readAsyncProbe(page, "__searchRulerProbe");

  // The query has to have matched, or every ordering claim below is vacuous — the same trap the
  // probe pads scrollback to avoid for the track's box.
  expect(p.searchMarkCount).toBeGreaterThan(0);

  const decoCentre = p.backgrounds.indexOf(p.decorationCenter);
  const decoFull = p.backgrounds.indexOf(p.decorationFull);
  const firstSearch = p.backgrounds.findIndex(
    (bg) => bg !== p.decorationCenter && bg !== p.decorationFull,
  );
  expect(decoCentre).toBeGreaterThanOrEqual(0);
  expect(decoFull).toBeGreaterThanOrEqual(0);

  // Within the gutter class: decoration marks, then search marks (the search set is the most recent
  // statement the consumer made — R3's second key, which has no meaning for a match).
  expect(decoCentre).toBeLessThan(firstSearch);
  // Across classes: the `full` mark paints above every gutter mark, from either source.
  expect(decoFull).toBeGreaterThan(firstSearch);
  expect(decoFull).toBe(p.backgrounds.length - 1);

  // #440: `setMarks` reuses its elements (re-creating them cost ~18 us per mark — 18 ms for 1000,
  // a whole 60 Hz frame for one call). Reuse makes two states reachable that re-creation could not:
  // a stale horizontal property from the previous mark's position class, and an untrimmed tail.
  expect(p.reusedAsLeft).toEqual({ left: "0px", right: "", width: "33%" });
  expect(p.afterEmpty).toBe(0);
});

test("a ruler mark is centred on its line and the track clips the overhang (#500 §3)", async ({ page }) => {
  await expect(page.getByRole("button", { name: "Decorate line: OFF" })).toBeVisible();

  const p = await readAsyncProbe(page, "__rulerLayerProbe");
  expect(p.marks).toHaveLength(2);
  const [gutter, full] = p.marks;

  // The track must have a real box, or every claim below is vacuous — the same trap the probe pads
  // scrollback to avoid.
  expect(p.track.height).toBeGreaterThan(0);

  const expected = p.track.top + p.ratio * p.track.height;
  const centre = (m: { top: number; bottom: number }): number => (m.top + m.bottom) / 2;
  // Sub-pixel layout, so compare to 0.1px rather than exactly.
  expect(centre(gutter!)).toBeCloseTo(expected, 1);
  expect(centre(full!)).toBeCloseTo(expected, 1);
  // …and the tops do NOT coincide, which is what fails if the offset is ever dropped.
  expect(full!.top).not.toBeCloseTo(gutter!.top, 1);

  // The containment half. A rect is reported whether or not an ancestor clips it, so this is a
  // CSS-level assertion by necessity and is stated as one: without it the first line's mark paints
  // over the terminal canvas above the track, and the last line's below it. Centring bounds that
  // escape to half a mark; the clip removes it.
  expect(p.track.overflow).toBe("hidden");
});

// #575: the widget used to blink the cursor unconditionally and never read the frame's blink mode,
// so an application asking for a STEADY cursor got a blinking one. The resolution itself is
// unit-tested; this is the only place the *wiring* can be proven — `JustermRenderer`'s constructor
// is private and needs a GL context, so vitest cannot reach `updateCursor` at all. It also had
// nowhere to run until this slice: the demo emitted no cursor fields, so no cursor was ever drawn.
test("a steady cursor stays put and a blinking one leaves the cell (#575)", async ({ page }) => {
  await expect(page.getByRole("button", { name: "Cursor blink: OFF" })).toBeVisible();

  const p = await readAsyncProbe(page, "__cursorBlinkProbe");

  // The cursor must actually paint, or every equality below is vacuously true on an empty cell.
  expect(p.steadyA, `background ${p.background}`).not.toBe(p.background);

  // THE FIX: the application asked for steady, so the cell must look identical a full blink
  // interval later. Pre-#575 the widget blinked regardless and this came back as the background.
  expect(p.steadyB).toBe(p.steadyA);

  // …and the assertion above only means something if blinking is observable at this pixel at all:
  // with the application asking to blink, the cursor leaves the cell and the background returns.
  expect(p.blinkOn).toBe(p.steadyA);
  expect(p.blinkOff).toBe(p.background);

  // The consumer override outranks the application (alacritty's `blinking_override().unwrap_or`):
  // the app is still asking to blink here, and the cursor stays.
  expect(p.forcedSteady).toBe(p.steadyA);
});

// #593: with no user input the cursor stops blinking and parks solid — both references do this
// (alacritty 5s, xterm.js 5min) and justerm-web blinked forever. The resolution is unit-tested; this
// proves the wiring, which vitest cannot reach (`JustermRenderer`'s constructor is private and needs
// a GL context). The probe drives the real consumer knob down to a 2s window rather than waiting out
// the five-minute default.
test("the cursor stops blinking after an idle period, and input revives it (#593)", async ({
  page,
}) => {
  await expect(page.getByRole("button", { name: "Cursor blink: OFF" })).toBeVisible();

  const p = await readAsyncProbe(page, "__blinkIdleProbe");

  // The cursor paints and genuinely blinks inside the idle window — without this the idle
  // assertions below could pass on a cell that never changes.
  expect(p.beforeOn, `background ${p.background}`).not.toBe(p.background);
  expect(p.beforeOff).toBe(p.background);

  // THE FIX: past the timeout with no input, the cursor is solid — and stays solid a full blink
  // interval later, so this is a stopped blink rather than a lucky sample on the ON phase.
  expect(p.idleA).toBe(p.beforeOn);
  expect(p.idleB).toBe(p.beforeOn);

  // …and it is not a one-way door: user input restarts the idle clock and the blink resumes.
  expect(p.afterInputOff).toBe(p.background);
});

// #592: the caret stops blinking while an IME composition is open. Two of the three references do
// this (alacritty `event.rs:1633`, ghostty `renderer/cursor.zig:47`); xterm.js has no rule. Measured
// before building: with the application silent — the default since #575 — the caret is already solid
// during composition, so this gate bites only where an application asked to blink, which is exactly
// the state this test sets up. The composition events go to the real hidden textarea, so the real
// CompositionController and Terminal wiring run; vitest cannot reach either.
test("the caret stops blinking while an IME composition is open (#592)", async ({ page }) => {
  await expect(page.getByRole("button", { name: "Cursor blink: OFF" })).toBeVisible();
  await page.locator("#term").dispatchEvent("mousedown"); // focus the hidden textarea

  const p = await readAsyncProbe(page, "__composeCaretProbe");

  // Control: with the application asking to blink and no composition, the caret really does blink at
  // this pixel — without it the composing assertions could pass on a cell that never changes.
  expect(p.idleOn, `background ${p.background}`).not.toBe(p.background);
  expect(p.idleOff).toBe(p.background);

  // THE FIX: mid-composition the caret is solid, and stays solid a full blink interval later.
  expect(p.composingA).toBe(p.idleOn);
  expect(p.composingB).toBe(p.idleOn);

  // …and it is not a one-way door: the composition ends and the blink resumes.
  expect(p.afterEndOff).toBe(p.background);
});

// #576: SGR 5 (blink) text was implemented on both sides of the widget and died in the middle —
// core carries the cell flag, the renderer conceals a blinking cell on the off phase, and the
// widget never flipped the phase, so `ESC[5m` text rendered identically to plain text. The phase
// arithmetic is unit-tested (`TextBlink`); this is the only place the wiring can be proven, since
// `JustermRenderer`'s constructor is private and needs a GL context. It also had nowhere to run
// until this slice: the demo emitted no blinking cell.
test("SGR 5 text blinks only when the consumer asks, and never by default (#576)", async ({
  page,
}) => {
  await expect(page.getByRole("button", { name: "SGR 5 text: OFF" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Text blink: OFF" })).toBeVisible();

  const p = await readAsyncProbe(page, "__textBlinkProbe");

  // The cell must actually paint, or every equality below is vacuously true on an empty cell.
  expect(p.defaultA, `background ${p.background}`).not.toBe(p.background);

  // THE DEFAULT: the application asked for blinking text and the consumer did not opt in, so the
  // text stays drawn across more than a full interval. This is the reference position — xterm.js
  // ships `blinkIntervalDuration: 0`, alacritty has no text blink, ghostty never draws the flag.
  expect(p.defaultB).toBe(p.defaultA);

  // THE FIX: once the consumer sets an interval, the same cells alternate between drawn and
  // background-only. Sampled a half-interval apart, so both phases appear.
  expect(new Set(p.phases)).toEqual(new Set([p.defaultA, p.background]));

  // …and that alternation is not an artefact of the probe re-emitting a frame each time: these
  // samples were read in the blink loop's own rAF turns, with no frame behind them (turns where
  // the loop did not present are dropped by the probe). An idle terminal depends on this path
  // alone — nothing re-emits a frame when there is no output.
  expect(p.loopSamples.length).toBeGreaterThanOrEqual(3); // one present per interval, 5 sampled
  expect(new Set(p.loopSamples)).toEqual(new Set([p.defaultA, p.background]));

  // Turning the blink off must leave the text SHOWN, not stuck in the phase it happened to be in —
  // the `beforeDisable` sample proves it really was off when the interval was cleared.
  expect(p.beforeDisable).toBe(p.background);
  expect(p.afterDisable).toBe(p.defaultA);
});

// #576 + #119: `prefers-reduced-motion` outranks both the application's SGR 5 and the consumer's
// interval. Derived rather than ported (no reference has the input) and settled on #575/#583:
// reduced motion can only ever subtract motion, so letting it win can never make steady text blink.
// Driven through Playwright's real media emulation, so the widget's own matchMedia listener runs.
test("prefers-reduced-motion pins blinking text visible (#576)", async ({ page }) => {
  await expect(page.getByRole("button", { name: "SGR 5 text: OFF" })).toBeVisible();

  await page.emulateMedia({ reducedMotion: "reduce" });
  const p = await readAsyncProbe(page, "__textBlinkProbe");

  // Every sample — including the ones taken with an interval set, and any the loop presented — is
  // the drawn cell. Nothing ever reaches the concealed phase. (`loopSamples` is expected to be
  // near-empty here for the same reason the assertion holds: with no phase to flip, the loop has
  // nothing to present.)
  const seen = new Set([...p.phases, ...p.loopSamples, p.beforeDisable, p.afterDisable]);
  expect(p.defaultA, `background ${p.background}`).not.toBe(p.background);
  expect(seen).toEqual(new Set([p.defaultA]));
});

// #577: the renderer has drawn translucent backgrounds since #298 — including the awkward half, a GL
// context created `alpha: true` / `premultipliedAlpha: false` precisely so its straight-colour output
// composites over the page — and the widget exposed no way to ask for it, so a see-through terminal
// was unreachable through the package. There is nothing pure to unit-test on the consumer side (the
// value is forwarded, and the clamp is deliberately the renderer's), so this browser round trip is
// the whole proof rather than a supplement to one.
//
// Read off the ALPHA channel, unlike every other pixel check in this file. The shader writes straight
// colour and blending is never enabled, so `setBgAlpha` moves the fourth channel and leaves RGB
// alone — comparing RGB would show no change and read as a failure. A screenshot cannot see this at
// all: compositing happens against the page, which is opaque here.
test("the consumer can make the terminal background translucent, live (#577)", async ({ page }) => {
  await expect(page.getByRole("button", { name: "Bg alpha: OPAQUE" })).toBeVisible();

  const p = await readAsyncProbe(page, "__bgAlphaProbe");
  const alpha = (px: number[]): number => px[3]!;
  const rgb = (px: number[]): string => `rgb(${px[0]},${px[1]},${px[2]})`;

  // The cells must actually paint, or every assertion below is vacuously true on a buffer nothing
  // wrote to. Background and ink have to differ, too — otherwise "ink stayed opaque" says nothing.
  expect(rgb(p.defaultBg)).not.toBe("rgb(0,0,0)");
  expect(rgb(p.defaultInk)).not.toBe(rgb(p.defaultBg));

  // UNSET = AS TODAY. The demo omits `bgAlpha` from its create options, so this is the boot state of
  // an unconfigured widget: fully opaque, exactly as before this slice.
  expect(alpha(p.defaultBg)).toBe(255);
  expect(alpha(p.defaultInk)).toBe(255);

  // THE FIX: at 0.5 a default-background cell becomes half transparent, so the page behind shows
  // through once the consumer makes it visible. Rounding lands on 127 or 128 depending on the GL
  // implementation's float→byte conversion, so this is a band rather than an equality.
  expect(alpha(p.translucentBg)).toBeGreaterThan(110);
  expect(alpha(p.translucentBg)).toBeLessThan(145);

  // …and the COLOUR did not move — only the channel that should. This is what separates "we made the
  // background translucent" from "we changed the background colour", which a same-turn readPixels
  // cannot otherwise tell apart.
  expect(rgb(p.translucentBg)).toBe(rgb(p.defaultBg));

  // Glyph pixels stay fully opaque. Not a nicety: text that faded with its background would be
  // unreadable over an arbitrary desktop, which is the whole use case. alacritty holds the same line
  // (`compute_bg_alpha` returns `0.` only for the named background colour, `content.rs:388`).
  //
  // **What this samples is a `█`, i.e. BACKGROUND-class ink** (ADR-0019 R1, via `builtin::owns`) —
  // `__bgAlphaProbe` borrows the SGR-5 run for its full blocks because they cover a cell under any
  // font. So it pins the opacity of the ink class R1 is *about*, not that of an ordinary letter,
  // which is the stronger read and is deliberate. Written down because the assertion's meaning moves
  // if R1 is ever read as making background-class ink translucent — #317 §2 raised that question and
  // ADR-0019 R1 now answers it here. A letter would pin only the uncontested half.
  expect(alpha(p.translucentInk)).toBe(255);
  expect(rgb(p.translucentInk)).toBe(rgb(p.defaultInk));

  // LIVE, WITH NO CONTENT FRAME. Read after `setBgAlpha` alone, with the demo's 300ms append timer
  // stopped — so the only thing that could have put those pixels on the canvas is the setter's own
  // present. With the timer left running this passes even if the setter never renders, which is the
  // trap #576 was caught by and recorded on the epic.
  expect(alpha(p.liveNoFrame)).toBeGreaterThan(50);
  expect(alpha(p.liveNoFrame)).toBeLessThan(80); // 0.25 → ~64

  // Not a one-way door: back to 1 restores the original pixel exactly.
  expect(p.restoredBg).toEqual(p.defaultBg);
});

// #577, the OTHER half of the knob. The test above drives the runtime setter; `create` runs once per
// page load, so the option is only reachable by booting with it set — which is what the demo's
// `?bgAlpha=` parameter exists for. Worth its own test rather than folded into the one above: the two
// paths are separate call sites, and the option's is the one a consumer writes first and the one that
// stays silent if it is dropped (the renderer's own default is opaque, so a missing `create` call
// looks exactly like a correct one until somebody asks for translucency).
test.describe("bgAlpha given at create boots translucent (#577)", () => {
  test.use({ bootUrl: "/?bgAlpha=0.6" });

  test("bgAlpha given at create boots translucent, without touching the setter", async ({
    page,
  }) => {
    await expect(page.getByRole("button", { name: "Bg alpha: 0.6" })).toBeVisible();

    const p = await readAsyncProbe(page, "__bgAlphaProbe");

    // `defaultBg` is read before the probe calls any setter, so this is purely what `create` applied.
    expect(p.defaultBg[3]).toBeGreaterThan(140);
    expect(p.defaultBg[3]).toBeLessThan(165); // 0.6 → ~153

    // Ink is opaque here too — the option and the setter reach the same renderer state, not two.
    expect(p.defaultInk[3]).toBe(255);
  });
});

// #577 downstream: a garbage `bgAlpha` must not blank the terminal.
//
// **This test could not exist before `justerm-renderer@0.8.0`** and is the reason this file's pin was
// raised. `set_bg_alpha` used to pass a non-finite value straight into the uniform, and because every
// fragment's alpha is derived from that uniform, the glyphs went transparent with the
// background — the whole terminal vanished, silently. Against the published 0.7.0 both assertions
// below read 0. So this is the widget-level counterpart of the renderer's own `bg-alpha.html` proof:
// that one guards the mechanism, this one guards that a *consumer* passing a bad number through the
// real published dependency still has a terminal.
//
// Reachable from type-correct code — TypeScript's `number` includes `NaN`, so `Number(configValue)`
// on a malformed config produces exactly this. The demo's `?bgAlpha=` parameter reproduces it via
// `Number("foo")`, which is the same path a consumer would take.
test.describe("a non-finite bgAlpha falls back to opaque (#577)", () => {
  test.use({ bootUrl: "/?bgAlpha=foo" });

  test("a non-finite bgAlpha falls back to opaque instead of blanking the terminal", async ({
    page,
  }) => {
    // Wait for the probe itself rather than for a button label: the other tests here gate on
    // `getByRole("button", { name: … })`, but this page boots with `Number("foo")`, so its button
    // reads `Bg alpha: NaN` — a label that exists only to describe a malformed input and is a
    // brittle thing to key a test on. The probe's existence is the actual precondition. (The hook's
    // own gate is the *Finish command* button, which `bgAlpha` does not touch, so it stays valid.)
    await page.waitForFunction(() => typeof window.__bgAlphaProbe === "function");
    const p = await readAsyncProbe(page, "__bgAlphaProbe");

    // Both channels, because the background alone was the *lesser* half of the old failure.
    expect(p.defaultBg[3]).toBe(255);
    expect(p.defaultInk[3]).toBe(255);

    // …and the terminal is genuinely drawn, not merely opaque-and-empty — otherwise a renderer that
    // cleared to an opaque nothing would satisfy the two lines above.
    expect(`rgb(${p.defaultInk.slice(0, 3)})`).not.toBe(`rgb(${p.defaultBg.slice(0, 3)})`);
  });
});

// #325: the device-pixel-ratio change — the last of epic #583, and the only knob in that set that is
// reached with NO consumer call at all.
//
// **The proof splits in two, and the split is measured rather than chosen.** A resolution `change`
// event cannot be produced in this harness: CDP's `Emulation.setDeviceMetricsOverride` *does* move
// `window.devicePixelRatio` and *does* re-evaluate the queries — a `screen and (resolution: 1dppx)`
// query flips `matches` to `false` and `(min-resolution: 1.5dppx)` to `true` — but it dispatches no
// `change` event, to a retained `MediaQueryList` or otherwise (measured 2026-08-10, three variants).
// So the listener half — arming, re-arming at the new ratio, detaching — is proven by unit test
// (`test/dpr-watcher.test.ts`, with the re-arm mutation-checked), and this test proves the half that
// only a real GL context can answer: that adopting a ratio re-bakes AND re-applies the canvas box.
//
// Driven through the demo's `__setDpr` hook, exactly as the renderer's own `demo/dpr-change.html`
// drives `set_device_pixel_ratio` — for the same reason, and it is the reason #322 shipped with one.
test("adopting a new device pixel ratio re-bakes and re-applies the canvas box (#325)", async ({
  page,
}) => {
  await expect(page.getByRole("button", { name: /Finish command/ })).toBeVisible();

  const before = await page.evaluate(() => window.__dprProbe!());
  // The canvas is 1:1 to start with: its CSS box times the density is the drawing buffer. Three
  // independent sources (DOM, WebGL, the browser), so this cannot agree with itself by construction.
  expect(before.appliedW * before.dpr).toBe(before.bufW);
  expect(before.appliedH * before.dpr).toBe(before.bufH);

  await page.evaluate(() => window.__setDpr!(2));
  const after = await page.evaluate(() => window.__dprProbe!());

  // The cell is re-derived at the new density — `round(metric * dpr)` — so the buffer grows with it.
  expect(after.cellW).toBeGreaterThan(before.cellW);
  expect(after.cellH).toBeGreaterThan(before.cellH);
  expect(after.bufW).toBe(after.cols * after.cellW);
  expect(after.bufH).toBe(after.rows * after.cellH);

  // THE POINT. The renderer never touches the DOM, so the display box is this package's to re-apply;
  // forget it and the browser scales a buffer twice the size of its box — the blur #322 removed,
  // reintroduced one layer out. `dpr` here is the ratio we injected, not the page's.
  expect(after.appliedW * 2).toBe(after.bufW);
  expect(after.appliedH * 2).toBe(after.bufH);
  // …and something genuinely moved, so the assertion above is not satisfied by nothing happening.
  //
  // The check is on the BUFFER, not on the CSS box. Whether the box moves at all is **font
  // dependent**: it moves only when `round(metric * dpr)` fails to divide back to the old CSS cell,
  // and that is a property of the metric's fractional part. Measured — this machine goes 19 -> 37
  // device px (box 703 -> 684.5) and CI's Linux font goes 19 -> 38 (box unchanged at 703), from the
  // *same* cell at dpr 1. An earlier revision asserted the box had moved and was red on CI for
  // exactly that reason: equal cells at one density say nothing about the next one.
  expect(after.bufH).toBeGreaterThan(before.bufH);

  // NO RE-FIT, deliberately: the grid is the consumer's (#417/#578), and xterm.js's
  // `handleDevicePixelRatioChange` calls no resize either. A widget that quietly re-derived the grid
  // here would desync the engine the consumer is driving.
  expect(after.cols).toBe(before.cols);
  expect(after.rows).toBe(before.rows);
});

// #339, re-homed by #773 — the browser's drawing-buffer clamp, and who adopts it.
//
// Until renderer 0.15.0 the renderer shrank the *grid* to what the granted buffer held, so
// `terminalSize()` was "the grid actually adopted" and this package needed no test of its own. The
// renderer clamps only the **surface** now (a buffer shared by N grids belongs to none of them), so
// the read-back is the widget's — and this is the only gate on it anywhere in `justerm-web`. Its
// absence is why five doc sites here could go on describing the old outcome without anything firing.
//
// Without the read-back the failure is silent in the way this repo treats as worst: no error, the
// grid keeps the columns it asked for, and every cell past the buffer's edge is clipped by the
// scissor — drawn nowhere, while `terminalSize()` still reports them to the application driving the
// engine.
test("an oversized fit adopts the grid the granted buffer holds (#339/#773)", async ({ page }) => {
  await expect(page.getByRole("button", { name: /Finish command/ })).toBeVisible();

  // 200000 CSS px is far past any implementation's buffer limit at any sane cell.
  const p = await page.evaluate(() => window.__oversizeProbe!(200_000));

  // PRECONDITION, not a result: if the request did not actually exceed what the implementation
  // allocates, every assertion below passes vacuously on a machine with a huge MAX_TEXTURE_SIZE.
  expect(p.askedCols * p.cellW).toBeGreaterThan(p.maxTexture);

  // The widget adopted less than it asked for…
  expect(p.cols).toBeLessThan(p.askedCols);
  // …and it is the largest grid the GRANTED buffer holds, not an arbitrary smaller one.
  expect(p.cols * p.cellW).toBeLessThanOrEqual(p.bufW);
  expect((p.cols + 1) * p.cellW).toBeGreaterThan(p.bufW);
  // …and the display box still describes the buffer that exists, so nothing is stretched.
  expect(Math.abs(p.appliedW * p.dpr - p.bufW)).toBeLessThan(1);
});

// #325 follow-up, found by measuring the slice rather than by reading it: **a GL restore is the one
// buffer change with no consumer call behind it.**
//
// `restore()` re-reads the LIVE device pixel ratio and re-derives the cell at it (`webgl.rs`, #269) —
// deliberately, because a DPR notification arriving while the context is lost is dropped rather than
// queued. So a density that moved during a loss is adopted there, the drawing buffer moves, and until
// this slice nothing re-applied the canvas display box.
//
// The setup is only expressible because of the negative result recorded above: CDP moves
// `devicePixelRatio` without dispatching a `change` event, so the widget's watcher genuinely does not
// see it and `restore()` is left as the only path that can adopt the new density — which is exactly
// the real-world case (a monitor switch that also resets the GPU) with the timing made deterministic.
test("a GL restore at a changed density re-applies the canvas box (#325)", async ({ page }) => {
  await expect(page.getByRole("button", { name: /Finish command/ })).toBeVisible();

  const boot = await page.evaluate(() => window.__dprProbe!());
  expect(boot.appliedW * boot.dpr).toBe(boot.bufW);
  expect(boot.appliedH * boot.dpr).toBe(boot.bufH);

  // **`1280x720` is this suite's own viewport, and that is load-bearing rather than incidental**
  // (measured #808). The negative result below is about a density move *alone*: move the viewport in
  // the same call and Chromium re-evaluates the queries and **does** dispatch `change`, so the
  // watcher adopts the new ratio before any loss and this test stops describing a restore. Keep these
  // two numbers equal to the configured viewport, or pass `0`/`0` to disable the size override.
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: 1280,
    height: 720,
    deviceScaleFactor: 2,
    mobile: false,
  });
  await page.waitForTimeout(300);

  // The watcher cannot see a CDP override, so nothing has been adopted yet. Asserted rather than
  // assumed: if Chromium ever starts dispatching the event, this test stops describing a restore and
  // says so here instead of quietly passing for the wrong reason.
  const injected = await page.evaluate(() => window.__dprProbe!());
  expect(injected.cellW).toBe(boot.cellW);
  expect(injected.cellH).toBe(boot.cellH);

  await page.evaluate(() => {
    const gl = document.querySelector<HTMLCanvasElement>("#term")!.getContext("webgl2")!;
    const ext = gl.getExtension("WEBGL_lose_context")!;
    ext.loseContext();
    setTimeout(() => ext.restoreContext(), 50);
  });
  await page.waitForTimeout(1200);

  const after = await page.evaluate(() => window.__dprProbe!());
  // The restore adopted the new density — without this the assertions below are vacuous.
  expect(after.cellH).toBeGreaterThan(boot.cellH);
  expect(after.bufH).toBe(after.rows * after.cellH);

  // THE POINT. Measured before the fix: buffer `2556x1369` under a canvas still styled `1278x703`.
  // The WIDTH was accidentally right (the cell doubled exactly, 9 -> 18) and only the height was
  // wrong — `703` against a correct `684.5` — so a width-only check would have passed.
  expect(after.appliedH * 2).toBe(after.bufH);
  expect(after.appliedW * 2).toBe(after.bufW);
  // Anti-vacuity on the buffer rather than the box, for the reason the test above states: whether
  // the CSS box moves across a density change is font dependent, and CI's font is not this one's.
  expect(after.bufH).toBeGreaterThan(boot.bufH);
});

// #580: the two cursor policy knobs — the last renderer setters with no consumer call site (#583).
//
// Both are decided at DRAW time against the resolved cell, which is why the proof is a real browser
// and not a fake backend: the widget hands the renderer a number and the renderer decides a colour or
// a width from it, so nothing short of the drawing buffer can say whether the number arrived.
//
// **The expected stroke is derived from the cell the probe reports, never written down.** The cell is
// the ink box of the font's `█` (ADR-0022), so it differs between this machine and CI — the same trap
// that made #578's first dpr-2 test pin `cellW === 18` and fail at 20. A *fraction* of the cell is
// portable only once the cell is measured rather than assumed.
const expectedStroke = (frac: number, cellW: number): number =>
  Math.max(1, Math.round(frac * cellW)); // mirrors `cursor::cursor_thickness`

/**
 * #580 — a lost GL context reads exactly like a knob that never arrived, so say which happened.
 *
 * **Measured, and the first diagnosis of it was wrong.** These two tests were intermittently red
 * with `Expected: 1, Received: 0`. `drawingBufferWidth/Height` were `0` at the failing sample, which
 * was first read as "the canvas has not been sized yet" — it is not: a timeline sampled from page
 * load shows the canvas at `300x150` from `t=7ms` and `1278x703` from `t≈140ms`, never `0`. Widening
 * the dump to the whole state found `canvas=1278x703 buf=0x0 lost=1`: the **context is lost**, and a
 * lost context zeroes the buffer dimensions while the element keeps its size.
 *
 * It is sporadic rather than accumulating — 2 of 6 runs in one session, 0 of 12 in the next, same
 * command, with no test that deliberately loses a context in either. So it is headless-Chromium /
 * SwiftShader instability, not something this widget or this page does, and it can hit any
 * `readPixels` probe here.
 *
 * What it cannot be allowed to do is look like a defect. Waiting is not the answer — a lost context
 * never gets its buffer back, so a gate on `drawingBufferWidth > 0` would convert a legible pixel
 * mismatch into a timeout. (An earlier revision of this file did exactly that, and it appeared to
 * work only because the runs it was measured on had not lost their context.) Asserting the
 * precondition is: the probe reports the context's own verdict, and this names it.
 */
const expectContextAlive = (p: { contextLost: boolean }): void => {
  expect(
    p.contextLost,
    "the WebGL context was lost during the probe — every pixel below reads 0, which is an environment failure and not a defect in the knob under test",
  ).toBe(false);
};

test("the cursor's thickness and contrast policies reach the renderer (#580)", async ({ page }) => {
  await expect(page.getByRole("button", { name: "Cursor blink: OFF" })).toBeVisible();

  const p = await page.evaluate(() => window.__cursorPolicyProbe!());
  expectContextAlive(p);

  // --- thickness ------------------------------------------------------------------------------
  // Unset at `create`, so this is the renderer's own default and the "byte for byte" acceptance box:
  // wiring the knob must not move the caret for a consumer who never asks for it.
  expect(p.thickness.boot).toBe(expectedStroke(0.15, p.cellW));
  expect(p.thickness.thick).toBe(expectedStroke(0.5, p.cellW));
  // The two exact assertions above agree vacuously if the cell is narrow enough that both fractions
  // round to the same pixel — assert they actually separate, so the test cannot pass on a cell where
  // it could no longer observe anything.
  expect(p.thickness.thick).toBeGreaterThan(p.thickness.boot);
  // Not a one-way door.
  expect(p.thickness.back).toBe(p.thickness.boot);

  // --- contrast -------------------------------------------------------------------------------
  // The caret is painted the cell's own background here, so with the guard at its floor it is
  // genuinely invisible. This assertion is what proves `cursorContrast: 1` reached the renderer at
  // all: the default would have rescued it.
  expect(p.contrast.guardOff).toBe(p.contrast.background);
  // …and with the guard on, it is rescued to the theme's default fg (`0xcdd6f4`) rather than merely
  // being "some other colour", which a wrong-but-visible fallback would also satisfy.
  expect(p.contrast.guardOn).toBe("rgb(205,214,244)");
  expect(p.contrast.guardOn).not.toBe(p.contrast.background);
  // THE COMPLETENESS CLAUSE. The third `setTheme` omits `cursorContrast` entirely, so the widget must
  // push the DEFAULT rather than leave the `1` from two samples ago standing. Goes red if the push is
  // made conditional on the field being present — which is how the sibling *options* are wired, and
  // therefore the plausible wrong shape rather than a hypothetical one.
  expect(p.contrast.guardReset).toBe(p.contrast.guardOn);

  // The boot caret on this page takes `defaultFg`, which contrasts with the cell, so no guard fires
  // and it is simply visible. The control for the booted-invisible page below.
  expect(p.contrast.boot).toBe("rgb(205,214,244)");
});

// #580, the CREATE half of both knobs. The test above drives the runtime paths (`setCursorThickness`
// and `setTheme`); `create` runs once per page load, so the option and the theme field are only
// reachable by booting with them set. Worth its own page for the same reason #577's is: the create
// call site is the one a consumer writes first and the one whose omission is silent — the renderer's
// defaults are a working caret, so a dropped `create` push looks exactly like a correct one.
//
// `cursorColor` is booted alongside because `cursorContrast` is otherwise undecidable at create: the
// default caret colour contrasts with every cell the demo draws, so the threshold has nothing to
// decide until the caret is put INTO the background.
test.describe("the cursor policies given at create take effect (#580)", () => {
  test.use({ bootUrl: "/?cursorThickness=0.5&cursorContrast=1&cursorColor=0x1e1e2e" });

  test("a cursor thickness and contrast given at create take effect, without touching a setter", async ({
    page,
  }) => {
    await expect(page.getByRole("button", { name: "Cursor blink: OFF" })).toBeVisible();

    const p = await page.evaluate(() => window.__cursorPolicyProbe!());
    expectContextAlive(p);

    // The OPTION half: read before the probe calls `setCursorThickness`, so this is purely `create`.
    expect(p.thickness.boot).toBe(expectedStroke(0.5, p.cellW));
    expect(p.thickness.boot).toBeGreaterThan(expectedStroke(0.15, p.cellW));

    // The THEME half: booted with the guard at its floor and a caret coloured like the cell, so the
    // caret `create` produced is invisible. On the default page above the same sample is `defaultFg`
    // — the two together are what separate "the field arrived" from "the caret happens to look
    // like this".
    expect(p.contrast.boot).toBe(p.contrast.background);
  });
});

// #578: the typography knobs. Unlike the other unwired-knob slices this one is not a pass-through —
// both setters MOVE THE CELL, and the cell is what the grid, the drawing buffer and every px->cell
// conversion derive from. So the thing under test is the *coupling*, not the setter: a spacing change
// that does not re-fit leaves the engine on the old grid while the renderer draws at the new cell,
// and the failure is silent — spans land outside the grid and the surface simply stops updating,
// the shape #547 describes.
//
// The invariant asserted at every stage is `buffer === grid x cell` (#331) together with
// `demo grid === renderer grid`. Three values, any of which a spacing change can desynchronise.
// **Why the width assertions below may be exact and the height ones may not.** The renderer's
// `device_cell` adds the spacing term to the font's advance — `dx = round(letterSpacing * dpr)`, then
// `w = char_px.0 + dx` — so a width *difference* depends only on the setting and the density, never on
// the font, and is assertable to the pixel on any machine. Height is **multiplicative**
// (`h = char_px.1 * lineHeight`), so a height difference scales with the font's own glyph box and only
// its *direction* is portable. Valid as long as neither result reaches `device_cell`'s
// `clamp(1, MAX_CELL_PX)` bounds, which the values used here are far from.
//
// No ABSOLUTE cell dimension is assertable either way: the cell derives from the font's `█` ink box
// (ADR-0022) and CI's Linux fonts differ from a local machine's. Learned here — the first version of
// the dpr-2 test below pinned `base.cellW === 18` and failed on CI at 20.
const agrees = (s: {
  cellW: number;
  cellH: number;
  cols: number;
  rows: number;
  demoCols: number;
  demoRows: number;
  bufW: number;
  bufH: number;
}): void => {
  // The renderer adopted what the demo believes it is driving.
  expect(s.demoCols).toBe(s.cols);
  expect(s.demoRows).toBe(s.rows);
  // …and the device buffer is exactly grid x cell, so nothing overhangs (#331).
  expect(s.bufW).toBe(s.cols * s.cellW);
  expect(s.bufH).toBe(s.rows * s.cellH);
};

test("a spacing change moves the cell and the whole geometry moves with it (#578)", async ({
  page,
}) => {
  await expect(page.getByRole("button", { name: "Letter spacing: 0px" })).toBeVisible();
  const p = await page.evaluate(() => window.__spacingProbe!());

  // Every stage is internally consistent — this is the acceptance criterion "engine grid, renderer
  // grid and px->cell geometry all agree afterwards", asserted at each step rather than only at the end.
  for (const s of [p.boot, p.base, p.spaced, p.tall, p.huge, p.restored]) agrees(s);

  // LETTER SPACING widens the cell, so fewer columns fit the same viewport. The `+ 4` below is
  // `round(letterSpacing * dpr)`, so it depends on the density — assert the precondition rather than
  // leaving the arithmetic implicit (the dpr-2 sibling test covers the scaled case).
  expect(p.dpr).toBe(1);
  expect(p.spaced.cellW).toBe(p.base.cellW + 4);
  expect(p.spaced.cellH).toBe(p.base.cellH); // height untouched — it is a *column* gap
  expect(p.spaced.cols).toBeLessThan(p.base.cols);

  // LINE HEIGHT heightens the cell, so fewer rows fit. 1.6x of a 19px cell measured as 30.
  expect(p.tall.cellH).toBeGreaterThan(p.base.cellH);
  expect(p.tall.cellW).toBe(p.base.cellW); // width untouched
  expect(p.tall.rows).toBeLessThan(p.base.rows);

  // THE ADOPTED VALUE, NOT THE REQUESTED ONE. An absurd multiplier is not honoured: the renderer
  // shrinks a cell the glyph atlas cannot hold (#359). Measured — 40x of a 19px cell would be 760,
  // and what came back was 254. The load-bearing part is the line after: the fit used 254, because
  // `bufH === rows * cellH` still holds. Had it fitted against the *requested* height the buffer and
  // the grid would disagree, which is precisely the silent desync this whole test exists for.
  expect(p.huge.cellH).toBeLessThan(p.base.cellH * p.hugeRequested);
  // …and it DID grow, so this is a clamp rather than the setter being ignored. Compared against
  // `base` rather than against `tall`: where the atlas clamp lands is font-dependent, so
  // `huge > tall` is a relation between two derived values that a different font could invert, while
  // `huge > base` is the claim itself.
  expect(p.huge.cellH).toBeGreaterThan(p.base.cellH);

  // DEFAULTS ARE A NO-OP: back to 0/1 returns to the base geometry exactly, not approximately.
  expect(p.restored).toEqual(p.base);
});

// #578 + ADR-0023: `letterSpacing` is CSS px because `fontSize` is, and the renderer applies
// `round(letterSpacing * dpr)`. This is the repo's one deliberate divergence from BOTH references on a
// public setter's unit — xterm.js adds it to an already-device-px char width
// (`WebglRenderer.ts:671`), alacritty adds `font.offset` raw to device-px metrics — so under their
// behaviour the same setting is a different visual gap per display, and moving a window between
// monitors re-lays-out the text.
//
// Worth its own test rather than a comment: the divergence is the *reason* the ADR exists, and nothing
// else in the suite would notice if the unit silently became device px. Driven at a real
// deviceScaleFactor, so the scaling actually happens rather than being asserted about.
test.describe("letterSpacing is CSS px, so its gap is density-independent (ADR-0023, #578)", () => {
  test.use({ deviceScaleFactor: 2 });

  test("a 4 CSS-px setting is 8 device px at dpr 2, not 4", async ({ page }) => {
    await expect(page.getByRole("button", { name: "Letter spacing: 0px" })).toBeVisible();
    const p = await page.evaluate(() => window.__spacingProbe!());

    // The control, asserting the CONDITION rather than a consequence of it. This line used to read
    // `expect(p.base.cellW).toBe(18)` — twice the 9 measured locally — and CI's Linux fonts give a
    // different glyph advance, so it was 20 there and the test failed on a machine difference while
    // the behaviour below was correct. The cell derives from the font's `█` ink box (ADR-0022), so
    // **no absolute cell dimension is portable**; only a difference between two cells measured on the
    // same machine is.
    expect(p.dpr).toBe(2);
    // THE CLAIM, and it is font-independent because the spacing term is added to the advance rather
    // than derived from it: a 4 CSS-px setting arrives as `round(4 * 2)` = 8 device px. Under either
    // reference's device-px reading it would be 4 here — half the gap for the same number, which is
    // the incoherence ADR-0023 spends reference parity to avoid.
    expect(p.spaced.cellW - p.base.cellW).toBe(8);

    agrees(p.spaced);
  });
});

// #578: the OPTION half. `create` runs once per page load, so `letterSpacing`/`lineHeight` passed
// there are only reachable by booting with them — and the claim is specifically that they are applied
// BEFORE the first fit, so the initial grid is computed at the consumer's cell rather than at the
// renderer's default and then corrected. One `setLetterSpacing` later the two are indistinguishable,
// which is why the probe snapshots the boot state before touching anything.
test.describe("letterSpacing / lineHeight given at create apply before the first fit (#578)", () => {
  test.use({ bootUrl: "/?letterSpacing=4&lineHeight=1.6" });

  test("the boot cell and the boot grid both carry the create-time options", async ({ page }) => {
    await expect(page.getByRole("button", { name: "Letter spacing: 4px" })).toBeVisible();
    const p = await page.evaluate(() => window.__spacingProbe!());

    // The boot cell carries BOTH options — not the renderer's 9x19 default.
    expect(p.boot.cellW).toBe(p.base.cellW + 4);
    expect(p.boot.cellH).toBeGreaterThan(p.base.cellH);

    // And the grid the page has been driving since load was computed at that cell: it agrees with
    // itself, and it is the smaller grid the bigger cell implies rather than the default one.
    agrees(p.boot);
    expect(p.boot.cols).toBeLessThan(p.base.cols);
    expect(p.boot.rows).toBeLessThan(p.base.rows);
  });
});

// #606: `Terminal.dispose()` is end of life, and the renderer it was handed must stop with it. The
// unit tests prove the widget *calls* dispose on a fake; this is the only place the consequence is
// observable — the rAF loop is real and it is what repaints the canvas. Counted rather than sampled
// for colour: a rAF turn the loop presented in reads as a pixel, one it skipped reads black, so
// "presents per 1.5s" is measurable and the claim (zero, after dispose) cannot pass by luck.
test("a disposed widget stops the renderer it was handed (#606)", async ({ page }) => {
  await expect(page.getByRole("button", { name: "Cursor blink: OFF" })).toBeVisible();

  const p = await readAsyncProbe(page, "__disposeProbe");

  // Control: the loop really is presenting while the widget is alive, or the assertion below would
  // hold for a renderer that never drew anything.
  expect(p.beforeDispose).toBeGreaterThan(0);

  // THE FIX: nothing the widget started reaches the renderer any more. Before #606 the widget could
  // not have stopped it even if it wanted to — `dispose` was not on the `Renderer` port.
  expect(p.afterDispose).toBe(0);

  // #773 follow-up — dispose also hands the **grid** back, which is the only way GPU memory is
  // returned without dropping the whole wasm instance (a consumer holding the object cannot).
  // Measured motivation: the glyph atlas is a fixed `tex_storage_3d(RGBA8, pw, ph * 32, 192)`
  // allocation whose size does not depend on how many glyphs were used — 4.2 MiB at an 8x16 cell,
  // 12.8 MiB at the 15x30 cell measured at dpr 2 — held per closed terminal, for as long as the
  // page lived.
  //
  // Asserted through geometry because **WebGL exposes no memory query**: a widget with no grid
  // throws from every per-grid path. That the release then happens is the renderer's own gate
  // (`per-config-atlas.html`: the last grid to leave a configuration deletes its atlas), not this
  // one's — this proves the call, that one proves the consequence.
  expect(p.geometryBeforeDispose).toBe(true); // control: it answered while alive
  expect(p.geometryAfterDispose).toBe(false);

  // …and `dispose` stays idempotent, which `removeGrid` is not on its own: it throws on an id it
  // does not know, so a second call would throw where the `Renderer` port requires silence.
  expect(p.secondDisposeThrew).toBe(false);
});

// #579: the widget's half of the renderer's context-loss surface. Nothing here is reachable from
// vitest — `WEBGL_lose_context` is a real extension, the deadline is a real timer armed inside wasm,
// and the notification arrives from a Rust-scheduled closure. The relay's rules are unit-tested
// against a fake; this is the round-trip.
test("a lost GL context reaches the consumer, once, and never after dispose (#579)", async ({
  page,
}) => {
  await expect(page.getByRole("button", { name: "Cursor blink: OFF" })).toBeVisible();

  const p = await readAsyncProbe(page, "__contextLossProbe");

  // The window ADR-0027 D4 turns on, asserted to EXIST before anything is asserted inside it. If a
  // browser ever destroyed a context and dispatched its event synchronously, this would report that
  // rather than let the rest pass vacuously — the vacuity that let #639's first fix go green.
  expect(p.raceWindow.glSaysLost).toBe(true);
  expect(p.raceWindow.widgetSaysLost).toBe(false);

  // …and once the event lands, the report agrees. Together these two are the measurement ADR-0027
  // asked #579 for: the flag semantics it kept are observably a *different answer* to a real
  // consumer, not a rounding of the context's own.
  expect(p.reportedAfterEvent).toBe(true);
  expect(p.overdueBeforeDeadline).toBe(false);

  // A loss that comes back in time says nothing to the consumer, clears the report, and repaints.
  expect(p.restoreCallbacks).toBe(0);
  expect(p.lostAfterRestore).toBe(false);
  expect(p.presentsAfterRestore).toBeGreaterThan(0);

  // A loss that outlives its deadline notifies exactly once — and stays at once, twice as long
  // again. The count rather than merely ">0", because "at most once per loss" is the contract
  // `context_loss.rs` states and a `>0` assertion cannot see it break.
  //
  // An earlier version of this comment claimed the count also beats xterm.js, which overwrites its
  // single timeout handle without clearing it on a second `webglcontextlost`. The *code* does that
  // (`WebglRenderer.ts:131`), but this probe never drove two losses with no restore between, so
  // whether a browser even delivers that second event on an already-lost context is unmeasured
  // here. Removed rather than softened: a comparative claim next to a passing assertion reads as
  // something the assertion checked.
  expect(p.overdueCallbacks).toBe(1);
  expect(p.overdueCallbacksLater).toBe(1);
  expect(p.overdueFlag).toBe(true);

  // THE DISPOSE GATE. The renderer's own canvas listeners survive `Terminal.dispose()` — they belong
  // to the wasm binding's `free()`, which the widget never calls — so a deadline still arms and
  // still expires here. Zero deliveries is the widget's relay refusing, which is xterm.js's
  // observable contract (its disposable clears the pending restore timeout).
  expect(p.callbacksAfterDispose).toBe(0);

  // …and the pull side still answers. Disposal ends work, not truthfulness: silencing the queries
  // would report a live context for a dead one.
  expect(p.reportsLostAfterDispose).toBe(true);
});

test("a cell-size change re-anchors the IME textarea at composition start (#631)", async ({
  page,
}) => {
  // The anchor is a DOM side effect (`ta.style.top`), so a synthetic unit cannot see it — and the
  // widget's per-frame cache is keyed on the cursor CELL, which a spacing change leaves identical.
  // The demo's cursor is a constant, so this reproduces the defect exactly: stationary cursor,
  // moved cell.
  await expect(page.getByRole("button", { name: "Line height: 1" })).toBeVisible();
  const p = await page.evaluate(() => window.__imeAnchorProbe!());

  // THE CONTROL, in the same run: the cell really moved. Direction only — a line-height delta is
  // multiplicative on a font-dependent cell, so no absolute height is portable across machines.
  expect(p.afterCellMove.cellH).toBeGreaterThan(p.base.cellH);
  expect(p.afterCellMove.cellW).toBeCloseTo(p.base.cellW, 3); // a ROW knob, not a column one

  // THE CLAIM: after a real `compositionstart`, the anchor matches the geometry in force NOW —
  // which is where the OS opens its candidate window. Asserted as a relation between two numbers
  // read in the same snapshot, so it holds whatever the font measured.
  expect(p.afterCompositionStart.top).toBeCloseTo(
    p.cursorRow * p.afterCompositionStart.cellH,
    3,
  );
  expect(p.afterCompositionStart.left).toBeCloseTo(
    p.cursorCol * p.afterCompositionStart.cellW,
    3,
  );

  // …and it actually MOVED to get there. Without this the assertion above could pass on a page
  // where the cell never changed, which is the tautology `theflow` Step 4 warns about.
  expect(p.afterCompositionStart.top).not.toBeCloseTo(p.base.top, 3);

  // Deliberately NOT asserted: that `afterCellMove.top` is stale. It is, today — the re-sync is at
  // the point of use, following xterm.js, whose own option-change path cannot reach its
  // `_syncTextArea` either. Pinning the staleness would freeze that choice and fail the day a push
  // signal (#630's third shape) replaces it, while the claim above stays the requirement either way.
});

// #632: `FitController`'s dedupe remembered only `cols`/`rows`, which a cell-size change leaves
// identical while making them describe a grid nobody holds. A later real container resize that
// proposed exactly that pair was then dropped in silence. Only provable live: the defect needs a
// real ResizeObserver, the real 100ms debounce and the real cell, and every number below is
// computed in the browser because the cell derives from the font's `█` ink box (ADR-0022) — no
// absolute cell dimension is portable (#578).
//
// #733: the 800×600 boot is asked for as a context option, not built by resizing a page that is
// already up. The old shape had to resize the demo `beforeEach` had booted and then throw it away
// (`about:blank`) so the fit that resize triggered could not answer the poll below — three
// navigations and a 400ms wait to arrange one boot at a known size, and a `toHaveLength(0)` whose
// whole job was to prove the discarded page had stayed quiet. `test.use({ viewport })` applies
// before the page exists, so `beforeEach`'s single navigation IS the boot under test and every one
// of those steps loses its reason. The mount fit therefore predates this body, which is why the
// count comes from the `consoleLines` fixture.
test.describe("a real resize proposing the pre-change grid still reaches the port (#632)", () => {
  test.use({ viewport: { width: 800, height: 600 } });

  test("a resize computing the remembered grid under a changed cell is not deduped away", async ({
    page,
    consoleLines,
  }) => {
    const fits = (): string[] => fitsIn(consoleLines);
    const gridOf = (line: string): { cols: number; rows: number } => {
      const m = line.match(/resize (\d+)x(\d+)/);
      if (!m) throw new Error(`unparseable fit log: ${line}`);
      return { cols: Number(m[1]), rows: Number(m[2]) };
    };

    // The observer fires once on mount; that flush is what loads the controller's memory.
    await expect.poll(() => fits().length).toBeGreaterThan(0);
    const remembered = gridOf(fits().at(-1)!);
    const before = await page.evaluate(() => window.__fitProbe!());

    // FIRST, that the boot really happened at the requested size. Nothing else below would notice
    // if `test.use({ viewport })` silently did not apply: every number after this is relative to
    // `remembered`, so the test would pass at the project's default 1280x720 just as well, and the
    // option would be a no-op nobody could see. `<= 800` rather than `=== 800` because a page
    // scrollbar comes out of `innerWidth`; the discriminating part is that it is not 1280.
    expect(before.innerWidth).toBeLessThanOrEqual(800);

    // A cell change, taken through the consumer contract (setter → fit → render). It does NOT go
    // through the controller, and — measured, not assumed — it does not fire the ResizeObserver
    // either, so the controller's memory is left describing the pre-change grid.
    await page.evaluate(() => window.__setLineHeight!(1.6));
    await page.waitForTimeout(400); // well past the debounce: prove no flush happened
    expect(fits()).toHaveLength(1);
    const after = await page.evaluate(() => window.__fitProbe!());

    // THE CONTROLS, in the same run. Direction only for the cell height — a line-height delta is
    // multiplicative on a font-dependent cell, so no absolute value is portable.
    expect(after.cssCellH).toBeGreaterThan(before.cssCellH);
    expect(after.cssCellW).toBeCloseTo(before.cssCellW, 3); // a row knob, not a column one
    // …and the renderer's grid really moved, so the remembered pair really is stale now.
    expect(after.rows).not.toBe(remembered.rows);

    // Now a genuine container resize that proposes EXACTLY the remembered pair under the NEW cell:
    // pick the height whose floor(height / newCell) lands back on the remembered row count.
    const targetHeight = Math.floor((remembered.rows + 0.5) * after.cssCellH);
    expect(Math.floor(targetHeight / after.cssCellH)).toBe(remembered.rows); // the construction holds
    await page.setViewportSize({ width: before.innerWidth, height: targetHeight });

    // THE CLAIM: it reaches the port. Under the `cols`/`rows`-only key this was deduped away and the
    // engine kept a grid the box no longer wanted — the silent desync #547 describes.
    await expect.poll(() => fits().length, { timeout: 4000 }).toBeGreaterThan(1);
    expect(gridOf(fits().at(-1)!)).toEqual(remembered);
  });
});

test("a pointer-down mid-composition leaves the IME anchor frozen (#649)", async ({ page }) => {
  // #637 closed the output-frame entrance; this is the FORCED one. `element` mousedown → onDown →
  // Terminal.focus() → a forced re-sync, which used to beat the composing guard and re-anchor onto
  // the superseded cursor cell. The unit suite cannot reach it: `Terminal` needs a DOM and vitest
  // runs in `environment: "node"`, which is why the issue's acceptance asks for the pin here.
  await expect(page.getByRole("button", { name: "Cursor drift: OFF" })).toBeVisible();
  const p = await page.evaluate(() => window.__imePointerProbe!());

  // The composition really was open when the pointer went down — read off behaviour (a composing
  // controller swallows CapsLock) rather than off our own flag, so the probe cannot assert its
  // own premise.
  expect(p.capsLockSwallowedAtPointerDown).toBe(true);

  // The setup held: an output frame did not move the anchor while composing (#637 still in force).
  expect(p.afterDrift.top).toBeCloseTo(p.atCompositionStart.top, 3);

  // THE CLAIM: the pointer-down did not move it either.
  expect(p.afterPointerDown.top).toBeCloseTo(p.atCompositionStart.top, 3);
  // …and it had somewhere else to go, so this is a suppressed move rather than an absent one.
  expect(p.driftedCell.top).not.toBeCloseTo(p.atCompositionStart.top, 3);

  // THE CONTROL, in the same run — and the only thing that distinguishes the two predicates:
  // immediately after `compositionend` the candidate window is gone (`composing` false) while the
  // deferred commit read is still queued (`active` true). The same pointer-down must now land on
  // the drifted cell. Keyed on `active`, the anchor would still be frozen here and every unit test
  // would still be green.
  expect(p.afterPointerDownPostEnd.top).toBeCloseTo(p.driftedCell.top, 3);
});

// #675 — an unmeasured cell used to kill the wheel *permanently*. The wheel listener is on the
// container (`terminal.ts` attaches to `element`) while `getGeometry` reads the canvas, so
// "container laid out, canvas not" is a structural state: a collapsed panel, a hidden tab, or the
// window before the first fit. A pixel-mode notch then divided by 0, the sub-line accumulator kept
// the result (`Infinity % 1` is `NaN`), and the widget handed the consumer a non-finite offset that
// came back on the next frame — so recovering the geometry did not recover the wheel.
//
// PIXEL mode on purpose: the LINE branch never divides by the cell and was never affected, which is
// also why the fix guards outputs per branch rather than refusing the whole context.
//
// The node suite cannot reach this: `vitest.config.ts` runs `environment: "node"`, so the
// `getGeometry` → `onWheel` → `onScroll` → frame → `track()` loop that *latched* it has no unit
// form. The decisions inside it are unit-tested; this is the loop.
test("a wheel survives an unmeasured cell instead of latching NaN (#675)", async ({ page }) => {
  const offsets: string[] = [];
  page.on("console", (m) => {
    const n = m.text().match(/\[wheel\] scroll → displayOffset (\S+)/);
    if (n?.[1] !== undefined) offsets.push(n[1]);
  });
  const notch = () =>
    page.evaluate(() => {
      const c = document.querySelector("#term") as HTMLElement;
      c.parentElement!.dispatchEvent(
        new WheelEvent("wheel", { deltaY: -600, deltaMode: 0, bubbles: true, cancelable: true }),
      );
    });
  const hideCanvas = (hidden: boolean) =>
    page.evaluate((h) => {
      (document.querySelector("#term") as HTMLElement).style.display = h ? "none" : "";
    }, hidden);

  // Control: a healthy notch scrolls, and the offset is a number.
  await notch();
  expect(offsets.length).toBeGreaterThan(0);
  const healthyCount = offsets.length;

  // The canvas loses its box; the container keeps its listener.
  await hideCanvas(true);
  await notch();
  await notch();
  await hideCanvas(false);

  // The gesture that used to be dead forever.
  await notch();
  await notch();

  // Two assertions, and the second is the one that fails without the fix: no offset was ever
  // non-finite, AND scrolling still happens after the geometry comes back.
  expect(offsets.filter((o) => !Number.isFinite(Number(o)))).toEqual([]);
  expect(offsets.length).toBeGreaterThan(healthyCount);
});

// #680 — a drag that outlives the canvas's box used to auto-scroll at DRAG_SCROLL_MAX_SPEED.
// `mousedown` is on the canvas, so a hidden canvas cannot be pressed — but `mousemove`/`mouseup`
// are window-scoped and the tick timer is already running, so a drag ALREADY IN PROGRESS survives
// the canvas losing its box. That is a collapsing panel or a tab switch mid-selection.
//
// Observed through the scrollbar thumb, which is passive: reading it does not scroll, unlike the
// wheel-log read-out that an earlier probe used and that reported its own effect. At displayOffset
// 0 the thumb sits at its maximum and scrolling into history lowers it, so the defect (≈120 lines
// "toward newer" over 8 ticks) slams it back to the top of its range.
//
// Judged on the delta against a same-duration control, not an absolute: the demo appends a line
// every 300ms, which raises the thumb on its own, and the maximum itself moves as scrollback grows.
test("a drag does not auto-scroll when the canvas loses its box (#680)", async ({ page }) => {
  const thumbTop = () =>
    page.evaluate(() => {
      const track = [...document.querySelectorAll("div")].find(
        (d) =>
          d.style.position === "absolute" &&
          d.style.right === "0px" &&
          d.style.height === "100%" &&
          d.querySelector("div"),
      );
      const t = track?.querySelector("div") as HTMLElement | undefined;
      return t ? parseFloat(t.style.top) : null;
    });
  const wheel = (dy: number) =>
    page.evaluate((d) => {
      document
        .querySelector("#term")!
        .parentElement!.dispatchEvent(
          new WheelEvent("wheel", { deltaY: d, deltaMode: 1, bubbles: true, cancelable: true }),
        );
    }, dy);
  const mouse = (type: string, clientY: number, held: boolean) =>
    page.evaluate(
      ([t, y, h]) => {
        const c = document.querySelector("#term") as HTMLElement;
        (t === "mousedown" ? c : window).dispatchEvent(
          new MouseEvent(t as string, {
            clientX: 200,
            clientY: y as number,
            button: 0,
            buttons: h ? 1 : 0,
            detail: 1,
            bubbles: true,
            cancelable: true,
          }),
        );
      },
      [type, clientY, held] as [string, number, boolean],
    );
  const hideCanvas = (h: boolean) =>
    page.evaluate((x) => {
      (document.querySelector("#term") as HTMLElement).style.display = x ? "none" : "";
    }, h);

  // Enough history that wheeling up cannot clamp against the top (same poll the #133 wheel test uses).
  await expect
    .poll(async () => (await thumbTop()) ?? 0, { timeout: 25_000, intervals: [400] })
    .toBeGreaterThanOrEqual(50);
  await wheel(-6);
  await wheel(-6);
  await page.waitForTimeout(150);

  // A drag begins normally, pointer well inside the terminal.
  await mouse("mousedown", 300, false);
  await mouse("mousemove", 300, true);
  await page.waitForTimeout(400);
  const control = (await thumbTop())!;

  // The panel collapses mid-drag, for the same duration.
  await hideCanvas(true);
  await mouse("mousemove", 300, true);
  await page.waitForTimeout(400);
  await hideCanvas(false);
  const after = (await thumbTop())!;
  await mouse("mouseup", 300, false);

  // Measured before the fix: control drifted +1.7 (line appends) while this arm jumped +18.8.
  expect(after - control).toBeLessThan(6);
});

// #249 / ADR-0028: the in-progress composition is drawn into the grid. Two of the three references
// do this (ghostty `renderer/generic.zig:2368`, alacritty `display/mod.rs:1189`); xterm's DOM box
// cannot transfer, because its cell width IS a browser advance and ours is the ink box of `█`
// (measured: −3.95 to −5.28 CSS px per Korean syllable on every real monospace font).
//
// Only e2e can see this. The widget needs a DOM and the unit suite runs in `environment: "node"`,
// which is the same blind spot #649 measured — a wrong composition predicate ships green there.
test("the in-progress composition is drawn, and the caret rides its end (#249)", async ({ page }) => {
  // Wait for the page to finish booting before reaching for a probe. `goto` resolves at the load
  // event, but the demo instantiates its wasm behind a top-level await, so the module body — and
  // every `window.__*Probe` assignment in it — can still be pending. Skipping this reads exactly
  // like a missing probe (`__preeditProbe is not a function`), which is indistinguishable from
  // having never written one.
  await expect(page.getByRole("button", { name: "Cursor blink: OFF" })).toBeVisible();
  await page.locator("#term").dispatchEvent("mousedown"); // focus the hidden textarea

  const p = await page.evaluate(() => window.__preeditProbe!());

  // 한글날 is three syllables = six cells. Every one of them carries ink while composing.
  const run = [0, 1, 2, 3, 4, 5];
  for (const c of run) {
    expect(p.composing[c], `cell ${c} of the composed run, idle=${p.idle} composing=${p.composing}`)
      .toBeGreaterThan(0);
  }
  // ADR-0028 D5 — the caret rides to the cell right after the run, and it is SOLID there: a
  // composition suppresses the blink (#592), so this is deterministic rather than phase-dependent.
  expect(p.composing[6], `the caret's cell, idle=${p.idle} composing=${p.composing}`)
    .toBeGreaterThan(0);
  // And everything stops there. Without this the assertions above would pass on a full-row repaint
  // just as happily as on a preedit. Index 7, not 6 — 6 is the caret's, which is the point above.
  expect(p.composing[7]).toEqual(p.idle[7]);

  // Discriminating: the composed cells do not merely have ink, they have DIFFERENT ink. A preedit
  // that failed to draw would leave the demo's own content here and still be "greater than 0".
  expect(p.composing.slice(0, 6)).not.toEqual(p.idle.slice(0, 6));

  // A settling `compositionupdate` carrying unchanged data changes nothing — a real IME emits one
  // per syllable (measured on a Windows Korean IME, #249).
  expect(p.unchangedUpdate).toEqual(p.composing);

  // The composition ends and the grid is exactly as it was. Not "close to": the pass replaces the
  // cells and must put every one of them back.
  expect(p.ended).toEqual(p.idle);

  // ADR-0028 D4 — the anchor rides the run's end while composing, and returns when it closes. The
  // hidden textarea is what the OS reads to place its candidate window, and 한글날 is six cells, so
  // this is a six-cell move, not a nudge.
  expect(p.anchorComposing).not.toBe(p.anchorIdle);
  expect(p.anchorEnded).toBe(p.anchorIdle);
});

/**
 * #801 — **the SOLE TENANT can be set aside and brought back.**
 *
 * `e2e/shared-surface.spec.ts` proves the two-terminal case; this exists because the single-terminal
 * arrangement is what every consumer of this package uses today, its code path genuinely differs, and
 * `show()` is exercised nowhere else — a shared tenant returns through `setViewportRect`, since its
 * DOM box is what was taken away.
 *
 * The difference that makes a separate test rather than an assumption: `applyGrid` re-sizes the
 * drawing buffer for a sole tenant *before* it consults the hidden flag, and this package's own
 * record states twice that a resized buffer is a cleared one. So the buffer and the grid are asserted
 * across the trip rather than inferred from the shared page's result.
 */
test("a sole tenant hides and comes back, with its buffer and grid intact (#801)", async ({
  page,
}) => {
  const p = await page.evaluate(() => window.__soleTenantHideProbe!());

  // The terminal is drawn and painting to begin with. Asserted rather than assumed: if the centre
  // pixel were already transparent, every claim below would hold vacuously.
  expect(p.before.drawn).toBe(true);
  expect(p.before.centre).not.toBe("0,0,0,0");

  // Hidden: the renderer stops drawing it, and the canvas goes back to bare buffer. `clear` is
  // transparent, so "nothing is placed here" and "this pixel was never written" are the same value —
  // which is exactly what a hidden grid should produce.
  expect(p.hidden.drawn).toBe(false);
  expect(p.hidden.centre).toBe("0,0,0,0");

  // …and nothing was given up. The buffer is the quantity at risk: a sole tenant sizes it inside the
  // same method that hides, before the hidden check.
  expect(p.hidden.bufW).toBe(p.before.bufW);
  expect(p.hidden.bufH).toBe(p.before.bufH);
  expect({ cols: p.hidden.cols, rows: p.hidden.rows }).toEqual({
    cols: p.before.cols,
    rows: p.before.rows,
  });

  // Shown: back, at the same size, with the same pixels — a placement, not a rebuild.
  expect(p.shown.drawn).toBe(true);
  expect(p.shown.centre).toBe(p.before.centre);
  expect(p.shown.bufW).toBe(p.before.bufW);
  expect(p.shown.bufH).toBe(p.before.bufH);
  expect({ cols: p.shown.cols, rows: p.shown.rows }).toEqual({
    cols: p.before.cols,
    rows: p.before.rows,
  });
});

/**
 * #810 — **a hidden pane's zero box does not re-grid the terminal.**
 *
 * The unit tests pin the arithmetic on both sizing paths; this drives the consumer-visible call
 * against the real wasm renderer, which is where a host hiding a pane actually lands: it keeps
 * fitting, and a `display: none` element measures `0x0`.
 *
 * The harm this prevents is not on screen, which is why the assertion is a grid and not a pixel. The
 * engine reflows through whatever columns it is given, and on the **alt screen** a resize is a re-fit
 * rather than a reflow (#567) — rows are dropped with nothing to restore them from, so a pane hidden
 * while running a full-screen TUI does not come back.
 */
test("a zero box leaves the grid alone, while a measured tiny one still re-grids (#810)", async ({
  page,
}) => {
  const p = await page.evaluate(() => window.__zeroBoxFitProbe!());

  // Asserted, not assumed: if the terminal were already at the floor, every claim below would hold
  // vacuously.
  expect(p.before.cols).toBeGreaterThan(2);

  expect(p.afterZeroBox, "a box with no area is not measured, so nothing is proposed").toEqual(
    p.before,
  );

  // The control. Same call, same terminal, a box that IS measured — it must re-grid, or the guard
  // has been widened into "any small box refuses" and the zero above proves nothing.
  expect(p.afterTinyBox).toEqual({ cols: 2, rows: 1 });
});
