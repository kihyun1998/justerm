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
  asks through `test.use()`.
- **A listener that must precede the hook is an `{ auto: true }` fixture.** A fixture the test merely
  *declares* is set up **after** `beforeEach` — measured, `["auto", "beforeEach", "declared-by-test",
  "test"]` — which would leave it attached exactly as late as one written by hand.
- **Asynchronous state is parked, then harvested**, never awaited straight out of an `evaluate`. See
  the invariant below; this is the one rule the two suites disagreed on, in silence, for months.
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
  genuinely slowing.
- **A hook that asserts nothing fails soft — and its budgets, not its `catch`, are what keep it
  soft.** The warm-up proves nothing `beforeEach` does not prove again, per test, with a better
  message, so a throw in it is logged and swallowed. But a hook's slot timeout is raced *outside* the
  hook body and cannot be caught, and a failed `beforeAll` skips the rest of the file — the same
  failure mode it exists to remove, one budget up. So every await in such a hook needs an explicit
  budget summing under the slot, the more so because a context built by hand off `browser` inherits
  none of the config's defaults.
- **A reader that supplies the thing under test cannot fail** (#776). The sharpest form found so
  far: a probe that calls `present()` before it reads pixels is *itself* what runs the renderer's
  deferred post-restore rebuild — so a restore proof written that way stayed green with the surface's
  entire `webglcontextrestored` listener deleted. The repair is to read inside the event's own turn,
  with nothing presented by the suite, which works because listeners fire in registration order and
  the drawing buffer is still intact within the task. The same slice found the flat version of it: an
  assertion on a coordinate the *page recorded* holds when the call that would have sent it is
  deleted, so a placement claim has to be a pixel at an independently derived point. Both were caught
  by mutation and neither by reading; assume a third and mutate.
- **A second page/spec pair is a second cold browser.** `beforeAll` runs once per file per worker,
  `browser` is worker-scoped, and playwright spreads files across workers — so a new spec file
  inherits no warm-up from an existing one and needs its own copy of the #735 hook. That was written
  down as a consequence before it had happened; #776 is it happening.
- **A gate and an eyeball are different tools.** `readPixels` reads a buffer the compositor never
  touched; a headless screenshot of a fractional-CSS canvas composites to white. Neither substitutes
  for the other, and wanting to *look* at renderer output is a reason to open a real browser, not to
  screenshot the headless run.
- **Isolation differs by suite, deliberately.** The web suite takes a fresh context per test; the
  renderer's screen proofs take a fresh browser per demo and burn the process's first navigation,
  whose composited copy is garbage.
- **`reuseExistingServer` is on outside CI**, so a dev server already listening — from another
  worktree — is silently adopted, and the suite then tests that checkout's sources.

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
  is a candidate rather than a finding. The hook works either way; a future claim about *why* needs
  its own measurement.
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
- **Nothing measures whether a proof still proves anything.** A probe that answers `NaN` made one
  spec vacuously green for its whole life; the non-vacuity assertion that caught it was added by
  hand, and no rule says the next probe owes one.
