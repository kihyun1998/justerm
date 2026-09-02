#!/usr/bin/env node
// thegraph invariant ① — justerm. Built from docs/agents/thegraph.md · thegraph stamp 18edd61 (kihyun-skills).
//
// Asserts every generated agent's TOOL GRANT against its own brief.
//
// Invariant ① licenses delegating `verify`, `sweep` and `reference`-fetch on the grounds that they
// read without adjudicating. A write-capable tool in the grant makes that false no matter what the
// prose above it says — so the licence is the GRANT, never the claim. Four agents in one upstream
// build each declared "Read-only" in their description, each was granted `Bash`, and none of their
// briefs asked for a command; one then mutated a live worktree while the refuter was reading it.
//
// Two things this checks, and the second is the one that was got wrong first:
//
//   1. A write-capable tool is granted ONLY where the brief carries a `**Runs:**` line NAMING it.
//      "A declaration that names no tool licenses nothing" — an empty or vague `Runs:` is a
//      formality, not a licence, so the tool must appear in it by name.
//
//   2. The check keys on the DEFAULT, never on the claim. Asking "does the description say
//      read-only?" is dodgeable by rephrasing, and was: an agent granted a shell whose description
//      read "proposes edits rather than making them" passed a check looking for that phrase. So
//      read-only is assumed of every delegated node and only an explicit declaration moves one.
//
// Usage:  node scripts/thegraph/grants.mjs

import { readFileSync, readdirSync } from "node:fs";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const AGENT_DIR = join(ROOT, ".claude", "agents");

// Read-only by PROPERTY — a tool that cannot change state. Anything absent from this set is treated
// as write-capable, unknown tools included: a new tool nobody classified must fail closed, or the
// check quietly stops covering whatever the harness gains next.
const READ_ONLY = new Set(["Read", "Grep", "Glob", "WebFetch", "WebSearch"]);

const problems = [];
let checked = 0;

const files = readdirSync(AGENT_DIR).filter((f) => f.startsWith("thegraph-") && f.endsWith(".md"));

// A check that inspects nothing passes for the wrong reason. Assert the corpus is non-empty first.
if (files.length === 0) {
  console.error(`FAIL  no thegraph-*.md agents found in ${AGENT_DIR} — this is not a pass`);
  process.exit(1);
}

for (const file of files) {
  const text = readFileSync(join(AGENT_DIR, file), "utf8");
  checked++;

  const fm = text.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  if (!fm) {
    problems.push(`${file}: no frontmatter — the grant cannot be read, so it cannot be licensed`);
    continue;
  }

  const toolsLine = fm[1].match(/^tools:\s*(.+)$/m);
  const granted = toolsLine ? toolsLine[1].split(",").map((t) => t.trim()).filter(Boolean) : [];
  const writeCapable = granted.filter((t) => !READ_ONLY.has(t));

  // The declaration is a PARAGRAPH, not a line — it runs from the `**Runs:**` marker to the next
  // blank line. A tool named on its second line is still named, and forcing every tool onto one
  // line would be the check dictating the prose rather than reading it.
  const bodyLines = text.slice(fm[0].length).split(/\r?\n/);
  const start = bodyLines.findIndex((l) => l.trimStart().startsWith("**Runs:**"));
  let declared = null;
  if (start !== -1) {
    const block = [];
    for (let i = start; i < bodyLines.length && bodyLines[i].trim() !== ""; i++) block.push(bodyLines[i]);
    declared = block.join(" ");
  }

  if (writeCapable.length === 0) {
    if (declared !== null) {
      problems.push(`${file}: declares Runs: but is granted no write-capable tool — the declaration licenses nothing and reads as though it did`);
    }
    continue;
  }

  if (declared === null) {
    problems.push(`${file}: granted ${writeCapable.join(", ")} with no **Runs:** declaration — invariant ① makes read-only the default, and only a declaration moves one`);
    continue;
  }

  // Token-split rather than a word-boundary escape on purpose: this file is generated, and
  // a backslash escape does not survive every way of writing it out. Splitting on non-letters
  // needs no escape, and asks the same question.
  const words = declared.split(/[^A-Za-z]+/);
  const unnamed = writeCapable.filter((t) => !words.includes(t));
  if (unnamed.length > 0) {
    problems.push(`${file}: **Runs:** does not name ${unnamed.join(", ")} — a declaration licenses only the tools it names`);
  }

  // The other direction: a brief may only name what the grant can reach. A `Runs:` naming a tool
  // that is not granted is a brief the node cannot follow — it reports a gap and returns less, or
  // substitutes something weaker and says nothing.
  for (const t of declared.match(/\b(Bash|Edit|Write|NotebookEdit|Task|Agent)\b/g) ?? []) {
    if (!granted.includes(t)) {
      problems.push(`${file}: **Runs:** names ${t}, which the grant does not carry — the brief is wider than the grant`);
    }
  }
}

for (const p of problems) console.log(`FAIL  ${p}`);
console.log(`${problems.length ? "FAIL" : "OK"}    ${checked} agent(s) checked, ${problems.length} problem(s)`);
process.exit(problems.length ? 1 : 0);
