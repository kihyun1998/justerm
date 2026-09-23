import type { Page } from "@playwright/test";

/**
 * Where {@link readAsyncProbe} parks a probe's outcome for the harvest to pick up. Declared once,
 * here, for every spec.
 */
declare global {
  interface Window {
    __probeSettled?: { ok: true; value: unknown } | { ok: false; error: string };
  }
}

/**
 * #731 — **start an async probe in one `evaluate`, park its outcome on `window`, harvest that.**
 * The harvest is a `waitForFunction` poller playwright holds by `objectId`, so the promise it awaits
 * stays reachable; a probe's promise returned straight out of `evaluate` would not
 * (`docs/map/invariant/an-awaited-in-page-promise-needs-an-anchor.md`). Rejections are parked too, so a
 * probe's own throw is reported as a rejection rather than as a harvest that never arrives.
 *
 * The one copy of this for every spec; each wraps it in a two-line typed alias over its own hook
 * names.
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
    // Name the missing hook, as `window.__xProbe!()` would have.
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
