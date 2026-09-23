// Fail a PR whose PUBLISHED NPM PACKAGE carries prose written for a reader of this repository.
//
// Every file in the package's own `files` allowlist is scanned for a bare `#NNN` / `ADR-NNNN`, and
// every rustdoc link label is checked against the same file's declarations — a label naming a method
// the file does not declare sends a caller to something that is not there. A directory that is not a
// built package is a hard error. Why each rule, and the 0.21.0 measurement:
// `docs/map/territory/published-surface.md`.
//
// Usage: node .github/scripts/check-published-package.mjs <pkg-dir> [<pkg-dir>...]
//
// **Running it locally after editing a doc-comment: `touch` the source first**, or cargo reuses the
// crate and `wasm-pack build` re-emits the previous `.d.ts`.

import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { join } from "node:path";

const dirs = process.argv.slice(2);
if (dirs.length === 0) {
  console.error("::error::usage: check-published-package.mjs <pkg-dir> [...] — no directory given, so nothing was checked");
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

/**
 * Every comment line in the file; a declaration is not prose.
 *
 * Both comment forms count, and the `//` half is not hypothetical: `colors.d.ts` is hand-written
 * rather than generated and opens with `// Types for the justerm-wasm-decode colour helpers (#36).`
 * A first version of this read only ` * ` lines and reported that file **clean**, which is the
 * shape of blind spot this whole issue is about — an instrument that cannot see a thing and a thing
 * that is not there produce the same output.
 *
 * A `//` line is weaker than a JSDoc block: tsserver surfaces only `/** … *\/` attached to a
 * declaration, so a hover never shows it. It is still in the published file, and these files are
 * small enough to be opened.
 */
function docProse(src) {
  return src
    .split(/\r?\n/)
    .filter((l) => /^\s*(\*|\/\/)/.test(l))
    .join("\n");
}

/**
 * A real markdown link — one whose target is a URL — blanked at equal length.
 *
 * The bare-vs-linked rule of `check-published-pointers.mjs` applies here for the same reason it
 * applies to a README: this prose CAN carry a URL and have it work. A `.d.ts` comment is JSDoc,
 * tsserver hands it to the editor as markdown, and the hover renders it as a clickable link. So
 * `[ADR-0017](https://…)` is resolvable from where it is printed and is not this gate's business,
 * while a bare `ADR-0017` is. Blanking at equal length keeps the label from being re-read as a bare
 * reference, and keeps the URL's own `docs/adr/…` path from counting as a repo-only path.
 *
 * Only `http` targets are blanked. `](Self::x)` is not a URL, and that is exactly what the link
 * check below exists to report.
 */
const blankUrlLinks = (s) => s.replace(/\[[^\]]*\]\(https?:\/\/[^)]*\)/g, (l) => l.replace(/[^\n]/g, " "));

/**
 * Every text file the package actually publishes, taken from `package.json`'s own `files`.
 *
 * **Derived, because guessing the extension is what this gate got wrong the first time.** It
 * scanned `*.d.ts`, which read as "the typings surface" and was really "the files I thought of":
 * `colors.js` is hand-written, ships beside `colors.d.ts`, and carried three repo-only pointers
 * that no run could see. `finish-pkg.mjs` pushes both onto `files` in the same line, so the answer
 * was already in the package — and `files` is npm's own allowlist, so anything not on it is not
 * published and anything added later is covered on the day it is added.
 *
 * `.wasm` is the only thing dropped: it is a binary, and a byte sequence matching `#\d+` in it is
 * not prose. `*_bg.js` stays even though it carries the same doc-comments as the `.d.ts` beside it
 * — measured at 19 and 5 identical occurrences, so it cannot fail alone today, but it is published
 * text and nothing guarantees that stays true. A `.map` is dropped for the same reason as `.wasm`:
 * it is generated JSON whose `sourcesContent` is a copy of files gated at their source.
 *
 * **An entry may be a directory**, and `justerm-web` is why this is not hypothetical: its `files` is
 * `["dist", …]`, and the 531 pointers in `dist/index.d.ts` — a package larger than either wasm one
 * — were outside this gate until it walked one.
 */
function publishedFiles(dir) {
  const manifest = join(dir, "package.json");
  if (!existsSync(manifest)) {
    console.error(`::error::${dir} has no package.json — this is not a built package, so its published set cannot be derived`);
    process.exit(2);
  }
  const files = JSON.parse(readFileSync(manifest, "utf8")).files;
  if (!Array.isArray(files) || files.length === 0) {
    console.error(`::error::${manifest} lists no \`files\` — npm's allowlist is what decides here, and an empty one is a broken package, not a clean one`);
    process.exit(2);
  }
  const skip = (f) => f.endsWith(".wasm") || f.endsWith(".map");
  const walk = (rel) => {
    const abs = join(dir, rel);
    if (!existsSync(abs)) return [];
    if (!statSync(abs).isDirectory()) return skip(rel) ? [] : [rel];
    return readdirSync(abs).flatMap((e) => walk(join(rel, e)));
  };
  return ["package.json", ...files].flatMap(walk);
}

const findings = [];
let filesScanned = 0;
const perDir = [];

for (const dir of dirs) {
  if (!existsSync(dir) || !statSync(dir).isDirectory()) {
    console.error(`::error::${dir} is not a directory — the package was not built, so nothing was scanned`);
    process.exit(2);
  }
  const shipped = publishedFiles(dir);
  if (shipped.length === 0) {
    console.error(`::error::${dir} publishes no text file — "nothing to scan" is not "nothing wrong"`);
    process.exit(2);
  }
  for (const file of shipped) {
    filesScanned++;
    const src = readFileSync(join(dir, file), "utf8");
    const prose = blankUrlLinks(docProse(src));
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
  console.log(`no repo-only prose in ${filesScanned} published file(s):`);
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
  `\n${findings.length} finding(s) across ${filesScanned} published file(s); ` +
    `${broken} of them name something the reader cannot call.`,
);
process.exit(1);
