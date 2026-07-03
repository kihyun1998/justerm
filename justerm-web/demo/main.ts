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
  AccessibleViewController,
  BeamtermRenderer,
  CommandAnnounceController,
  CommandNavController,
  computeLinks,
  copySelection,
  DecorationRegistry,
  DomAccessibleView,
  LinkController,
  MarkerKind,
  Scrollbar,
  ScreenReaderState,
  SearchController,
  SelectionController,
  StubCommandNavPort,
  StubFrameSource,
  Terminal,
  TERSE_ANNOUNCE_TEXT,
  VERBOSE_ANNOUNCE_TEXT,
} from "../src/index";
import { FitController, observeResize } from "../src/index";
import type { AccessiblePort, SignalSink } from "../src/index";
import type { CellGeometry, FitInput, LogicalLine, ResizePort, SearchPort, SelectionPort } from "../src/index";
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

// #120 S2: marker-anchored decorations. The registry is consumer-side; the
// renderer pulls its rects per frame (joined with the frame's markerPositions)
// and composes them into each cell's colour. The Decorate button below toggles a
// full-row bottom decoration on the last finished command's marker.
const decorations = new DecorationRegistry();
renderer.setDecorationSource((f) => decorations.decorationsForFrame(f));

// Seed a few lines so the accessible view has content immediately (an empty
// document at summon is poor UX) and the command-nav stub's lines (0/2/4) resolve
// to real document rows from the first frame — mirroring production, where
// `command_lines` only ever yields document lines that exist in `accessible_text`.
const log: string[] = Array.from({ length: 8 }, (_, i) => `seed row ${i} — select · find=Ctrl-F`);
let displayOffset = 0;

// S14 (#119): the screen-reader mirror. Mounted off-screen beside the canvas; it
// reads each frame's viewport text (its own CellMirror) into a hidden row tree
// and announces new output via aria-live. Turn on a screen reader (NVDA/VO) to
// hear appended rows; Tab into the hidden list to walk rows. Boundary focus
// scrolls the (demo) backend via onScroll.
// #161: one SR-active gate shared by the output announce (#119) and the command
// announce (#160), so the Screen reader button toggles both. Defaults active; a real host would set it
// from its platform screen-reader detection.
const srState = new ScreenReaderState();
const a11y = new Accessibility(document, renderer.cellPalette, renderer.cellFlags, {
  screenReaderState: srState,
  onScroll: (lines) => {
    displayOffset = Math.min(Math.max(displayOffset - lines, 0), maxOffset());
    render();
  },
  // #152: bridge an AT text selection in the row tree to the selection seam. A real
  // consumer passes the same SelectionPort the mouse uses; the demo logs the resulting
  // (row, col, side) so the DOM glue (getSelection → row/column resolution → bridge)
  // can be driven and asserted headlessly (the mouse path proves the port→core leg).
  selectionPort: {
    begin: (row, col, side, ty) => console.log(`[a11y-sel] begin ${row},${col} ${side} ${ty}`),
    extend: (row, col, side) => console.log(`[a11y-sel] extend ${row},${col} ${side}`),
    clear: () => console.log("[a11y-sel] clear"),
    text: () => Promise.resolve(null),
  },
});
document.body.appendChild(a11y.root);
canvas.addEventListener("blur", () => a11y.onBlur());

// S14/#149 end-to-end spike: the Alt screen button toggles the flag on emitted frames.
// With it ON, the controller must stop announcing new output (a TUI repaint isn't
// "new output") while the hidden row tree keeps mirroring — the alt-screen bit
// (#149 wire v9) driving the announce policy (#119), assembled.
let altScreen = false;

// #150 accessible view: the Accessible view button summons the whole-log document (a real backend runs
// core `accessible_text`; the demo joins its log), Escape closes + returns focus.
canvas.tabIndex = 0; // make the canvas a focus target for restore
const accessiblePort: AccessiblePort = { text: async () => log.join("\n") };
const accessibleView = new DomAccessibleView(document, () => viewCtrl.close());
document.body.appendChild(accessibleView.el);
const viewCtrl = new AccessibleViewController(accessiblePort, accessibleView, {
  restoreFocus: () => canvas.focus(),
});

