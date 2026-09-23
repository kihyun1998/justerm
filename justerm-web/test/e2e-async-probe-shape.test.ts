/**
 * #731 — **no `evaluate` in the e2e suite may hand `awaitPromise` a promise nothing keeps
 * reachable** (`docs/map/invariant/an-awaited-in-page-promise-needs-an-anchor.md`). In practice: an
 * evaluate callback may not be `async`, and may not resolve to one of the demo's promise-returning
 * `window.__*` hooks; those go through `readAsyncProbe`.
 *
 * A structural proxy: it fails when the shape that admits the hazard comes back, never when the
 * hazard fires. It needs no browser, so it runs in `pnpm test`.
 *
 * The hook names and the file sets are **derived from the directories**, never listed — the one thing
 * this check must not do is silently stop covering a probe added after it was written.
 */
import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (rel: string): string =>
  readFileSync(new URL(rel, import.meta.url), "utf8").replace(/\r\n/g, "\n");

/**
 * Every source file in `dir` that `keep` accepts, enumerated from the directory.
 *
 * **Every page's hooks are checked against every e2e module's calls**, rather than pairing a spec to
 * the page it drives: pairing is not statically derivable — a spec picks its page at `goto` time and
 * `test.use({ bootUrl })` moves it — and the union is strictly stronger.
 */
const sourcesIn = (dir: string, keep: (f: string) => boolean): { name: string; src: string }[] =>
  readdirSync(new URL(`../${dir}/`, import.meta.url))
    .filter(keep)
    .sort()
    .map((name) => ({ name: `${dir}/${name}`, src: read(`../${dir}/${name}`) }));

/** Every demo page's module. `.ts` only — the pages themselves are `.html` and declare nothing. */
const demoSources = sourcesIn("demo", (f) => f.endsWith(".ts"));
/**
 * Every e2e module, specs **and** their helpers.
 *
 * `e2e/probe.ts` is in deliberately — a regression there reinstates the hazard for every spec at
 * once — but the general checks cover only its async-callback half; the rest is pinned by the
 * over-fitted `it` below (`docs/map/territory/browser-proof-harness.md` § Known holes).
 */
const e2eSources = sourcesIn("e2e", (f) => f.endsWith(".ts"));

/** The shared helper, by name — the one file the checks above cannot fully cover. */
const PROBE_HELPER = "e2e/probe.ts";

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
 * Deliberate order: comments go first and double quotes are emptied before single ones, or prose
 * apostrophes pair across lines and delete whole calls — a silent pass (see
 * `docs/map/territory/browser-proof-harness.md`). A reduction, not a parser: a trailing `//` comment
 * whose own quotes or parens are unbalanced would produce a loud false positive.
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
 * Does this evaluate call **resolve to** `hook`'s promise? Two refinements:
 *
 * - **Return position.** A callback that *starts* the hook and parks its outcome must name it;
 *   `void window.__x(b64).then(…)` is the fix, not the defect. Only `=> window.__x(` and
 *   `return window.__x(` hand the promise back to `awaitPromise`.
 * - **A word boundary.** `__composited` is a prefix of `__compositedSettled`, so a bare
 *   `includes` flags the harvest that reads the parked slot.
 *
 * Bound: a promise assigned to a local and returned escapes (`docs/map/territory/browser-proof-harness.md`).
 */
const resolvesTo = (call: string, hook: string): boolean =>
  new RegExp(`(?:=>|return)window\\.${hook}(?![A-Za-z0-9_])`).test(call);

describe("the e2e suite never awaits an unanchored in-page promise (#731)", () => {
  const hooks = [...new Set(demoSources.flatMap((f) => asyncHookNames(f.src)))];
  const calls = e2eSources.flatMap((f) =>
    evaluateCalls(f.src).map((call) => ({ file: f.name, call })),
  );

  // Non-vacuity first: both halves of the check must have found their material, or every
  // assertion below passes by describing nothing. This is the failure the check itself can have.
  it("finds the demo pages' async hooks and the e2e modules' evaluate calls", () => {
    // The enumeration found files at all. Without this the two counts below describe nothing, and a
    // moved or renamed folder would read exactly like a codebase with no probes in it.
    expect(demoSources.length, "no demo modules found").toBeGreaterThan(0);
    expect(e2eSources.length, "no e2e modules found").toBeGreaterThan(0);
    expect(hooks.length, "no `__x?: (…) => Promise<…>` found in any demo module").toBeGreaterThan(5);
    expect(calls.length, "no `.evaluate(` found in any e2e module").toBeGreaterThan(20);
    // …and that the two sides talk about the same hooks at all, so a rename cannot empty this.
    const e2eText = e2eSources.map((f) => f.src).join("\n");
    expect(
      hooks.some((h) => e2eText.includes(h)),
      "the e2e modules name none of the demo pages' async hooks",
    ).toBe(true);
  });

  it("resolves no async hook inside an evaluate", () => {
    const offenders = calls.flatMap(({ file, call }) =>
      hooks.filter((h) => resolvesTo(call, h)).map((h) => `${h} in ${file}: ${call.slice(0, 90)}`),
    );
    expect(
      offenders,
      "read these through `readAsyncProbe` — starting the hook and harvesting its parked result " +
        "are two separate evaluates, so the awaited promise is one playwright retains by objectId",
    ).toEqual([]);
  });

  it("the shared helper does not hand back its probe's promise", () => {
    // Over-fitted to one file, deliberately — see `e2eSources`: `resolvesTo` cannot see this
    // file's regression, because the helper never names a hook literally.
    const helper = e2eSources.find((f) => f.name === PROBE_HELPER);
    expect(helper, `${PROBE_HELPER} not found — this check has nothing to say`).toBeDefined();
    const code = codeOnly(helper!.src);
    expect(code, "the helper must not return the probe's promise to `awaitPromise`").toContain(
      "voidprobe().then(",
    );
    expect(code).not.toContain("returnprobe(");
  });

  it("passes no async callback to an evaluate", () => {
    // The other way in: an `async` callback returns a promise whatever its body does, and that one
    // has no anchor either.
    const offenders = calls.filter(({ call }) => /^\.evaluate(Handle)?\(async/.test(call));
    expect(offenders.map(({ file, call }) => `${file}: ${call.slice(0, 90)}`)).toEqual([]);
  });
});
