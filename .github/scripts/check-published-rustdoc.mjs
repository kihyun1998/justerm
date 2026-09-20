// Fail a PR whose RENDERED rustdoc points at something only this repo can resolve.
//
// A crate that reaches crates.io gets a docs.rs page built from its `///` and `//!` comments, and
// that page — not the README — is what a consumer lands on from the "Documentation" link. The prose
// ships verbatim: `docs/map/territory/published-surface.md` records the class already, as
// `Engine::resize` carrying *"(Soft-wrap reflow lands in #7.)"* on docs.rs for six weeks after #7
// closed. Measured on the published `justerm-core` 0.21.0: **407 bare pointers across 54 of 58 item
// pages**, and on `justerm` 0.5.1 a `//!` header ending "See ADR-0010 in the repository."
//
// This is `check-published-pointers.mjs`'s rule on a third surface, and the rule is the same one:
// a **bare** ADR or issue number is rejected, a linked one passes, because a doc-comment renders
// markdown and can therefore carry a URL. `[#844](https://github.com/…/issues/844)` is fine.
//
// Why a separate script rather than a branch inside that one: their PRECONDITIONS differ. That one
// reads files in the checkout and runs from a clean clone with no toolchain; this one reads
// `cargo doc` output and is meaningless before the build. Folding them together would make the
// cheap per-PR check depend on a Rust build, and a gate that cannot run locally stops being run.
//
// ## What it scans, and why that is the crate's own prose
//
// Rustdoc emits two kinds of text into a page, and only one of them is this repository's to edit:
//
//   - `<div class="docblock">` — the item's OWN documentation, which is a `///` or `//!` in this
//     tree. This is what the gate reads.
//   - `<dd>` in an item table — a one-line summary of a listed item, which is a truncation of the
//     docblock on that item's own page.
//
// Reading docblocks only therefore loses nothing: measured on `justerm-core`, all 28 distinct
// pointers appearing in an index `<dd>` also appear in some page's docblock.
//
// ## A re-export of a dependency carries prose this repo cannot edit
//
// Rustdoc INLINES a `pub use` of another crate's item — not as a link, but as a full generated page
// under this crate's doc directory, carrying that dependency's published doc-comments. `justerm`
// (the 0.5.1 tombstone) is the live case: it re-exports `justerm-core = "0.6"`, so 37 of its 38
// pages are core 0.6's, already on crates.io and immutable. Reporting them would be 57 findings
// nobody can act on.
//
// The discriminator is derived rather than listed: a page rustdoc generated from THIS crate's own
// source links into that crate's own source view (`../src/<module>/…`), and an inlined foreign page
// has no such link because the dependency's source was never rendered here. Measured: it splits
// `justerm` into 1 own page (its `//!`) + 37 foreign, and `justerm-core` into 60 own + `all.html`,
// which is a generated index holding no docblock at all.
//
// ## Deliberately NOT checked
//
// **The source view.** Rustdoc also emits `src/<crate>/*.rs.html`, reachable from every item's
// `Source` link, and it is the whole file — 1118 hits for `justerm-core`, including 760 ordinary
// `//` comments and the `//!` headers of private modules. `CLAUDE.md` sends the why, the trap and
// the measured value to `docs/map/`, so a `//` comment naming a territory note is that policy
// working, not a defect. The line drawn here is what a reader is shown BY DEFAULT versus what they
// get after clicking into the source.
//
// **Whether the prose is accurate or well written.** No machine judges that; neither does this.
//
// Usage: node .github/scripts/check-published-rustdoc.mjs
//   Run it AFTER `cargo doc`. The crate list is derived rather than restated: a `[package]` whose
//   manifest does not say `publish = false` is one that reaches crates.io, so docs.rs builds it —
//   which means a newly published crate is covered on the day it is added. A crate whose docs are
//   missing is a hard error, never a silent pass: `justerm-facade` is outside the root workspace
//   (`docs/map/invariant/workspace-exclusion-is-gate-invisibility.md`), so `cargo doc --workspace`
//   does not build it and only its own `--manifest-path` run does.

import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join } from "node:path";

