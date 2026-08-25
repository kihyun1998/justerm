#!/usr/bin/env node
// thegraph `gate` node — justerm. Built from docs/agents/thegraph.md · thegraph stamp 89b477a (kihyun-skills).
//
// Runs every gate for a scope, each one BARE. `test … | tail -1 && commit` always commits, because
// a pipeline's exit status is the last command's and `tail` always succeeds — a gate you cannot
// fail is not a gate. That is the whole reason this node is a script: the shape cannot be got wrong
// twice.
//
// Usage:  node scripts/thegraph/gates.mjs <core|web|renderer|supply-chain|all> [--list]
//
// Scope exists because two gates are expensive and conditional: the renderer's pixel proofs only
// matter if the GL layer moved, and the web e2e run takes minutes. Running everything by reflex is
// how a gate stops being run at all.

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

// ── the gate list ────────────────────────────────────────────────────────────────────────────────
//
// `manual: true`  — listed but not runnable as pass/fail. Reported as OWED, never as passed.
// `optional: true`— run only when its condition holds; the runner says which it skipped and why.

const GATES = [
  // core / wasm
  { scope: "core", cmd: "cargo test --workspace" },
  { scope: "core", cmd: "cargo fmt --all --check", note: "pinned toolchain 1.96.0" },
  { scope: "core", cmd: "cargo clippy --workspace --all-targets -- -D warnings", note: "the `-- -D warnings` IS the gate" },
  { scope: "core", cmd: "cargo check --manifest-path fuzz/Cargo.toml", note: "own [workspace] — --workspace does not build it" },
  { scope: "core", cmd: "cargo build -p justerm-wasm-decode --tests --target wasm32-unknown-unknown", note: "tests/web.rs is wasm32-only and 0-compiles on host" },
  { scope: "core", cmd: 'cargo doc --workspace --no-deps', env: { RUSTDOCFLAGS: "-D warnings" }, note: "rustdoc lints ≠ clippy ≠ doctests" },
  { scope: "core", cmd: "node .github/scripts/check-map-links.mjs docs CLAUDE.md CONTEXT.md README.md" },
  // `argv`, not `cmd`, and the reason is measured. CI runs this as a multi-line bash `run:` block on
  // ubuntu; spelling it as one shell string here made it a PERMANENT FALSE RED on Windows, where
  // `shell: true` resolves to `%ComSpec%` = cmd.exe, which does not group with single quotes and
  // consumes the `||` as its own operator — bash then receives an unterminated `-c` and answers
  // "line 2: syntax error: unexpected end of file". The check itself was fine the whole time (run by
  // hand over all 43 notes it exits 0), so every local `core` run reported a red that was not one:
  // the exact failure this runner exists to prevent, pointed the wrong way. Found while gating #448.
  // Going through `argv` skips the shell entirely, on both platforms, and keeps the command TEXT
  // identical to CI's — which is the point of a mirror.
  { scope: 'core',
    cmd: 'bash -c \'bad=0; for f in docs/map/territory/*.md docs/map/invariant/*.md; do node .github/scripts/check-map-note.mjs "$f" || bad=1; done; exit $bad\'',
    argv: ["bash", "-c", 'bad=0; for f in docs/map/territory/*.md docs/map/invariant/*.md; do node .github/scripts/check-map-note.mjs "$f" || bad=1; done; exit $bad'],
    note: 'note SCHEMA ≠ note LINKS — a different tool from the line above' },
  { scope: "core", cmd: "node .github/scripts/check-tool-pins.mjs", note: "compares the WORKFLOWS to each other; does not look at your local wasm-pack — preflight.mjs does" },

  // web
  { scope: "web", cwd: "justerm-web", cmd: "pnpm typecheck", note: "3 tsconfigs; running one silently leaks coverage" },
  { scope: "web", cwd: "justerm-web", cmd: "pnpm test" },
  { scope: "web", cwd: "justerm-web", cmd: "pnpm build", note: "guards output paths only; catches no type error typecheck missed" },
  { scope: "web", cwd: "justerm-web", cmd: "pnpm demo", manual: true,
    note: "MANUAL: a real-browser look. Listed rather than dropped because leaving it out is how a visual check stops being owed — a synthetic-input unit is not a substitute for one" },
  { scope: "web", cwd: "justerm-web", cmd: "pnpm test:e2e", optional: "the change is a11y/UI-observable",
    note: "needs port 5173 free — a `pnpm demo` from ANOTHER checkout is silently adopted, which makes a green run untrustworthy too" },

  // renderer — outside every cargo umbrella; `--workspace` and `fmt --all` visit ZERO files here
  { scope: "renderer", cmd: "cargo fmt --manifest-path justerm-renderer/Cargo.toml --check" },
  { scope: "renderer", cmd: "cargo test --manifest-path justerm-renderer/Cargo.toml", note: "pure layer only; webgl.rs/rasterizer.rs are wasm32-only" },
  { scope: "renderer", cmd: "cargo clippy --manifest-path justerm-renderer/Cargo.toml --target wasm32-unknown-unknown --all-targets" },
  { scope: "renderer", cmd: "cargo build --manifest-path justerm-renderer/Cargo.toml --target wasm32-unknown-unknown", note: "the GL/wasm layer 0-compiles on host" },
  { scope: "renderer", cmd: "cargo doc --manifest-path justerm-renderer/Cargo.toml --no-deps --target wasm32-unknown-unknown", env: { RUSTDOCFLAGS: "-D warnings" } },
  { scope: "renderer", cwd: "justerm-renderer", cmd: "pnpm run test:unit" },
  { scope: "renderer", cwd: "justerm-renderer", cmd: "pnpm run test:proofs", optional: "the GL layer changed",
    note: "your local wasm-pack must match WASM_PACK_VERSION or this is not the gate CI runs — both go green" },

  // supply-chain — path-filtered to .github/workflows/**, absent otherwise
  { scope: "supply-chain", cmd: `cargo run -- scan --strict ${ROOT}`, cwd: "../just-shield", optional: "the change touches .github/workflows/**",
    note: "point it at the REPO ROOT: given the wrong path it reports '0 workflows scanned' AND a green 'no violations' — a vacuous pass" },
];

