// Manual S3 harness — a scrolling log. This SIMULATES A BACKEND spewing output
// (like `tail -f`): a timer pushes frames; justerm-web only renders them. It is
// NOT wheel-driven — in frame mode the backend owns the viewport offset and
// scrollback (ADR-0011), so wheel scrollback navigation belongs to the consumer
// (#111 / in-wasm), not this widget. Pushes a full frame of numbered rows, then a
// scroll-op frame every 600ms (shift up 1 + a new bottom line). Expected result:
// the rows scroll upward continuously, proving the cell mirror applies scroll-op
// damage through beamterm (which has no scroll primitive).
// Run: `pnpm demo` (Vite + vite-plugin-wasm handle the TS + both WASM modules).
import { BeamtermRenderer, StubFrameSource, Terminal } from "../src/index";
import type { DecodedFrame } from "../src/types";

const COLS = 80;
const ROWS = 24;

const renderer = await BeamtermRenderer.create({
  canvasSelector: "#term",
  fontFamily: "monospace",
  fontSize: 16,
  theme: {
    ansi: [
      0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f,
      0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
    ],
    defaultFg: 0xcdd6f4,
    defaultBg: 0x1e1e2e,
  },
});

const source = new StubFrameSource();
new Terminal(source, renderer).mount();

// Build a frame from text rows (one span each), optionally with a scroll op.
function rowsFrame(
  kind: number,
  rows: { line: number; text: string }[],
  scroll?: { top: number; bottom: number; count: number },
): DecodedFrame {
  const codepoints: number[] = [];
  const spans: number[] = [];
  let offset = 0;
  for (const r of rows) {
    const chars = [...r.text];
    spans.push(r.line, 0, chars.length - 1, offset, chars.length);
    for (const c of chars) codepoints.push(c.codePointAt(0)!);
    offset += chars.length;
  }
  const n = codepoints.length;
  return {
    cols: COLS,
    rows: ROWS,
    kind,
    codepoints,
    fg: new Array(n).fill(0),
    bg: new Array(n).fill(0),
    flags: new Array(n).fill(0),
    extra: new Array(n).fill(0),
    spans,
    sideTable: [],
    ...(scroll
      ? { hasScroll: true, scrollTop: scroll.top, scrollBottom: scroll.bottom, scrollCount: scroll.count }
      : {}),
  } as DecodedFrame;
}

// Initial full viewport: rows 0..23.
source.push(rowsFrame(0, Array.from({ length: ROWS }, (_, i) => ({ line: i, text: `row ${i}` }))));

// Scroll a fresh line in at the bottom every 600ms.
let next = ROWS;
setInterval(() => {
  source.push(
    rowsFrame(1, [{ line: ROWS - 1, text: `row ${next++}` }], { top: 0, bottom: ROWS - 1, count: 1 }),
  );
}, 600);
