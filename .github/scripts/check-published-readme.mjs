// Fail a publish whose README still makes a claim that expires.
//
// Every publish snapshots a README into a registry: crates.io and npm both copy it at publish time
// and it becomes the package's front page — the first thing a new consumer reads. Nothing gates it
// on the way there. `justerm-renderer/README.md` announced "Under construction … this is the
// scaffold (#259) … the GPU pipeline lands in #260+" through SIX published `renderer-v*` tags, so
// anyone who found the package read that the renderer draws nothing.
//
// The class this catches is narrow on purpose: a **maturity claim**, i.e. a sentence that is true
// when written and silently false later. It is checked at publish rather than on every PR because
// that is exactly when it turns: an in-progress crate may honestly call itself a scaffold in the
// repo, but publishing that sentence to a registry ships a lie with a version number on it.
//
// Deliberately NOT checked here: prose accuracy (a machine cannot judge it) and the constants a
// README quotes — those are pinned by a unit test next to the crate that owns them, where they run
// on every PR (see justerm-wasm-decode/tests/readme_pins.rs).
//
// Usage: node .github/scripts/check-published-readme.mjs <path-to-README>

import { readFileSync } from "node:fs";

const path = process.argv[2];
if (!path) {
  console.error("::error::usage: check-published-readme.mjs <path-to-README>");
  process.exit(2);
}

// Each pattern is a claim that dates itself. Keep this list tight — a gate that cries wolf is a
// gate people learn to ignore, and this one fires at the worst possible moment (tag already pushed).
const EXPIRING = [
  [/under construction/i, 'a "under construction" banner'],
  [/\bthis is the scaffold\b/i, 'a "this is the scaffold" status'],
  [/\blands in #\d+/i, 'a "lands in #N" promise'],
  [/\bnot yet implemented\b/i, 'a "not yet implemented" note'],
  [/\bcoming soon\b/i, 'a "coming soon" note'],
  [/\bwork in progress\b/i, 'a "work in progress" banner'],
];

let text;
try {
  text = readFileSync(path, "utf8");
} catch (e) {
  console.error(`::error::cannot read ${path}: ${e.message}`);
  process.exit(2);
}

const hits = [];
for (const [re, what] of EXPIRING) {
  const m = text.match(re);
  if (!m) continue;
  const line = text.slice(0, m.index).split("\n").length;
  hits.push({ what, line, quote: m[0] });
}

if (hits.length === 0) {
  console.log(`${path}: no expiring maturity claims`);
  process.exit(0);
}

for (const h of hits) {
  console.error(
    `::error file=${path},line=${h.line}::${path}:${h.line} still carries ${h.what} ("${h.quote}") — ` +
      `this publish would snapshot it onto the package's registry front page. Update the README to ` +
      `describe what actually ships, then re-tag.`,
  );
}
process.exit(1);
