// #731 — the proof specs may not hand `awaitPromise` a promise nothing keeps reachable.
//
// `page.evaluate(() => window.__someAsyncHook())` makes Chromium park a
// `Runtime.callFunctionOn({ awaitPromise: true })` handler on a promise the page no longer names.
// When that handler is lost, playwright reports "Execution context was destroyed, most likely
// because of a navigation" — a sentence about a page lifecycle that never moved. It cost
// justerm-web a full investigation (CI run 30979831545, 2026-08-05).
//
// This harness mostly got it right on its own: `proofs.spec.mjs` waits for `__done` and *then*
// reads `__proof`, which is the safe shape. `screen-composited.spec.mjs` did not, and #731 fixed
// it. The rule now has a check on both sides of the family instead of only on justerm-web's, which
// is the asymmetry that let one of two sibling harnesses drift in the first place.
//
// It is a **proxy** and cannot fail when the hazard fires — only when the shape that admits it
// comes back. The hazard is timing-dependent and did not reproduce on a 28-core host across four
// conditions (2026-08-10); the shape is what recurs.
//
// Run: `node --test e2e/harness-shape.test.mjs` (via `pnpm run test:unit`).
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";

const read = (p) => readFileSync(new URL(p, import.meta.url), "utf8").replace(/\r\n/g, "\n");

/** Every promise-returning `window.__*` hook any demo page installs. Derived, never listed. */
function asyncHooks() {
  const dir = new URL("../demo/", import.meta.url);
  const names = new Set();
  for (const f of readdirSync(dir)) {
    if (!f.endsWith(".html") && !f.endsWith(".js")) continue;
    for (const m of read(`../demo/${f}`).matchAll(/window\.(__\w+)\s*=\s*async\b/g)) names.add(m[1]);
  }
  return [...names];
}

/**
 * Reduce a spec to code the paren balance can trust: comment-only lines dropped, whitespace
 * squashed, string literals emptied — double quotes before single ones, because prose apostrophes
 * inside double-quoted strings would otherwise pair with each other and delete whole calls.
 * A reduction, not a parser; its blind spot is a trailing `//` after code, which fails loud.
 */
const codeOnly = (src) =>
  src
    .split("\n")
    .filter((l) => !/^\s*(\/\/|\*|\/\*)/.test(l))
    .join("\n")
    .replace(/\s+/g, "")
    .replace(/"(?:[^"\\]|\\.)*"/g, '""')
    .replace(/'(?:[^'\\]|\\.)*'/g, "''")
    .replace(/`(?:[^`\\]|\\.)*`/g, "``");

/** Every `…evaluate( … )` / `…evaluateHandle( … )` call. Both reach the same `awaitPromise`. */
function evaluateCalls(src) {
  const s = codeOnly(src);
  const out = [];
  for (const needle of [".evaluate(", ".evaluateHandle("]) {
    for (let i = s.indexOf(needle); i !== -1; i = s.indexOf(needle, i + 1)) {
      let depth = 0;
      let j = i + needle.length - 1;
      for (; j < s.length; j++) {
        if (s[j] === "(") depth++;
        else if (s[j] === ")" && --depth === 0) break;
      }
      out.push(s.slice(i, j + 1));
    }
  }
  return out;
}

/**
 * Does this evaluate call **resolve to** `hook`'s promise? Two refinements, both forced by real
 * false positives this check produced against the very fix it guards:
 *
 * - **Return position.** A callback that *starts* the hook and parks its outcome must name it;
 *   `void window.__composited(b64).then(…)` is the repair, not the defect. Only `=> window.__x(`
 *   and `return window.__x(` hand the promise back to `awaitPromise`.
 * - **A word boundary.** `__composited` is a prefix of `__compositedSettled`, so a bare `includes`
 *   flags the harvest that reads the parked slot.
 *
 * The bound, stated because it is real: a callback that assigns the promise to a local and returns
 * *that* escapes. Closing it means parsing.
 */
const resolvesTo = (call, hook) =>
  new RegExp(`(?:=>|return)window\\.${hook}(?![A-Za-z0-9_])`).test(call);

const SPECS = readdirSync(new URL("./", import.meta.url)).filter((f) => f.endsWith(".spec.mjs"));

test("the demo pages declare async hooks and the specs evaluate — both halves found (#731)", () => {
  // Non-vacuity: without this, every assertion below passes by describing nothing.
  assert.ok(asyncHooks().length > 0, "no `window.__x = async` found under demo/");
  assert.ok(SPECS.length > 0, "no *.spec.mjs found next to this file");
  const calls = SPECS.flatMap((f) => evaluateCalls(read(`./${f}`)));
  assert.ok(calls.length > 2, `only ${calls.length} evaluate calls found across ${SPECS.length} specs`);
});

test("no proof spec resolves an async hook inside an evaluate (#731)", () => {
  const hooks = asyncHooks();
  const offenders = SPECS.flatMap((f) =>
    evaluateCalls(read(`./${f}`)).flatMap((call) =>
      hooks.filter((h) => resolvesTo(call, h)).map((h) => `${f}: ${h} in ${call.slice(0, 80)}`),
    ),
  );
  assert.deepEqual(
    offenders,
    [],
    "start the hook in one evaluate, park its outcome, poll for it — see screen-composited.spec.mjs",
  );
});

test("no proof spec passes an async callback to an evaluate (#731)", () => {
  const offenders = SPECS.flatMap((f) =>
    evaluateCalls(read(`./${f}`))
      .filter((c) => /^\.evaluate(Handle)?\(async/.test(c))
      .map((c) => `${f}: ${c.slice(0, 80)}`),
  );
  assert.deepEqual(offenders, [], "an async callback returns a promise with no anchor either");
});
