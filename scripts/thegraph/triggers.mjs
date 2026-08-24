#!/usr/bin/env node
// thegraph `verify` guard — justerm. Built from docs/agents/thegraph.md · thegraph stamp 89b477a (kihyun-skills).
//
// Decides whether a diff touches a SACRED path, which is what makes the completeness pass mandatory
// regardless of the enumeration-risk judgement, and what buys the second (refuting) lens.
//
// Usage:  node scripts/thegraph/triggers.mjs [<git-diff-range>]      # default: master...HEAD
//         node scripts/thegraph/triggers.mjs --files a.rs b.ts       # explicit list
//
// justerm has no money path, no production mutation and nothing destructive. Here a path is sacred
// when it is IRREVERSIBLE (already published — crates.io and npm are immutable, and nothing but a
// yank comes back) or SILENT (a wrong answer with no crash, user-visible state quietly corrupted).

import { spawnSync } from "node:child_process";

// ── the guards ───────────────────────────────────────────────────────────────────────────────────
//
// EVERY ENTRY IS A GLOB. None is a call-site list, and that is not a style choice.
//
// Three artifacts once each hand-wrote a DIFFERENT set of sites affected by the alt-screen floor,
// and on 2026-07-28 all three were wrong against the code — the first-ever discovery was missing
// from two of them while its issue number sat in the same paragraph. A guard must be NAMED or it
// guards nothing; a site enumeration must be DERIVED or it rots. So the guard here matches paths,
// and the sites are produced by the grep that
// docs/map/invariant/alt-screen-buffer-floor.md carries.

const SACRED = [
  {
    name: "wire",
    why: "crates.io and npm are immutable. A consumer decoding a wrong layout gets garbage cells, not an error.",
    globs: [
      /^justerm-core\/src\/serialize\.rs$/,
      // A WIRE_VERSION bump reaches these even when serialize.rs is untouched.
      /^justerm-core\/src\/lib\.rs$/,
      /^justerm-wasm-decode\/src\//,
      /^justerm-web\/types\.ts$/,
    ],
    // A content trigger as well as a path one: the version constant can move in a file the globs
    // above do not name.
    contentPattern: /WIRE_VERSION/,
  },
  {
    name: "release",
    why: "Publishing is tag-driven and automatic: pushing vX.Y.Z ships to both registries with no confirmation step.",
    globs: [/^\.github\/workflows\/publish-.*\.ya?ml$/, /^\.github\/scripts\/check-published-readme\.mjs$/, /^docs\/agents\/release\.md$/],
  },
  {
    name: "alt-screen absolute index",
    why: "On the alt screen an unfloored index reads the wrong region and returns PLAUSIBLE text — no error anywhere.",
    globs: [
      /^justerm-core\/src\/term\.rs$/,
      /^justerm-core\/src\/term\//,
    ],
    // Not a site list — a smell in the DIFF. Every miss so far was a fresh unfloored walk that
    // never mentioned the helper, so searching for the helper's name cannot find them.
    contentPattern: /scrollback\.len\(\)|abs_floor/,
  },
];

// ── collect the changed files ────────────────────────────────────────────────────────────────────

const argv = process.argv.slice(2);
let files;
let range = null;

if (argv[0] === "--files") {
  files = argv.slice(1);
} else {
  range = argv[0] ?? "master...HEAD";
  const r = spawnSync("git", ["diff", "--name-only", range], { encoding: "utf8" });
  if (r.status !== 0) {
    console.error(`triggers: \`git diff --name-only ${range}\` failed — ${(r.stderr || "").trim()}`);
    process.exit(2);
  }
  files = r.stdout.split("\n").map((s) => s.trim()).filter(Boolean);
}

if (files.length === 0) {
  // An empty diff is not "clean" — it is a guard that had nothing to evaluate, and the two look
  // identical in a summary. Say which happened.
  console.log(`NO FILES in ${range ?? "the given list"} — the guard did not evaluate. This is not a pass.`);
  process.exit(2);
}

// ── evaluate ─────────────────────────────────────────────────────────────────────────────────────

const hits = [];

for (const s of SACRED) {
  const matched = files.filter((f) => s.globs.some((g) => g.test(f)));
  let contentHit = null;
  if (s.contentPattern && range) {
    const d = spawnSync("git", ["diff", "-U0", range, "--", ...files], { encoding: "utf8" });
    const added = (d.stdout || "").split("\n").filter((l) => l.startsWith("+") && !l.startsWith("+++"));
    if (added.some((l) => s.contentPattern.test(l))) contentHit = String(s.contentPattern);
  }
  if (matched.length || contentHit) hits.push({ s, matched, contentHit });
}

console.log(`files considered: ${files.length}${range ? `  (${range})` : ""}`);

if (hits.length === 0) {
  console.log("\nNo sacred path touched. The completeness pass is gated on enumeration risk as usual,");
  console.log("and the refuting second lens is NOT bought — it exists only for the paths below.");
  console.log("\nGuards evaluated (a guard that matches nothing is indistinguishable from a missing");
  console.log("guard unless it is named, so they are named here):");
  for (const s of SACRED) console.log(`  · ${s.name}`);
  process.exit(0);
}

console.log("\nSACRED PATH TOUCHED — the completeness pass is MANDATORY, and so is the refuting lens.");
for (const { s, matched, contentHit } of hits) {
  console.log(`\n▸ ${s.name}`);
  console.log(`  why: ${s.why}`);
  for (const f of matched) console.log(`  path: ${f}`);
  if (contentHit) console.log(`  added lines match: ${contentHit}`);
}
console.log(`\nSite enumeration is NOT this script's job and never will be — for the alt-screen`);
console.log(`floor, derive the call sites with the grep in`);
console.log(`docs/map/invariant/alt-screen-buffer-floor.md rather than trusting any list.`);
process.exit(1);
