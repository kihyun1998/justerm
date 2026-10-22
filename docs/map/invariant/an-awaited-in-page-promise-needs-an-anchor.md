# Invariant — a promise awaited across the CDP boundary must be reachable from something the driver retains

## The fact

Every playwright page-side call asks Chromium for `Runtime.callFunctionOn({ awaitPromise: true })`.
That is not the hazard — `waitForFunction`, `expect(locator).toBeVisible()` and `jsonValue()` all go
through it, and believing otherwise nearly produced a rule forbidding the harness itself. What
separates a safe await from an unsafe one is **who else can reach the promise**:

| Shape | Who holds the promise | Outcome |
|---|---|---|
| `page.evaluate(() => window.__someAsyncHook())` | v8_inspector's handler, weakly — the page named it once and dropped it | the handler can be lost; the driver is told nothing useful |
| `page.waitForFunction(…)` | playwright's own poller, returned `returnByValue: false` and passed back by `objectId` as the argument it awaits | strongly reachable for the whole wait |

Traced, not reasoned (`DEBUG=pw:protocol`, `playwright-core@1.61.1`, 2026-08-10): a 1.5 s
`waitForFunction` sends `Runtime.callFunctionOn` with `"(h) => h.result"`, `awaitPromise: true`, and
the request stays outstanding the whole wait while the poller lives behind an `objectId` playwright
releases only afterwards.

So the rule is not "do not await". It is: **start the asynchronous work in one call, park its outcome
on `window`, and harvest that** — a property read whose await is over a promise the driver anchors.

## Why it is cross-cutting

Two independent harnesses in two packages, with no shared code, both this family's real-browser proof.
They met the question separately and answered it differently:

| Harness | What it reads out of the page | Shape |
|---|---|---|
| `justerm-web/e2e/demo.spec.ts` | promise-returning `window.__*` hooks (ten at #731) | awaited until #731, now parked via `readAsyncProbe` |
| `justerm-web/e2e/shared-surface.spec.ts` | two hooks on the two-terminal page (#776) | parked from the start, through the shared `justerm-web/e2e/probe.ts — readAsyncProbe` |
| `justerm-renderer/e2e/proofs.spec.mjs` | `window.__proof`, gated on `window.__done` | parked from the start |
| `justerm-renderer/e2e/screen-composited.spec.mjs` | `window.__composited(png)` | awaited until #731 |

Neither could see the other's answer; nothing in either package's gates compares them, and the safe
shape was never written down — it was how `__done` happened to be built. And the consequence is not a
failed assertion but a misdiagnosis (below), paid by whoever believes the message.

## Territories it holds in

- [browser proof harness](../territory/browser-proof-harness.md) — both suites, their in-page hooks,
  and the two guards: `justerm-web/test/e2e-async-probe-shape.test.ts` and
  `justerm-renderer/e2e/harness-shape.test.mjs`. Each derives the async hook names from its own
  package's demo source (`window.__x = async` — 25 in `justerm-web/demo`, 1 in `justerm-renderer/demo`
  at HEAD 7bb31f7), never from a list. Both are structural proxies: they fail when the shape that admits
  the hazard returns, never when the hazard fires.
- [CI & supply chain](../territory/ci-and-supply-chain.md) — `web-e2e` and `renderer-proofs` run the
  suites; a harness that misreports its own failure spends a gate's credibility.
- [widget lifecycle](../territory/widget-lifecycle.md) — context loss, dispose and restore are proven
  only through parked hooks, so this territory's evidence travels the read path this governs.

The sibling [an IME composition is browser-owned state](composition-is-browser-owned-state.md) routes
work here ("a new consumer of composition state owes an e2e control"), so its rule inherits whatever
this read path gets wrong.

Derivable half — the two shapes, in any spec:

```sh
rg -n '\.evaluate(Handle)?\(\s*async' justerm-web/e2e justerm-renderer/e2e   # an async callback
rg -n '=>\s*window\.__\w+\(' justerm-web/e2e justerm-renderer/e2e            # a hook in return position
```

Non-derivable half — which hooks are asynchronous is a property of the demo pages, not the specs, and
no naming convention marks them (the renderer's is not called `*Probe`).

## What a violation looks like

A failure whose stated cause is false. The error names a navigation; the trace shows two document
loads and no third; `Page.*` / `Runtime.*` listeners across the call record zero events; the probe ran
and its result is on `window`. Underneath, visible only under `DEBUG=pw:protocol`:
`{"code":-32000,"message":"Promise was collected"}`, which playwright rewrites — like every protocol
error that is neither a JS exception nor a closed session — into "Execution context was destroyed, most
likely because of a navigation" (`rewriteError` at `coreBundle.js:35099` in `playwright-core@1.61.1`,
Chromium path; the Firefox backend's near-identical copy is the one #731's body cited by mistake).

The tell: the failure is specific to the promise-returning path; a plain `page.evaluate` against the same
page immediately afterwards succeeds.

## Discovery history

| Occurrence | Site | Issue |
|---|---|---|
| 1st | `demo.spec.ts`'s #480 spec failed on CI (run `30979831545`, 2026-08-05) reporting a navigation that never happened; diagnosed after an investigation spent on page lifecycle, and coincidentally silenced by #730's timing change | #731 |
| 2nd | `screen-composited.spec.mjs` awaited `window.__composited`, which awaits `img.decode()`; never seen failing, found by walking the sibling package while fixing the first | #731 |

The second matters: the issue scoped itself to `justerm-web`, and a fix stopping there would have left
the other harness holding the defect.

Measured vs not (#731): measured are the CI failure, the rewrite and the protocol trace. Not
established is that V8 collected the promise: a forced `HeapProfiler.collectGarbage` collected 0 of 4
promise shapes (timer, rAF, async loop, detached resolver), and the pre-fix spec passed 12/12 on a
28-core host, single- and double-navigation, idle and at 4x oversubscription. v8_inspector emits the
same message when the `InjectedScript` holding a pending handler is torn down, so two mechanisms remain
candidates. Parking removes both, which is why the repair does not depend on the answer.

## Where it will recur

1. **A new asynchronous in-page hook.** `window.__x = async …` is one obvious spec line away from the
   unsafe shape.
2. **A third harness in a third package.** Narrowed by #776 (2026-08-24): the web guard enumerates page
   and spec files from the directory, so a new suite inside `justerm-web` is covered on landing
   (measured: the hard-coded version stayed green with a deliberate unanchored read in the new spec),
   and the extracted `probe.ts` is shared and scanned (mutation-confirmed). Unchanged: a new package
   brings its own harness and guard and no knowledge of these.
3. **A rationale that licenses it.** The comment "`page.evaluate` awaits a returned promise, so the
   specs are unchanged" sat in `justerm-web/demo/main.ts`, the file a guard derives hook names from; at
   HEAD it survives only as a quoted retraction there.
4. **A driver upgrade.** The anchoring asymmetry is playwright's implementation, not its contract,
   pinned here at `^1.61.1` in both `package.json`s; a major bump could move it either way.