// #160 command announce: an OSC-133 CommandFinished mark on a frame → a screen-
// reader announce + an exit-driven success/fail earcon. The Finish command button simulates a command
// finishing (toggling exit 0/1) so a real SR reads the outcome and the tones
// distinguish success from failure by ear. The mark rides `markerPositions` (the
// #159 wire); in a real backend it comes from core parsing OSC 133.
// A SEPARATE, *polite* live region (not #119's output region): VSCode speaks the
// outcome on a polite `status()` channel that doesn't interrupt ongoing speech,
// and sharing #119's region would let an output flush clobber the announce.
const cmdLive = document.createElement("div");
cmdLive.setAttribute("aria-live", "polite");
cmdLive.setAttribute("aria-atomic", "true");
cmdLive.setAttribute("data-testid", "command-live"); // e2e hook (#160 announce)
Object.assign(cmdLive.style, {
  position: "absolute",
  width: "1px",
  height: "1px",
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
});
document.body.appendChild(cmdLive);
const audio = new AudioContext();
function beep(freq: number): void {
  const osc = audio.createOscillator();
  const gain = audio.createGain();
  osc.frequency.value = freq;
  osc.connect(gain).connect(audio.destination);
  gain.gain.setValueAtTime(0.1, audio.currentTime);
  gain.gain.exponentialRampToValueAtTime(0.001, audio.currentTime + 0.15);
  osc.start();
  osc.stop(audio.currentTime + 0.15);
}
const cmdSignal: SignalSink = {
  commandSucceeded: () => {
    console.log("[demo] signal: command succeeded");
    beep(880); // high tone = success
  },
  commandFailed: () => {
    console.log("[demo] signal: command failed");
    beep(220); // low tone = failure
  },
};
// #167: the controller owns SR-gating via the `auto` policy state, so the sinks
// are passed RAW (not wrapped by srState.gate*). `screenReaderActive` feeds the
// shared #161 state into the default all-`auto` policy — identical suppression to
// the old blanket wrap, but now an `on` modality could override SR-off. cmdCtrl
// still tracks every finished mark, so no backlog replays when SR flips on.
const cmdCtrl = new CommandAnnounceController(
  {
    announce: (text) => {
      cmdLive.textContent = text;
    },
    clear: () => {
      cmdLive.textContent = "";
    },
  },
  cmdSignal,
  {
    screenReaderActive: () => srState.isActive(),
    // #179: the announce *text* is consumer policy (ADR-0017). The injected
    // formatter dispatches to a preset by the live `terseAnnounce` toggle, so the
    // Terse button flips verbose ("Command failed, exit N") ↔ VSCode-parity terse
    // ("Command failed") through the real controller, not a fixed string.
    announceText: (outcome, exit) =>
      (terseAnnounce ? TERSE_ANNOUNCE_TEXT : VERBOSE_ANNOUNCE_TEXT)(outcome, exit),
  },
);
let nextMarkId = 1;
let commandMarks: number[] = [];
let cmdFailToggle = false;
let terseAnnounce = false;
// #120 S2: the Decorate button drops a marker at a visible content row and a
// full-row bottom decoration on it, so the green tint composes under real glyphs.
const DECO_MARKER_ID = 9000;
const DECO_ROW = 2;
let decorationMarks: number[] = [];
let lineDecoration: { dispose(): void } | undefined;
// #189: the live decoration is scoped to the buffer it was created on (mirroring
// core's per-buffer markers, #187) — its marker only rides that buffer's frames,
// and an alt-scoped decoration is disposed on alt-leave (core's clearAllMarkers on
// ?1049l). Undefined ⇔ no live decoration.
let decorationBuffer: "primary" | "alt" | undefined;
// The decoration's marker rides the CURRENT frame only when its buffer is the active
// one — so a primary decoration is absent from the alt frame (no cross-buffer bleed,
// like core omitting primary markers on an alt frame) and vice versa.
const decorationOnScreen = () =>
  lineDecoration !== undefined && (decorationBuffer === "alt") === altScreen;

// #166 command navigation: Prev/Next walk the command history inside the
// accessible view. A real backend returns core `command_lines` (document line +
// text + exit); the demo presets three whose `line`s index into the log. Nav
// reveals the line (DomAccessibleView.reveal), announces the command on the same
// polite region (#160), and reuses the exit-driven earcon (cmdSignal). Summoning
// the view (re)loads the list and resets the cursor to the end.
const navPort = new StubCommandNavPort();
navPort.list = [
  { line: 0, command: "echo hello", exit: 0 },
  { line: 2, command: "false", exit: 1 },
  { line: 4, command: "ls -la", exit: 0 },
];
const navCtrl = new CommandNavController(
  navPort,
  {
    announce: (text) => {
      cmdLive.textContent = text;
    },
    clear: () => {
      cmdLive.textContent = "";
    },
  },
  cmdSignal,
  accessibleView,
);

