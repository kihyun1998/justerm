// Fail a PR whose generated `.d.ts` carries prose written for a reader of this repository.
//
// wasm-bindgen copies a `///` comment **verbatim** into the `.d.ts` it generates, and that file is
// what an editor shows on hover — read more often than the README by anyone actually calling the
// API. Neither crate is on docs.rs (`publish = false`, npm only), so rustdoc's syntax resolves in
// no context that ships.
//
// Measured on the published 0.21.0 tarballs (`npm pack`), which is the artifact, not a proxy:
//
//                              #NNN   ADR   [`x`](target)   [`x`]
//   justerm_renderer.d.ts       101    19            51        4
//   justerm_wasm_decode.d.ts     39     5             4       19
//
// Three classes, and they are NOT equally bad:
//
//   1. **A bare `#NNN` or `ADR-NNNN`** — the same defect #949 fixed in the descriptions and READMEs
//      and #953 in the rendered rustdoc, arriving on the third published surface. 164 of them.
//   2. **A dead link target.** Every one of the 55 explicit links is `](Self::x)`, which is not a
//      URL in any context that ships. So "does the target resolve" is already answered for all of
//      them: no. This class is noise rather than a trap.
//   3. **A label naming something the reader cannot call.** THIS is the trap, and it is why the
//      gate exists. `addGrid`'s own tooltip says *"draws only once [`set_viewport`] says where"* —
//      `set_viewport` is `setViewport`, so a consumer following the tooltip calls a method that is
//      not there. 14 in the renderer, 6 in the decoder, including `MARKER_STRIDE`, which is
//      declared under no spelling at all.
//
// Class 3 is checked against the SAME FILE's own declarations, so it needs no roster: a label is
// broken iff the `.d.ts` that carries it does not declare it. That also makes the check immune to
// a rename — both halves move together or the gate fires.
//
// ## Where it runs, and why it takes an argument
//
// The two packages are built by two different CI jobs — `wasm` runs `wasm-pack build` for the
// decoder, `renderer-proofs` reaches the renderer's through `pnpm run test:proofs` -> `build:wasm`.
// Neither job has the other's artifact, so this takes the package directory to check and is invoked
// once in each. A directory holding no `.d.ts` is a hard error: "nothing to scan" and "nothing
// wrong" must not look alike.
//
// Usage: node .github/scripts/check-published-dts.mjs <pkg-dir> [<pkg-dir>...]

import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { join } from "node:path";

const dirs = process.argv.slice(2);
if (dirs.length === 0) {
  console.error("::error::usage: check-published-dts.mjs <pkg-dir> [...] — no directory given, so nothing was checked");
  process.exit(2);
}

/** Every declaration name the file itself publishes — what a consumer can actually call. */
function declaredNames(src) {
  const out = new Set();
  // A class member / interface property, indented (wasm-bindgen emits 4 spaces; be width-agnostic).
  for (const m of src.matchAll(/^\s+(?:readonly\s+|static\s+)?([A-Za-z_$][\w$]*)\s*[(:<]/gm)) out.add(m[1]);
  // A const-enum member.
  for (const m of src.matchAll(/^\s+([A-Za-z_$][\w$]*)\s*=\s*[\d"']/gm)) out.add(m[1]);
  // Top-level exports.
  for (const m of src.matchAll(/^export\s+(?:declare\s+)?(?:function|class|interface|enum|const|type|let|var)\s+([A-Za-z_$][\w$]*)/gm)) out.add(m[1]);
  return out;
}

const REPO_ONLY = [
  [/\bADR[-\s]?\d+/gi, "an ADR reference"],
  [/#\d+/g, "an issue reference"],
  [/\b(?:CLAUDE|CONTEXT)\.md\b|\bdocs\/(?:map|adr|agents|architecture)[A-Za-z0-9/._-]*/g, "a repo-only path"],
];

/** Only the doc-comment lines: a ` * ` block. Declarations themselves are not prose. */
function docProse(src) {
  return src
    .split(/\r?\n/)
    .filter((l) => /^\s*\*/.test(l))
    .join("\n");
}

const findings = [];
let filesScanned = 0;
const perDir = [];

for (const dir of dirs) {
  if (!existsSync(dir) || !statSync(dir).isDirectory()) {
    console.error(`::error::${dir} is not a directory — the package was not built, so nothing was scanned`);
    process.exit(2);
  }
  // `*_bg.wasm.d.ts` is a machine-written import stub with no prose in it.
  const dts = readdirSync(dir).filter((f) => f.endsWith(".d.ts") && !f.endsWith("_bg.wasm.d.ts"));
  if (dts.length === 0) {
    console.error(`::error::${dir} holds no .d.ts — "nothing to scan" is not "nothing wrong"`);
    process.exit(2);
  }
  for (const file of dts) {
    filesScanned++;
    const src = readFileSync(join(dir, file), "utf8");
    const prose = docProse(src);
    const declared = declaredNames(src);
    let n = 0;

    for (const [re, what] of REPO_ONLY) {
      for (const m of prose.matchAll(re)) {
        findings.push({ file, kind: what, quote: m[0], hint: "say the thing itself — the number means nothing to someone reading this on npm" });
        n++;
      }
    }
    // A rustdoc link of any shape. The target is dead in every case; the LABEL is the trap.
    for (const m of prose.matchAll(/\[`([^`]+)`\](\([^)]*\))?/g)) {
      const label = m[1].replace(/\(\)$/, "").split("::").pop();
      const broken = !declared.has(label);
      findings.push({
        file,
        kind: broken ? "a link whose LABEL names no declaration in this file" : "rustdoc link syntax, which resolves nowhere here",
        quote: m[0],
        hint: broken
          ? `"${label}" is not callable; this file declares no such name. Use the name it ships under, as plain inline code.`
          : "drop the brackets and the target — a `.d.ts` has no page to link to; plain inline code reads correctly.",
      });
      n++;
    }
    perDir.push({ dir, file, n });
  }
}

if (findings.length === 0) {
  console.log(`no repo-only prose in ${filesScanned} generated .d.ts file(s):`);
  for (const p of perDir) console.log(`  ${p.dir}/${p.file}`);
  process.exit(0);
}

// Broken labels first: they are the ones that send a caller to a method that does not exist.
findings.sort((a, b) => Number(b.kind.includes("LABEL")) - Number(a.kind.includes("LABEL")));
for (const f of findings) {
  console.error(`::error::${f.file}: ${f.kind} — \`${f.quote}\`. ${f.hint}`);
}
const broken = findings.filter((f) => f.kind.includes("LABEL")).length;
console.error(
  `\n${findings.length} finding(s) across ${filesScanned} generated .d.ts file(s); ` +
    `${broken} of them name something the reader cannot call.`,
);
process.exit(1);
