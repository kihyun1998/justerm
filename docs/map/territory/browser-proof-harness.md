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
- **The boot gate is a proxy and says so.** `justerm-web` waits for a control bar that mounts ~350
  lines before the probe assignments, sound only because the `justerm-wasm-decode` import between
  them resolves on the microtask queue. One task-yielding `await` after the bar mounts breaks every
  probe-reading test at once.
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

- `justerm-web/e2e/demo.spec.ts` — the widget suite; `readAsyncProbe` is the parked-read helper, and
  the file header holds the one-navigation rule
- `justerm-web/demo/main.ts` — where every `window.__*Probe` is installed; `pollForCaret` is why the
  blink probes poll a state instead of sampling at a fixed offset
- `justerm-web/test/e2e-async-probe-shape.test.ts` — the web guard for the invariant below.
  `codeOnly` reduces a spec to code, `evaluateCalls` balances parentheses, `resolvesTo` decides
  return position
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
- **`#735` — the first test of a run pays a cold boot**, and under host contention that was measured
  at 79% of `beforeEach`'s 5s budget. Open, with the fix measured and not yet taken.
- **The boot gate is a proxy** whose soundness rests on an import resolving on the microtask queue.
  Nothing enforces that; the comment beside it is the whole defence.
- **Neither guard covers a third suite**, and both derive their hook names per package. A new harness
  starts with no check, the same way a new excluded crate starts with no gate.
- **`check-map-note.mjs`'s `SRC_ROOTS` does not include any `e2e/`, `test/` or `demo/` tree**, so a
  symbol this note names resolves only because it is written as a full path from the repo root.
- **Nothing measures whether a proof still proves anything.** A probe that answers `NaN` made one
  spec vacuously green for its whole life; the non-vacuity assertion that caught it was added by
  hand, and no rule says the next probe owes one.