// --- Demo control bar: clickable, labelled buttons instead of F-key shortcuts
// (discoverable, show current state, and no F5=refresh footgun). Each action is a
// named function; toggles reflect their state in the button label. ---
function toggleAltScreen(): void {
  altScreen = !altScreen;
  // #189: leaving the alt screen disposes any alt-scoped decoration. core fires
  // `MarkerDisposed` on any alt-leave (?47l/?1047l/?1049l all route through the
  // per-buffer `clearAllMarkers`, term.rs `switch_to_primary`, #187); a real consumer
  // forwards that to `decorations.onMarkerDisposed`. The demo forwards it directly so
  // the alt-line highlight clears on alt-leave, primary decorations untouched.
  if (!altScreen && decorationBuffer === "alt") {
    decorations.onMarkerDisposed(DECO_MARKER_ID);
    lineDecoration = undefined;
    decorationMarks = [];
    decorationBuffer = undefined;
    decoBtn.textContent = "Decorate line: OFF";
    console.log("[demo] alt-leave disposed the alt-scoped decoration (#189)");
  }
  altBtn.textContent = `Alt screen: ${altScreen ? "ON" : "OFF"}`;
  console.log(`[demo] altScreen = ${altScreen} (announce ${altScreen ? "SUPPRESSED" : "on"})`);
  render(); // repaint: the frame's altScreen flips and any alt decoration clears
}
function summonAccessibleView(): void {
  // whole-buffer document for the screen reader; the query can reject (IPC).
  // On open, (re)load the command list so nav starts from the end (#166).
  viewCtrl
    .summon()
    .then(() => navCtrl.load())
    .catch((err) => console.error("[demo] accessible view failed", err));
}
function navPrevCommand(): void {
  navCtrl.previous().catch((err) => console.error("[demo] nav prev failed", err));
}
function navNextCommand(): void {
  navCtrl.next().catch((err) => console.error("[demo] nav next failed", err));
}
function finishCommand(): void {
  // Simulate a command finishing, alternating success/failure. A stride-5 marker
  // record `(id, row, kind, exitPresent, exitBits)` rides the next frame.
  const exit = cmdFailToggle ? 1 : 0;
  cmdFailToggle = !cmdFailToggle;
  commandMarks = [nextMarkId++, ROWS - 1, MarkerKind.CommandFinished, 1, exit];
  console.log(`[demo] simulated command finish, exit ${exit}`);
  render({ scrollCount: 0 }); // a Partial frame carries the mark → cmdCtrl announces
  cmdBtn.textContent = `Finish command (next exit ${cmdFailToggle ? 1 : 0})`;
}
function toggleDecorateLine(): void {
  // #120 S2: toggle a full-row bottom decoration anchored to a marker at a visible
  // content row. It projects each frame (marker row × registry) and the renderer
  // composes its bg UNDER the glyphs — a green line highlight, legible text on top.
  if (lineDecoration) {
    lineDecoration.dispose();
    lineDecoration = undefined;
    decorationMarks = [];
    decorationBuffer = undefined;
  } else {
    // #189: scope the decoration to the buffer it's created on. On the alt screen it
    // becomes an alt-scoped decoration (rides only alt frames, disposed on alt-leave);
    // on primary it's a primary decoration (absent from alt frames — no bleed).
    decorationBuffer = altScreen ? "alt" : "primary";
    decorationMarks = [DECO_MARKER_ID, DECO_ROW, MarkerKind.Plain, 0, 0];
    lineDecoration = decorations.register({
      markerId: DECO_MARKER_ID,
      x: 0,
      width: COLS,
      height: 3, // #202: a multi-row highlight — tints 3 rows from the marker down
      layer: "bottom",
      bg: 0x008f00, // green — distinct from defaultBg, glyphs stay readable above
      // #120 S3: also mark it on the overview ruler (orange), to demo the scrollbar
      // mark alongside the cell tint.
      overviewRulerOptions: { color: 0xff8800 },
    });
  }
  decoBtn.textContent = `Decorate line: ${lineDecoration ? "ON" : "OFF"}`;
  render(); // repaint (Full) so the decoration composes into the current cells
}
function toggleTerse(): void {
  // #179: flip the announce-text verbosity. Verbose (default) speaks the exit code
  // on failure; terse drops it (VSCode parity). Only the failure wording changes —
  // success is "Command succeeded" either way.
  terseAnnounce = !terseAnnounce;
  terseBtn.textContent = `Announce: ${terseAnnounce ? "TERSE" : "VERBOSE"}`;
  console.log(`[demo] announceText = ${terseAnnounce ? "terse" : "verbose"}`);
}
function toggleScreenReader(): void {
  // Route through the a11y seam (not srState directly) so reactivation re-syncs
  // the row tree (#169). The shared srState still updates, so the command
  // announce/signal gate (#160/#161) sees it too.
  a11y.setScreenReaderActive(!srState.isActive());
  srBtn.textContent = `Screen reader: ${srState.isActive() ? "ON" : "OFF"}`;
  console.log(
    `[demo] screenReaderActive = ${srState.isActive()} (announce/earcon ${srState.isActive() ? "on" : "SUPPRESSED"}, tree churn ${srState.isActive() ? "on" : "SKIPPED"})`,
  );
}

