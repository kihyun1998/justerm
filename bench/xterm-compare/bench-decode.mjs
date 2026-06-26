// Time the WASM per-frame decode penterm's webview runs: justerm-wasm-decode
// decodeFrame(wireBytes) -> structure-of-arrays. This is the ONLY WASM cost in
// penterm's pipeline (feed() runs native in the Tauri backend; beamterm renders
// the SoA on the GPU, out of scope here). Matched to examples/time_decode.rs:
// batched (decode is µs-scale), median ns/frame, frames/sec.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const HERE = dirname(fileURLToPath(import.meta.url));
const wasm = require(join(HERE, '..', '..', 'justerm-wasm-decode', 'pkg-node', 'justerm_wasm_decode.js'));

const framePath = process.argv[2];
const bytes = new Uint8Array(readFileSync(framePath));

const WARMUP = 5, SAMPLES = 15, DECODES_PER = 2000;
const median = xs => { const s = [...xs].sort((a, b) => a - b); return s[s.length >> 1]; };

// Whether to also force the renderer's array reads (decodeFrame is lazy via
// getters that mint the typed-array views on access).
const touch = process.argv.includes('--touch');

function batch() {
  const t0 = process.hrtime.bigint();
  for (let i = 0; i < DECODES_PER; i++) {
    const f = wasm.decodeFrame(bytes);
    if (touch) { void f.codepoints.length; void f.fg.length; void f.bg.length; void f.flags.length; }
    f.free?.();
  }
  const t1 = process.hrtime.bigint();
  return Number(t1 - t0) / 1e9 / DECODES_PER;
}

// sanity: the decode actually produced the frame
const f0 = wasm.decodeFrame(bytes);
console.error(`decoded frame: ${f0.cols}x${f0.rows} kind=${f0.kind} codepoints=${f0.codepoints.length}`);
f0.free?.();

for (let i = 0; i < WARMUP; i++) batch();
const per = [];
for (let i = 0; i < SAMPLES; i++) per.push(batch());
const s = median(per);
console.log(`wasm decodeFrame${touch ? '+read' : ''}  ${(s * 1e6).toFixed(2)} us/frame   ${(1 / s).toFixed(0)} frames/s   (${bytes.byteLength} wire bytes)`);
