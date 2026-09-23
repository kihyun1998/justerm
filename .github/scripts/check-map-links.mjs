#!/usr/bin/env node
// Validate every relative markdown link under the given roots — INCLUDING `#section-anchors`.
//
// Also: invariant↔territory reciprocity, no copied ADR status, and `.md` paths cited from code
// comments. Why each exists: `docs/map/territory/ci-and-supply-chain.md`.
//
// The slug rule mirrors GitHub's: strip inline markdown, lowercase, drop everything that is not
// [a-z0-9 _-], then spaces -> hyphens. Duplicate headings get GitHub's `-1`, `-2` suffixes.
//
// Usage: node .github/scripts/check-map-links.mjs docs/map [more roots...]

import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { join, dirname, resolve } from 'node:path';

const roots = process.argv.slice(2);
if (roots.length === 0) {
  console.error('usage: check-map-links.mjs <root> [root...]');
  process.exit(2);
}

/** GitHub's heading -> anchor slug. */
function slugify(heading) {
  return heading
    .replace(/`([^`]*)`/g, '$1') // code spans
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1') // links -> their text
    .replace(/[*_~]/g, '') // emphasis markers
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9 _-]/g, '')
    .replace(/ /g, '-');
}

/** Every anchor a markdown file exposes, with GitHub's duplicate-suffix rule. */
function anchorsOf(file) {
  const seen = new Map();
  const out = new Set();
  // Deliberately /\r?\n/, not '\n': a CRLF checkout otherwise matches zero headings (see
  // `docs/map/territory/ci-and-supply-chain.md`).
  for (const line of readFileSync(file, 'utf8').split(/\r?\n/)) {
    const m = /^#{1,6}\s+(.*)$/.exec(line);
    if (!m) continue;
    const base = slugify(m[1]);
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    out.add(n === 0 ? base : `${base}-${n}`);
  }
  return out;
}

/** Roots may be a directory to walk or a single `.md` file (so `CLAUDE.md` can be gated too). */
function markdownFiles(root) {
  if (!statSync(root).isDirectory()) return root.endsWith('.md') ? [root] : [];
  const found = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir)) {
      const p = join(dir, entry);
      if (statSync(p).isDirectory()) walk(p);
      else if (entry.endsWith('.md')) found.push(p);
    }
  };
  walk(root);
  return found;
}

const anchorCache = new Map();
const problems = [];
let checked = 0;

for (const root of roots) {
  for (const file of markdownFiles(root)) {
    // Blank out fenced blocks and inline code spans FIRST — documentation about links quotes
    // link-shaped text. Spaces keep offsets, and therefore line numbers, intact.
    const body = readFileSync(file, 'utf8')
      .replace(/```[\s\S]*?```/g, (m) => ' '.repeat(m.length))
      .replace(/`[^`\n]*`/g, (m) => ' '.repeat(m.length));
    // ](relative/path.md) or ](relative/path.md#anchor) — skip absolute URLs.
    for (const m of body.matchAll(/\]\((?!https?:|#)([^)\s]+?\.md)(#[^)\s]*)?\)/g)) {
      checked++;
      const [, relPath, rawAnchor] = m;
      const target = resolve(dirname(file), relPath);
      if (!existsSync(target)) {
        problems.push(`${file}: target does not exist -> ${relPath}`);
        continue;
      }
      if (!rawAnchor) continue;
      const anchor = rawAnchor.slice(1);
      if (!anchorCache.has(target)) anchorCache.set(target, anchorsOf(target));
      if (!anchorCache.get(target).has(anchor)) {
        problems.push(
          `${file}: no such heading -> ${relPath}#${anchor}\n` +
            `    (a broken anchor is SILENT — it lands on the top of the file)`,
        );
      }
    }
  }
}

