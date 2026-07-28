#!/usr/bin/env node
// Validate every relative markdown link under the given roots — INCLUDING `#section-anchors`.
//
// Why this exists as a gate rather than as a habit. `docs/map/` is a *link graph*: its value is
// entirely in edges that resolve, and it links out to `docs/adr/` and `docs/agents/reference-facts.md`
// — files it must never edit and whose headings it does not control. Two failure modes, and only one
// of them is loud:
//
//   - a missing FILE is at least visible on GitHub (404) and in an editor.
//   - a missing ANCHOR is SILENT. GitHub and Obsidian both fall back to the top of the target
//     document, so a reference link that used to land on one verified row quietly starts pointing at
//     a 200-line file, and the reader never learns they were sent to the wrong place. That is the
//     defect shape this repo keeps paying for (theflow Step 6): a surface that describes something
//     accurately when written, with nothing checking it afterwards.
//
// And the anchors here are *known* to be volatile: reference-facts.md headings embed issue numbers
// and verification dates (`## Damage / dirty tracking (#536, verified 2026-07-28)`), so a routine
// re-verification edits the slug and breaks the link without touching the linking file.
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
  // Split on /\r?\n/, not '\n'. With CRLF checkouts a '\n' split leaves a trailing '\r' on every
  // line, and `.` in a JS regex does NOT match '\r' (it is a line terminator) while `$` does not
  // match before one either — so `^#{1,6}\s+(.*)$` matches ZERO headings, every anchor set comes
  // back empty, and the checker reports every correct link as broken. It did exactly that on its
  // first run against 11 valid links.
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
    // Blank out fenced blocks and inline code spans FIRST. Documentation about links contains
    // link-shaped text: `docs/agents/release.md` explains crates.io's rewriting by quoting
    // `[x](../CLAUDE.md)` inside a code span, and reading that as a link reports a break that does
    // not exist. A gate people learn to ignore is not a gate, so false positives are the failure
    // mode to design against here — replacing with spaces keeps offsets, and therefore line numbers,
    // intact.
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
//
// This is not symmetry for its own sake. The map's reading protocol says a change starts at its
// territory and follows that section as a checklist — so an invariant the territory does not list is
// invisible at exactly the moment it is needed. The reverse direction is NOT free here: Obsidian
// backlinks are a separate panel, absent on GitHub, and not the section the reader was told to read.
//
// Found by this check on its first run: `wide-glyph-and-soft-wrap` did not list `row-keyed-side-maps`,
// while #557 (a wrap-clearing fix entering at exactly that territory) changed `hyperlink` — a
// row-keyed side map.
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

if (problems.length > 0) {
  console.error(`check-map-links: ${problems.length} problem(s) of ${checked} links checked\n`);
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}
console.log(`check-map-links: ${checked} links OK across ${roots.join(', ')}`);
