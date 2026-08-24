#!/usr/bin/env node
// Read a line out of a pinned reference tree and emit the `Site` cell for it, so a row in
// docs/agents/reference-facts.md is never typed from memory.
//
// Why this is a GENERATOR and not a checker. That file's own Rule 1 is "`file:line` or it does
// not go in", and on 2026-07-29 five rows broke it in two days (#610): one named the wrong file,
// two were off by one line, two quoted the right line and drew a wrong conclusion from it. Every
// one was caught by review; tooling caught zero. The obvious repair — a check that re-reads each
// citation and fails when the quote is not there — was measured against the real file before it
// was built, and it does not survive contact with the data:
//
//   citations in the file                                    218
//   would FAIL if the quote must sit on the cited line       149   <- on the CORRECT file
//   would FAIL if the quote must appear anywhere in the file   91
//
// The rows are not sloppy; they are *readable*. They cite the construct and quote a normalized
// form of it — `point.column <= 1 && point.line != topmost_line()` for a source line that reads
// `if point.column <= 1 && point.line != self.topmost_line() {` — and upstream wraps calls across
// lines (`copyCellsFrom(...)` spans InputHandler.ts:605-606), so a per-line substring test can
// never match one. A gate with 149 false failures is the thing check-map-links.mjs's header warns
// about: people learn to ignore it. And the two off-by-ones would have slipped through anyway,
// since one line of tolerance is inside any tolerance a real check would need.
//
// So the failure is not un-caught citations, it is HAND-COPIED ones. All five came from
// transcribing a lens report instead of re-opening the source. This tool removes the transcription:
// you point it at a line, it prints what is actually there and hands you the cell to paste. What it
// cannot do is judge whether that line means what you say it means — errors 2 and 3 above were a
// correct citation with a wrong reading, and no tool reaches that. That stays a review's job, and
// this comment exists so the tool's presence does not make the review feel redundant.
//
// Usage:
//   cite.mjs <tree> <path>:<line>[-<end>]     print those lines, emit the Site cell
//   cite.mjs <tree> <path> --find <text>      print every line containing <text>, with numbers
//   cite.mjs --pins                           check the local trees against the recorded pins
//
//   <tree> is alacritty | ghostty | xterm.js | three.js | xterm; <path> may be partial (`term/mod.rs`) as
//   long as it resolves to one file.

import { readFileSync, existsSync } from 'node:fs';
import { execSync } from 'node:child_process';
import { join, resolve } from 'node:path';

const TREES = ['alacritty', 'ghostty', 'xterm.js', 'three.js', 'xterm'];

// The pin table moved to the graph build when `xterm` was added (2026-08-24). `theflow.md` is an
// INPUT to that build and is not edited by it, so its table is the older four-tree version and is
// deliberately left alone — which is exactly why this constant must name the authoritative file
// rather than "the bindings doc". Two tables and a reader pointed at the wrong one is how a pin
// silently stops being checked.
const PINS_DOC = 'docs/agents/thegraph.md';

const die = (msg, code = 2) => {
  console.error(`cite: ${msg}`);
  process.exit(code);
};

// The trees live beside the MAIN checkout, not beside the cwd. theflow.md § "Step 1" writes the
// path as `../.refs/`, which is true from the main checkout and false from every worktree this
// project's flow creates per issue — there `../` is `.claude/worktrees/`. Anchor on the common git
// dir instead, which is the main checkout's `.git` from inside a worktree and `.git` from the main
// checkout, so one expression is correct in both.
function refsRoot() {
  const commonDir = execSync('git rev-parse --git-common-dir', { encoding: 'utf8' }).trim();
  return resolve(commonDir, '..', '..', '.refs');
}

/** The pinned SHA per tree, read from the table in theflow.md — that table is authoritative. */
function readPins() {
  // The TREES live beside the main checkout (see refsRoot), but the pin DOC is a file under version
  // control and must come from the WORKING tree — otherwise a branch that adds a tree or refreshes a
  // pin cannot verify its own table until it merges, which is precisely when verification is owed.
  const repoRoot = execSync('git rev-parse --show-toplevel', { encoding: 'utf8' }).trim();
  const doc = join(repoRoot, PINS_DOC);
  if (!existsSync(doc)) return new Map();
  const pins = new Map();
  for (const line of readFileSync(doc, 'utf8').split(/\r?\n/)) {
    const m = /^\|\s*([\w.]+)\s*\|[^|]*\|\s*`([0-9a-f]{40})`\s*\|/.exec(line);
    if (m && TREES.includes(m[1])) pins.set(m[1], m[2]);
  }
  return pins;
}

