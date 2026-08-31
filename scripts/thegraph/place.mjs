#!/usr/bin/env node
// thegraph `place` guard — justerm. Built from docs/agents/thegraph.md · thegraph stamp bf223be (kihyun-skills).
//
// Matches changed paths against the tree rule in docs/agents/thegraph.md § `place`. The rule is a
// path list and a diff is a path list, so the check is a match — prose alone would leave the node a
// bar with no firing mechanism, which is the exact defect its own argument is against.
//
// Usage:  node scripts/thegraph/place.mjs [<git-diff-range>]      # default: master...HEAD
//         node scripts/thegraph/place.mjs --files a.rs b/c.ts     # a PLANNED list, before writing
//
// Run it twice: at `place` over the paths you intend to write, and again on the final diff. The
// first run is the one that pays — a file in the wrong directory breaks the seam while producing no
// error, no failing test and no warning, so nothing later in the run can observe it.
//
// IT REPORTS; IT DOES NOT ADJUDICATE. Choosing between the tree rule and what a named peer does is
// a judgement and stays on the main thread.

import { spawnSync } from "node:child_process";

// ── the tree rule, as ordered path patterns ──────────────────────────────────────────────────────
//
// First match wins, so the specific entries precede the general ones. `owns` is what the directory
// is FOR — a directory nobody named is a directory nobody owns, and a rule stated as a layer
// ("the engine layer") cannot be matched against a diff at all.

