#!/usr/bin/env node
// Verify ONE map note — the per-note check that makes "verify as you finish it" affordable.
//
// The batch gate (check-map-links.mjs) runs in CI over the whole tree; this one is for the author,
// mid-write (why: `docs/map/territory/ci-and-supply-chain.md`).
//
//   node .github/scripts/check-map-note.mjs docs/map/territory/selection.md
//
// Checks, in the order they pay off:
//   1. the section set is complete (aggregates are exempt — they own no detail)
//   2. every symbol named under ## Code resolves somewhere in the tree
//   3. nothing restates a value another artifact owns (a record's status)
// Links and anchors are left to the batch gate, which already resolves them across the graph.

import { readFileSync, existsSync } from 'node:fs';
import { extname } from 'node:path';
import { execFileSync } from 'node:child_process';

const file = process.argv[2];
if (!file || !existsSync(file)) {
  console.error('usage: check-map-note.mjs <docs/map/**/note.md>');
  process.exit(2);
}

// Three note kinds, three schemas (aggregates are checked for their "owns no detail" line only).
const TERRITORY_SECTIONS = [
  '## What it is',
  '## Governing decisions',
  '## Design model',
  '## Code',
  '## Reference behaviour',
  '## Cross-cutting invariants',
  '## Blast radius',
  '## Known holes',
];
const INVARIANT_SECTIONS = [
  '## The fact',
  '## Why it is cross-cutting',
  '## Territories it holds in',
  '## What a violation looks like',
  '## Discovery history',
  '## Where it will recur',
];
// The tree is every git-tracked file with one of these extensions, under the working directory.
const SRC_EXT = new Set(['.rs', '.ts', '.mjs', '.yml', '.toml']);

const raw = readFileSync(file, 'utf8');
const isAggregate = raw.startsWith('# Aggregate');
const problems = [];

// 1 — sections, per note kind
const isInvariant = /[\\/]invariant[\\/]/.test(file);
if (isAggregate) {
  if (!raw.includes('owns no detail')) problems.push('aggregate note does not say it owns no detail');
} else {
  const want = isInvariant ? INVARIANT_SECTIONS : TERRITORY_SECTIONS;
  const lines = raw.split(/\r?\n/);
  for (const s of want) {
    if (!lines.some((l) => l.startsWith(s))) problems.push(`missing section: ${s}`);
  }
}

// 2 — symbols named under ## Code
const codeSection = /^## Code\r?\n([\s\S]*?)^## /m.exec(raw)?.[1] ?? '';
// `**None.**` under `## Code` — a design recorded and not built — stands the symbol check down, so
// the prose after it may name things that do not exist yet.
const noCode = /^\s*\*\*None\.\*\*/.test(codeSection);
if (codeSection.trim() && !noCode) {
  const allPaths = execFileSync('git', ['ls-files', '-z'], { encoding: 'utf8' })
    .split('\0')
    .filter((p) => p && existsSync(p));
  const tree = allPaths
    .filter((p) => SRC_EXT.has(extname(p)))
    .map((p) => readFileSync(p, 'utf8'))
    .join('\n');

  // Notes write a full path once and then bare siblings — `…/src/palette.rs` · `attrs.rs` · `color.rs`
  // — so a basename that exists anywhere in the tree resolves.

  const files = new Set([...codeSection.matchAll(/`([\w./-]+\.(?:rs|ts|mjs|yml|toml))`/g)].map((m) => m[1]));
  for (const f of files) {
    const known = existsSync(f) || allPaths.some((p) => p.endsWith('/' + f) || p.endsWith('/' + f.split('/').pop()));
    if (!known) problems.push(`## Code names a missing file: ${f}`);
  }

  const syms = new Set([
    ...[...codeSection.matchAll(/`(?:[A-Za-z_]+::)?([a-z_][a-z0-9_]{2,})`/g)].map((m) => m[1]),
    ...[...codeSection.matchAll(/`([A-Z][A-Za-z0-9_]+)`/g)].map((m) => m[1]),
    // camelCase — a TypeScript function, method or field, or a wasm-bindgen `js_name`
    ...[...codeSection.matchAll(/`(?:[A-Za-z_]+\.)?([a-z][a-z0-9]*[A-Z][A-Za-z0-9]*)`/g)].map((m) => m[1]),
  ]);
  for (const s of syms) {
    if (files.has(s)) continue;
    // declaration, wasm-bindgen export name, call/field, enum variant, macro, or TOML key.
    const pats = [
      // Rust and TypeScript declaration keywords; `impl` because a note may name a *foreign* trait
      // the tree implements but does not declare.
      new RegExp(`(?:fn|struct|enum|const|static|type|trait|mod|class|interface|let|var|impl)\\s+${s}\\b`),
      new RegExp(`\\bjs_name\\s*=\\s*${s}\\b`),
      new RegExp(`\\b${s}\\s*[:(!]`),
      new RegExp(`^\\s*${s}\\s*,?\\s*$`, 'm'),
      new RegExp(`^\\s*${s}\\s*=`, 'm'),
    ];
    if (!pats.some((p) => p.test(tree))) problems.push(`## Code names an unresolved symbol: ${s}`);
  }
}

// 3 — restated status
const prose = raw
  .replace(/```[\s\S]*?```/g, '')
  .replace(/`[^`\n]*`/g, '');
for (const m of prose.matchAll(
  /(ADR-\d{4}[^.\n]{0,120}?\b(?:is |still |remains |currently )(?:proposed|accepted)\b|\bStatus:\s*(?:proposed|accepted))/gi,
)) {
  problems.push(`restates a record's status: "${m[0].trim().slice(0, 60)}" — say "check its Status line"`);
}

if (problems.length) {
  console.error(`${file}: ${problems.length} problem(s)\n`);
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}
console.log(`${file}: ok${isAggregate ? ' (aggregate)' : ''}`);
