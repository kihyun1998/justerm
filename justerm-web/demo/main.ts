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
  Accessibility,
  BeamtermRenderer,
  computeLinks,
  copySelection,
  LinkController,
  Scrollbar,
  SearchController,
  SelectionController,
  StubFrameSource,
  Terminal,
} from "../src/index";
import type { CellGeometry, LogicalLine, SearchPort, SelectionPort } from "../src/index";
import type { DecodedFrame } from "../src/types";
import { FakeSelectionEngine } from "./fake-select";
import { FakeSearchEngine } from "./fake-search";

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

// S14 (#119): the screen-reader mirror. Mounted off-screen beside the canvas; it
// reads each frame's viewport text (its own CellMirror) into a hidden row tree
// and announces new output via aria-live. Turn on a screen reader (NVDA/VO) to
// hear appended rows; Tab into the hidden list to walk rows. Boundary focus
// scrolls the (demo) backend via onScroll.
const a11y = new Accessibility(document, renderer.cellPalette, renderer.cellFlags, {
  onScroll: (lines) => {
    displayOffset = Math.min(Math.max(displayOffset - lines, 0), maxOffset());
    render();
  },
});
document.body.appendChild(a11y.root);
canvas.addEventListener("blur", () => a11y.onBlur());
window.addEventListener("keydown", (e) => {
  // Forward printable keystrokes for echo dedup (this demo doesn't echo, so it's
  // a no-op here — the dedup is unit-tested; this wires the seam).
  if (e.key.length === 1) a11y.onKey(e.key);
});

/** Absolute log line shown at viewport row 0 for the current scroll. */
const viewTop = (): number => Math.max(0, log.length - ROWS - displayOffset);
const maxOffset = (): number => Math.max(0, log.length - ROWS);

const engine = new FakeSelectionEngine(() => log, viewTop, () => ROWS);
const searchEngine = new FakeSearchEngine();

// `out` set = an incremental output frame (Partial). `scrollCount > 0` only when
// the buffer is full and content actually scrolled off the top — sending a
// phantom scroll while the screen is still filling shifts the mirror wrongly
// (a real backend emits the scroll op only on a real scroll). A repaint
// (scrollbar/selection) passes nothing → a Full frame.
function viewportFrame(out?: { scrollCount: number }): DecodedFrame {
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
    // Incremental output → Partial; a repaint (scrollbar/selection) → Full.
    kind: out ? 1 : 0,
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
    matchSpans: searchEngine.matchSpans(top, ROWS), // S9: search matches on the view
    ...(out && out.scrollCount > 0
      ? { hasScroll: true, scrollTop: 0, scrollBottom: ROWS - 1, scrollCount: out.scrollCount }
      : {}),
  } as DecodedFrame;
}

const bar = new Scrollbar(document.body, {
  onScroll: (offset) => {
    displayOffset = offset;
    render();
  },
});

function render(out?: { scrollCount: number }): void {
  const frame = viewportFrame(out);
  source.push(frame);
  a11y.onFrame(frame); // S14: mirror the viewport + announce new output
  bar.update({ displayOffset, scrollbackLen: maxOffset(), rows: ROWS });
  updateLinks();
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

// --- S9 wiring: search box → SearchController → fake search engine ---

const searchPort: SearchPort = {
  search: async (q) => {
    const n = searchEngine.search(q, log);
    render(); // matchSpans now carry the highlights
    return n;
  },
  showMatch: async (i) => {
    const m = searchEngine.match(i);
    if (!m) return;
    // Off-screen match → scroll it to the viewport centre (xterm); on-screen →
    // leave the scroll. Then select it so the active match shows in the
    // *selection* colour over the muted match colour (the 2-tier emphasis).
    const row = m.startLine - viewTop();
    if (row < 0 || row >= ROWS) {
      const centred = log.length - ROWS - (m.startLine - Math.floor(ROWS / 2));
      displayOffset = Math.min(Math.max(centred, 0), maxOffset());
    }
    const vrow = m.startLine - viewTop();
    engine.begin(vrow, m.startCol, "left", "char");
    engine.extend(vrow, m.endCol, "right");
    render();
  },
  clear: () => {
    searchEngine.clear();
    engine.clear();
    render();
  },
};

const search = new SearchController(searchPort);

const box = document.createElement("div");
box.style.cssText =
  "position:fixed;top:8px;right:24px;display:none;gap:6px;align-items:center;background:#313244;color:#cdd6f4;font:14px monospace;padding:6px 10px;border-radius:6px;z-index:10";
const input = document.createElement("input");
input.placeholder = "search";
input.style.cssText =
  "background:#1e1e2e;color:#cdd6f4;border:1px solid #45475a;padding:2px 6px;font:14px monospace;outline:none";
const countLabel = document.createElement("span");
countLabel.textContent = "0/0";
box.append(input, countLabel);
document.body.append(box);

function updateCount(): void {
  const r = search.result();
  countLabel.textContent = `${r.current}/${r.total}`;
}
input.addEventListener("input", () => void search.search(input.value).then(updateCount));
input.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    void (e.shiftKey ? search.prev() : search.next()).then(updateCount);
  } else if (e.key === "Escape") {
    e.preventDefault();
    box.style.display = "none";
    search.clear();
    updateCount();
  }
});
window.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
    e.preventDefault();
    box.style.display = "flex";
    input.focus();
    input.select();
  }
});