// ── the CI cross-check ───────────────────────────────────────────────────────────────────────────
//
// This list is a copy of CI, and a copy that does not know it is one drifts. Two gates were missing
// from the hand-written matrix until it cost a red CI — both were steps of the SAME `test` job, so
// "I ran the local matrix" read as complete. So the list names its authoritative source and checks
// itself against it rather than restating it. This does not parse YAML: it looks for each command's
// distinguishing fragment in the workflow text, which is enough to catch a step CI gained and this
// file did not.

const CI_WORKFLOW = ".github/workflows/test.yml";
const CI_EXEMPT = new Set(["pnpm demo"]); // manual, never a CI step

function crossCheckAgainstCi() {
  let text;
  try {
    text = readFileSync(resolve(ROOT, CI_WORKFLOW), "utf8");
  } catch {
    return [`could not read ${CI_WORKFLOW} — the cross-check did not run, which is NOT a pass`];
  }
  const drift = [];
  // Every `node .github/scripts/*.mjs` step in CI must appear here. That family is the one that
  // grew silently, and it is greppable without a YAML parser.
  for (const m of text.matchAll(/node \.github\/scripts\/([\w.-]+\.mjs)/g)) {
    const script = m[1];
    if (!GATES.some((g) => g.cmd.includes(script))) {
      drift.push(`CI runs .github/scripts/${script} and this list does not — add it`);
    }
  }
  for (const g of GATES) {
    const m = g.cmd.match(/\.github\/scripts\/([\w.-]+\.mjs)/);
    if (m && !text.includes(m[1])) {
      drift.push(`this list runs ${m[1]} and ${CI_WORKFLOW} does not mention it — stale or moved`);
    }
  }
  return drift;
}

// ── run ──────────────────────────────────────────────────────────────────────────────────────────

const scope = process.argv[2];
const listOnly = process.argv.includes("--list");
const SCOPES = ["core", "web", "renderer", "supply-chain", "all"];

if (!SCOPES.includes(scope)) {
  console.error(`usage: node scripts/thegraph/gates.mjs <${SCOPES.join("|")}> [--list]`);
  process.exit(2);
}

const selected = GATES.filter((g) => scope === "all" || g.scope === scope);

const drift = crossCheckAgainstCi();
for (const d of drift) console.log(`DRIFT   ${d}`);

if (listOnly) {
  for (const g of selected) {
    const tag = g.manual ? "MANUAL  " : g.optional ? "OPTIONAL" : "GATE    ";
    console.log(`${tag} ${g.cwd ? `(cd ${g.cwd}) ` : ""}${g.cmd}`);
    if (g.optional) console.log(`         └ only when: ${g.optional}`);
    if (g.note) console.log(`         └ ${g.note}`);
  }
  process.exit(drift.length ? 1 : 0);
}

const owed = [];
const skipped = [];
const failed = [];

for (const g of selected) {
  if (g.manual) {
    owed.push(g);
    continue;
  }
  if (g.optional && !process.argv.includes("--include-optional")) {
    skipped.push(g);
    continue;
  }
  console.log(`\n=== ${g.cwd ? `(cd ${g.cwd}) ` : ""}${g.cmd}`);
  // BARE. Inherited stdio, no pipe, exit status read directly.
  // A gate carrying `argv` is spawned with NO shell — see the note on the one that does.
  const opts = {
    stdio: "inherit",
    cwd: resolve(ROOT, g.cwd ?? "."),
    env: { ...process.env, ...(g.env ?? {}) },
  };
  const r = g.argv
    ? spawnSync(g.argv[0], g.argv.slice(1), opts)
    : spawnSync(g.cmd, { ...opts, shell: true });
  if (r.status !== 0) failed.push({ g, status: r.status });
}

console.log("\n──────── gate report ────────");
console.log(`ran     ${selected.length - owed.length - skipped.length}`);
for (const g of skipped) console.log(`SKIPPED ${g.cmd}  — only when: ${g.optional}`);
for (const g of owed) console.log(`OWED    ${g.cmd}  — manual, not a pass/fail command`);
for (const f of failed) console.log(`FAILED  ${f.g.cmd}  (exit ${f.status})`);
if (drift.length) console.log(`DRIFT   ${drift.length} — this list disagrees with ${CI_WORKFLOW}`);

// A skipped optional gate and an owed manual one are NOT failures; drift is, because a list that
// disagrees with CI is the failure mode this script exists to prevent.
process.exit(failed.length || drift.length ? 1 : 0);