const SKIP_DIRS = new Set(["node_modules", "target", "pkg", "pkg-bundler", "pkg-web", "pkg-node", ".git", "dist"]);

// Each pattern is a pointer that resolves only inside this repository. Kept identical to
// `check-published-pointers.mjs` so the two surfaces cannot drift into different rules.
const UNRESOLVABLE = [
  [/\bADR[-\s]?\d+/gi, "an ADR reference"],
  [/#\d+/g, "an issue reference"],
  [/\b(?:CLAUDE|CONTEXT)\.md\b|\bdocs\/(?:map|adr|agents|architecture)[A-Za-z0-9/._-]*/g, "a repo-only path"],
];

/** Every Cargo manifest in the tree, so the crate list is walked rather than listed. */
function manifests(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (SKIP_DIRS.has(entry)) continue;
    if (statSync(path).isDirectory()) manifests(path, out);
    else if (entry === "Cargo.toml") out.push(path);
  }
  return out;
}

/**
 * The crate name if this manifest reaches crates.io, else null.
 *
 * `publish = false` is the only thing that stops a `[package]` from being published there — and it
 * does NOT mean "unpublished", which is the trap `check-published-pointers.mjs` was written for:
 * `justerm-wasm-decode` and `justerm-renderer` carry it and still reach npm through wasm-pack. What
 * it does mean is no crates.io release, and therefore no docs.rs page — which is this gate's
 * subject, so here the flag is decisive.
 */
function cratesIoName(path) {
  const text = readFileSync(path, "utf8");
  if (!/^\[package\]/m.test(text)) return null; // a virtual manifest publishes nothing
  if (/^publish\s*=\s*false/m.test(text)) return null;
  return text.match(/^name\s*=\s*"([^"]+)"/m)?.[1] ?? null;
}

/**
 * Every `<div class="docblock…">` in a page, with div nesting BALANCED.
 *
 * A non-greedy `…</div>` stops at the first close tag, and a docblock containing any nested `<div>`
 * — which every rendered code example does — is truncated there.
 *
 * **Measured, because the honest version of this claim is narrower than it reads**: swapping in the
 * non-greedy one-liner drops 46_836 of 505_293 scanned characters (9.3%) across `justerm-core`'s own
 * pages and finds **the same 372 pointers**. The text it loses is rendered examples, and no pointer
 * sits after a nested `<div>` inside its own block today — so this function is *correct* rather than
 * currently load-bearing, and a mutation of it does not redden. That is why the counters below are
 * printed on every run: a shrink here cannot fail the gate, so it has to be visible instead.
 *
 * Code is scanned along with prose deliberately: measured 0 of the 372 sit inside a `<pre>` or
 * `<code>`, so excluding them would buy nothing and would add a second thing to keep true.
 */
function docblocks(html) {
  const out = [];
  const open = /<div class="docblock[^"]*"[^>]*>/g;
  let m;
  while ((m = open.exec(html))) {
    let depth = 1;
    const from = m.index + m[0].length;
    const tag = /<\/?div\b[^>]*>/g;
    tag.lastIndex = from;
    let t;
    while (depth > 0 && (t = tag.exec(html))) {
      depth += t[0].startsWith("</") ? -1 : 1;
      if (depth === 0) out.push(html.slice(from, t.index));
    }
  }
  return out;
}

/**
 * The prose a reader sees, with every `<a>…</a>` blanked out first.
 *
 * A pointer that was linked is resolvable and not this gate's business. Blanking label and target
 * together keeps the label from being re-read as a bare reference.
 *
 * Entities are decoded only AFTER tags are stripped, because decoding first would turn `&lt;div&gt;`
 * into a tag, and `&amp;` is decoded last so that an `&amp;lt;` cannot become a `<`. Measured over
 * `justerm-core`'s own docblocks: `&amp;` 427, `&lt;` 147, `&gt;` 120 — and **no numeric entity at
 * all**. Rustdoc writes a plain apostrophe as `’` and leaves the one in `` `&'a str` `` literal, so
 * the numeric branch matches nothing here today; it is kept because the failure it would prevent is
 * silent (`&#39;` read as the issue reference `#39`) and costs nothing to hold.
 */
