// Fail a PR whose published prose points at something only this repo can resolve.
//
// A package's `description` and its README are not internal notes — they are what a registry prints
// to someone who has never seen this repo. Two crates shipped "… See ADR-0008." and
// "… (ADR-0018, supersedes ADR-0002)" to npm through **21 tags each**, because `publish = false`
// reads like "not published" when it only means "not to crates.io": both reach npm via `wasm-pack`,
// which copies `description` and `readme` from Cargo.toml into the `pkg/package.json` it generates.
// The READMEs alongside them carried bare `#258`, `#776`, `#810` and an "Epic #287, in progress",
// plus a `## Build & test` section of `cargo`/`pnpm` commands aimed at people with a checkout.
//
// The class this catches is narrow on purpose: an **unresolvable internal pointer**, i.e. an ADR
// number or an issue number that the reader cannot follow from where it is printed. The argument is
// structural rather than stylistic, and it differs per surface:
//
//   - `description` is a bare string with nowhere to put a link, so ANY such pointer is
//     unresolvable by construction.
//   - a README can link out, and several do, so only a **bare** pointer is rejected here.
//     `[ADR-0010](https://github.com/…/0010-….md)` is fine; a naked `ADR-0010` or `#776` is not.
//
// Runs on every PR, unlike `check-published-readme.mjs` which runs at publish time. The difference
// is when the text turns false: an expiring maturity claim ("under construction") is honest in the
// repo and only becomes a lie once snapshotted, so it can only be judged at the tag. A repo-only
// pointer is wrong the moment it is typed — so it is caught here, where a fix is a commit rather
// than a re-tag, and where npm's refusal to re-publish a version cannot bite.
//
// Deliberately NOT checked here: whether the prose is *accurate* or well written (a machine cannot
// judge it), expiring maturity claims (the publish-time gate owns those), and contributor-only
// content such as build commands — no pattern separates `pnpm build` in a usage example from
// `pnpm build` in a Develop section, so that stays a human call. It also reads only a single-line
// `description = "…"`; a multi-line TOML string would be skipped silently — no manifest has one
// today, and the walk below reports its own coverage so that stays visible.
//
// Usage: node .github/scripts/check-published-pointers.mjs
//   Walks from the repo root and derives the package list rather than restating it: every manifest
//   that carries a description is one whose description ships, and npm `private: true` is the only
//   thing that stops it. A new published package is therefore covered on the day it is added.
//
//        node .github/scripts/check-published-pointers.mjs --pkg <dir>
//   Scans a built wasm-pack output directory instead. A `.d.ts` is the third published surface and
//   the one an editor shows on hover, but it does not exist until `wasm-pack build` runs — so this
//   mode runs in the CI jobs that already build, not in the PR gate above. wasm-bindgen copies a
//   crate's `///` comments into it verbatim, which is how 101 issue numbers and 51 rustdoc
//   intra-doc links reached npm; 14 of those links named Rust methods the JS API does not have.
//   Nothing there can link, so the `description` rule applies: no pointer at all.

import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";

const SKIP_DIRS = new Set(["node_modules", "target", "pkg", "pkg-bundler", "pkg-web", "pkg-node", ".git", "dist"]);

// Each pattern is a pointer that resolves only inside this repository.
const UNRESOLVABLE = [
  [/\bADR[-\s]?\d+/gi, "an ADR reference"],
  [/#\d+/g, "an issue reference"],
];

function manifests(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (SKIP_DIRS.has(entry)) continue;
    if (statSync(path).isDirectory()) {
      manifests(path, out);
    } else if (entry === "Cargo.toml" || entry === "package.json") {
      out.push(path);
    }
  }
  return out;
}

/** The shipped description, or null when this manifest publishes none. */
function description(path) {
  const text = readFileSync(path, "utf8");
  if (path.endsWith("package.json")) {
    const pkg = JSON.parse(text);
    // `private` is npm's own statement that this manifest never reaches a registry.
    if (pkg.private) return null;
    return pkg.description ?? null;
  }
  // Cargo: `publish = false` does NOT mean unpublished here — wasm-pack lifts the field to npm.
  // Carrying a description at all is the signal that the string travels.
  const m = text.match(/^description\s*=\s*"((?:[^"\\]|\\.)*)"/m);
  return m ? m[1] : null;
}

