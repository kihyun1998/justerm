# Cross-cutting invariant — a promise awaited across the CDP boundary must be reachable from something the driver retains

## The fact

Every one of playwright's page-side calls asks Chromium for
`Runtime.callFunctionOn({ awaitPromise: true })`. **That is not the hazard**, and believing it was is
how this invariant nearly landed as a rule forbidding the harness itself: `waitForFunction`,
`expect(locator).toBeVisible()` and `jsonValue()` all go through it.

What separates a safe await from an unsafe one is **who else can reach the promise**.

| Shape | Who holds the promise | Outcome |
|---|---|---|
| `page.evaluate(() => window.__someAsyncHook())` | v8_inspector's handler, weakly — the page named it once and dropped it | the handler can be lost; the driver is told nothing useful |
| `page.waitForFunction(…)` | playwright's own poller object, returned `returnByValue: false` and passed back **by `objectId`** as the argument it awaits | strongly reachable for the whole wait |

Traced rather than reasoned (`DEBUG=pw:protocol`, `playwright-core@1.61.1`, 2026-08-10): a
`waitForFunction` over a 1.5 s wait sends `Runtime.callFunctionOn` with `"(h) => h.result"`,
`awaitPromise: true`, and that request stays outstanding for the entire wait — while the poller it
reads lives behind an `objectId` playwright releases only afterwards.

So the rule a harness must hold is not *"do not await"*. It is: **start the asynchronous work in one
call, park its outcome on `window`, and harvest that** — an ordinary property read, whose await is
over a promise the driver itself anchors.

## Why it is cross-cutting

**Two independent harnesses, in two packages, with no shared code**, and both are this family's
Step-4 proof — the place where "it works in a real browser" is established. They arrived at the
question separately and answered it differently, which is the shape that makes this an invariant
rather than one suite's convention:

| Harness | What it reads out of the page | Which shape it used |
|---|---|---|
| `justerm-web/e2e/demo.spec.ts` | ten promise-returning `window.__*Probe` hooks | awaited — until #731 |
| `justerm-web/e2e/shared-surface.spec.ts` | two, on the two-terminal page (#776) | parked, from the start — it takes the same helper, now shared as `justerm-web/e2e/probe.ts` |
| `justerm-renderer/e2e/proofs.spec.mjs` | `window.__proof`, gated on `window.__done` | parked, from the start |
| `justerm-renderer/e2e/screen-composited.spec.mjs` | `window.__composited(png)` | awaited — until #731 |

Neither harness could see the other's answer. Nothing in either package's gates compares them, and
the safe one was never written down as a rule — it was simply how `__done` happened to be built.

**And the consequence is not a failed assertion.** The driver reports a protocol error it cannot
interpret as *"Execution context was destroyed, most likely because of a navigation"* — a sentence
about page lifecycle, emitted for a page that never navigated. That is the expensive part: the cost
is paid by whoever reads the message and believes it.

## Territories it holds in

- [browser proof harness](../territory/browser-proof-harness.md) — where both suites and their
  in-page hooks live; this is the fact that territory exists around
- [CI & supply chain](../territory/ci-and-supply-chain.md) — `web-e2e` and `renderer-proofs` are the
  jobs that run them, and a harness that misreports its own failure spends a gate's credibility
- [widget lifecycle](../territory/widget-lifecycle.md) — context loss, dispose and restore are proven
  through two of the parked hooks and nowhere else, so this territory's evidence travels the read
  path this invariant governs

The sibling invariant [an IME composition is browser-owned
state](composition-is-browser-owned-state.md) routes work here in the same way — *"a new consumer of
composition state owes an e2e control, not a unit test"* — so a rule it hands to the harness inherits
whatever the harness's read path can get wrong.

Derivable half — the two shapes, in any spec:

```sh
rg -n '\.evaluate(Handle)?\(\s*async' justerm-web/e2e justerm-renderer/e2e   # an async callback
rg -n '=>\s*window\.__\w+\(' justerm-web/e2e justerm-renderer/e2e            # a hook in return position
```

Non-derivable half — **which `window.__*` hooks are asynchronous**. That is a property of the demo
pages, not of the specs, and the two guards below each derive it from its own package's demo source
rather than from a list. There is no naming convention to lean on: the renderer's async hook is not
called `*Probe`, and nothing enforces that the web's are.

## What a violation looks like

**A failure whose stated cause is false**, on a page that did nothing of the kind. The reported error
names a navigation; the trace shows two document loads and no third; `Page.*` / `Runtime.*` listeners
across the failing call record zero events; the probe ran to completion and its result is still on
`window`. The protocol error underneath — visible only under `DEBUG=pw:protocol` — is
`{"code":-32000,"message":"Promise was collected"}`, and playwright rewrites *every* protocol error
that is neither a JS exception nor a closed session into that one sentence
(`rewriteError`, the Chromium path; the Firefox backend has its own near-identical copy, which is the
one #731's body cited by mistake).

The tell, when you suspect it: the failure is **specific to the promise-returning path**. A plain
`page.evaluate` against the same page, immediately afterwards, succeeds.

## Discovery history

| Occurrence | Site | Issue |
|---|---|---|
| 1st | `e2e/demo.spec.ts`'s `#480` spec failed on CI (run `30979831545`, 2026-08-05) reporting a navigation that never happened. Diagnosed only after a full investigation had been spent on page lifecycle and `goto` ordering — and then *coincidentally* silenced by #730, which changed the probe's timing for unrelated reasons | #731 |
| 2nd | `justerm-renderer/e2e/screen-composited.spec.mjs` awaited `window.__composited`, which itself awaits `img.decode()`. Never observed failing; found by walking the sibling corpus while fixing the first | #731 |

**What the second occurrence is worth is the point of recording both.** It had not bitten, and it was
in a *different package* from the issue — the issue scoped itself to `justerm-web` and named ten
probes. A fix that stopped at the issue's boundary would have left one of the two harnesses holding
the defect, which is the same shape as `abs_floor` being found three times over months.

**What is measured, and what is not.** Measured: the CI failure, the rewrite, and the protocol trace
above. *Not* established: that V8 actually collected the probe's promise. A forced
`HeapProfiler.collectGarbage` collects **0 of 4** promise shapes (timer-resolved, rAF-resolved,
async-function await loop, detached resolver), and the pre-fix spec passes 12/12 on a 28-core host —
single- and double-navigation, idle and at 4x oversubscription. v8_inspector emits the same message
when the `InjectedScript` holding a pending handler is torn down, so there are two candidate
mechanisms and neither is pinned. **Parking the value removes both**, which is why the repair does not
depend on the answer — and why the guards below are honest about being *structural proxies* that
cannot fire when the hazard does.

## Where it will recur

1. **A new asynchronous in-page hook.** Anything shaped `window.__x = async …` is one obvious spec
   line away from the unsafe shape, and the obvious line is the one a reader writes first.
2. **A third harness — in a third *package*.** This narrowed on 2026-08-24 (#776) and it is worth
   being precise about how much. A second suite inside `justerm-web` no longer escapes: its guard
   enumerates the page and spec files from the directory rather than reading two names, so a new
   pair is covered the moment it lands — measured, since the hardcoded version stayed green with a
   deliberate unanchored read sitting in the new spec. The extraction of the helper to
   `justerm-web/e2e/probe.ts` narrowed it again, from the other side: a second suite now shares the
   safe mechanism instead of copying it, and the guard scans that file too, which a mutation
   confirmed is live coverage rather than an aspiration. What is **unchanged** is the original
   hazard: a new *package* brings its own harness, its own guard and no knowledge of these — which
   is exactly how the two existing ones came to disagree in silence.
3. **A rationale that licenses it.** The stale comment `page.evaluate` *"awaits a returned promise, so
   the specs are unchanged"* sat inside `justerm-web/demo/main.ts` — the very file a guard derives
   hook names from — reading as permission. A rule with a live counter-example next to it loses.
4. **A driver upgrade.** The anchoring asymmetry is playwright's implementation, not its contract. It
   is pinned here at `1.61.1`; a major bump is the event that could move it, in either direction.
