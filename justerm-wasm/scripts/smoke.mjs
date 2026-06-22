// Smoke-test an assembled wasm-pack output dir the way a consumer imports it.
//
// Unit/boundary tests only see the source; this checks the packaging glue that
// `finish-pkg.mjs` produced — the colour helpers are present and compute correct
// values, and package.json is version-locked to the crate and lists them. Exits
// non-zero on any failure so CI fails on a packaging regression (#37).
//
// Usage: `node scripts/smoke.mjs <out-dir>` (run after build + finish-pkg).
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const crateDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const arg = process.argv[2];
if (!arg) {
  console.error("usage: smoke.mjs <out-dir>");
  process.exit(1);
}
const out = resolve(arg);

function fail(msg) {
  console.error(`smoke FAIL (${arg}): ${msg}`);
  process.exit(1);
}

// 1. Colour helpers import from the assembled package and compute known values.
const colors = await import(pathToFileURL(join(out, "colors.js")));
for (const name of ["resolveRgb", "decodeColorRef", "FG", "BG"]) {
  if (!(name in colors)) fail(`colors.js missing export ${name}`);
}
const palette = { colors: new Uint32Array(256), defaultFg: 0x111111, defaultBg: 0x222222 };
if (colors.resolveRgb((2 << 24) | 0x0a141e, palette, colors.FG) !== 0x0a141e) {
  fail("resolveRgb Rgb passthrough");
}
if (colors.resolveRgb(0, palette, colors.FG) !== 0x111111) fail("resolveRgb Default fg");
if (colors.resolveRgb(0, palette, colors.BG) !== 0x222222) fail("resolveRgb Default bg");
if (colors.decodeColorRef((1 << 24) | 196).index !== 196) fail("decodeColorRef Indexed");

// 2. package.json is version-locked to the crate and ships the colour helpers.
const pkg = JSON.parse(readFileSync(join(out, "package.json"), "utf8"));
const meta = JSON.parse(
  execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
    cwd: crateDir,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  }),
);
const want = meta.packages.find((p) => p.name === "justerm-wasm").version;
if (pkg.version !== want) fail(`package.json version ${pkg.version} != crate ${want}`);
for (const f of ["colors.js", "colors.d.ts"]) {
  if (!pkg.files.includes(f)) fail(`package.json files missing ${f}`);
}

console.log(`smoke OK: ${arg} (v${pkg.version})`);