function prose(html) {
  return html
    .replace(/<a\b[^>]*>[\s\S]*?<\/a>/g, (link) => link.replace(/[^\n]/g, " "))
    .replace(/<(script|style)[\s\S]*?<\/\1>/g, " ")
    .replace(/<[^>]*>/g, " ")
    .replace(/&#(\d+);/g, (_, n) => String.fromCharCode(Number(n)))
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&");
}

/** Where `cargo doc` puts a crate's pages — in the workspace target, or the crate's own. */
function docDir(manifestPath, crate) {
  const module = crate.replace(/-/g, "_");
  const own = join(manifestPath.replace(/Cargo\.toml$/, ""), "target", "doc", module);
  return [join("target", "doc", module), own].find((d) => existsSync(d)) ?? null;
}

const crates = [];
for (const path of manifests(".")) {
  const crate = cratesIoName(path);
  if (crate) crates.push({ path, crate, dir: docDir(path, crate) });
}

if (crates.length === 0) {
  console.error("::error::found no crates.io crates to check — the walk is broken, not the tree");
  process.exit(2);
}

const missing = crates.filter((c) => c.dir === null);
if (missing.length > 0) {
  for (const c of missing) {
    console.error(
      `::error file=${c.path}::${c.crate} reaches crates.io, so docs.rs builds it, but its rendered ` +
        `docs are not in this tree — nothing was scanned for it. Run \`cargo doc\` first; a crate ` +
        `outside the root workspace needs its own \`--manifest-path\` run.`,
    );
  }
  process.exit(2);
}

const hits = [];
let pages = 0;
let inlined = 0;
let blocks = 0;
let chars = 0;
for (const { crate, dir } of crates) {
  const module = crate.replace(/-/g, "_");
  for (const file of readdirSync(dir)) {
    if (!file.endsWith(".html")) continue;
    const html = readFileSync(join(dir, file), "utf8");
    // A page generated from this crate's own source links into its own source view. One inlined
    // from a dependency does not, and its prose is already published and unreachable from here.
    if (!html.includes(`src/${module}/`)) {
      inlined++;
      continue;
    }
    pages++;
    for (const block of docblocks(html)) {
      blocks++;
      chars += block.length;
      const text = prose(block);
      for (const [re, what] of UNRESOLVABLE) {
        for (const m of text.matchAll(re)) {
          const at = Math.max(0, m.index - 70);
          hits.push({ crate, file, what, quote: m[0], context: text.slice(at, m.index + 70).replace(/\s+/g, " ").trim() });
        }
      }
    }
  }
}

if (pages === 0) {
  console.error("::error::every page was read as inlined from a dependency — the ownership test is broken, not the tree");
  process.exit(2);
}

// What was read, printed whether or not anything was found. A docblock extractor that silently
// started truncating cannot redden this gate (see `docblocks`), so the size of what it scanned is
// reported instead of asserted — a floor here would be a threshold to quietly lower later.
const scanned =
  `${pages} own page(s), ${blocks} docblock(s), ${chars} chars; ` +
  `${inlined} page(s) inlined from a dependency and skipped`;

if (hits.length === 0) {
  console.log(`no repo-only pointers in the rendered docs of ${crates.length} crates.io crate(s) — ${scanned}:`);
  for (const c of crates) console.log(`  ${c.crate}  (${c.dir})`);
  process.exit(0);
}

const byPage = new Map();
for (const h of hits) byPage.set(`${h.crate}/${h.file}`, (byPage.get(`${h.crate}/${h.file}`) ?? 0) + 1);

for (const h of hits) {
  console.error(
    `::error::${h.crate} — ${h.file}: a doc-comment carries ${h.what} ("${h.quote}") that a docs.rs ` +
      `reader cannot resolve: …${h.context}… ` +
      `Either link it ([${h.quote}](https://github.com/kihyun1998/justerm/…)) or say the thing itself — ` +
      `the number means nothing to someone reading this on docs.rs.`,
  );
}
console.error(`\n${hits.length} bare pointer(s) across ${byPage.size} rendered page(s) — scanned ${scanned}.`);
process.exit(1);
