#!/usr/bin/env node
// thegraph environment preconditions — justerm.
// Built from docs/agents/thegraph.md § Environment preconditions · thegraph stamp 18edd61 (kihyun-skills).
//
// Usage:  node scripts/thegraph/preflight.mjs
//
// Every check here covers a failure that is SILENT. That is the whole reason this is a script and
// not a paragraph: none of these produces an error, so none of them is noticed by working carefully.
// A wrong `../` returns zero hits (which reads as "no prior art"); an adopted dev server makes a
// GREEN run untrustworthy; a mismatched wasm-pack passes locally and passes in CI, differently.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { resolve, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

/** Splits a child process's stdout into lines, tolerating CRLF. Written once here rather than at
 *  each call site below. */
const NL = /\r?\n/;


const results = [];
const ok = (name, detail) => results.push({ level: "ok", name, detail });
const warn = (name, detail, fix) => results.push({ level: "warn", name, detail, fix });
const bad = (name, detail, fix) => results.push({ level: "bad", name, detail, fix });

const run = (cmd, args, opts = {}) => spawnSync(cmd, args, { encoding: "utf8", ...opts });

// ── 1. where am I, and does `../` mean what this repo's docs think it means ──────────────────────
//
// Every sibling path in docs/agents/*.md (../.refs/, ../penterm/, ../just-shield) is written
// relative to the MAIN CHECKOUT. A worktree elsewhere silently redirects all of them.

const gitCommon = run("git", ["rev-parse", "--path-format=absolute", "--git-common-dir"], { cwd: HERE });
const mainCheckout = gitCommon.status === 0
  ? resolve(gitCommon.stdout.trim(), "..")
  : null;

const isWorktree = mainCheckout && resolve(mainCheckout) !== resolve(HERE);

if (!mainCheckout) {
  bad("git", "could not resolve the main checkout", "run inside the repository");
} else if (!isWorktree) {
  ok("location", `main checkout (${HERE}) — every ../ path resolves as the docs assume`);
} else if (resolve(HERE, "..") === resolve(mainCheckout, "..")) {
  ok("location", `worktree BESIDE the main checkout (${basename(HERE)}) — ../ resolves the same way it does there`);
} else {
  bad(
    "location",
    `worktree at ${HERE}, main checkout at ${mainCheckout} — they do NOT share a parent, so every ../ path in docs/agents/ points somewhere else`,
    "move it to ../justerm-wt-<issue>, beside the main checkout, or use absolute paths everywhere",
  );
}

// ── 2. the pinned reference trees ────────────────────────────────────────────────────────────────
//
// Delegates the SHA comparison to cite.mjs --pins, which owns the pin table, rather than carrying a
// second copy of it here. A second copy is the thing that goes stale.

