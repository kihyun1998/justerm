// Cross-check the async write() throughput against the fully-synchronous
// writeSync() path (no event loop), and assert the parser actually populated the
// buffer (rule out "callback fires without doing work"). If writeSync ~= write,
// the async measurement was fair; if writeSync is much faster, write() penalised
// xterm and the comparison must switch to the sync path.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import pkg from '@xterm/headless';
const { Terminal } = pkg;
const HERE = dirname(fileURLToPath(import.meta.url));

const COLS = 80, ROWS = 24, SCROLLBACK = 100, WARMUP = 5, SAMPLES = 15;
const TARGET = 32 * 1024 * 1024;
const INPUTS = ['ascii', 'ansi', 'cjk', 'scrolling', 'flood'];

function load(name) {
  const b = readFileSync(join(HERE, 'inputs', `${name}.bin`));
  const u = new Uint8Array(b.buffer, b.byteOffset, b.byteLength);
  const reps = Math.max(1, Math.ceil(TARGET / u.byteLength));
  const out = new Uint8Array(u.byteLength * reps);
  for (let i = 0; i < reps; i++) out.set(u, i * u.byteLength);
  return out;
}
const median = xs => { const s = [...xs].sort((a, b) => a - b); const m = s.length >> 1; return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2; };

// Async write(data, cb): time start -> drained callback.
function timeAsync(data) {
  return new Promise(res => {
    const t = new Terminal({ cols: COLS, rows: ROWS, scrollback: SCROLLBACK, allowProposedApi: true });
    const t0 = process.hrtime.bigint();
    t.write(data, () => { const t1 = process.hrtime.bigint(); const line = t.buffer.active.getLine(0)?.translateToString(true) ?? ''; t.dispose(); res({ s: Number(t1 - t0) / 1e9, sample: line.slice(0, 24) }); });
  });
}
// Sync writeSync(data): fully synchronous parse, no event loop.
function timeSync(data) {
  const t = new Terminal({ cols: COLS, rows: ROWS, scrollback: SCROLLBACK, allowProposedApi: true });
  const t0 = process.hrtime.bigint();
  t._core.writeSync(data); // public Terminal hides writeSync; CoreTerminal has it
  const t1 = process.hrtime.bigint();
  const line = t.buffer.active.getLine(0)?.translateToString(true) ?? '';
  t.dispose();
  return { s: Number(t1 - t0) / 1e9, sample: line.slice(0, 24) };
}

for (const name of INPUTS) {
  const data = load(name); const mb = data.byteLength / 1e6;
  for (let i = 0; i < WARMUP; i++) { await timeAsync(data); timeSync(data); }
  const a = []; const sy = []; let sampleA = '', sampleS = '';
  for (let i = 0; i < SAMPLES; i++) { const r = await timeAsync(data); a.push(r.s); sampleA = r.sample; }
  for (let i = 0; i < SAMPLES; i++) { const r = timeSync(data); sy.push(r.s); sampleS = r.sample; }
  const mbpsA = mb / median(a), mbpsS = mb / median(sy);
  console.log(`${name.padEnd(10)} async ${mbpsA.toFixed(1).padStart(7)}  sync ${mbpsS.toFixed(1).padStart(7)} MB/s   ratio ${(mbpsS / mbpsA).toFixed(2)}x   buf[0]="${sampleS.trim()}"`);
}