function headOf(treeDir) {
  try {
    return execSync(`git -C "${treeDir}" rev-parse HEAD`, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch {
    return null;
  }
}

/**
 * A pin mismatch is the one thing here worth reporting unprompted: reference-facts.md states that
 * every line number in it is valid *at the recorded SHAs*, so a refreshed tree silently invalidates
 * the whole column. Unlike a quote check this has no false-positive mode — the two SHAs match or
 * they do not.
 */
function checkPins(root, only) {
  const pins = readPins();
  const rows = [];
  let bad = 0;
  for (const t of TREES) {
    if (only && t !== only) continue;
    const dir = join(root, t);
    const head = headOf(dir);
    const pin = pins.get(t);
    if (!head) { rows.push(`  ${t.padEnd(10)} no checkout at ${dir}`); bad++; continue; }
    // A tree on disk with no recorded pin is NOT a clean state — every line number cited from it
    // is unverifiable. Counting it as neither pass nor fail is the vacuous pass this project has
    // already been bitten by once (a scanner given the wrong path reported `0 scanned` AND green).
    if (!pin) { rows.push(`  ${t.padEnd(10)} ${head.slice(0, 12)}  NO PIN RECORDED in ${PINS_DOC}  <- unverifiable, not clean`); bad++; continue; }
    if (head === pin) rows.push(`  ${t.padEnd(10)} ${head.slice(0, 12)}  matches pin`);
    else { rows.push(`  ${t.padEnd(10)} ${head.slice(0, 12)}  != pin ${pin.slice(0, 12)}  <- line numbers recorded at the pin may have moved`); bad++; }
  }
  return { rows, bad };
}

/** Resolve a possibly-partial path against the tree's tracked files. */
function resolveInTree(dir, path) {
  const listed = execSync(`git -C "${dir}" ls-files`, { encoding: 'utf8', maxBuffer: 1 << 28 })
    .split('\n')
    .filter(Boolean);
  const want = path.replace(/\\/g, '/');
  const hits = listed.filter((rel) => rel === want || rel.endsWith('/' + want));
  return hits;
}

const argv = process.argv.slice(2);
if (argv.length === 0 || argv[0] === '-h' || argv[0] === '--help') {
  console.log(readFileSync(new URL(import.meta.url), 'utf8').split('\n').filter((l) => l.startsWith('//')).join('\n'));
  process.exit(argv.length === 0 ? 2 : 0);
}

const root = refsRoot();
if (!existsSync(root)) {
  die(
    `no reference trees at ${root}\n` +
      `       They are not in the repo — clone them with the recipe in ${PINS_DOC} § "Step 1".`,
  );
}

if (argv[0] === '--pins') {
  const { rows, bad } = checkPins(root, null);
  console.log(`cite: local trees vs ${PINS_DOC}`);
  for (const r of rows) console.log(r);
  process.exit(bad ? 1 : 0);
}

const [tree, spec, ...rest] = argv;
if (!TREES.includes(tree)) die(`unknown tree "${tree}" — expected one of ${TREES.join(', ')}`);

const findIdx = rest.indexOf('--find');
const findText = findIdx >= 0 ? rest.slice(findIdx + 1).join(' ') : null;
if (findIdx >= 0 && !findText) die('--find needs some text to look for');

const treeDir = join(root, tree);
if (!existsSync(treeDir)) die(`no checkout for ${tree} at ${treeDir}`);

// `path:line`, `path:a-b`, or a bare `path` when --find does the locating.
const m = /^(.*?):(\d+)(?:-(\d+))?$/.exec(spec);
if (!m && !findText) die(`expected <path>:<line> or <path> --find <text>, got "${spec}"`);
const path = m ? m[1] : spec;

const hits = resolveInTree(treeDir, path);
if (hits.length === 0) die(`no file matching "${path}" in ${tree}`, 1);
if (hits.length > 1) die(`"${path}" is ambiguous in ${tree}:\n` + hits.map((h) => `         ${h}`).join('\n'), 1);
const rel = hits[0];
const src = readFileSync(join(treeDir, rel), 'utf8').split(/\r?\n/);

// Report the pin before the content, so a stale tree is seen before its line numbers are trusted.
const { rows: pinRows, bad: pinBad } = checkPins(root, tree);
for (const r of pinRows) if (pinBad || !r.includes('matches pin')) console.log(`cite:${r}`);

if (findText) {
  const found = [];
  src.forEach((line, i) => {
    if (line.includes(findText)) found.push(i + 1);
  });
  if (found.length === 0) die(`"${findText}" does not appear in ${tree}/${rel}`, 1);
  console.log(`${tree}/${rel} — ${found.length} line(s) containing "${findText}"\n`);
  for (const n of found) console.log(`  ${String(n).padStart(5)}| ${src[n - 1]}`);
  console.log(`\nCite one with:  cite.mjs ${tree} ${path}:${found[0]}`);
  process.exit(0);
}

const start = Number(m[2]);
const end = m[3] ? Number(m[3]) : start;
if (start < 1 || end < start) die(`bad range ${start}-${end}`);
if (end > src.length) die(`${tree}/${rel} has ${src.length} lines; ${end} is past the end`, 1);

console.log(`${tree}/${rel}\n`);
// Two lines of surrounding context, because the mistake this tool exists to prevent is an
// off-by-one: seeing the neighbours is what makes a wrong line obvious rather than plausible.
for (let n = Math.max(1, start - 2); n <= Math.min(src.length, end + 2); n++) {
  const cited = n >= start && n <= end;
  console.log(`  ${cited ? '>' : ' '} ${String(n).padStart(5)}| ${src[n - 1]}`);
}
const range = end === start ? `${start}` : `${start}-${end}`;
console.log(`\nSite cell:  \`${path}:${range}\``);