const RULE = [
  // — the shape/Term-half split, declared in the doc-comments at both ends (#587, #601) —
  { re: /^justerm-core\/src\/term\/[^/]+\.rs$/, owns: "the `Term` half — cell-aware logic needing the whole buffer" },
  { re: /^justerm-core\/src\/term\.rs$/, owns: "the module root beside `src/term/`" },
  { re: /^justerm-core\/src\/[^/]+\.rs$/, owns: "the returned shape, and the coordinate model it documents" },

  // — Rust crates —
  { re: /^justerm-(core|wasm-decode|renderer|facade)\/src\//, owns: "crate source, one module per concern" },
  { re: /^justerm-(core|wasm-decode|facade)\/tests\/[^/]+\.rs$/, owns: "integration through the public API, one file per behaviour" },
  // Recorded material is not test source and does not live beside it by accident: .gitattributes
  // pins each kind (*.raw binary, *.golden LF, *.sh LF) because a line-ending conversion rewrites
  // bytes inside a captured stream and the replayed golden breaks (#20).
  { re: /^justerm-core\/tests\/fixtures\/.+\.(raw|golden|sh)$/, owns: "recorded material — captured VT streams, screen-dump goldens, and the capture-*.sh that record them" },
  { re: /^justerm-[^/]+\/tests\/.+\.proptest-regressions$/, owns: "proptest's committed failure corpus, written by the runner" },
  { re: /^justerm-(core|renderer)\/benches\//, owns: "criterion benches" },
  { re: /^justerm-(core|wasm-decode)\/examples\//, owns: "cargo example binaries" },
  { re: /^justerm-wasm-decode\/js\//, owns: "hand-written JS shipped with the binding" },
  { re: /^justerm-wasm-decode\/scripts\//, owns: "the binding package's own tooling" },
  { re: /^justerm-renderer\/(demo|e2e|scripts)\//, owns: "the renderer's browser harness / proofs / tooling" },
  { re: /^justerm-(core|wasm-decode|renderer|facade)\/(Cargo\.toml|Cargo\.lock|README\.md|LICENSE.*|package\.json|pnpm-lock\.yaml|playwright\.config\.mjs)$/, owns: "crate manifest / published README" },
  { re: /^fuzz\//, owns: "cargo-fuzz targets (own [workspace], package `justerm-fuzz`)" },

  // — the TS package —
  { re: /^justerm-web\/src\/.+\.ts$/, owns: "SHIPPED source only — tsconfig.json includes exactly `src`" },
  { re: /^justerm-web\/test\//, owns: "vitest units" },
  { re: /^justerm-web\/e2e\//, owns: "Playwright browser proofs" },
  { re: /^justerm-web\/demo\//, owns: "the runnable browser harness pages" },
  { re: /^justerm-web\/(types\.ts|package\.json|README\.md|LICENSE.*|tsconfig.*\.json|.*\.config\.(ts|mjs)|pnpm-lock\.yaml)$/, owns: "package manifest / wire mirror / config" },

  // — everything that is not a crate —
  { re: /^bench\/[^/]+\//, owns: "a cross-implementation comparison harness belonging to no crate" },
  { re: /^scripts\/thegraph\//, owns: "the scripts this build generates" },
  { re: /^\.claude\/agents\/.+\.md$/, owns: "the agents this build generates" },
  { re: /^\.github\/scripts\//, owns: "CI's scripts" },
  { re: /^\.github\/workflows\//, owns: "CI's workflows" },
  { re: /^docs\/architecture\.md$/, owns: "the authoritative contract spec" },
  { re: /^docs\/adr\/.+\.md$/, owns: "decision records" },
  { re: /^docs\/map\/(README\.md|territory\/|invariant\/)/, owns: "the territory graph" },
  { re: /^docs\/agents\/.+\.md$/, owns: "agent-read bindings and accumulated reference facts" },
  { re: /^docs\/perf\/.+\.md$/, owns: "measurement write-ups (harness in bench/<name>/)" },
  { re: /^teach\//, owns: "the learning-course workspace" },
  { re: /^(CLAUDE|CONTEXT|README)\.md$|^Cargo\.(toml|lock)$|^\.git(ignore|attributes)$|^rustfmt\.toml$/, owns: "repo root" },
  // Claimed rather than left over: an unclaimed path is this script's finding, so plumbing that is
  // nobody's seam has to be named or every diff touching a licence file reports a new top-level area.
  { re: /^(LICENSE-\w+|rust-toolchain\.toml|\.mcp\.json)$|^\.github\/dependabot\.yml$|\/\.gitignore$/, owns: "toolchain and repo plumbing — no seam, named so it is not a finding" },
];

// ── violations: placements that break a seam while breaking nothing visible ──────────────────────
//
// Each one is a rule the tree already keeps, written down so a diff can be checked against it. They
// are separate from RULE because a path can be CLAIMED and still be wrong.

const VIOLATIONS = [
  {
    re: /^justerm-web\/src\/.*\.(test|spec)\.ts$/,
    say: "a test under `<pkg>/src`. tsconfig.json includes exactly `src` and that is the gate making\n" +
         "       `process`/`Buffer` a compile error in shipped code — co-locating dissolves it and breaks\n" +
         "       no test. Units go in justerm-web/test/, browser proofs in justerm-web/e2e/.",
  },
  {
    re: /^justerm-renderer\/tests\//,
    say: "justerm-renderer has no `tests/` directory, and not by preference: webgl.rs and\n" +
         "       rasterizer.rs are wasm32-only and 0-compile on host, so a host test target cannot\n" +
         "       build. Pure modules test inline (26 of 28 do); the GL layer is proven in e2e/.",
  },
  {
    re: /^docs\/research\//,
    say: "UNCLASSIFIED, not a violation — read the last row of the divergence list before adding\n" +
         "       here. docs/research/ and docs/agents/reference-facts.md hold the same kind of thing\n" +
         "       (prior art read from real source, cited file:line) and nothing has decided which is\n" +
         "       the rule. `reference` routes only to reference-facts.md, so a survey filed here is\n" +
         "       invisible to the path that exists to stop an agent starting from a blank tree.",
    soft: true,
  },
];

// ── collect the paths ────────────────────────────────────────────────────────────────────────────

const argv = process.argv.slice(2);
let files;
let range = null;

if (argv[0] === "--files") {
  files = argv.slice(1);
} else {
  range = argv[0] ?? "master...HEAD";
  const r = spawnSync("git", ["diff", "--name-only", range], { encoding: "utf8" });
  if (r.status !== 0) {
    console.error(`place: \`git diff --name-only ${range}\` failed — ${(r.stderr || "").trim()}`);
    process.exit(2);
  }
  files = r.stdout.split("\n").map((s) => s.trim()).filter(Boolean);
}

if (files.length === 0) {
  // An empty list is not "every path is placed" — it is a guard that had nothing to evaluate, and
  // the two read identically in a summary.
  console.log(`NO FILES in ${range ?? "the given list"} — the rule did not evaluate. This is not a pass.`);
  process.exit(2);
}

// ── evaluate ─────────────────────────────────────────────────────────────────────────────────────

const placed = [];
const unclaimed = [];
const flagged = [];

for (const f of files) {
  const v = VIOLATIONS.find((x) => x.re.test(f));
  if (v) flagged.push({ f, v });
  const hit = RULE.find((r) => r.re.test(f));
  if (hit) placed.push({ f, owns: hit.owns });
  else if (!v) unclaimed.push(f);
}

console.log(`paths considered: ${files.length}${range ? `  (${range})` : ""}`);
console.log(`placed by the tree rule: ${placed.length}`);

if (flagged.length) {
  console.log("\n── FLAGGED ─────────────────────────────────────────────────────────────────────");
  for (const { f, v } of flagged) console.log(`  ${v.soft ? "?" : "!"} ${f}\n       ${v.say}`);
}

if (unclaimed.length) {
  console.log("\n── NOT CLAIMED BY ANY RULE ─────────────────────────────────────────────────────");
  for (const f of unclaimed) console.log(`  · ${f}`);
  console.log("\nA path the rule does not claim is one of two things, and only a person can say which:");
  console.log("  · the change needs a NEW TOP-LEVEL AREA — that is `place`'s out-edge to `decide`;");
  console.log("  · the rule is stale and the build needs re-grilling — write it to `build_gaps`,");
  console.log("    naming which slot you substituted judgement for.");
  console.log("Either way it does not get resolved by widening this list in passing.");
}

const soft = flagged.filter((x) => x.v.soft).length;
const hard = flagged.length - soft;
if (!hard && !unclaimed.length) {
  // Do not say "none is flagged" while a `?` sits above it. A soft flag is a surfaced
  // unclassified difference — visible on purpose, and a summary that erases it is a false pass.
  if (soft) {
    console.log(`\nEvery path is claimed. The ${soft} soft flag(s) above are surfaced, not failures:`);
    console.log("a difference nobody has decided stays visible rather than being resolved by majority.");
  } else {
    console.log("\nEvery path is claimed and none is flagged.");
  }
  console.log("Note what this does NOT say: it matches paths, so a file in the right directory and");
  console.log("wrong in every other way passes.");
  process.exit(0);
}
process.exit(1);
