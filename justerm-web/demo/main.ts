// Manual S4 harness — a scrolling log with a draggable scrollbar. The demo plays
// a tiny "backend": it holds the full log, renders the viewport window at the
// current display offset, and re-renders when the scrollbar drag requests a new
// offset. A timer appends lines (following the bottom only when not scrolled up),
// so the thumb shrinks as history grows. Drag the thumb to scroll back.
// Run: `pnpm demo` (NOT `vite demo`).
import { BeamtermRenderer, Scrollbar, StubFrameSource, Terminal } from "../src/index";
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

const log: string[] = [];
let displayOffset = 0;

function viewportFrame(): DecodedFrame {
  const total = log.length;
  const scrollbackLen = Math.max(0, total - ROWS);
  const top = Math.max(0, total - ROWS - displayOffset);
  const rows = log.slice(top, top + ROWS);
  const codepoints: number[] = [];
  const spans: number[] = [];
  let offset = 0;
  rows.forEach((text, line) => {
    const chars = [...text];
    if (chars.length === 0) return;
    spans.push(line, 0, chars.length - 1, offset, chars.length);
    for (const c of chars) codepoints.push(c.codePointAt(0)!);
    offset += chars.length;
  });
  const n = codepoints.length;
  return {
    cols: COLS,
    rows: ROWS,
    kind: 0,
    codepoints,
    fg: new Array(n).fill(0),
    bg: new Array(n).fill(0),
    flags: new Array(n).fill(0),
    extra: new Array(n).fill(0),
    spans,
    sideTable: [],
    displayOffset,
    scrollbackLen,
  } as DecodedFrame;
}

const bar = new Scrollbar(document.body, {
  onScroll: (offset) => {
    displayOffset = offset;
    render();
  },
});

function render(): void {
  source.push(viewportFrame());
  bar.update({ displayOffset, scrollbackLen: Math.max(0, log.length - ROWS), rows: ROWS });
}

// Append a line every 300ms; follow the bottom only when not scrolled up.
let next = 0;
setInterval(() => {
  log.push(`row ${next++} — drag the scrollbar on the right to scroll back`);
  if (displayOffset === 0) render();
  else bar.update({ displayOffset, scrollbackLen: Math.max(0, log.length - ROWS), rows: ROWS });
}, 300);
render();