// Reciprocity: an invariant note names the territories it holds in, and each of those territories
// must name it back under `## Cross-cutting invariants`.
const MAP_ROOT = 'docs/map';
if (existsSync(join(MAP_ROOT, 'invariant'))) {
  for (const inv of markdownFiles(join(MAP_ROOT, 'invariant'))) {
    const invName = inv.split(/[\\/]/).pop();
    const body = readFileSync(inv, 'utf8');
    for (const m of body.matchAll(/\]\(\.\.\/territory\/([a-z0-9-]+\.md)\)/g)) {
      const terr = join(MAP_ROOT, 'territory', m[1]);
      if (!existsSync(terr)) continue;
      const section = readFileSync(terr, 'utf8')
        .split(/\r?\n/)
        .reduce((acc, line) => {
          if (/^## /.test(line)) acc.inSection = /^## Cross-cutting invariants/.test(line);
          else if (acc.inSection) acc.text += line + '\n';
          return acc;
        }, { inSection: false, text: '' }).text;
      if (!section.includes(`invariant/${invName}`)) {
        problems.push(
          `${terr}: ## Cross-cutting invariants does not list ${invName}, which claims this territory\n` +
            `    (one-way edge — the territory is the entry point, so the reader never sees it)`,
        );
      }
    }
  }
}

// A decision record's `Status:` line is authoritative and must not be copied into the map. Say
// "check its Status line" instead.
for (const file of markdownFiles(MAP_ROOT)) {
  const body = readFileSync(file, 'utf8')
    .replace(/```[\s\S]*?```/g, (m) => ' '.repeat(m.length))
    .replace(/`[^`\n]*`/g, (m) => ' '.repeat(m.length));
  // "ADR-0024 ... proposed" / "proposed ... ADR-0024" within one sentence
  for (const m of body.matchAll(
    /(ADR-\d{4}[^.\n]{0,120}?\b(?:is |still |remains |currently )(?:proposed|accepted)\b|\bStatus:\s*(?:proposed|accepted))/gi,
  )) {
    problems.push(
      `${file}: restates a decision record's status — "${m[0].trim().slice(0, 70)}"\n` +
        `    (the ADR's own Status: line is authoritative; a copy here has no gate. Say "check its Status line")`,
    );
  }
}

// A repo path cited from a CODE comment is a link too. Scoped deliberately tight: comment lines only,
// backticked only, and only paths under a known top-level directory ending in `.md` — citations, not
// every string that looks like one.
const CODE_ROOTS = [
  'justerm-core',
  'justerm-renderer',
  'justerm-web',
  'justerm-wasm-decode',
  '.github/scripts', // these cite docs too, including the comment you are reading
];
const SKIP_DIRS = new Set(['node_modules', 'target', 'dist', 'pkg', '.git']);
const CITED_PATH = /`((?:docs|teach|justerm-[a-z-]+)\/[A-Za-z0-9_./-]+\.md)`/g;
const isComment = (line) => /^\s*(\/\/|\/\*|\*|#)/.test(line);

function sourceFiles(root) {
  if (!existsSync(root)) return [];
  const found = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir)) {
      if (SKIP_DIRS.has(entry)) continue;
      const p = join(dir, entry);
      if (statSync(p).isDirectory()) walk(p);
      else if (/\.(rs|ts|tsx|mjs|js)$/.test(entry)) found.push(p);
    }
  };
  walk(root);
  return found;
}

for (const codeRoot of CODE_ROOTS) {
  for (const file of sourceFiles(codeRoot)) {
    const lines = readFileSync(file, 'utf8').split(/\r?\n/);
    lines.forEach((line, i) => {
      if (!isComment(line)) return;
      for (const m of line.matchAll(CITED_PATH)) {
        checked++;
        if (!existsSync(m[1])) {
          problems.push(
            `${file}:${i + 1}: comment cites a repo file that does not exist -> ${m[1]}\n` +
              `    (nothing else checks this — a renamed target leaves the comment pointing at nothing, silently)`,
          );
        }
      }
    });
  }
}

if (problems.length > 0) {
  console.error(`check-map-links: ${problems.length} problem(s) of ${checked} links checked\n`);
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}
console.log(`check-map-links: ${checked} links OK across ${roots.join(', ')}`);
