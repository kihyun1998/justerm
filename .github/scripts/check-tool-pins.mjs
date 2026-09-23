// Fail a PR whose build-tool pins have drifted apart across workflows.
//
// `wasm-pack` emits both published wasm artifacts, so #616 pinned it; this checks that every
// workflow invoking it reads one declared pin, and that the declarations agree. When to MOVE the pin
// is `docs/agents/release.md`'s.
//
// Deliberately NOT checked: whether the pinned version is current — see
// `docs/map/territory/ci-and-supply-chain.md`.
//
// Usage: node .github/scripts/check-tool-pins.mjs [workflow-dir]

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2] ?? ".github/workflows";
const DECL = /^\s*WASM_PACK_VERSION:\s*"([^"]+)"\s*$/;
const USE = /cargo install wasm-pack\b(?<rest>[^\n]*)/;
const EXPECTED_USE = '--version "${{ env.WASM_PACK_VERSION }}"';

const files = readdirSync(dir).filter((f) => f.endsWith(".yml") || f.endsWith(".yaml"));
const declared = []; // { file, line, version }
const uses = []; // { file, line, rest }

for (const file of files) {
  const path = join(dir, file);
  readFileSync(path, "utf8")
    .split(/\r?\n/)
    .forEach((text, i) => {
      const d = DECL.exec(text);
      if (d) declared.push({ path, line: i + 1, version: d[1] });
      const u = USE.exec(text);
      // Skip prose: only a `run:` line actually invokes the tool.
      if (u && /^\s*run:/.test(text)) uses.push({ path, line: i + 1, rest: u.groups.rest });
    });
}

const errors = [];

// 1. Every invocation reads the declared pin — never a literal, never bare.
for (const u of uses) {
  if (!u.rest.includes(EXPECTED_USE)) {
    errors.push(
      `${u.path}:${u.line}: 'cargo install wasm-pack' must pass ${EXPECTED_USE} — found "${u.rest.trim()}". ` +
        `A bare install takes whatever is newest at run time, and a hardcoded version is the copy that drifts.`,
    );
  }
}

// 2. A file that invokes the tool must declare the pin it reads.
for (const u of uses) {
  if (!declared.some((d) => d.path === u.path)) {
    errors.push(
      `${u.path}:${u.line}: invokes wasm-pack but the file declares no WASM_PACK_VERSION — ` +
        `the expression would expand to empty and cargo would reject the command.`,
    );
  }
}

// 3. All declarations agree. This is the failure the check exists for.
const versions = [...new Set(declared.map((d) => d.version))];
if (versions.length > 1) {
  errors.push(
    `WASM_PACK_VERSION disagrees across workflows: ` +
      declared.map((d) => `${d.path}:${d.line} = ${d.version}`).join(", ") +
      `. CI would test the artifact with one tool and publish it with another.`,
  );
}

if (declared.length === 0 && uses.length === 0) {
  console.log("check-tool-pins: no wasm-pack usage found — nothing to check");
  process.exit(0);
}

if (errors.length === 0) {
  console.log(
    `check-tool-pins: wasm-pack pinned to ${versions[0]} in ${declared.length} workflow(s), ` +
      `${uses.length} invocation(s) read it`,
  );
  process.exit(0);
}

for (const e of errors) console.error(`::error::${e}`);
process.exit(1);