const refsDir = mainCheckout ? resolve(mainCheckout, "..", ".refs") : null;
if (!refsDir || !existsSync(refsDir)) {
  bad("refs", `../.refs not found (looked in ${refsDir ?? "?"})`, "clone the trees — see docs/agents/thegraph.md § reference");
} else {
  const trees = readdirSync(refsDir, { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name);
  const expected = ["alacritty", "ghostty", "xterm.js", "three.js", "xterm"];
  const missing = expected.filter((t) => !trees.includes(t));
  if (missing.length) {
    warn("refs", `present: ${trees.join(", ")} — MISSING: ${missing.join(", ")}`,
      "a missing tree returns zero hits, which reads exactly like 'no prior art'");
  } else {
    ok("refs", `all five trees present in ${refsDir}`);
  }

  const cite = resolve(mainCheckout, ".github/scripts/cite.mjs");
  if (existsSync(cite)) {
    const r = run("node", [cite, "--pins"], { cwd: mainCheckout });
    const out = `${r.stdout || ""}${r.stderr || ""}`.trim();
    // Split once, outside the branch. The failure arm reads `rows` too, and it escaped a
    // ReferenceError only because `offenders` is non-empty whenever there is anything to report —
    // so the `||` fallback short-circuited past it on every path that had ever run.
    const rows = out.split(NL);
    if (r.status === 0) {
      // Count what cite ACTUALLY verified, not what is on disk. `trees.length` is the directory
      // count, and `../.refs/` may hold checkouts this repo does not pin (it does) — reporting
      // those as "matching" would be a success line making a claim nothing checked.
      const matched = rows.filter((l) => /matches pin/.test(l)).length;
      const unpinned = trees.filter((t) => !rows.some((l) => l.trim().startsWith(t + " ")));
      ok("pins", `${matched} tree(s) verified against the recorded SHAs` +
        (unpinned.length ? `  (present but not pinned, so not checked: ${unpinned.join(", ")})` : ""));
    } else {
      // Show the rows that FAILED, not the last few. A tail printed the three MATCHING trees while
      // the mismatched one scrolled off — a failure message describing the healthy part of the
      // system, which is worse than no message. Caught by mutating a pin, never by reading it: the
      // check itself was correct throughout, and only its report was wrong.
      const offenders = rows.filter((l) => /!=|NO PIN/.test(l)).map((l) => l.trim());
      bad("pins", offenders.join(" | ") || rows.slice(-1)[0],
        "a moved pin makes every recorded file:line unverifiable at once, and nothing else reports it");
    }
  } else {
    warn("pins", "cite.mjs not found — SHAs unverified", "pins were not checked; this is not a pass");
  }
}

// ── 3. who owns port 5173 ────────────────────────────────────────────────────────────────────────
//
// playwright.config.ts sets reuseExistingServer: !process.env.CI, so a `pnpm demo` already
// listening from ANOTHER checkout is silently adopted — the worktree's specs then run against that
// checkout's demo/ and src/. It makes a GREEN run untrustworthy exactly as readily as a red one.

const net = process.platform === "win32"
  ? run("netstat", ["-ano"])
  : run("sh", ["-c", "lsof -iTCP:5173 -sTCP:LISTEN -n -P || true"]);
const netOut = net.stdout || "";
const listening = process.platform === "win32"
  ? netOut.split("\n").filter((l) => /:5173\s/.test(l) && /LISTENING/i.test(l))
  : netOut.split("\n").filter(Boolean).slice(1);

if (listening.length === 0) {
  ok("port 5173", "free — playwright will start its own server");
} else {
  warn("port 5173", `already held: ${listening[0].trim()}`,
    "find out WHICH checkout owns it before trusting a red run — or a green one");
}

// ── 4. local wasm-pack vs the CI pin ─────────────────────────────────────────────────────────────
//
// check-tool-pins.mjs compares the WORKFLOWS to each other and never looks at the local binary.
// A local version that differs means the pixel proofs ran against different codegen and a different
// wasm-opt than the ones that will judge the PR — and both go green.

let pinned = null;
if (mainCheckout) {
  const wfDir = resolve(mainCheckout, ".github/workflows");
  if (existsSync(wfDir)) {
    for (const f of readdirSync(wfDir)) {
      const m = readFileSync(resolve(wfDir, f), "utf8").match(/WASM_PACK_VERSION:\s*["']?([\d.]+)/);
      if (m) { pinned = m[1]; break; }
    }
  }
}
const local = run("wasm-pack", ["--version"]);
const localVer = (local.stdout || "").match(/([\d.]+)/)?.[1] ?? null;

if (!pinned) warn("wasm-pack", "no WASM_PACK_VERSION found in any workflow", "the pin could not be read");
else if (!localVer) warn("wasm-pack", `CI pins ${pinned}; wasm-pack not on PATH`, "only matters if you run test:proofs");
else if (localVer === pinned) ok("wasm-pack", `${localVer} — matches the CI pin`);
else bad("wasm-pack", `local ${localVer} ≠ CI ${pinned}`,
  `cargo install wasm-pack --locked --version ${pinned} — otherwise test:proofs is not the gate CI runs, and both go green`);

// ── 5. just-shield's argument ────────────────────────────────────────────────────────────────────
//
// Given the wrong path it reports "0 workflows scanned" AND a green "no violations" — a vacuous
// pass. The correct argument is the REPO ROOT of the tree you are editing, not .github/workflows,
// and from a worktree that is the WORKTREE root.

const shield = mainCheckout ? resolve(mainCheckout, "..", "just-shield") : null;
if (shield && existsSync(shield)) {
  ok("just-shield", `present at ${shield} — scan with:  cargo run -- scan --strict ${HERE}`);
} else {
  warn("just-shield", "not found beside the main checkout",
    "only needed for a change touching .github/workflows/**");
}

// ── 6. the delegated agents' tool grants ─────────────────────────────────────────────────────────
//
// Invariant ① licenses delegating `verify`, `sweep` and `reference`-fetch on the grounds that they
// read without adjudicating, and the licence is the tool GRANT rather than the brief's claim. A
// corrupted grant belongs on this list by its own criterion: nothing errors, the agent simply holds
// a tool nobody declared, and it surfaces when that agent mutates the worktree a second one is
// reading — which is how it surfaced upstream, and the second agent then reported a failure it
// could not reproduce, correctly, from inside evidence the first had manufactured.
//
// `gates.mjs` runs the same script, so this is not a second check — it is the EARLIER of the two,
// and the ordering is the whole point. A gate sees a violation after the work; CI would see it
// after the merge, and would not see it at all while the file is uncommitted. Only this position is
// before the run that the violation damages.

const grantsScript = resolve(HERE, "scripts", "thegraph", "grants.mjs");
if (!existsSync(grantsScript)) {
  bad("agent grants", "scripts/thegraph/grants.mjs is missing",
    "invariant ① is unchecked without it — restore it or re-run /grill-the-graph");
} else {
  const grants = run(process.execPath, [grantsScript], { cwd: HERE });
  const out = `${grants.stdout || ""}${grants.stderr || ""}`.trim();
  if (grants.error) {
    bad("agent grants", `grants.mjs could not be run: ${grants.error.message}`,
      "a check that did not run is not a pass");
  } else if (grants.status === 0) {
    ok("agent grants", out.split(NL).pop() || "clean");
  } else {
    const fail = out.split(NL).find((l) => l.startsWith("FAIL")) || "grants.mjs exited non-zero";
    bad("agent grants", fail,
      "node scripts/thegraph/grants.mjs — a delegated node carries no write-capable tool unless its brief declares that tool by name");
  }
}

// ── report ───────────────────────────────────────────────────────────────────────────────────────

const icon = { ok: "  ok  ", warn: " warn ", bad: " BAD  " };
console.log("thegraph preflight — justerm\n");
for (const r of results) {
  console.log(`${icon[r.level]} ${r.name.padEnd(12)} ${r.detail}`);
  if (r.fix) console.log(`              ↳ ${r.fix}`);
}

const bads = results.filter((r) => r.level === "bad").length;
const warns = results.filter((r) => r.level === "warn").length;
console.log(`\n${results.length - bads - warns} ok · ${warns} warn · ${bads} bad`);
if (bads === 0 && warns === 0) console.log("\nEvery check here covers a SILENT failure. A clean run is the only evidence they are absent.");
process.exit(bads ? 1 : 0);
