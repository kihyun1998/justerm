/**
 * #731 — **no `evaluate` in the e2e suite may hand `awaitPromise` a promise nothing keeps
 * reachable.** In practice: an evaluate callback may not be `async`, and may not resolve to one of
 * the demo's promise-returning `window.__*` hooks. Those go through `readAsyncProbe`, which parks
 * the outcome on `window` and harvests it from an object playwright retains by `objectId`.
 *
 * The defect this guards is not one the suite can reproduce on demand. `Runtime.callFunctionOn({
 * awaitPromise: true })` on a promise the page no longer names came back
 * `{"code":-32000,"message":"Promise was collected"}` on CI (2026-08-05, run `30979831545`, the
 * `#480` spec), and playwright rewrote it into "Execution context was destroyed, most likely
 * because of a navigation" — a sentence about page lifecycle, for a failure that had nothing to do
 * with it. Nothing navigated: the trace held two document loads and no third, and `Page.*` /
 * `Runtime.*` listeners across the failing call saw zero events.
 *
 * So the assertion here is **structural, and it is a proxy** — it cannot fail when the hazard
 * fires, only when the shape that admits it comes back. That is deliberate and it is the honest
 * bound on this file: the hazard is timing-dependent (measured 2026-08-10 — a forced
 * `HeapProfiler.collectGarbage` collects 0 of 4 promise shapes, and the pre-fix `#480` spec passes
 * 12/12 on a 28-core host, single- and double-navigation, idle and at 4x oversubscription). What
 * can be pinned is the shape, and the shape is what recurs: a new async probe read the obvious way
 * puts it straight back.
 *
 * Why it lives in `pnpm test` rather than in the e2e suite: both jobs run on every PR, so it is not
 * about reach. It is that a rule about how the suite is *written* needs no browser to check, so it
 * answers in milliseconds and cannot be lost among browser flake.
 *
 * The hook names are **derived from `demo/main.ts`**, never listed here. A hand-kept list is the
 * copy that goes stale, and the one thing this check must not do is silently stop covering a probe
 * added after it was written.
 */
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (rel: string): string =>
  readFileSync(new URL(rel, import.meta.url), "utf8").replace(/\r\n/g, "\n");

const demoSrc = read("../demo/main.ts");
const specSrc = read("../e2e/demo.spec.ts");

/**
 * Every promise-returning `window.__*` hook the demo declares.
 *
 * The `Probe` suffix is deliberately **not** required: nothing enforces that convention, and a hook
 * that returns a promise is the hazard whatever it is called.
 */
const asyncHookNames = (src: string): string[] => [
  ...new Set(
    [...src.matchAll(/(__\w+)\?:\s*\([^)]*\)\s*=>\s*Promise</g)].map((m) => m[1]!),
  ),
];

/**
 * Reduce a spec to something the paren balance below can trust: comment-only lines dropped,
 * whitespace squashed, string literals emptied.
 *
 * Both steps are load-bearing and the **order within them is too**, which is the part that bit
 * while writing this. A `)` inside a string literal would otherwise close the balance early and end
 * the slice before the call it is meant to inspect — a *silent pass*, not the loud false positive
 * an earlier draft of this comment claimed. But blanking naively over a file whose doc-comments say
 * "playwright's" and "probe's" pairs those apostrophes across prose and deletes whole calls, which
 * is a silent pass of its own. So: comments go first, and double quotes are emptied before single
 * ones, because this file's own strings are double-quoted and hold apostrophes
 * (`"a decoration's ruler mark…"`, `"[data-testid='command-live']"`).
 *
 * This is a reduction, not a parser. Its remaining blind spot is a trailing `//` comment on a line
 * that also holds code; nothing here writes one, and the failure would be a loud false positive
 * rather than a miss.
 */
