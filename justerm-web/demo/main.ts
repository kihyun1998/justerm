// Manual S4 + S8 harness — a scrolling log you can drag-select. The demo plays a
// tiny "backend": it holds the full log, renders the viewport window at the
// current display offset, and re-renders on scrollbar drag or selection change.
// A timer appends lines (following the bottom only when not scrolled up).
//
// S8 — try it: drag to select (char), double-click a word, triple-click a line,
// Alt-drag a block, drag past the top/bottom edge to auto-scroll, Ctrl/Cmd-C to
// copy, middle-click to paste the primary buffer, quick Alt-click to log a
// cursor-move. The selection model is a DEMO fake ({@link FakeSelectionEngine});
// the real one is the backend.
// Run: `pnpm demo` (NOT `vite demo`).
import {
  BeamtermRenderer,
  copySelection,
  Scrollbar,
  SelectionController,
  StubFrameSource,
  Terminal,
} from "../src/index";
import type { CellGeometry, SelectionPort } from "../src/index";
import type { DecodedFrame } from "../src/types";
import { FakeSelectionEngine } from "./fake-select";

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
    selectionBg: 0x45475a, // demo placeholder — #115 owns the real blend
  },
});

const canvas = document.querySelector<HTMLCanvasElement>("#term")!;
canvas.style.cursor = "text";

// Match the canvas backing buffer to its CSS box × devicePixelRatio (the crisp
// HiDPI pattern), then let beamterm tell us the grid it fits. Sizing the buffer
// to the CSS box keeps on-screen px == CSS px per cell, so pointer→cell mapping
// (which works in CSS px) is exact; deriving COLS/ROWS from the backend avoids a
// hardcoded grid that wouldn't match the window.
let COLS = 80;
let ROWS = 24;
function fit(): void {
  const dpr = window.devicePixelRatio || 1;
  const box = canvas.getBoundingClientRect();
  canvas.width = Math.max(1, Math.round(box.width * dpr));
  canvas.height = Math.max(1, Math.round(box.height * dpr));
  renderer.resize(canvas.width, canvas.height);
  const ts = renderer.terminalSize();
  COLS = ts.cols;
  ROWS = ts.rows;
}
fit();

const source = new StubFrameSource();
new Terminal(source, renderer).mount();

const log: string[] = [];
let displayOffset = 0;

/** Absolute log line shown at viewport row 0 for the current scroll. */
const viewTop = (): number => Math.max(0, log.length - ROWS - displayOffset);
const maxOffset = (): number => Math.max(0, log.length - ROWS);

const engine = new FakeSelectionEngine(() => log, viewTop, () => ROWS);

function viewportFrame(): DecodedFrame {
  const top = viewTop();
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
    scrollbackLen: maxOffset(),
    selectionSpans: engine.range(), // S8: the live selection projected onto the view
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
  bar.update({ displayOffset, scrollbackLen: maxOffset(), rows: ROWS });
}

// --- S8 wiring: SelectionController → fake engine, DOM mouse → controller ---

// The fake backend behind the write-side seam: apply each command, re-render so
// the new selection's overlay spans reach the renderer.
const port: SelectionPort = {
  begin: (r, c, s, ty) => {
    engine.begin(r, c, s, ty);
    render();
  },
  extend: (r, c, s) => {
    engine.extend(r, c, s);
    render();
  },
  clear: () => {
    engine.clear();
    render();
  },
  text: async () => engine.text(),
};

// Cell size in CSS px = the displayed box ÷ the grid — DPR-independent, so it
// matches the CSS-pixel pointer coords. (Reading cellSize() in buffer px would
// be off by devicePixelRatio and the selection would land on the wrong row.)
const getGeometry = (): CellGeometry => {
  const r = canvas.getBoundingClientRect();
  return { originX: r.left, originY: r.top, cellWidth: r.width / COLS, cellHeight: r.height / ROWS };
};

let primaryBuffer = "";
const controller = new SelectionController(port, getGeometry, {
  getRows: () => ROWS,
  isAtBottom: () => displayOffset === 0,
  // Drag past an edge: positive = scroll toward newer (offset → 0).
  onScroll: (lines) => {
    displayOffset = Math.min(Math.max(displayOffset - lines, 0), maxOffset());
    render();
  },
  onMoveCursor: (c) => console.log(`[alt-click] move cursor to row ${c.row}, col ${c.col}`),
  onPrimarySelection: (t) => {
    primaryBuffer = t;
  },
  onPaste: () => {
    if (primaryBuffer) {
      log.push(`[middle-click paste] ${primaryBuffer.replace(/\n/g, " ⏎ ")}`);
      render();
    }
  },
});

let tickTimer: number | undefined;
canvas.addEventListener("mousedown", (e) => {
  e.preventDefault();
  controller.mouseDown(e, e.detail);
  tickTimer = window.setInterval(() => controller.tick(), 50);
});
window.addEventListener("mousemove", (e) => controller.mouseMove(e));
window.addEventListener("mouseup", (e) => {
  controller.mouseUp(e);
  if (tickTimer !== undefined) {
    clearInterval(tickTimer);
    tickTimer = undefined;
  }
});
canvas.addEventListener("contextmenu", (e) => e.preventDefault());

window.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "c") {
    void copySelection(port, (t) => navigator.clipboard.writeText(t)).then((ok) => {
      if (ok) console.log("[copy] selection → clipboard");
    });
  }
});

// Append a line every 300ms; follow the bottom only when not scrolled up.
let next = 0;
setInterval(() => {
  log.push(`row ${next++} — drag to select · dbl=word · trpl=line · Alt=block · Ctrl/Cmd-C=copy`);
  if (displayOffset === 0) render();
  else bar.update({ displayOffset, scrollbackLen: maxOffset(), rows: ROWS });
}, 300);
render();
