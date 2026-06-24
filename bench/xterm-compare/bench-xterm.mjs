// Throughput of @xterm/headless write() over the byte-identical streams justerm
// dumps from benches/inputs.rs. Renderer-free (headless) = same scope as
// justerm's feed(). Matched methodology with the Rust side (examples/time_feed.rs):
// fresh terminal per sample, WARMUP warm-up writes, SAMPLES timed, median MB/s,
// MB = bytes / 1e6 (decimal). See docs/perf/xterm-comparison.md.
//
// Measurement notes (the write() pitfalls the loop prompt calls out):
//  - write(data, cb): the callback fires once the chunk is fully parsed+drained,
//    so we time start -> cb (full drain), not the synchronous return of write().
//  - data is a Uint8Array, so xterm runs its own UTF-8 decode (apples-to-apples
//    with vte), not a pre-decoded string.
//  - JIT warm-up before timing; report the median to shrug off GC blips.
//  - Caveat (recorded, not hidden): xterm's WriteBuffer yields to the event loop
//    every ~12ms for responsiveness, so a single huge write spans several loop
//    turns. justerm's feed() is fully synchronous. This slightly taxes xterm on
//    the 5 MiB flood; it is xterm's deliberate design, not a measurement bug.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import pkg from '@xterm/headless';

const { Terminal } = pkg;
const HERE = dirname(fileURLToPath(import.meta.url));

const COLS = 80;
const ROWS = 24;
const SCROLLBACK = 100; // == justerm FLOOD_CAP, so flood drives the at-cap recycle path
const WARMUP = 5;
const SAMPLES = 15;
// Tile every stream up to this size before timing. xterm's WriteBuffer defers
// its write() callback to an event-loop turn (~12ms granularity), so a payload
// that parses in <12ms measures the scheduler floor, not throughput. At ~8 MiB
// real parse time dwarfs the floor (<0.2%). All streams are whole CRLF lines /
// reset-terminated, so concatenation is a valid longer stream of the same mix.
const TARGET_BYTES = 32 * 1024 * 1024;
const INPUTS = ['ascii', 'ansi', 'cjk', 'scrolling', 'flood'];

function loadInput(name) {
  const buf = readFileSync(join(HERE, 'inputs', `${name}.bin`));
  const unit = new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
  const reps = Math.max(1, Math.ceil(TARGET_BYTES / unit.byteLength));
  const out = new Uint8Array(unit.byteLength * reps);
  for (let i = 0; i < reps; i++) out.set(unit, i * unit.byteLength);
  return out;
}

// One timed write of `data` into a fresh terminal; resolves with seconds elapsed.
// Terminal construction is OUTSIDE the timed region (justerm excludes engine
// setup via iter_batched), so we time only write -> full drain.
function timeOnce(data) {
  return new Promise((resolve) => {
    const term = new Terminal({ cols: COLS, rows: ROWS, scrollback: SCROLLBACK, allowProposedApi: true });
    const t0 = process.hrtime.bigint();
    term.write(data, () => {
      const t1 = process.hrtime.bigint();
      term.dispose();
      resolve(Number(t1 - t0) / 1e9);
    });
  });
}

function median(xs) {
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

const results = {};
for (const name of INPUTS) {
  const data = loadInput(name);
  const mb = data.byteLength / 1e6;
  for (let i = 0; i < WARMUP; i++) await timeOnce(data);
  const times = [];
  for (let i = 0; i < SAMPLES; i++) times.push(await timeOnce(data));
  const medS = median(times);
  const mbps = mb / medS;
  results[name] = { bytes: data.byteLength, median_s: medS, mbps };
  console.error(`xterm  ${name.padEnd(10)} ${mbps.toFixed(1).padStart(7)} MB/s  (median ${(medS * 1e3).toFixed(2)} ms over ${SAMPLES})`);
}
console.log(JSON.stringify({ engine: 'xterm-headless', cols: COLS, rows: ROWS, scrollback: SCROLLBACK, warmup: WARMUP, samples: SAMPLES, results }, null, 2));
