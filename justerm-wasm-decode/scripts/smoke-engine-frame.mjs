// End-to-end decode smoke for a frame a real `Engine` produced (#657).
//
// Sibling of `smoke-decode.mjs`, and deliberately not a replacement. That one decodes a
// hand-built `Frame` struct, which pins the encoder against known values but leaves the
// engine out of the loop — a wire group the engine stopped populating would still round
// trip, because the fixture decides the contents. This one's bytes come from
// `examples/gen_engine_frame.rs`, which feeds VT to `Engine` and encodes whatever came
// out, so `feed -> frame -> encode -> decodeFrame` is observed as one composition.
//
// It asserts the two columns nothing else proves end to end — the grapheme cluster
// (`extra` into `sideTable`) and the OSC 8 hyperlink (`link` into `linkTable`) — plus
// the span directory's stride-5 layout, which is a *meaning* no type can carry.
//
// Every expected value below is read off `gen_engine_frame.rs`'s `INPUT`, not guessed.
//
// Usage: `node scripts/smoke-engine-frame.mjs <nodejs-pkg-dir> <fixture-file>`
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const pkgDir = resolve(process.argv[2] ?? "");
const fixturePath = resolve(process.argv[3] ?? "");
if (!process.argv[2] || !process.argv[3]) {
  console.error("usage: smoke-engine-frame.mjs <nodejs-pkg-dir> <fixture-file>");
  process.exit(1);
}

function fail(msg) {
  console.error(`smoke-engine-frame FAIL: ${msg}`);
  process.exit(1);
}
const eq = (actual, expected, what) => {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) fail(`${what}: got ${a}, expected ${e}`);
};

// nodejs target is CommonJS; the colour helpers are ESM.
const require = createRequire(import.meta.url);
const wasm = require(join(pkgDir, "justerm_wasm_decode.js"));
const colors = await import(pathToFileURL(join(pkgDir, "colors.js")));

const frame = wasm.decodeFrame(new Uint8Array(readFileSync(fixturePath)));

// The engine printed "ReLINK" from column 0 of row 0 and left the cursor after it, so the
// damage run is seven cells wide. `1` is Partial.
eq([frame.cols, frame.rows, frame.kind], [80, 24, 1], "cols/rows/kind");

// Span directory, stride 5: (line, left, right, cell_offset, count). The stride and the
// order of these five fields are pure convention — identical `Uint32Array`s carry any
// other layout equally well, which is why asserting them needs a real frame.
eq(Array.from(frame.spans), [0, 0, 6, 0, 7], "span directory");

const text = Array.from(frame.codepoints)
  .map((c) => String.fromCodePoint(c))
  .join("");
eq(text, "ReLINK ", "codepoints");

// The grapheme cluster. `extra` is 1-based into `sideTable`; `0` is "no cluster".
eq(Array.from(frame.extra), [0, 1, 0, 0, 0, 0, 0], "extra (cluster indices)");
eq(frame.sideTable, ["́"], "sideTable");

// The load-bearing half of that pair, and the reason a value-level check exists at all: the
// side table holds the cell's **trailing combining marks only** — the base character stays
// in `codepoints`. A consumer that renders `sideTable[extra - 1]` as the cell's text drops
// the base and draws a bare accent. Nothing in the types says which of the two it is.
if (text[1] !== "e") fail(`the base character left codepoints[1]: got ${JSON.stringify(text[1])}`);
if (frame.sideTable[0].includes("e")) {
  fail(`sideTable carries the whole grapheme, not just the marks: ${JSON.stringify(frame.sideTable[0])}`);
}

// The OSC 8 hyperlink: cells 2..5 ("LINK") carry a 1-based index into `linkTable`.
eq(Array.from(frame.link), [0, 0, 1, 1, 1, 1, 0], "link (hyperlink indices)");
eq(frame.linkTable, ["https://example.com"], "linkTable");

// Colour, resolved the way a consumer does. Index 196 is the xterm cube's pure red, so the
// SGR the engine parsed has to survive as a tagged reference all the way across.
const palette = {
  colors: wasm.buildPalette(new Uint32Array(16)),
  defaultFg: 0x000000,
  defaultBg: 0x000000,
};
const fgRgb = colors.resolveRgb(frame.fg[0], palette, colors.FG);
if (fgRgb !== 0xff0000) {
  fail(`resolveRgb(fg[0]) = 0x${fgRgb.toString(16)}, expected 0xff0000 (Indexed 196)`);
}

frame.free();
console.log(
  "smoke-engine-frame OK: engine -> frame -> encode -> decodeFrame, cluster and link resolved",
);