const codeOnly = (src: string): string =>
  src
    .split("\n")
    .filter((l) => !/^\s*(\/\/|\*|\/\*)/.test(l))
    .join("\n")
    .replace(/\s+/g, "")
    .replace(/"(?:[^"\\]|\\.)*"/g, '""')
    .replace(/'(?:[^'\\]|\\.)*'/g, "''")
    .replace(/`(?:[^`\\]|\\.)*`/g, "``");

/**
 * The source text of every `…evaluate( … )` / `…evaluateHandle( … )` call, found by balancing
 * parentheses from each call's opening one over whitespace-squashed, string-blanked source.
 *
 * The needle is the **method**, not `page.evaluate`: `locator.evaluate`, `frame.evaluate`,
 * `handle.evaluate` and `page.evaluateHandle` all reach the same `Runtime.callFunctionOn` with
 * `awaitPromise: true`, and `evaluateHandle` is one token away from the site this change repaired.
 */
const evaluateCalls = (src: string): string[] => {
  const squashed = codeOnly(src);
  const out: string[] = [];
  for (const needle of [".evaluate(", ".evaluateHandle("]) {
    for (let i = squashed.indexOf(needle); i !== -1; i = squashed.indexOf(needle, i + 1)) {
      let depth = 0;
      let j = i + needle.length - 1;
      for (; j < squashed.length; j++) {
        if (squashed[j] === "(") depth++;
        else if (squashed[j] === ")" && --depth === 0) break;
      }
      out.push(squashed.slice(i, j + 1));
    }
  }
  return out;
};

/**
 * Does this evaluate call **resolve to** `hook`'s promise? Two refinements, both forced by real
 * false positives rather than imagined ones — the renderer's copy of this check produced each:
 *
 * - **Return position.** A callback that *starts* the hook and parks its outcome must name it;
 *   `void window.__x(b64).then(…)` is the fix, not the defect. Only `=> window.__x(` and
 *   `return window.__x(` hand the promise back to `awaitPromise`.
 * - **A word boundary.** `__composited` is a prefix of `__compositedSettled`, so a bare
 *   `includes` flags the harvest that reads the parked slot.
 *
 * The bound, stated because it is real: a callback that assigns the promise to a local and returns
 * *that* escapes. Closing it means parsing, and every mutation this rule was built against — and
 * the seven in the PR — goes through one of the two forms above.
 */
const resolvesTo = (call: string, hook: string): boolean =>
  new RegExp(`(?:=>|return)window\\.${hook}(?![A-Za-z0-9_])`).test(call);

describe("the e2e suite never awaits an unanchored in-page promise (#731)", () => {
  const hooks = asyncHookNames(demoSrc);
  const calls = evaluateCalls(specSrc);

  // Non-vacuity first: both halves of the check must have found their material, or every
  // assertion below passes by describing nothing. This is the failure the check itself can have.
  it("finds the demo's async hooks and the spec's evaluate calls", () => {
    expect(hooks.length, "no `__x?: (…) => Promise<…>` found in demo/main.ts").toBeGreaterThan(5);
    expect(calls.length, "no `.evaluate(` found in e2e/demo.spec.ts").toBeGreaterThan(20);
    // …and that the two files talk about the same hooks at all, so a rename cannot empty this.
    expect(
      hooks.some((h) => specSrc.includes(h)),
      "the spec names none of the demo's async hooks",
    ).toBe(true);
  });

  it("resolves no async hook inside an evaluate", () => {
    const offenders = calls.flatMap((call) =>
      hooks.filter((h) => resolvesTo(call, h)).map((h) => `${h} in ${call.slice(0, 90)}`),
    );
    expect(
      offenders,
      "read these through `readAsyncProbe` — starting the hook and harvesting its parked result " +
        "are two separate evaluates, so the awaited promise is one playwright retains by objectId",
    ).toEqual([]);
  });

  it("passes no async callback to an evaluate", () => {
    // The other way in: an `async` callback returns a promise whatever its body does, and that one
    // has no anchor either.
    const offenders = calls.filter((c) => /^\.evaluate(Handle)?\(async/.test(c));
    expect(offenders.map((c) => c.slice(0, 90))).toEqual([]);
  });
});
