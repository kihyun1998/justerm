// Post-build: fold the hand-written colour helpers and the licences into a wasm-pack output dir.
//
// wasm-pack emits only the wasm-bindgen surface (decodeFrame / buildPalette /
// flags / DecodedFrame). The per-cell colour helpers (resolveRgb / decodeColorRef)
// are hand-written JS (AC3: no per-cell WASM crossing), so they ship as a sibling
// module the consumer imports from `justerm-wasm-decode/colors.js`. This copies them into
// the package and lists them in `files` so they are published.
//
// The licences are the same story, and they were missing from the published tarball up to and
// including 0.6.0. npm auto-includes only `license`, `licence` or `copying` (optionally with a dot
// extension) — `npm-packlist/lib/index.js` injects `'!/license{,.*[^~$]}'` and friends — so
// `LICENSE-MIT` and `LICENSE-APACHE` are NOT auto-included, and `package.json`'s `files` allowlist
// then excludes them outright. Copying them next to the package (as the publish workflow used to do)
// achieves nothing on its own: they must also be listed. Declaring `license: "MIT OR Apache-2.0"`
// without shipping the texts breaks both licences (Apache-2.0 §4(a); MIT's "include this notice").
//
// Usage: `node scripts/finish-pkg.mjs <out-dir>` (run after `wasm-pack build`).
import { execFileSync } from "node:child_process";
import { copyFileSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const crateDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const out = process.argv[2];
if (!out) {
  console.error("usage: finish-pkg.mjs <out-dir>");
  process.exit(1);
}
const outDir = resolve(out);

for (const f of ["colors.js", "colors.d.ts"]) {
  copyFileSync(join(crateDir, "js", f), join(outDir, f));
}

// The licences live at the repo root, one level above the crate.
const LICENSES = ["LICENSE-MIT", "LICENSE-APACHE"];
for (const f of LICENSES) {
  copyFileSync(join(crateDir, "..", f), join(outDir, f));
}

// wasm-pack does not resolve `version.workspace = true`, so it writes a wrong
// version into package.json. Overwrite it with the version cargo resolved, so the
// npm artifact stays version-locked to the crate (the publish gate also checks it).
const meta = JSON.parse(
  execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
    cwd: crateDir,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  }),
);
const version = meta.packages.find((p) => p.name === "justerm-wasm-decode").version;

const pkgPath = join(outDir, "package.json");
const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
pkg.version = version;
// Copying is not enough: `files` is an allowlist, so anything not listed here is dropped from the
// tarball even though it sits in the directory.
for (const f of ["colors.js", "colors.d.ts", ...LICENSES]) {
  if (!pkg.files.includes(f)) pkg.files.push(f);
}
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
console.log(`finish-pkg: ${out} -> version ${version}, + colors.js/.d.ts, + ${LICENSES.join(" ")}`);
