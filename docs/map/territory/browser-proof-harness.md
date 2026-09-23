# Territory — browser proof harness

## What it is

The two Playwright suites that are this family's **Step-4 proof** — the place where "it works in a
real browser" stops being a claim. `justerm-web` drives the real widget over the real published wasm
decoder against a real GL context; `justerm-renderer` loads a page per proof and reads pixels back.
Both read their evidence out of the page through `window.__*` hooks the demo installs, so the demo is
not a fixture beside the harness — it *is* half of it.

The territory's defining property is that **the harness is an instrument, and a broken instrument
reports its own fault as a property of the code under test**. Every recurring defect here has that
shape: a stale console line answering a poll about a different page, a cold boot read as a timeout, a
lost protocol handler reported as a navigation. None of them is a wrong assertion; each is a true
assertion about the wrong thing.

## Governing decisions

Nothing governs harness structure. The tie-breaker table in `docs/agents/theflow.md` carries no row
for this layer, which is a recorded answer rather than an omission — it means a reference cannot make
a justerm shape here wrong, only corroborate one.

## Design model

- **One navigation per test, and it is `beforeEach`'s** (#733). A second `goto` leaves the first
  document running underneath — its `ResizeObserver`, its debounced fit, its timers — and a
  `page.on("console")` listener does not reset across navigations, so a log belonging to the page a
  test is leaving can answer a poll about the page it is entering. A test needing a different *boot*
  asks through `test.use()`. That is why `bootUrl` is an **option** rather than a literal `"/"`: a
  `baseURL` cannot carry a query string (`new URL("/", "http://host/?bgAlpha=0.6")` is
  `http://host/`), so a query-string boot could not *be* the single navigation, and the rule would
  have had three standing exceptions (`?bgAlpha=0.6`, `?bgAlpha=foo`, `?letterSpacing=…`).
- **The boot window is machine speed, not a property.** `beforeEach` returns once the control bar is
  visible, before the mount fit's 100ms debounce is guaranteed to have fired, so a body-attached
  listener may or may not see the first `[fit] resize` — measured both ways on one machine, idle and
  under load. That is what made #653 read as flaky for three CI runs while every local run passed; a
  test that watches the boot takes the `consoleLines` fixture instead.
- **A listener that must precede the hook is an `{ auto: true }` fixture.** A fixture the test merely
  *declares* is set up **after** `beforeEach` — measured, `["auto", "beforeEach", "declared-by-test",
  "test"]` — which would leave it attached exactly as late as one written by hand.
- **Asynchronous state is parked, then harvested**, never awaited straight out of an `evaluate`. See
  the invariant below; this is the one rule the two suites disagreed on, in silence, for months.
  Parking changes no value: the harvest returns through `jsonValue()` where the old shape returned
  through `evaluate`, and `NaN`, `±Infinity`, `-0`, `undefined`-valued keys, `null` and numbers past
  `MAX_SAFE_INTEGER` round-trip byte-identically in `playwright-core@1.61.1`, which routes both
  through `parseEvaluationResultValue` (measured when the shape was introduced).
