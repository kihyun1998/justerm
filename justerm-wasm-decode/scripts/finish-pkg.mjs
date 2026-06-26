// Post-build: fold the hand-written colour helpers into a wasm-pack output dir.
//
// wasm-pack emits only the wasm-bindgen surface (decodeFrame / buildPalette /
// flags / DecodedFrame). The per-cell colour helpers (resolveRgb / decodeColorRef)
// are hand-written JS (AC3: no per-cell WASM crossing), so they ship as a sibling
// module the consumer imports from `justerm-wasm-decode/colors.js`. This copies them into
// the package and lists them in `files` so they are published.
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
for (const f of ["colors.js", "colors.d.ts"]) {
  if (!pkg.files.includes(f)) pkg.files.push(f);
}
writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
console.log(`finish-pkg: ${out} -> version ${version}, + colors.js/.d.ts`);
