# Territory — browser proof harness

## What it is

The two Playwright suites where "it works in a real browser" stops being a claim. `justerm-web`'s
suite drives the real widget over the real published wasm decoder against a real GL context;
`justerm-renderer`'s loads one page per proof and reads pixels back. Both read their evidence out of
the page through `window.__*` hooks the demo pages install, so the demo is not a fixture beside the
harness — it is half of it. CI runs them as the `web-e2e` and `renderer-proofs` jobs.

The defining property: **the harness is an instrument, and a broken instrument reports its own fault
as a property of the code under test.** Every recurring defect here has that shape — a stale console
line answering a poll about a different page, a cold boot read as a timeout, a lost protocol handler
reported as a navigation. None is a wrong assertion; each is a true assertion about the wrong thing.

## Governing decisions

**None.** Nothing governs harness structure. `docs/agents/theflow.md`'s tie-breaker table (a retired
process document) has no row for this layer, which means a reference can corroborate a justerm shape
here but not make one wrong. Its § "Step 4 — proof method per layer" is the operational list it
used to point at, not a record.

## Design model

All from the source and the measurements attributed below; no record states any of it.

- **One navigation per test, and it is `beforeEach`'s** (#733). A second `goto` leaves the first
  document running underneath (its `ResizeObserver`, debounced fit, timers), and a
  `page.on("console")` listener does not reset across navigations, so a log from the page a test is
  leaving can answer a poll about the page it is entering. A test needing a different boot sets
  `bootUrl` through `test.use()`. It is an option rather than a literal `"/"` because a `baseURL`
  cannot carry a query string (`new URL("/", "http://host/?bgAlpha=0.6")` is `http://host/`), so a
  query-string boot could not otherwise be the single navigation.
