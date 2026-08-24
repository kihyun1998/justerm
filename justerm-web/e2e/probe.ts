import type { Page } from "@playwright/test";

/**
 * Where {@link readAsyncProbe} parks a probe's outcome for the harvest to pick up.
 *
 * Declared here rather than in a spec because the slot is the *mechanism's*, not any one suite's:
 * two spec files each declaring it is two declarations of one global, and the second page to be
 * written is the one that would get it subtly wrong.
 */
declare global {
  interface Window {
    __probeSettled?: { ok: true; value: unknown } | { ok: false; error: string };
  }
}

/**
 * #731 — **start an async probe in one `evaluate`, park its outcome on `window`, harvest that.
 * Never hand `awaitPromise` a promise nothing keeps reachable.**
 *
 * `await page.evaluate(() => window.__someProbe!())` asks Chromium for
 * `Runtime.callFunctionOn({ awaitPromise: true })` on a promise the page no longer names. On
 * 2026-08-05 that handler came back `{"code":-32000,"message":"Promise was collected"}` — and
 * playwright rewrites *every* protocol error that is neither a JS exception nor a closed session
 * into "Execution context was destroyed, most likely because of a navigation"
 * (`playwright-core@1.61.1`, `rewriteError` at `coreBundle.js:35099`). Nothing had navigated, which
 * is what made it expensive: the reported cause was not the cause.
 *
 * **The obvious rule — "never await across CDP" — is wrong.** `waitForFunction` is *also*
 * `awaitPromise: true`, as is every `expect(locator).toBeVisible()`; a rule forbidding that forbids
 * the harness. What differs is **reachability**: the poller below is returned
 * `returnByValue: false`, so playwright holds it by `objectId` and passes that objectId back as the
 * argument it awaits. A probe's promise returned straight out of `evaluate` has no such anchor.
 *
 * Rejections are parked too, so a probe's own throw is reported as a rejection rather than as a
 * harvest that never arrives.
 *
 * **Extracted from `e2e/demo.spec.ts` by #776**, which added the second spec file. It was a private
 * function there for as long as there was one suite; a second suite copying it would be two copies
 * of a rule whose whole point is that it is easy to get wrong — and `test/e2e-async-probe-shape.ts`
 * guards the *shape*, not the helper, so a divergent copy would pass every check. The extraction is
 * verbatim apart from the type parameter: the union of probe names lives with the `Window`
 * declaration in each spec, so each wraps this with a two-line alias that recovers its own return
 * types.
 *
 * Serialisation is unchanged by the extraction and was measured across the same boundary when the
 * park-and-harvest shape was introduced: the harvest comes back through `jsonValue()` where the old
 * shape came back through `evaluate`'s own return, and `NaN`, `±Infinity`, `-0`, `undefined`-valued
 * keys, `null` and numbers past `MAX_SAFE_INTEGER` all round-trip byte-identically in
 * `playwright-core@1.61.1`, which routes both through the same `parseEvaluationResultValue`.
 *
 * @param name the `window.__*` hook to run. It must return a promise; a synchronous probe is read
 *   with a plain `evaluate` and needs none of this.
 * @param timeout the harvest budget. Defaults to 15s — three times the slowest probe measured in
 *   this repo, and under half the 30s test timeout, so a probe that never settles fails with THIS
 *   call named rather than as a bare "test timeout exceeded".
 */
export async function readAsyncProbe<T>(page: Page, name: string, timeout = 15_000): Promise<T> {
  await page.evaluate((n) => {
    delete window.__probeSettled;
    const probe = (window as unknown as Record<string, unknown>)[n] as
      | (() => Promise<unknown>)
      | undefined;
    // Throw the message the old shape threw. `window.__xProbe!()` on a missing probe said
    // "window.__xProbe is not a function"; binding it to a local first says only "probe is not a
    // function", which drops the one word that identifies it.
    if (typeof probe !== "function") throw new Error(`window.${n} is not a function`);
    void probe().then(
      (value) => {
        window.__probeSettled = { ok: true, value };
      },
      (error) => {
        window.__probeSettled = { ok: false, error: String(error) };
      },
    );
  }, name);

  const handle = await page.waitForFunction(() => window.__probeSettled, null, { timeout });
  const settled = await handle.jsonValue();
  await handle.dispose();
  // `waitForFunction` only resolves on a truthy value, so this cannot fire — but the slot is
  // optional and saying so here is cheaper than an assertion that hides which half went wrong.
  if (!settled) throw new Error(`${name} harvested an empty slot`);
  if (!settled.ok) throw new Error(`${name} rejected in the page: ${settled.error}`);
  return settled.value as T;
}