/**
 * A README with every markdown link blanked out. A pointer that was linked is resolvable and not
 * this gate's business; blanking label and target together keeps the label from being re-read as a
 * bare reference. The replacement is the SAME LENGTH as what it replaces, newlines kept, so every
 * offset still matches the file on disk — otherwise the reported line number drifts upward by
 * however much earlier text was removed, and points the reader at the wrong line.
 */
function readmeProse(path) {
  return readFileSync(path, "utf8").replace(/\[[^\]]*\]\([^)]*\)/g, (link) =>
    link.replace(/[^\n]/g, " "),
  );
}

const pkgArg = process.argv.indexOf("--pkg");
if (pkgArg !== -1) {
  const dir = process.argv[pkgArg + 1];
  if (!dir || !existsSync(dir)) {
    console.error(`::error::--pkg needs a built package directory (got ${dir ?? "nothing"})`);
    process.exit(2);
  }
  // Only the text a consumer reads: typings and the JS glue beside them.
  const files = readdirSync(dir).filter((f) => f.endsWith(".d.ts") || f.endsWith(".js"));
  if (files.length === 0) {
    console.error(`::error::${dir} holds no .d.ts or .js — the build did not produce a package`);
    process.exit(2);
  }
  let bad = 0;
  for (const f of files) {
    const text = readFileSync(join(dir, f), "utf8");
    for (const [re, what] of [...UNRESOLVABLE, [/\]\((?:Self|crate)::/g, "a rustdoc intra-doc link"]]) {
      for (const m of text.matchAll(re)) {
        const line = text.slice(0, m.index).split("\n").length;
        bad++;
        console.error(
          `::error file=${join(dir, f)},line=${line}::${f}:${line} carries ${what} ("${m[0]}") — ` +
            `wasm-bindgen copied it out of a \`///\` comment. This is what an editor shows on hover, ` +
            `and nothing in a .d.ts can link. Say what the method does; a rustdoc link must name the ` +
            `JS method instead, since the Rust name is often not the exported one.`,
        );
      }
    }
  }
  if (bad === 0) console.log(`${dir}: ${files.length} shipped files carry no repo-only pointers`);
  process.exit(bad === 0 ? 0 : 1);
}

const surfaces = [];
for (const path of manifests(".")) {
  const desc = description(path);
  if (desc === null) continue;
  surfaces.push({ label: `${path} (description)`, file: path, text: desc, bareOnly: false });
  const readme = join(dirname(path), "README.md");
  // Registries force-include a README next to a published manifest whether or not `files` lists it.
  if (existsSync(readme)) {
    surfaces.push({ label: `${readme} (published README)`, file: readme, text: readmeProse(readme), bareOnly: true });
  }
}

if (surfaces.length === 0) {
  console.error("::error::found no published surfaces to check — the walk is broken, not the tree");
  process.exit(2);
}

const hits = [];
for (const s of surfaces) {
  for (const [re, what] of UNRESOLVABLE) {
    for (const m of s.text.matchAll(re)) {
      const line = s.text.slice(0, m.index).split("\n").length;
      hits.push({ ...s, what, quote: m[0], line });
    }
  }
}

if (hits.length === 0) {
  console.log(`no repo-only pointers across ${surfaces.length} published surfaces:`);
  for (const s of surfaces) console.log(`  ${s.label}`);
  process.exit(0);
}

for (const h of hits) {
  const where = h.bareOnly ? `${h.file}:${h.line}` : h.file;
  console.error(
    `::error file=${h.file},line=${h.line}::${where}: ${h.bareOnly ? "published README" : "`description`"} ` +
      `carries ${h.what} ("${h.quote}") that a registry reader cannot resolve. ` +
      (h.bareOnly
        ? `Either link it ([${h.quote}](https://github.com/kihyun1998/justerm/…)) or say the thing ` +
          `itself — the number means nothing to someone reading this on npm or crates.io.`
        : `A description has no room for a link: say what the package does, and put the pointer in ` +
          `the crate's \`//!\` header or its README.`),
  );
}
process.exit(1);