- **A boot gate is either a proxy with a validity condition, or a node the subject emits.** Both
  shapes are in the tree and the difference is worth knowing before writing a third. `demo/index.html`
  waits for a control bar that mounts ~350 lines before the probe assignments — sound only because
  the `justerm-wasm-decode` import between them resolves on the microtask queue, so one
  task-yielding `await` after the bar mounts would break every probe-reading test at once.
  `demo/shared-surface.html` (#776) instead writes its ready line as the page's **last statement**,
  after both terminals are mounted and every probe is installed, so it carries no such condition. The
  second is the shape xterm.js reached too (it waits for `.xterm-rows`, a node the terminal renders);
  the first is not wrong, it is a proxy whose soundness someone has to keep true.
- **A one-off cost is charged to the widest clock that can hold it** (#735). Playwright bounds three
  different things with three different budgets: `page.goto` by the navigation timeout, an
  `expect(locator)` by the **5s expect timeout**, a `beforeAll` hook by the **30s test timeout**. The
  first navigation of a browser process costs far more than every later one — so *where a suite takes
  its browser from decides who pays*. `justerm-web` takes the worker-scoped `browser` fixture, so one
  process serves every test and the cost lands on the **first test**, the one under the tightest
  clock; it therefore warms the process in `beforeAll`, from a context it discards. The renderer's
  screen proofs launch a browser **per test** (`screen-composited.spec.mjs`) and so pay it every
  time — which is why they already carry a `warmUp`, reached from the compositing symptom rather than
  from a budget, and why they are not exposed: their budget is a 60s test timeout, not 5s.
  The two rejected repairs are the ones that make the gate stop reporting: a retry runs against an
  already-warm process so it always passes, and a bigger `expect` timeout hides a boot that is
  genuinely slowing. A separate, discarded context is enough, counter-intuitively — contexts share no
  HTTP cache, but they share the process, and the process is what is cold. Uncontended it costs one
  navigation, ~200ms. **The boot gate itself no longer meets that 5s clock** — it took the second
  rejected repair, below — so this reasoning now holds only for the first test's *later* `expect`s;
  see Known holes.
- **The boot gate's own timeout is 30s, not the 5s default**, because it asserts *that* the app
  booted, not how fast; every other `expect` keeps 5s, being a claim about behaviour where a slow one
  is worth seeing. Measured on `dc85158` (2026-08-25): the gate timed out once in `web-e2e` on a
  markdown-only commit, at 5.9s, while its three parameterised-boot siblings in the same run finished
  in 1.3/1.4/2.1s including assertions — one boot stalled, the run was not slow. Locally the same URL
  boots in 274ms median / 544ms worst of 8.
- **A hook that asserts nothing fails soft — and its budgets, not its `catch`, are what keep it
  soft.** The warm-up proves nothing `beforeEach` does not prove again, per test, with a better
  message, so a throw in it is logged and swallowed. But a hook's slot timeout is raced *outside* the
  hook body and cannot be caught, and a failed `beforeAll` skips the rest of the file — the same
  failure mode it exists to remove, one budget up. So every await in such a hook needs an explicit
  budget summing under the slot, the more so because a context built by hand off `browser` inherits
  none of the config's defaults — not `baseURL`, and not `navigationTimeout`, so an unbudgeted `goto`
  would take playwright's own 30s and blow the slot alone. The #735 hook's `GOTO_BUDGET_MS +
  BAR_BUDGET_MS` = 20s leaves ~10s for `newContext` / `newPage` / `close`. Sources (playwright
  1.61.1): the slot is `Promise.race([cb(), running.timeoutPromise])`
  (`playwright/lib/worker/workerProcessEntry.js:425-428`); a failed `beforeAll` skips the rest of the
  file (`:1795`, `_skipRemainingTestsInSuite`); `workers` defaults to 50% of logical cores
  (`playwright/lib/common/index.js:595`).
- **A seed runs to a condition, never to a count** (#818). How many output rows move the scrollbar
  thumb to a given pixel depends on the fitted row count, which follows the cell — the font's ink box
  (ADR-0022) — so it differs between a workstation and CI. A fixed seed passed locally and timed out
  on CI. No absolute cell dimension is portable.
- **A reader that supplies the thing under test cannot fail** (#776). The sharpest form found so
  far: a probe that calls `present()` before it reads pixels is *itself* what runs the renderer's
  deferred post-restore rebuild — so a restore proof written that way stayed green with the surface's
  entire `webglcontextrestored` listener deleted. The repair is to read inside the event's own turn,
  with nothing presented by the suite, which works because listeners fire in registration order and
  the drawing buffer is still intact within the task. The same slice found the flat version of it: an
  assertion on a coordinate the *page recorded* holds when the call that would have sent it is
  deleted, so a placement claim has to be a pixel at an independently derived point. Both were caught
  by mutation and neither by reading; assume a third and mutate.
- **A placement proof is laid out so that every wrong answer is visible** (#776's two-pane page).
  Neither pane sits at the origin or fills the canvas — a sole tenant at `(0, 0)` exercises no
  coordinate, and a pane at `y = 0` would assert GL's bottom-origin flip only at the one value where a
  sign error is invisible. The panes do not overlap: grids paint in registration order and a later
  grid's `clear` *replaces* what is under it, so every per-pane pixel claim would report the topmost
  grid (the renderer's `context-loss-grids.html` keeps its four rects apart for the same reason).
  Both panes share one ANSI palette and differ only in `defaultBg`, and each background differs from
  the other and from the page's checkerboard, so one sampled pixel names who painted it — a page
  background matching a terminal's is the #577 failure, green for six slices, and here it would read
  the gutter sample as "one grid spanning both", the opposite conclusion.
- **A size claim stands on a quantity derived from neither the buffer nor the grant.** `cssWidth()`
  is `bufW / dpr` inside the renderer, so `bufW === round(cssW * dpr)` holds for any buffer — measured
  7×5 device px short and green; the canvas element's box is written by `resizeSurface` from
  `cssWidth()`, the same number in DOM shape, and measured green too. The two-pane suite reads the
  **stage's** CSS box, which the page sizes from its intended layout.
- **A second page gets its own page, not a widening of `demo/main.ts`.** `main.ts` accumulated one
  slice at a time and its probes are calibrated against each other (a cursor cell no other probe
  samples, rows reserved per feature); editing it quietly changes what unrelated assertions mean.
  The two-pane page existed because, measured at the start of #776, nothing outside `src/` had
  called `TerminalSurface.open`, `JustermRenderer.attach`, `observeViewportRect` or
  `onDensityChange` — the adapter's `composedSurface === false` branch had never run in a browser.
- **A second page/spec pair is a second cold browser.** `beforeAll` runs once per file per worker,
  `browser` is worker-scoped, and playwright spreads files across workers — so a new spec file
  inherits no warm-up from an existing one and needs its own copy of the #735 hook. That was written
  down as a consequence before it had happened; #776 is it happening.
- **A gate and an eyeball are different tools.** `readPixels` reads a buffer the compositor never
  touched; a headless screenshot of a fractional-CSS canvas composites to white. Neither substitutes
  for the other, and wanting to *look* at renderer output is a reason to open a real browser, not to
  screenshot the headless run.
- **Isolation differs by suite, deliberately.** The web suite takes a fresh context per test; the
  renderer's screen proofs launch a browser per demo and burn one navigation first, because **the
  first document a headless Chromium process renders composites garbage** — solid white at
  `deviceScaleFactor != 1`, solid black at 1 — while `readPixels` in that same page returns the
  correct frame. Measured independent of canvas size, of the CSS box (integer, fractional or unset),
  of the DPR and of WebGL; not cured by ten extra `requestAnimationFrame`s, a 300 ms sleep, a
  throwaway `page.screenshot()` or `--run-all-compositor-stages-before-draw`; cured by one prior
  navigation to a real document anywhere in the process (`about:blank` does not count — observed, no
  source says why). Headed Chromium never shows it. Sharing the pixel runner's browser would hide
  it, because another proof has already warmed that process: with its own browser, deleting `warmUp`
  reddens the first density, and warming per context is redundant — one navigation warms the process.
- **A composited proof refuses a uniform region before measuring it, and checks per cell.**
  `toHaveScreenshot`'s stability loop cannot stand in: the garbage frame is stable, and two identical
  blank frames agree. A blur metric reads solid white as perfectly sharp and a coverage metric reads
  solid black as nothing-drawn-as-expected, so `isUniform` runs first — and treats a degenerate
  (`NaN`) split as uniform, since `NaN >= 0.9` is false. A tone histogram is blind to structure:
  shrink the CSS box to 80% and the surviving pixels are still 50/50, so a claim about *where* the
  image landed is checked per cell and pinned against the drawing buffer's own dimensions.
- **Both guards' `codeOnly` reduction is order-sensitive, and both wrong orders pass silently.** A
  `)` inside a string literal closes the paren balance early and ends the slice before the call it
  should inspect; blanking strings naively over prose that says "playwright's" and "probe's" pairs
  those apostrophes across lines and deletes whole calls. So comment lines go first, and double
  quotes are emptied before single ones — the specs' own strings are double-quoted and hold
  apostrophes (`"a decoration's ruler mark…"`, `"[data-testid='command-live']"`). Trailing `//`
  comments after code are common in the specs and all balance today; an unbalanced one would be a
  loud false positive, not a miss.
- **Every pixel proof runs at 1, 1.1, 1.5 and 2.** 1.5 is Windows at 150% scaling, 2 is Retina, and
  1.1 is browser zoom at 110% — the density at which every proof's grid overhung its drawing buffer
  (#331). A sweep of only the easy ratios proves the easy half of the contract.
- **The pixel runner asserts two identities on every page that publishes `gridFit`.** Grid equals
  drawing buffer (#331): since #773 that is an arrangement the *page* establishes through `fitGrid`,
  so it catches a page that sized its surface from something other than its own grid's cells. And
  `canvas.width` equals `drawingBufferWidth` (#339) — the one that can fail: the attribute is what was
  asked, the buffer is what WebGL granted (Chromium: `canvas.width = 16385` keeps the attribute and
  returns a `MAX_TEXTURE_SIZE` buffer), and `resizeSurface` must adopt the grant. Neither is waived on
  `oversized.html`, the one page exercising a clamp: it re-fits the surface alongside the grid as a
  consumer would, and an opt-out would trade equality for containment exactly where a larger buffer
  most needs to show.
- **A red proof names its failing checks and prints the page's `measured`** (#791). Several pages
  derive their expectations from the host's fonts (#578), so a CI-only red is otherwise
  undiagnosable without reproducing the runner's font stack.
- **A proof never re-derives the device cell, and never sizes its geometry from a constant.**
  Neither `cssCellWidth() * dpr` nor `drawingBufferWidth / COLS` recovers the integer the rasteriser
  ink-scans; both were in use before #328 and misread the buffer at `devicePixelRatio !== 1`, and
  since #331/#335 `cell_width()`/`cell_height()` report it exactly. A margin sized from a constant
  fails on a font that scans differently: `cursor.html`'s fixed `letterSpacing(120)` cleared the cell
  height by ~2 device px and went red at dpr 1.1/1.5 on CI only (#374), so `spacingForThickBar`
  sizes it from the measured cell.
- **Neither suite adopts a server already on its port** (`reuseExistingServer: false`, #945). A
  listener there may be another worktree's, and adopting it tests that checkout's sources — red
  when a probe is missing, and **green** when the foreign tree happens to behave the same, which is
  the case nothing catches. It bit on #649 and again on #945, and was one of #818's candidate causes.
  Until #945 it was on outside CI, and the only guard was `scripts/thegraph/preflight.mjs`'s
  port-owner check, which retired with that script (`42e3a70`) and was not moved anywhere. Now an
  occupied port fails at start with *"is already used"*: stop the listener (your own `pnpm demo`
  included) rather than re-enabling reuse.

## Code

- `justerm-web/e2e/demo.spec.ts` — the single-terminal widget suite, and the source of every
  convention here; its header holds the one-navigation rule
- `justerm-web/demo/main.ts` — where that page's `window.__*Probe` hooks are installed; `pollForCaret`
  is why the blink probes poll a state instead of sampling at a fixed offset
- `justerm-web/e2e/probe.ts` — `readAsyncProbe`, the park-and-harvest helper. A private function
  inside the spec until #776 gave the package a second suite; extracted rather than copied, because a
  rule whose whole point is that it is easy to get wrong should not exist twice. Each suite keeps a
  two-line typed alias over it, since the union of hook names lives beside its own global declaration
- `justerm-web/demo/shared-surface.ts` · `justerm-web/demo/shared-surface.html` ·
  `justerm-web/e2e/shared-surface.spec.ts` —
  the two-terminal suite (#776, Epic #287 S8): two grids on one canvas at two font sizes, proving
  placement, per-grid independence and one-loss-one-recovery. Deliberately thin where `justerm-web/demo/main.ts` is
  accumulated, and the only page whose *page background* is part of an assertion — the canvas area no
  grid was placed over stays transparent, so a sample there separates two rects on one buffer from
  one grid spanning both
- `justerm-web/test/e2e-async-probe-shape.test.ts` — the web guard for the invariant below.
  `codeOnly` reduces a spec to code, `evaluateCalls` balances parentheses, `resolvesTo` decides
  return position. It **enumerates `demo/*.ts` and `e2e/*.ts` from the directory** (#776): reading
  two filenames was the same stale list the guard's own header forbids, one shape up, and with it a
  deliberate unanchored evaluate call in the new spec measured green
- `justerm-renderer/e2e/proofs.spec.mjs` — the per-page pixel proofs, across four device ratios
- `justerm-renderer/e2e/screen-composited.spec.mjs` — the composited-screenshot proofs; `warmUp`
  burns the first navigation
- `justerm-renderer/e2e/harness-shape.test.mjs` — the renderer guard, same rule, own derivation
- `justerm-renderer/e2e/proof.test.mjs` and `justerm-renderer/demo/proof.js` — the pixel helpers every
  proof reads its evidence through, and their unit tests
- `justerm-web/playwright.config.ts` · `justerm-renderer/playwright.config.mjs`
- `docs/agents/theflow.md` §"Step 4 — proof method per layer" is the operational list, and is **not** a
  decision record

## Reference behaviour

`docs/agents/reference-facts.md` §
["How a comparable project structures a Playwright suite's page setup"](../../agents/reference-facts.md) —
xterm.js's suite, read at the pinned SHA. It converges with this territory on one navigation and on
parking asynchronous state, and **cannot arbitrate** the fixture question at all (its tree contains no
`test.extend`). Where it differs — one page reused per test *file*, reset in-page — that is a design
proposal, not a finding.

The section exists at all because a harness question wants `test/`, and the sparse checkout had only
`src`: the reference corpus read as absent when in fact it was simply not checked out.

**The first-surface compositing defect, upstream.** `page.screenshot()` is CDP
`Page.captureScreenshot` (Playwright `screenshotter.ts` / `crPage.ts` — no frame wait, no
BeginFrame), which lands in `GetSnapshotFromBrowser(from_surface: true)` and copies from a surface the
first real navigation has not presented. Chromium names the failure in `page_handler.cc` ("capturing
a surface snapshot will stall because the surface is never presented"), crbug 377715191 / Playwright
#33330. Playwright 1.61 already passes `--enable-features=CDPScreenshotNewSurface`
(`chromiumSwitches.ts`), Chromium's remedy for that class, and it does not cover the first-surface
case — do not rediscover it as the fix. That the white-vs-black split comes from reading a
default-cleared buffer is a hypothesis, not a citation.

## Cross-cutting invariants

- [an awaited in-page promise needs an anchor](../invariant/an-awaited-in-page-promise-needs-an-anchor.md)
  — the defining hazard: a promise nothing retains can lose its handler, and the driver reports that
  as a navigation
- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the renderer's suites sit in a crate no `--workspace` command reaches, so every check they get is
  one someone named by hand

## Blast radius

- [CI & supply chain](ci-and-supply-chain.md) — `web-e2e` and `renderer-proofs` are these suites; a
  harness that reports machine speed as a defect spends that gate's credibility
- [widget lifecycle](widget-lifecycle.md) — context loss, dispose and restore are proven here and
  nowhere else
- [accessibility](accessibility.md) — announce and signal paths are asserted through SR-consumed
  proxies in this suite
- [cell geometry](cell-geometry.md) · [glyph atlas](glyph-atlas.md) ·
  [cell compositing](cell-compositing.md) — every renderer proof is a pixel read at four device
  ratios, and an absolute cell dimension is not portable across the fonts CI has
- Every territory whose contract is only observable in a browser: the harness is the sole witness, so
  a hole here converts a guarantee into a convention

## Known holes / open

- **The hazard the invariant describes does not reproduce locally** (four conditions, 2026-08-10), so
  both guards are **structural proxies**: they fail when the shape returns, never when the hazard
  fires. That bound is written into each guard rather than left to be discovered.
- **Which process-local cache the `#735` warm-up actually refills is unpinned.** Holding the dev
  server warm in both arms proved the cost is per-browser-process (8 of 10 paired reps, median
  4865ms → 2191ms), but the instrument's wasm attribution did not separate them, so V8's code cache
  is a candidate rather than a finding (median 167ms cold vs 186ms warm). The hook works either way;
  a future claim about *why* needs its own measurement. The instrument, so it can be re-run:
  `addInitScript` wrapping `WebAssembly.{instantiate,compile}{,Streaming}` before any page script,
  `getEntriesByType("resource")` for the resource wall, the control-bar locator for the bar. Of two
  designs only the second isolates the caches: (a) one arm per invocation against a fresh vite and a
  fresh chromium — its warm-up warms both caches; (b) one pre-warmed vite for the run, arms
  interleaved, a fresh chromium each. At the gate itself, on a 28-core host under heavy contention
  with only the first test run: hook off → 10084ms (passed) and 15277ms (failed, `Timeout: 5000ms`,
  the shape of #653); on → 2928ms and 3338ms. The issue's own sweep put the cold boot at 4024ms of
  the 5000ms budget, warm boots ~490–909ms.
- **The `#735` exposure that is demonstrated is a loaded developer host, not CI.** The four most
  recent green `web-e2e` runs had first-test durations of 517/601/645/629 ms. The issue's reasoning
  that few-core shared runners are permanently oversubscribed was not confirmed against the run
  history, so the warm-up's value on CI is insurance, not a repair of an observed failure there.
- **The boot gate is a proxy** whose soundness rests on an import resolving on the microtask queue.
  Nothing enforces that; the comment beside it is the whole defence.
- **Neither guard covers a third suite**, and both derive their hook names per package. A new harness
  starts with no check, the same way a new excluded crate starts with no gate. **The `#735` warm-up
  has the same shape one level down**: it lives in `demo.spec.ts` and `browser` is worker-scoped
  while `workers` defaults to 50% of the cores, so a *second spec file in the same package* runs in
  its own worker against its own cold browser and needs its own copy. Nothing warns whoever adds it.
- **`check-map-note.mjs`'s `SRC_ROOTS` does not include any `e2e/`, `test/` or `demo/` tree**, so a
  symbol this note names resolves only because it is written as a full path from the repo root.
- **The #735 warm-up's stated reason and the boot gate's 30s timeout contradict each other, and
  nobody has decided which stands.** #735 rejected "a bigger `expect` timeout" because it hides a
  boot that is genuinely slowing, and warmed the process so the gate's 5s would hold; `dc85158` then
  gave that same gate `timeout: 30_000` (in both specs) on the ground that it asserts *that* the app
  booted. Both mechanisms remain. What the warm-up still buys — the first test's later 5s `expect`s,
  on a process the gate's wait has already warmed — is unmeasured.
- **The one-navigation rule has a standing exception**: the #914 test in `demo.spec.ts` calls
  `page.goto("/")` in its body. Nothing flags it.
- **Both guards' `resolvesTo` misses a promise assigned to a local and then returned**; closing it
  means parsing, and both guards are reductions by design. On the web side `e2e/probe.ts` is the
  worst case of it — the helper dereferences its hook into a local, so `return probe().then(` passes
  every general check — and it is pinned only by one check over-fitted to that file.
- **Nothing measures whether a proof still proves anything.** A probe that answers `NaN` made one
  spec vacuously green for its whole life; the non-vacuity assertion that caught it was added by
  hand, and no rule says the next probe owes one.