- **The boot window is machine speed, not a property.** `beforeEach` returns once the control bar
  is visible, before the mount fit's 100 ms debounce is guaranteed to have fired, so a listener
  attached by the test may or may not see the first `[fit] resize` — measured both ways on one
  machine (#653, which read as flaky for three CI runs). A test that watches the boot takes the
  `consoleLines` fixture.
- **A listener that must precede the hook is an `{ auto: true }` fixture.** A fixture a test merely
  declares is set up after `beforeEach` (measured order: auto, `beforeEach`, declared, test).
- **Asynchronous state is parked, then harvested**, never awaited straight out of an `evaluate` (the
  invariant below). The two suites disagreed on this in silence for months. Parking changes no value:
  the harvest returns through `jsonValue()`, and `NaN`, `±Infinity`, `-0`, `undefined`-valued keys,
  `null` and numbers past `MAX_SAFE_INTEGER` round-trip identically in `playwright-core@1.61.1`
  (measured when the shape was introduced, #731).
- **A boot gate is a proxy with a validity condition, or a node the subject emits.** `demo/index.html`
  waits for a control bar that mounts before the probe assignments in `demo/main.ts` — sound only
  because the `justerm-wasm-decode` import between them resolves on the microtask queue, so one
  task-yielding `await` there would break every probe-reading test at once.
  `demo/shared-surface.html` (#776) writes its ready line as the page's last statement, after every
  probe is installed, so it carries no such condition. The second is also xterm.js's shape (it waits
  for `.xterm-rows`).
- **A one-off cost is charged to the widest clock that can hold it** (#735). Playwright bounds
  `page.goto` by the navigation timeout, `expect(locator)` by the 5 s expect timeout, and a
  `beforeAll` by the 30 s test timeout. A browser process's first navigation costs far more than any
  later one, so where a suite takes its browser from decides who pays. `justerm-web` uses the
  worker-scoped `browser`, so the first test would pay under the tightest clock; it warms the process
  in `beforeAll` from a discarded context. The renderer's screen proofs launch a browser per test and
  carry their own `warmUp` under a 60 s budget. The two rejected repairs are the ones that stop the
  gate reporting: a retry runs warm and always passes (both configs set `retries: 0`), and a bigger
  `expect` timeout hides a boot that is genuinely slowing. A separate context is enough: contexts
  share no HTTP cache but share the process, and the process is what is cold (~200 ms uncontended).
- **The boot gate's own timeout is 30 s, not 5 s** (`dc85158`, 2026-08-25), because it asserts
  *that* the app booted, not how fast; every other `expect` keeps 5 s. Measured on that commit: the
  gate timed out once in `web-e2e` on a markdown-only change, at 5.9 s, while three parameterised
  siblings in the same run finished in 1.3–2.1 s; locally the same URL boots in 274 ms median.
  This undercuts #735's stated reason (see Known holes).
- **A hook that asserts nothing fails soft, and its budgets keep it soft.** The warm-up proves
  nothing `beforeEach` does not, so a throw is logged and swallowed. But a hook's slot timeout is
  raced outside the hook body and cannot be caught, and a failed `beforeAll` skips the rest of the
  file, so every await carries an explicit budget: `GOTO_BUDGET_MS + BAR_BUDGET_MS` = 20 s inside the
  30 s slot. A context built by hand off `browser` inherits none of the config's defaults — not
  `baseURL`, not `navigationTimeout`. Sources (playwright 1.61.1): the slot is
  `Promise.race([cb(), running.timeoutPromise])` (`playwright/lib/worker/workerProcessEntry.js:425-428`);
  a failed `beforeAll` skips the rest of the file (`:1795`, `_skipRemainingTestsInSuite`); `workers`
  defaults to 50% of logical cores (`playwright/lib/common/index.js:595`).
- **A second spec file is a second cold browser.** `beforeAll` runs once per file per worker, so a
  new spec inherits no warm-up and needs its own copy; #776's `shared-surface.spec.ts` carries one.
- **A seed runs to a condition, never to a count** (#818). How many rows move the scrollbar thumb
  depends on the fitted row count, which follows the font's cell (ADR-0022), so it differs between a
  workstation and CI; a fixed seed passed locally and timed out on CI. `seedUntilThumb` loops on the
  thumb instead.
- **A present is the unit of cost headless, and `__seedRows` makes one.** The runner's WebGL is
  SwiftShader: a present costs ~60–70 ms there against ~2 ms on a GPU (`__seedRows(n)` headless
  0.55 / 3.3 / 9.2 / 18.2 s for n = 10 / 50 / 150 / 300, headed on a GTX 1050 Ti 0.03 / 0.14 / 0.29 /
  0.52 s). The cost lands after the helper returns, on the next Playwright call: six 150-row tests
  spent it resolving `locator('#term')` (~11 s locally, 17–30 s on CI) against the 30 s test timeout,
  and the three #913 tests failed about half their CI runs from `27c73ce` to `5e607f2`. So the helper
  drives the real `appendTick` but withholds each tick's frame and presents once at the end. The
  trap: a test that needs per-tick frames (scroll ops, a per-row announce) must not seed.
- **On the runner, the demo's live output alone keeps the page mostly busy.** Its 300 ms tick
  presents, and a runner present cost ~180 ms on renderer 0.23 and ~238 ms on 0.24 (4 cores; 128 →
  142 columns after #962, plus #966's band reads). Measured on CI with a temporary probe: the main
  thread was blocked ~57% of the time on 0.23 and ~77% on 0.24, and a 31% dearer present stretched
  the suite ~1.7×. Locally a present is ~60 ms and the page ~20% busy, so none of it reproduces off the
  runner. A test whose assertion depends on which rows are on screen pauses output first
  (`__output(false)`): #429's drag-selected row scrolled away while its steps ran 9 s → 23 s.
- **A caret-blink probe stops the output timer first.** Its sample is `render()` plus a synchronous
  `readPixels`, which waits for every present queued before it; with the timer running a sample took
  ~730 ms against a 600 ms blink. A CI timeline missed the OFF phase in 9 of 30 poll windows with the
  timer running and 0 of 31 with it stopped — why #575 / #592 / #593 failed together on CI only.
  `pollForCaret` polls a state instead of sampling at a fixed offset.
- **A reader that supplies the thing under test cannot fail** (#776). A probe that calls `present()`
  before reading pixels is itself what runs the renderer's deferred post-restore rebuild, so a restore
  proof written that way stayed green with the surface's whole `webglcontextrestored` listener
  deleted. The repair reads inside the event's own turn, with nothing presented by the suite. The flat
  version: an assertion on a coordinate the page recorded holds when the call that would have sent it
  is deleted, so a placement claim must be a pixel at an independently derived point. Both were
  caught by mutation, not reading.
- **A placement proof is laid out so every wrong answer is visible** (#776's two-pane page). No pane
  sits at the origin or fills the canvas (a pane at `y = 0` asserts GL's bottom-origin flip only where
  a sign error is invisible). Panes do not overlap, because a later grid's `clear` replaces what is
  under it (the renderer's `context-loss-grids.html` keeps its rects apart for the same reason). Both
  share one palette and differ only in `defaultBg`, and each differs from the page's checkerboard, so
  one sampled pixel names who painted it; a page background matching a terminal's is the #577
  failure.
- **A size claim stands on a quantity derived from neither the buffer nor the grant.** `cssWidth()`
  is `bufW / dpr` inside the renderer, so `bufW === round(cssW * dpr)` holds for any buffer (measured
  7×5 device px short and green), and the canvas box is written from the same number. The two-pane
  suite reads the stage's CSS box, which the page sizes from its intended layout.
- **A second page gets its own page, not a widening of `demo/main.ts`.** `main.ts`'s probes are
  calibrated against each other (a cursor cell no other probe samples, rows reserved per feature), so
  editing it changes what unrelated assertions mean. At the start of #776 nothing outside `src/` had
  called `TerminalSurface.open`, `JustermRenderer.attach`, `observeViewportRect` or `onDensityChange`.
- **Isolation differs by suite, deliberately.** The web suite takes a fresh context per test. The
  renderer's screen proofs launch a browser per page and burn one navigation first, because **the
  first document a headless Chromium process renders composites garbage** — solid white at
  `deviceScaleFactor != 1`, solid black at 1 — while `readPixels` in the same page is correct.
  Measured independent of canvas size, CSS box, DPR and WebGL; not cured by extra
  `requestAnimationFrame`s, a sleep, a throwaway screenshot or
  `--run-all-compositor-stages-before-draw`; cured by one prior navigation to a real document
  (`about:blank` does not count, observed). Headed Chromium never shows it. With its own browser,
  deleting `warmUp` reddens the first density.
- **A composited proof refuses a uniform region before measuring it, and checks per cell.**
  `toHaveScreenshot`'s stability loop cannot stand in (the garbage frame is stable). A blur metric
  reads solid white as sharp and a coverage metric reads solid black as nothing-drawn, so `isUniform`
  runs first and treats a degenerate (`NaN`) split as uniform. A tone histogram is blind to structure,
  so a claim about *where* the image landed is checked per cell against the drawing buffer's size.
- **A gate and an eyeball are different tools.** `readPixels` reads a buffer the compositor never
  touched; a headless screenshot of a fractional-CSS canvas composites to white. Wanting to look at
  renderer output is a reason to open a real browser.
- **Every pixel proof runs at 1, 1.1, 1.5 and 2.** 1.5 is Windows at 150%, 2 is Retina, and 1.1 is
  110% zoom — the density at which every proof's grid overhung its drawing buffer (#331).
- **The pixel runner asserts two identities on every page that publishes `gridFit`.** Grid equals
  drawing buffer (#331; since #773 an arrangement the page makes through `fitGrid`), and
  `canvas.width` equals the granted buffer (#339) — the one that can fail, since Chromium keeps an
  attribute of 16385 and grants a `MAX_TEXTURE_SIZE` buffer, and `resizeSurface` must adopt the grant.
  Neither is waived on `oversized.html`, which re-fits the surface with the grid as a consumer would.
- **A red proof names its failing checks and prints the page's `measured`** (#791), because several
  pages derive expectations from the host's fonts (#578). WSL Ubuntu 24.04 reproduces CI: #1026's two
  reds came back with CI's `measured` byte-identical (DejaVu Sans Mono). Unset `DISPLAY` first — with
  WSLg's X server gone, ANGLE's SwiftShader fails `xcb_connect` and no page gets WebGL2.
- **A proof never re-derives the device cell, and never sizes geometry from a constant.** Neither
  `cssCellWidth() * dpr` nor `drawingBufferWidth / COLS` recovers the integer the rasteriser
  ink-scans (both misread at `devicePixelRatio !== 1` before #328; `cell_width()` / `cell_height()`
  report it since #331/#335). `cursor.html`'s fixed `letterSpacing(120)` went red at dpr 1.1/1.5 on
  CI only (#374), so `spacingForThickBar` sizes the margin from the measured cell.
- **A page asking "does this ink leave the glyph box" draws its reference at the renderer's sub-pixel
  phase.** The rasteriser bakes at a whole-row top plus an unrounded ascent (`metrics::line_box`), so
  `neighbour-ink.html` and `side-ink.html` compare against the unrounded ascent; #1026 compared a
  fractional-baseline reach against a rounded box and four checks went red on CI only.
- **A window a page requires is a property of the face, so its sweep is too.** `line-box-cell.html`
  needs a size where the ink-scan baseline and xterm's disagree; on DejaVu Sans Mono at dpr 1 none of
  10–20 CSS px does, so the sweep runs to 22. That is the page's precondition, not the mutation's
  catcher.
- **Neither suite adopts a server already on its port** (`reuseExistingServer: false`, #945). A
  listener there may be another worktree's, and adopting it tests that checkout's sources — green
  when the foreign tree behaves the same. It bit on #649 and #945 and was one of #818's candidate
  causes. An occupied port now fails at start with *"is already used"*.
- **Both guards' `codeOnly` reduction is order-sensitive, and both wrong orders pass silently.** A `)`
  inside a string closes the paren balance early; blanking strings naively over prose apostrophes
  pairs them across lines and deletes whole calls. So comment lines go first, and double quotes are
  emptied before single ones. An unbalanced trailing `//` comment would be a loud false positive.
- **The web guard enumerates `demo/*.ts` and `e2e/*.ts` from the directory** (#776); reading two
  filenames was the stale list the guard's own header forbids, and with it a deliberate unanchored
  evaluate in the new spec measured green.
- **`Terminal`'s decisions are unit-tested; its wiring is carried by these proofs.** It owns no
  transport and no GL, so `routeWheel`, `PointerRouter`, `scrollsToBottomOnInput` and
  `rendererNotifyingSink` run against fakes; `attach` needs a DOM, so only the browser reaches it.
- **What bounds the input.** The suites drive pages they own; the outside quantities are the
  runner's speed and fonts, which nothing bounds — hence conditions over counts and budgets over
  retries.

## Code

- `justerm-web/e2e/demo.spec.ts` — the single-terminal suite: `bootUrl`, `consoleLines`,
  `seedUntilThumb`, `GOTO_BUDGET_MS`, `BAR_BUDGET_MS`, the `beforeAll` warm-up and `beforeEach` boot
  gate
- `justerm-web/e2e/shared-surface.spec.ts` · `justerm-web/demo/shared-surface.ts` — the two-terminal
  suite (#776): placement, per-grid independence, one loss and one recovery
- `justerm-web/e2e/probe.ts` — `readAsyncProbe`, the park-and-harvest helper shared by both web
  suites
- `justerm-web/demo/main.ts` — the `window.__*Probe` hooks, `__seedRows`, `__output`, `appendTick`,
  `pollForCaret`
- `justerm-web/test/e2e-async-probe-shape.test.ts` — the web guard: `codeOnly`, `evaluateCalls`,
  `resolvesTo`
- `justerm-web/playwright.config.ts` — `reuseExistingServer`, `retries`
- `justerm-renderer/e2e/proofs.spec.mjs` — `RATIOS`, `runProof`, the `gridFit` identities
- `justerm-renderer/e2e/screen-composited.spec.mjs` — `warmUp`, per-page browser
- `justerm-renderer/e2e/harness-shape.test.mjs` — the renderer guard, same rule, own derivation
- `justerm-renderer/demo/proof.js` · `justerm-renderer/e2e/proof.test.mjs` — `isUniform`,
  `spacingForThickBar`, `fitGrid`, and their unit tests
- `justerm-renderer/playwright.config.mjs` — the 60 s test timeout, `reuseExistingServer`
- `justerm-web/src/terminal.ts` — `Terminal`, whose `attach` only these suites reach

## Reference behaviour

- [How a comparable project structures a Playwright suite's page setup](../../agents/reference-facts.md#how-a-comparable-project-structures-a-playwright-suites-page-setup-733-verified-2026-08-06)
  — xterm.js's suite at the pinned SHA. It converges on one navigation and on parking asynchronous
  state, and cannot arbitrate the fixture question (its tree has no `test.extend`). Where it differs
  — one page reused per file, reset in-page — that is a design proposal, not a finding.

**The first-surface compositing defect, upstream** (not in the fact store). `page.screenshot()` is
CDP `Page.captureScreenshot` (Playwright `screenshotter.ts` / `crPage.ts`), which copies from a surface
the first real navigation has not presented; Chromium names the failure in `page_handler.cc`
("capturing a surface snapshot will stall because the surface is never presented"), crbug 377715191
/ Playwright #33330. Playwright 1.61 already passes `--enable-features=CDPScreenshotNewSurface`
(`chromiumSwitches.ts`), which does not cover the first-surface case — do not rediscover it as the
fix. That the white-vs-black split comes from a default-cleared buffer is a hypothesis.

## Cross-cutting invariants

- [an awaited in-page promise needs an anchor](../invariant/an-awaited-in-page-promise-needs-an-anchor.md)
  — the defining hazard: a promise nothing retains can lose its handler, and the driver reports that
  as a navigation
- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the renderer's suites sit in a crate no `--workspace` command reaches, so every check they get is
  one someone named by hand

## Blast radius

- [CI gates](ci-and-supply-chain.md) — `web-e2e` and `renderer-proofs` are these suites; a harness
  that reports machine speed as a defect spends that gate's credibility
- [widget lifecycle](widget-lifecycle.md) — context loss, dispose and restore are proven here and
  nowhere else; a lifecycle change that a probe's own `present()` happens to repair stays green
- [accessibility](accessibility.md) — announce and signal paths are asserted through what a screen
  reader consumes; a change to the live region's text is a change to the evidence
- [cell geometry](cell-geometry.md) · [glyph atlas](glyph-atlas.md) ·
  [cell compositing](cell-compositing.md) — every renderer proof reads pixels at four ratios and
  assumes the cell is read from the renderer; an absolute cell dimension is not portable to CI's fonts
- [multi-viewport](multi-viewport.md) — the two-pane page is the only place a shared-surface claim is
  proven; a placement change must keep its rects apart and off the origin
- Every territory whose contract is only observable in a browser: the harness is the sole witness, so
  a hole here turns a guarantee into a convention

## Known holes / open

- **#818 — a full `test:e2e` run occasionally fails one test that passes alone.** An observation
  log with no diagnosis; the recurring entry is `shared-surface.spec.ts`'s *"two grids draw on one
  canvas, each in its own rect, with the buffer bare between them"* (#448, #841, PR #943). One
  occurrence was diagnosed as an insufficient budget (PR #822), and the shared-dev-server candidate
  was closed off by #945. New occurrences go there.
- **The awaited-promise hazard does not reproduce locally** (four conditions, 2026-08-10), so both
  guards are structural proxies: they fail when the shape returns, never when the hazard fires.
- **#735's stated reason and the boot gate's 30 s timeout contradict each other**, and nobody has
  decided which stands. #735 rejected a bigger `expect` timeout and warmed the process so the gate's
  5 s would hold; `dc85158` then gave that gate 30 s in both specs. What the warm-up still buys — the
  first test's later 5 s `expect`s — is unmeasured.
- **Which process-local cache the #735 warm-up refills is unpinned.** Holding the dev server warm in
  both arms proved the cost is per browser process (8 of 10 paired reps, median 4865 → 2191 ms), but
  V8's code cache is a candidate, not a finding (167 ms cold vs 186 ms warm). To re-run:
  `addInitScript` wrapping `WebAssembly.{instantiate,compile}{,Streaming}`,
  `getEntriesByType("resource")`, and one pre-warmed vite with interleaved arms and a fresh chromium
  each — the per-invocation design warms both caches. At the gate on a contended 28-core host: hook
  off 10084 ms (pass) and 15277 ms (fail, the #653 shape); on 2928 and 3338 ms.
- **#735's demonstrated exposure is a loaded developer host, not CI.** Four green `web-e2e` runs had
  first-test durations of 517–645 ms; that shared runners are permanently oversubscribed was not
  confirmed, so on CI the warm-up is insurance.
- **The boot gate is a proxy** resting on an import resolving on the microtask queue; the comment
  beside it is the whole defence.
- **Neither guard covers a third suite**, and both derive hook names per package. A new spec file in
  either package also needs its own copy of the #735 warm-up, and nothing warns whoever adds it.
- **The one-navigation rule has a standing exception**: the #914 test in `demo.spec.ts` calls
  `page.goto("/")` in its body, after `beforeEach` has navigated. Nothing flags it.
- **Both guards' `resolvesTo` misses a promise assigned to a local and then returned**; closing it
  means parsing. On the web side `e2e/probe.ts` is the worst case (`return probe().then(` passes every
  general check) and is pinned only by one check over-fitted to that file.
- **Nothing measures whether a proof still proves anything.** A probe answering `NaN` made one spec
  vacuously green for its whole life; the non-vacuity assertion that caught it was added by hand, and
  no rule says the next probe owes one.
