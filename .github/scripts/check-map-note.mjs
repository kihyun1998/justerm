#!/usr/bin/env node
// Verify ONE map note — the per-note check that makes "verify as you finish it" affordable.
//
// The batch gate (check-map-links.mjs) runs in CI over the whole tree. This one is for the author,
// mid-write, and exists because of a measured failure: 27 notes were written and verified once at
// the end, and every defect that pass found was the same class, spread across notes written hours
// apart. Checking after the third would have ended the class there — but only if checking is cheap
// enough that nobody defers it.
//
//   node .github/scripts/check-map-note.mjs docs/map/territory/selection.md
//
// Checks, in the order they pay off:
//   1. the section set is complete (aggregates are exempt — they own no detail)
//   2. every symbol named under ## Code resolves somewhere in the tree
//   3. nothing restates a value another artifact owns (a record's status)
// Links and anchors are left to the batch gate, which already resolves them across the graph.

import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { join, extname } from 'node:path';

const file = process.argv[2];
if (!file || !existsSync(file)) {
  console.error('usage: check-map-note.mjs <docs/map/**/note.md>');
  process.exit(2);
}

// Three note kinds, three schemas. Applying the territory schema to an invariant note is the first
// thing this script got wrong, and it reported three real notes as broken.
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
const SRC_ROOTS = [
  'justerm-core/src',
  'justerm-renderer/src',
  'justerm-wasm-decode/src',
  'justerm-web/src',
  '.github',
];
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
if (codeSection.trim()) {
  const blob = [];
  const walk = (dir) => {
    if (!existsSync(dir)) return;
    for (const e of readdirSync(dir)) {
      const p = join(dir, e);
      if (statSync(p).isDirectory()) walk(p);
      else if (SRC_EXT.has(extname(p))) blob.push(readFileSync(p, 'utf8'));
    }
  };
  SRC_ROOTS.forEach(walk);
  for (const extra of ['Cargo.toml', 'justerm-facade/Cargo.toml', 'justerm-renderer/Cargo.toml']) {
    if (existsSync(extra)) blob.push(readFileSync(extra, 'utf8'));
  }
  const tree = blob.join('\n');

  // Notes write a full path once and then bare siblings — `…/src/palette.rs` · `attrs.rs` · `color.rs`.
  // Resolving a bare name against the repo root reported every one of those as missing, so accept a
  // basename that exists anywhere in the source roots.
  const allPaths = [];
  const collect = (dir) => {
    if (!existsSync(dir)) return;
    for (const e of readdirSync(dir)) {
      const p = join(dir, e);
      if (statSync(p).isDirectory()) collect(p);
      else allPaths.push(p.replace(/\\/g, '/'));
    }
  };
  SRC_ROOTS.forEach(collect);

  const files = new Set([...codeSection.matchAll(/`([\w./-]+\.(?:rs|ts|mjs|yml|toml))`/g)].map((m) => m[1]));
  for (const f of files) {
    const known = existsSync(f) || allPaths.some((p) => p.endsWith('/' + f) || p.endsWith('/' + f.split('/').pop()));
    if (!known) problems.push(`## Code names a missing file: ${f}`);
  }

  const syms = new Set([
    ...[...codeSection.matchAll(/`(?:[A-Za-z_]+::)?([a-z_][a-z0-9_]{2,})`/g)].map((m) => m[1]),
    ...[...codeSection.matchAll(/`([A-Z][A-Za-z0-9_]+)`/g)].map((m) => m[1]),
  ]);
  for (const s of syms) {
    if (files.has(s)) continue;
    // declaration, call/field, enum variant, macro, or TOML key — the four shapes that produced
    // false positives when only declarations were matched.
    const pats = [
      new RegExp(`(?:fn|struct|enum|const|static|type|trait|mod)\\s+${s}\\b`),
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