const controls = document.createElement("div");
Object.assign(controls.style, {
  position: "fixed",
  bottom: "0",
  left: "0",
  right: "0",
  display: "flex",
  gap: "8px",
  alignItems: "center",
  padding: "6px 10px",
  background: "#181825",
  borderTop: "1px solid #313244",
  font: "12px system-ui, sans-serif",
  // Above the accessible-view overlay (z 100) so command nav (#166) stays
  // reachable while the view is open.
  zIndex: "200",
});
function demoButton(
  label: string,
  onClick: () => void,
  restoreFocus = true,
): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.textContent = label;
  Object.assign(b.style, {
    cursor: "pointer",
    padding: "4px 10px",
    background: "#313244",
    color: "#cdd6f4",
    border: "1px solid #45475a",
    borderRadius: "4px",
    font: "inherit",
  });
  b.addEventListener("click", () => {
    onClick();
    // Return focus to the canvas so keyboard interaction continues — except for
    // command nav, which moves focus to the revealed accessible-view line (#166).
    if (restoreFocus) canvas.focus();
  });
  return b;
}
const viewBtn = demoButton("Accessible view (log)", summonAccessibleView);
const altBtn = demoButton("Alt screen: OFF", toggleAltScreen);
const cmdBtn = demoButton("Finish command (next exit 0)", finishCommand);
const decoBtn = demoButton("Decorate line: OFF", toggleDecorateLine);
const terseBtn = demoButton("Announce: VERBOSE", toggleTerse);
const srBtn = demoButton("Screen reader: ON", toggleScreenReader);
const prevBtn = demoButton("Prev command", navPrevCommand, false);
const nextBtn = demoButton("Next command", navNextCommand, false);
controls.append(viewBtn, altBtn, cmdBtn, decoBtn, terseBtn, srBtn, prevBtn, nextBtn);
document.body.appendChild(controls);

// Forward printable keystrokes for echo dedup (this demo doesn't echo, so it's a
// no-op here — the dedup is unit-tested; this wires the seam).
window.addEventListener("keydown", (e) => {
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
    altScreen, // #149: drives the a11y announce policy (Alt screen button)
    selectionSpans: engine.range(), // S8: the live selection projected onto the view
    matchSpans: searchEngine.matchSpans(top, ROWS), // S9: search matches on the view
    // #160 command marks (Finish command) + #120 S2 decoration marker (Decorate line).
    // #189: the decoration marker rides a frame only when its buffer is active, so a
    // primary decoration is omitted from alt frames (and vice versa) — no bleed.
    markerPositions: [...commandMarks, ...(decorationOnScreen() ? decorationMarks : [])],
    // #120 S3: every live marker's absolute buffer line. The demo pins the ruler
    // marker near the TOP of the buffer (line 3) so its ruler mark shows there
    // regardless of scroll — an off-viewport anchor the viewport marker group can't.
    // Only for a primary decoration on the primary screen: the ruler is a scrollback
    // navigator, suppressed on alt (rulerMarksForFrame), and alt has no scrollback.
    markerLines: decorationOnScreen() && !altScreen ? [DECO_MARKER_ID, 3] : [],
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
  cmdCtrl.onFrame(frame); // #160: announce + signal a finished command
  bar.update({ displayOffset, scrollbackLen: maxOffset(), rows: ROWS });
  bar.setMarks(decorations.rulerMarksForFrame(frame)); // #120 S3: overview-ruler marks
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

// #114 S11: auto-fit. On container (viewport) resize, compute the grid from the CSS box +
// the renderer's cell size and drive a debounced resize INTENT — the backend's job is to
// apply Engine::resize + PTY SIGWINCH (here the demo just logs the intent so the fit path
// is observable). The demo scrollbar is an overlay (no layout width), so scrollbarWidth 0.
const readFitInput = (): FitInput => {
  const boxPx = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const cell = renderer.cellSize(); // buffer px → CSS px per cell (DPR-independent)
  return {
    parentWidth: boxPx.width,
    parentHeight: boxPx.height,
    padding: { top: 0, bottom: 0, left: 0, right: 0 },
    cellWidth: cell.width / dpr,
    cellHeight: cell.height / dpr,
    scrollbarWidth: 0,
    scrollback: maxOffset(),
  };
};
const fitPort: ResizePort = {
  resize: (cols, rows) => console.log(`[fit] resize ${cols}x${rows}`),
};
const fitController = new FitController({ port: fitPort });
// Keep the disposer + controller so a real consumer tears them down on unmount (the
// ResizeObserver + the pending debounce timer). The demo lives for the page lifetime so it
// never calls these, but capturing them models the convention — and Terminal-level fit
// ownership (who calls disposeFit + fitController.dispose) lands with the widget integration
// in S16 (#133), which this demo wiring stands in for.
const disposeFit = observeResize(canvas, readFitInput, fitController);
void disposeFit;