// --- S10 wiring: link hover/click. The demo only exercises plain-URL detection
// (regex) over the visible rows; OSC8 (osc8Links) is unit-tested. In frame mode
// the logical-line text + cell map come from core (viewport_logical_lines); the
// demo builds them from the unwrapped log directly.

const linkLabel = document.createElement("div");
linkLabel.style.cssText =
  "position:fixed;bottom:8px;left:8px;display:none;background:#313244;color:#89b4fa;font:13px monospace;padding:4px 8px;border-radius:6px;z-index:10";
document.body.append(linkLabel);

const linkCtrl = new LinkController({
  onHover: (l) => {
    canvas.style.cursor = "pointer";
    linkLabel.textContent = `🔗 ${l.uri}  (Ctrl/Cmd-click to open)`;
    linkLabel.style.display = "block";
  },
  onLeave: () => {
    canvas.style.cursor = "text";
    linkLabel.style.display = "none";
  },
  // The library never opens anything — onActivate is the seam. *How* to open is
  // consumer policy; this demo (a consumer) opens a new tab, severing `opener`
  // for security (xterm's handleLink does the same). A native consumer (penterm)
  // would call its shell-open instead.
  onActivate: (uri) => {
    console.log(`[link] open ${uri}`);
    window.open(uri, "_blank", "noopener,noreferrer");
  },
});

let lastPointer: [number, number] | undefined;

function visibleLogicalLines(): LogicalLine[] {
  const top = viewTop();
  return log.slice(top, top + ROWS).map((text, r) => ({
    text,
    cells: [...text].map((_, c) => [r, c] as [number, number]),
  }));
}
function updateLinks(): void {
  const regex = visibleLogicalLines().flatMap((l) => computeLinks(l));
  linkCtrl.setLinks([], regex);
  if (lastPointer) linkCtrl.pointerMove(lastPointer[0], lastPointer[1]); // re-hover after re-set
}
function cellFromEvent(e: globalThis.MouseEvent): [number, number] {
  const g = getGeometry();
  return [
    Math.floor((e.clientY - g.originY) / g.cellHeight),
    Math.floor((e.clientX - g.originX) / g.cellWidth),
  ];
}

window.addEventListener("mousemove", (e) => {
  if (e.buttons !== 0) return; // dragging → selection owns it, not link hover
  lastPointer = cellFromEvent(e);
  linkCtrl.pointerMove(lastPointer[0], lastPointer[1]);
});
canvas.addEventListener("click", (e) => {
  if (e.ctrlKey || e.metaKey) {
    const [row, col] = cellFromEvent(e);
    linkCtrl.click(row, col);
  }
});

// Append a line every 300ms; follow the bottom only when not scrolled up. Each
// append is "output" — search re-highlights (debounced) and links re-detect.
let next = 0;
setInterval(() => {
  log.push(`row ${next++} — select · find=Ctrl-F · link: https://github.com/kihyun1998/justerm`);
  search.onFrame();
  updateCount();
  // Real scroll amount: 0 while the screen is still filling, 1 once full (the top
  // line actually scrolls off). Following → emit it; scrolled up → scrollbar only.
  const scrollCount = Math.max(0, log.length - ROWS) - Math.max(0, log.length - 1 - ROWS);
  if (displayOffset === 0) render({ scrollCount });
  else bar.update({ displayOffset, scrollbackLen: maxOffset(), rows: ROWS });
}, 300);
render();
