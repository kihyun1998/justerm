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
  CommandAnnounceController,
  CommandNavController,
  computeLinks,
  copySelection,
  DecorationRegistry,
  DomAccessibleView,
  JustermRenderer,
  LinkController,
  MarkerKind,
  MouseEvents,
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
import type {
  CellGeometry,
  FitInput,
  InputSink,
  LogicalLine,
  ResizePort,
  SearchOptions,
  SearchPort,
  SearchResult,
  SelectionPort,
} from "../src/index";
import type { DecodedFrame } from "../src/types";
import { FakeSelectionEngine } from "./fake-select";
import { FakeSearchEngine } from "./fake-search";

const renderer = await JustermRenderer.create({
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

// The widget, assigned below once its wiring deps exist. Focus-restore paths
// (accessible view, control buttons) return focus HERE — the real input target is
// the widget's hidden IME textarea, not the canvas (#116).
let term: Terminal | undefined;
const focusTerminal = (): void => term?.focus();

// Size the renderer to the available CSS box, then read back the grid it fits. The
// JustermRenderer adapter takes a CSS box, divides by the cell to a grid, sizes the
// renderer's device buffer to a grid-exact multiple (#331) AND sets the canvas's CSS
// display box to `cssWidth/cssHeight` so the buffer is crisp, not scaled. Because the
// adapter shrinks the canvas to the grid, measure the VIEWPORT (the #term box is
// 100vw/vh), not the canvas — measuring the canvas would feed back its own shrunk size
// and never re-grow. Pointer→cell mapping stays CSS px (rect ÷ COLS), dpr-independent.
let COLS = 80;
let ROWS = 24;
function fit(): void {
  renderer.resize(Math.max(1, window.innerWidth), Math.max(1, window.innerHeight));
  const ts = renderer.terminalSize();
  COLS = ts.cols;
  ROWS = ts.rows;
}
fit();

// S16 (#133): the Terminal is created *after* its wiring deps (getGeometry,
// displayOffset, render) exist — see the `new Terminal(...)` below with options.
const source = new StubFrameSource();

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

// S16 (#133): the "App mouse" button flips whether the frame advertises mouse
// tracking (the #129 mouseWantedEvents mask). With it ON, the widget routes a
// wheel notch to the app (logged via the input sink) instead of scrolling
// scrollback — the app-vs-local wheel branch, driven by the real frame mask.
let appMouse = false;

// #150 accessible view: the Accessible view button summons the whole-log document (a real backend runs
// core `accessible_text`; the demo joins its log), Escape closes + returns focus.
canvas.tabIndex = 0; // make the canvas a focus target for restore
const accessiblePort: AccessiblePort = { text: async () => log.join("\n") };
const accessibleView = new DomAccessibleView(document, () => viewCtrl.close());
document.body.appendChild(accessibleView.el);
const viewCtrl = new AccessibleViewController(accessiblePort, accessibleView, {
  restoreFocus: () => focusTerminal(),
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
// #480: the decoration anchors to a FIXED absolute buffer line — the content line under viewport
// row DECO_ROW at the moment it is registered — captured here, never re-derived from the viewport.
// Its viewport row is derived per frame (`decoAbsLine - top`), so the highlight scrolls WITH the
// content while the ruler mark stays at the buffer position, exactly as a real core marker does.
// Gated by `decorationOnScreen()`, so its value when no decoration is live is irrelevant.
// The #461 probe drives it above the viewport top (the "anchor scrolled off the top" case the
// seeded demo cannot otherwise reach) by setting it directly.
let decoAbsLine = 0;
let lineDecoration: { dispose(): void } | undefined;
// #458: two extra marker ids the precedence probe switches on, so a frame can carry TWO anchors
// covering the same cell — the only shape that can distinguish registration-order precedence from
// core's marker-emission order. Emitted (in this array's order, i.e. "core order") only while the
// probe sets `precedenceLine`; the seeded demo never has two decoration anchors otherwise.
const PRECEDENCE_MARKER_IDS = [9001, 9002] as const;
let precedenceLine: number | undefined;
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
// #417: a runtime font-size change exercises the wired setFontSize (#406). A bigger font makes a
// bigger cell, so the SAME viewport fits fewer columns — re-fit + repaint, and log the new grid so
// the effect is observable (a consumer would drive fit off this exactly like a container resize).
let demoFontSize = 16;
function toggleFontSize(): void {
  demoFontSize = demoFontSize === 16 ? 20 : 16;
  renderer.setFontSize(demoFontSize);
  fit(); // COLS/ROWS re-derive from the viewport ÷ the new (larger/smaller) cell
  render();
  fontBtn.textContent = `Font: ${demoFontSize}px`;
  console.log(`[demo] font size ${demoFontSize}px → grid ${COLS}x${ROWS}`);
}
// #420: a runtime theme swap exercises the wired setTheme (renderer setPalette #405). Two schemes
// with opposite defaults (dark ↔ light) so any sampled pixel changes; the demo samples the drawing
// buffer (device px — readPixels there is reliable, unlike a composited screenshot #352) and logs it.
const themeDark = {
  ansi: [0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f, 0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff],
  defaultFg: 0xcdd6f4, defaultBg: 0x1e1e2e, selectionBg: 0x45475a,
};
const themeLight = {
  ansi: [0xffffff, 0xdd5555, 0x55aa55, 0xaaaa00, 0x5555dd, 0xaa55aa, 0x00aaaa, 0x202020, 0x808080, 0xff0000, 0x00aa00, 0xaaaa00, 0x0000ff, 0xaa00aa, 0x008888, 0x000000],
  defaultFg: 0x101010, defaultBg: 0xf0f0f0, selectionBg: 0xb0c4de,
};
let themeIsLight = false;
function toggleTheme(): void {
  themeIsLight = !themeIsLight;
  renderer.setTheme(themeIsLight ? themeLight : themeDark); // rebuilds palette → setPalette → renders
  // Sample the drawing buffer's centre after setTheme has re-resolved + presented.
  const gl = canvas.getContext("webgl2")!;
  const [w, h] = [gl.drawingBufferWidth, gl.drawingBufferHeight];
  const px = new Uint8Array(4);
  gl.readPixels(w >> 1, h >> 1, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
  themeBtn.textContent = `Theme: ${themeIsLight ? "light" : "dark"}`;
  console.log(`[demo] theme=${themeIsLight ? "light" : "dark"} centre=rgb(${px[0]},${px[1]},${px[2]})`);
}
function toggleDecorateLine(): void {
  // #120 S2: toggle a full-row bottom decoration anchored to a marker at a visible
  // content row. It projects each frame (marker row × registry) and the renderer
  // composes its bg UNDER the glyphs — a green line highlight, legible text on top.
  if (lineDecoration) {
    lineDecoration.dispose();
    lineDecoration = undefined;
    decorationBuffer = undefined;
  } else {
    // #189: scope the decoration to the buffer it's created on. On the alt screen it
    // becomes an alt-scoped decoration (rides only alt frames, disposed on alt-leave);
    // on primary it's a primary decoration (absent from alt frames — no bleed).
    decorationBuffer = altScreen ? "alt" : "primary";
    decoAbsLine = viewTop() + DECO_ROW; // #480: capture the absolute buffer line under row DECO_ROW
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

function toggleAppMouse(): void {
  // S16 (#133): flip the frame's mouse-tracking mask. ON → the widget reports a
  // wheel notch to the app (input sink logs it); OFF → wheel scrolls scrollback.
  appMouse = !appMouse;
  appMouseBtn.textContent = `App mouse: ${appMouse ? "ON" : "OFF"}`;
  console.log(`[demo] appMouse = ${appMouse} (wheel → ${appMouse ? "app (intent)" : "scrollback"})`);
  render(); // re-emit so the next frame carries the new mask
}
// #117: push consumer events through the source's event channel (a real backend
// drains them from core). The widget routes each to the events handlers above.
let titleN = 0;
let cwdN = 0;
function emitTitle(): void {
  source.pushEvent({ type: "title", title: `justerm — tab ${++titleN}` });
}
function emitBell(): void {
  source.pushEvent({ type: "bell" });
}
function emitCwd(): void {
  source.pushEvent({ type: "cwd", cwd: `file://host/home/ki/dir${++cwdN}` });
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
    // Return focus to the widget's input textarea so keyboard/IME continues — except
    // for command nav, which moves focus to the revealed accessible-view line (#166).
    if (restoreFocus) focusTerminal();
  });
  return b;
}
const viewBtn = demoButton("Accessible view (log)", summonAccessibleView);
const altBtn = demoButton("Alt screen: OFF", toggleAltScreen);
const cmdBtn = demoButton("Finish command (next exit 0)", finishCommand);
const decoBtn = demoButton("Decorate line: OFF", toggleDecorateLine);
const terseBtn = demoButton("Announce: VERBOSE", toggleTerse);
const srBtn = demoButton("Screen reader: ON", toggleScreenReader);
const appMouseBtn = demoButton("App mouse: OFF", toggleAppMouse);
const titleBtn = demoButton("Set title", emitTitle); // #117
const bellBtn = demoButton("Bell", emitBell); // #117
const cwdBtn = demoButton("Set cwd", emitCwd); // #117
const prevBtn = demoButton("Prev command", navPrevCommand, false);
const nextBtn = demoButton("Next command", navNextCommand, false);
const fontBtn = demoButton("Font: 16px", toggleFontSize); // #417: runtime setFontSize
const themeBtn = demoButton("Theme: dark", toggleTheme); // #420: runtime setTheme
controls.append(
  viewBtn,
  altBtn,
  cmdBtn,
  decoBtn,
  terseBtn,
  srBtn,
  appMouseBtn,
  titleBtn,
  bellBtn,
  cwdBtn,
  prevBtn,
  nextBtn,
  fontBtn,
  themeBtn,
);
document.body.appendChild(controls);

// Echo-dedup (#119) is fed from the OUTBOUND intents so it covers IME commits and
// pasted runs too (a `text` intent), not just single keydowns — otherwise a screen
// reader announces IME-typed characters twice (once as they're typed, once as the
// shell echoes them). Wired via the input sink below.

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
  // #255: emit EVERY cell of every row (pad to COLS with spaces), like a real core —
  // which sends the whole viewport, not just non-empty content. Blank cells then paint
  // space-on-defaultBg (dark); a sparse frame left them unpainted, showing beamterm's
  // GL-default (blue) since `batch.clear` doesn't back-fill un-drawn cells.
  for (let line = 0; line < ROWS; line++) {
    const chars = [...(rows[line] ?? "")];
    chars.length = COLS; // clamp long lines; pad short ones (holes → spaces below)
    spans.push(line, 0, COLS - 1, offset, COLS);
    for (const c of chars) codepoints.push(c ? c.codePointAt(0)! : 0x20);
    offset += COLS;
  }
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
    // S16/#129: the wheel-routing mask. App mouse ON = Normal protocol (DOWN|UP|
    // WHEEL) → the widget sends a wheel notch to the app; OFF = 0 → scrollback.
    mouseWantedEvents: appMouse ? MouseEvents.Down | MouseEvents.Up | MouseEvents.Wheel : 0,
    selectionSpans: engine.range(), // S8: the live selection projected onto the view
    matchSpans: searchEngine.matchSpans(top, ROWS), // S9: search matches on the view
    // #429: the ACTIVE match rides its own wire group (also present in matchSpans;
    // the renderer's ranking paints it in the active colour, above the selection).
    activeMatchSpans: searchEngine.activeMatchSpans(top, ROWS),
    // #160 command marks (Finish command) + #120 S2 decoration marker (Decorate line).
    // #189: the decoration marker rides a frame only when its buffer is active, so a
    // primary decoration is omitted from alt frames (and vice versa) — no bleed.
    // #120 S2/#480: the decoration is anchored to a FIXED absolute buffer line (`decoAbsLine`),
    // so its viewport row is DERIVED here (`decoAbsLine - top`). markerPositions carries that row
    // only while it is on the viewport (`0 <= row < ROWS`), mirroring core's `marker_positions`
    // filter EXACTLY (term.rs: `m.line.checked_sub(top)?` drops an ABOVE-top marker, `row < rows`
    // drops a BELOW-viewport one). markerLines below carries the absolute line unconditionally, so
    // an off-viewport anchor (above-top, #461) still resolves from it — like core's ruler group.
    markerPositions: [
      ...commandMarks,
      ...(decorationOnScreen() && decoAbsLine - top >= 0 && decoAbsLine - top < ROWS
        ? [DECO_MARKER_ID, decoAbsLine - top, MarkerKind.Plain, 0, 0]
        : []),
      // #458 probe: both anchors on the SAME line, emitted in id order — "core order".
      ...(precedenceLine !== undefined && precedenceLine - top >= 0 && precedenceLine - top < ROWS
        ? PRECEDENCE_MARKER_IDS.flatMap((id) => [
            id,
            precedenceLine! - top,
            MarkerKind.Plain,
            0,
            0,
          ])
        : []),
    ],
    // #120 S3: every live marker's absolute buffer line — the FIXED `decoAbsLine`, so the ruler
    // mark stays at the buffer position as you scroll (the #480 slide is gone) and an above-top
    // anchor still resolves. Only a primary decoration on the primary screen: the ruler is a
    // scrollback navigator, suppressed on alt (rulerMarksForFrame), and alt has no scrollback.
    markerLines: [
      ...(decorationOnScreen() && !altScreen ? [DECO_MARKER_ID, decoAbsLine] : []),
      // #458 probe: same two anchors in the absolute group, same id order.
      ...(precedenceLine !== undefined
        ? PRECEDENCE_MARKER_IDS.flatMap((id) => [id, precedenceLine!])
        : []),
    ],
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
  return { originX: r.left, originY: r.top, cellWidth: r.width / COLS, cellHeight: r.height / ROWS, cols: COLS, rows: ROWS };
};

// S16 (#133): mount the widget as a COMPLETE terminal — it captures input, routes
// the wheel, and restarts the cursor blink on typing. In frame mode the sink
// forwards intents to the backend's encoders (encode_key/…); the demo has no
// backend, so it logs them — proving keys/paste/focus (and a wheel notch when
// "App mouse" is ON) reach the seam. The wheel's LOCAL branch scrolls scrollback
// via onScroll — the SAME shape the scrollbar drag uses (one coherent request).
const inputSink: InputSink = {
  send: (intent) => {
    if (intent.kind === "key") {
      console.log(`[input] key ${JSON.stringify(intent.event.key)} mods=${intent.event.mods}`);
      // Feed printable typed chars to the a11y echo-dedup (#119).
      if (intent.event.key.type === "char") a11y.onKey(intent.event.key.char);
    } else if (intent.kind === "mouse")
      console.log(`[input] mouse ${intent.event.button} @${intent.event.col},${intent.event.row}`);
    else if (intent.kind === "paste") console.log(`[input] paste ${JSON.stringify(intent.text)}`);
    else if (intent.kind === "text") {
      console.log(`[input] text ${JSON.stringify(intent.text)}`); // #116 IME commit
      a11y.onKey(intent.text); // dedup the committed run so its echo isn't re-announced
    } else console.log(`[input] focus ${intent.focused}`);
  },
};
// #116: the widget mounts its hidden IME textarea into `element`, which a canvas
// can't parent — so wrap the canvas in a relative container and hand THAT over. The
// canvas keeps the pointer (selection); the textarea is the keyboard/IME target.
const termContainer = document.createElement("div");
Object.assign(termContainer.style, { position: "relative", width: "100vw", height: "100vh" });
document.body.insertBefore(termContainer, canvas);
termContainer.appendChild(canvas);
term = new Terminal(source, renderer, {
  element: termContainer,
  input: inputSink,
  getGeometry,
  // Local wheel scroll → move the demo backend's viewport and re-render. Clamped
  // by the widget already; this just applies the requested offset.
  onScroll: (offset) => {
    displayOffset = offset;
    console.log(`[wheel] scroll → displayOffset ${offset}`); // observable signal (e2e/live proxy)
    render();
  },
  // #117: fire-and-forget consumer notifications. A real backend drains core events
  // and pushes them through the source's event channel; the demo pushes them from the
  // buttons below. onTitle drives the document title (xterm parity), onBell/onCwd log.
  events: {
    onTitle: (t) => {
      document.title = t;
      console.log(`[event] title ${JSON.stringify(t)}`);
    },
    onBell: () => console.log("[event] bell"),
    onCwd: (uri) => console.log(`[event] cwd ${JSON.stringify(uri)}`),
  },
});
term.mount();

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
  search: async (q, options) => {
    const n = searchEngine.search(q, log, options);
    render(); // matchSpans now carry the highlights
    return n;
  },
  showMatch: async (i) => {
    const m = searchEngine.match(i);
    if (!m) return;
    // Off-screen match → scroll it to the viewport centre (xterm); on-screen →
    // leave the scroll. Then designate it on the ACTIVE channel (#429) so it
    // paints in its own colour above selection + matches — it is NOT selected:
    // the selection stays the user's, coexisting with search navigation.
    const row = m.startLine - viewTop();
    if (row < 0 || row >= ROWS) {
      const centred = log.length - ROWS - (m.startLine - Math.floor(ROWS / 2));
      displayOffset = Math.min(Math.max(centred, 0), maxOffset());
    }
    searchEngine.setActive(i);
    render();
  },
  // The scroll-free re-designation channel (#429): after an output re-search the
  // engine's active designation reset, so restore it without moving the viewport.
  designateMatch: async (i) => {
    searchEngine.setActive(i);
    render();
  },
  clear: () => {
    // Search state only — a live selection is the USER's (#429; pre-#429 the
    // selection was the active-match emphasis, which is why this used to clear it).
    searchEngine.clear();
    render();
  },
};

// Real wasm regex validator (core's dialect, #316 D2) — the search box red-flags
// an invalid regex-mode query as-you-type rather than showing a silent 0 matches.
// JS `RegExp` can't stand in: its grammar differs from core's `regex` crate.
const { isValidRegex } = await import("justerm-wasm-decode");
const search = new SearchController(searchPort, { isValidRegex });

const box = document.createElement("div");
box.style.cssText =
  "position:fixed;top:8px;right:24px;display:none;gap:8px;align-items:center;background:#313244;color:#cdd6f4;font:14px monospace;padding:6px 10px;border-radius:6px;z-index:10";
const input = document.createElement("input");
input.placeholder = "search";
input.style.cssText =
  "background:#1e1e2e;color:#cdd6f4;border:1px solid #45475a;padding:2px 6px;font:14px monospace;outline:none";

// Mode toggles (#316) — regex / whole-word / case-sensitive, mirroring xterm.
function modeToggle(id: string, label: string): HTMLInputElement {
  const cb = document.createElement("input");
  cb.type = "checkbox";
  cb.id = `search-${id}`;
  cb.style.cssText = "margin:0;cursor:pointer";
  const l = document.createElement("label");
  l.htmlFor = cb.id;
  l.textContent = label;
  l.style.cssText = "cursor:pointer;user-select:none;font-size:12px";
  const wrap = document.createElement("span");
  wrap.style.cssText = "display:inline-flex;gap:3px;align-items:center";
  wrap.append(cb, l);
  box.append(wrap);
  return cb;
}
const countLabel = document.createElement("span");
countLabel.id = "search-count"; // e2e reads it to prove the wasm validator ran (#346)
countLabel.textContent = "0/0";

// #439: SR announce for the search count — a DEDICATED polite region (the #160
// precedent: sharing #119's output or #160's command region would let a flush
// clobber it), visually hidden like cmdLive. Post-#429 the current match is no
// longer a selection, so this is the only AT-perceivable side effect of search
// navigation.
const searchLive = document.createElement("div");
searchLive.setAttribute("aria-live", "polite");
searchLive.setAttribute("aria-atomic", "true");
searchLive.setAttribute("data-testid", "search-live"); // e2e hook (#439)
Object.assign(searchLive.style, {
  position: "absolute",
  width: "1px",
  height: "1px",
  overflow: "hidden",
  clipPath: "inset(50%)",
  whiteSpace: "nowrap",
});
document.body.append(searchLive);

// #439: VS Code's SimpleFindWidget wording VERBATIM ("{x} of {y} found for
// '{q}'" / "No results found for '{q}'", spoken on its polite `status()`
// channel). Spoken on user-driven count updates only (typing, Enter/Shift-
// Enter): a debounced background re-search updates neither the label nor the
// announce, so a streaming terminal cannot spam the SR. Gated by the SR-active
// state (#161); the invalid-regex state stays visual-only (updateCount returns
// before this — no reference wording exists to mirror, red-flag only).
function announceSearchCount(r: SearchResult): void {
  // Closing the box (Escape) resets the count with the query text still in the
  // input — without the visibility guard that would falsely announce "No
  // results" for a query that merely closed (VS Code announces nothing on hide).
  if (!srState.isActive() || box.style.display === "none" || input.value === "") return;
  searchLive.textContent =
    r.total === 0
      ? `No results found for '${input.value}'`
      : `${r.current} of ${r.total} found for '${input.value}'`;
}
const regexToggle = modeToggle("regex", ".*");
const wordToggle = modeToggle("word", "W");
const caseToggle = modeToggle("case", "Aa");
box.insertBefore(input, box.firstChild);
box.append(countLabel);
document.body.append(box);

function currentOptions(): SearchOptions {
  return {
    regex: regexToggle.checked,
    wholeWord: wordToggle.checked,
    // Checked = force case-sensitive; unchecked = smart-case (omit the override).
    caseSensitive: caseToggle.checked || undefined,
  };
}

function updateCount(): void {
  if (search.isInvalidRegex()) {
    countLabel.textContent = "invalid";
    input.style.borderColor = "#f38ba8"; // red — regex the engine can't run
    return;
  }
  input.style.borderColor = "#45475a";
  const r = search.result();
  countLabel.textContent = `${r.current}/${r.total}`;
  announceSearchCount(r); // #439: same user-driven cadence as the visible label
}
function runSearch(): void {
  void search.search(input.value, currentOptions()).then(updateCount);
}
input.addEventListener("input", runSearch);
for (const t of [regexToggle, wordToggle, caseToggle]) t.addEventListener("change", runSearch);
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

// --- #429 e2e probe: the active-match paint has no DOM proxy --------------
// Like the #420 theme sample, the proof reads the DRAWING BUFFER directly
// (readPixels on the device buffer is reliable where a composited screenshot
// is not, #352). Samples 2px inside a cell's top-left corner: under a SOLID
// highlight (#426) that corner is pure highlight bg — glyph ink sits mid-cell
// (probe with a query whose first glyph has no ascender, e.g. "select").
/** #457: real pixels for a right-anchored decoration that overflows the LEFT edge.
 * Before the viewport clip, the negative `left` wrapped in the u32 wire and the
 * decoration painted nothing — so both `overflow*` samples equalled the baseline. */
/** #461: rows 0..3 with a 5-row decoration anchored 2 rows ABOVE the viewport top. Rows 0-2
 * are the visible tail of its span and must be decorated; row 3 is past it and must not be. */
interface AboveTopProbe {
  /** Row 0 with no decoration — the undecorated colour. */
  baseline: string;
  /** Viewport rows 0, 1, 2, 3 with the above-top decoration registered. */
  rows: string[];
}

interface DecorationProbe {
  /** Row centre-left / centre-right with NO decoration — the undecorated colour. */
  baselineLeft: string;
  baselineRight: string;
  /** The same two cells with the overflowing right-anchored decoration registered. */
  overflowLeft: string;
  overflowRight: string;
}

interface SearchProbe {
  /** rgb of the active match's first cell; null when nothing is active/visible. */
  active: string | null;
  /** rgb of the first NON-active match cell on screen; null when none. */
  other: string | null;
  /** The active span `(row, left, right)` — navigation moves it. */
  activeSpan: number[];
  /** ALL on-screen match triples from the same snapshot — locating the active
   * span inside this list proves navigation drift-free (rows shift as the demo
   * appends, but both come from one probe). */
  matchSpans: number[];
  /** The live selection spans — coexistence with the search overlays (#429). */
  selectionSpans: number[];
}
/** #480: a decoration anchors to a BUFFER line, so scrolling moves its viewport row (and the
 * highlight with it) but NOT its overview-ruler mark, which stays at the buffer position. Reads
 * the frame the demo emits at two scroll offsets — the absolute line must be identical, the derived
 * viewport row must track the scroll. */
interface RulerAnchorProbe {
  /** The decoration's absolute buffer line (markerLines → ruler mark) before / after scrolling. */
  line0: number;
  lineScrolled: number;
  /** Its derived viewport row (markerPositions) at those offsets — this DOES change. */
  row0: number;
  rowScrolled: number;
  /** Rows scrolled between the two reads. */
  scrolledBy: number;
}

/** #458: two decorations on DIFFERENT markers covering the SAME cell — the last registered must
 * win at the pixel. Each field is the rgb of that cell under one registration scenario, all read
 * from the real drawing buffer, so the whole chain (projection order → wire → renderer per-property
 * last-wins) is proven rather than just the emitted rects. */
interface PrecedenceProbe {
  /** The cell with no decoration at all. */
  baseline: string;
  /** Only the marker core emits FIRST is decorated. */
  firstMarkerOnly: string;
  /** Only the marker core emits SECOND is decorated. */
  secondMarkerOnly: string;
  /** Both, with the SECOND-emitted marker's decoration registered first — so registration order
   * and core's marker order disagree, and the first-emitted marker's colour must win. */
  bothFirstMarkerRegisteredLast: string;
  /** The same pair registered the other way round — the other colour must win. */
  bothSecondMarkerRegisteredLast: string;
}

/** #498: the ordered backgrounds of the overview-ruler mark elements the scrollbar actually built,
 * in DOM order — which is paint order for same-z-index positioned siblings. A full-width mark must
 * come after (i.e. above) a gutter one even when registered first. */
interface RulerLayerProbe {
  marks: { background: string; left: number; right: number; top: number; bottom: number }[];
}

declare global {
  interface Window {
    __searchProbe?: () => SearchProbe;
    __rulerLayerProbe?: () => RulerLayerProbe;
    __decorationProbe?: () => DecorationProbe;
    __precedenceProbe?: () => PrecedenceProbe;
    __aboveTopProbe?: () => AboveTopProbe;
    __rulerAnchorProbe?: () => RulerAnchorProbe;
  }
}
window.__aboveTopProbe = (): AboveTopProbe => {
  // #461: a multi-row decoration whose marker sits ABOVE the viewport top must paint the rows
  // of it that are still visible, not vanish. Drive it for real: shift the marker's absolute
  // line up by 2 with height 5, so it spans viewport rows -2..2 and rows 0..2 are on screen.
  const gl = canvas.getContext("webgl2")!;
  const { width: cw, height: ch } = renderer.cellSize();
  const sample = (row: number, col: number): string => {
    const x = Math.round(col * cw) + 2;
    const y = gl.drawingBufferHeight - 1 - (Math.round(row * ch) + 2);
    const px = new Uint8Array(4);
    gl.readPixels(x, y, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    return `rgb(${px[0]},${px[1]},${px[2]})`;
  };
  if (lineDecoration) {
    lineDecoration.dispose();
    lineDecoration = undefined;
    decorationBuffer = undefined;
    decoBtn.textContent = "Decorate line: OFF";
  }
  render();
  const baseline = sample(0, 0);

  decorationBuffer = altScreen ? "alt" : "primary";
  decoAbsLine = viewTop() - 2; // #480/#461: absolute line = viewport row -2 (2 above the top)
  lineDecoration = decorations.register({
    markerId: DECO_MARKER_ID,
    x: 0,
    width: COLS,
    height: 5,
    layer: "bottom",
    bg: 0x008f00,
  });
  render();
  const rows = [sample(0, 0), sample(1, 0), sample(2, 0), sample(3, 0)];
  lineDecoration.dispose();
  lineDecoration = undefined;
  decorationBuffer = undefined;
  render();
  return { baseline, rows };
};
window.__rulerAnchorProbe = (): RulerAnchorProbe => {
  // #480: the decoration's absolute buffer line (markerLines → ruler mark) must be invariant under
  // scroll; only its viewport row moves. The seeded demo has no scrollback, so force some, decorate
  // at the current view, and read the frame the demo emits at two scroll offsets. Demo state is
  // restored afterwards so the visible UI is unchanged.
  const savedLen = log.length;
  const savedOffset = displayOffset;
  if (lineDecoration) {
    lineDecoration.dispose();
    lineDecoration = undefined;
    decorationBuffer = undefined;
    decoBtn.textContent = "Decorate line: OFF";
  }
  const scrolledBy = 3;
  while (maxOffset() < scrolledBy) log.push(`ruler-anchor pad ${log.length}`);

  displayOffset = 0;
  decorationBuffer = "primary";
  decoAbsLine = viewTop() + DECO_ROW; // anchor to the buffer line under row DECO_ROW at offset 0
  lineDecoration = decorations.register({
    markerId: DECO_MARKER_ID,
    x: 0,
    width: COLS,
    layer: "bottom",
    bg: 0x008f00,
    overviewRulerOptions: { color: 0xff8800 },
  });
  const rowOf = (mp: number[]): number => {
    for (let i = 0; i + 5 <= mp.length; i += 5) if (mp[i] === DECO_MARKER_ID) return mp[i + 1]!;
    return Number.NaN; // omitted → off-viewport
  };
  const read = (): { line: number; row: number } => {
    const f = viewportFrame();
    return { line: (f.markerLines as number[])[1]!, row: rowOf(f.markerPositions as number[]) };
  };
  const a = read();
  displayOffset = scrolledBy; // scroll up by `scrolledBy` rows → the buffer line sits that much lower
  const b = read();

  lineDecoration.dispose();
  lineDecoration = undefined;
  decorationBuffer = undefined;
  log.length = savedLen;
  displayOffset = savedOffset;
  render();
  return { line0: a.line, lineScrolled: b.line, row0: a.row, rowScrolled: b.row, scrolledBy };
};
window.__rulerLayerProbe = (): RulerLayerProbe => {
  // #498: a `full`-width ruler mark must paint ABOVE the gutter ones. The registry orders the array;
  // `scrollbar.setMarks` turns that into DOM order; CSS then paints later same-z-index siblings on
  // top. A unit test can only see the array, so read the real DOM the demo built (vitest runs in a
  // `node` environment, so this link has no unit-level home at all).
  if (lineDecoration) {
    lineDecoration.dispose();
    lineDecoration = undefined;
    decorationBuffer = undefined;
    decoBtn.textContent = "Decorate line: OFF";
  }
  // The scrollbar hides itself with no scrollback (`scrollbarMetrics.visible`), and a hidden track
  // lays its marks out at zero size — which would make any geometric claim about them vacuous.
  // Force some scrollback first, exactly as the #480 ruler-anchor probe does, and restore after.
  const savedLen = log.length;
  const savedOffset = displayOffset;
  while (maxOffset() < 3) log.push(`ruler-layer pad ${log.length}`);
  const [a, b] = PRECEDENCE_MARKER_IDS;
  precedenceLine = viewTop() + DECO_ROW;
  // Registered full-FIRST, gutter-SECOND: registration order alone would put the gutter mark last.
  const full = decorations.register({
    markerId: a,
    overviewRulerOptions: { color: 0xaa0000 },
  });
  const gutter = decorations.register({
    markerId: b,
    overviewRulerOptions: { color: 0x00aa00, position: "left" },
  });
  render();
  // `track` is private to Scrollbar; the demo reaches it to observe what it built (TS `private` is
  // compile-time only). The mark elements are the ones the scrollbar paints through.
  const track = (bar as unknown as { track: HTMLDivElement }).track;
  // Select by the marker attribute `setMarks` stamps, not by an incidental style: any future
  // non-interactive child of the track would join a `pointer-events` filter.
  const marks = [...track.querySelectorAll<HTMLElement>("[data-ruler-mark]")].map((el) => {
    const r = el.getBoundingClientRect();
    return { background: el.style.background, left: r.left, right: r.right, top: r.top, bottom: r.bottom };
  });
  full.dispose();
  gutter.dispose();
  precedenceLine = undefined;
  log.length = savedLen;
  displayOffset = savedOffset;
  render();
  return { marks };
};
window.__precedenceProbe = (): PrecedenceProbe => {
  // #458: precedence between decorations on DIFFERENT markers is REGISTRATION order — the last
  // registered wins. A projection unit test only sees the emitted rect order; this drives the real
  // wasm renderer and reads the pixel, so it also proves the renderer resolves per-property
  // last-in-wire-order (#452) the way the projection assumes.
  const gl = canvas.getContext("webgl2")!;
  const { width: cw, height: ch } = renderer.cellSize(); // device px
  const sample = (): string => {
    // Read the cell's own corner, away from glyph ink, in the SAME synchronous turn as the draw
    // (no preserveDrawingBuffer — a later read races the present and returns black).
    const x = Math.round(1 * cw) + 2;
    const y = gl.drawingBufferHeight - 1 - (Math.round(DECO_ROW * ch) + 2);
    const px = new Uint8Array(4);
    gl.readPixels(x, y, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    return `rgb(${px[0]},${px[1]},${px[2]})`;
  };
  // Clear any live demo decoration so the baseline is genuinely undecorated.
  if (lineDecoration) {
    lineDecoration.dispose();
    lineDecoration = undefined;
    decorationBuffer = undefined;
    decoBtn.textContent = "Decorate line: OFF";
  }
  render();
  const baseline = sample();

  const [first, second] = PRECEDENCE_MARKER_IDS; // the order the frame emits them ("core order")
  const RED = 0x99_00_00;
  const BLUE = 0x00_00_99;
  precedenceLine = viewTop() + DECO_ROW; // both anchors on one line → both cover the same cell
  // Each scenario registers from scratch, so registration order is exactly what it says.
  const run = (regs: { markerId: number; bg: number }[]): string => {
    const live = regs.map((r) =>
      decorations.register({ markerId: r.markerId, x: 0, width: COLS, layer: "bottom", bg: r.bg }),
    );
    render();
    const rgb = sample();
    for (const d of live) d.dispose();
    return rgb;
  };
  const firstMarkerOnly = run([{ markerId: first, bg: RED }]);
  const secondMarkerOnly = run([{ markerId: second, bg: BLUE }]);
  // Register the SECOND-emitted marker's decoration first: core order says blue wins, registration
  // order says red. (Pre-#458 this returned blue.)
  const bothFirstMarkerRegisteredLast = run([
    { markerId: second, bg: BLUE },
    { markerId: first, bg: RED },
  ]);
  // The mirror: same two markers, opposite registration order → blue must win.
  const bothSecondMarkerRegisteredLast = run([
    { markerId: first, bg: RED },
    { markerId: second, bg: BLUE },
  ]);

  precedenceLine = undefined;
  render();
  return {
    baseline,
    firstMarkerOnly,
    secondMarkerOnly,
    bothFirstMarkerRegisteredLast,
    bothSecondMarkerRegisteredLast,
  };
};
window.__decorationProbe = (): DecorationProbe => {
  // #457: a right-anchored decoration WIDER than the viewport overflows the left edge.
  // Its raw `left` is negative; the wire carries u32 columns, so an unclipped value
  // wraps and the decoration paints nothing (or, for a negative `right`, the whole
  // row). Register the real thing and read real pixels — the projection unit test
  // cannot see whether anything reached the screen.
  const gl = canvas.getContext("webgl2")!;
  const { width: cw, height: ch } = renderer.cellSize(); // device px
  const sample = (row: number, col: number): string => {
    const x = Math.round(col * cw) + 2;
    // readPixels counts rows from the BOTTOM of the buffer.
    const y = gl.drawingBufferHeight - 1 - (Math.round(row * ch) + 2);
    const px = new Uint8Array(4);
    gl.readPixels(x, y, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    return `rgb(${px[0]},${px[1]},${px[2]})`;
  };
  // Draw and read in the SAME synchronous turn (no preserveDrawingBuffer).
  // Clear any live demo decoration first so the baseline is genuinely undecorated —
  // and leave the UI truthful about it rather than pretending to restore it.
  if (lineDecoration) {
    lineDecoration.dispose();
    lineDecoration = undefined;
    decorationBuffer = undefined;
    decoBtn.textContent = "Decorate line: OFF";
  }
  render();
  const baselineLeft = sample(DECO_ROW, 0);
  const baselineRight = sample(DECO_ROW, COLS - 1);

  decoAbsLine = viewTop() + DECO_ROW; // #480: on-viewport at row DECO_ROW
  // Assign to `lineDecoration` (not a local): `decorationOnScreen()` gates the marker
  // onto the frame by it, so a locally-held handle would leave `markerPositions` empty
  // and the registry would project nothing — a false negative that looks exactly like
  // the #457 bug.
  decorationBuffer = altScreen ? "alt" : "primary";
  lineDecoration = decorations.register({
    markerId: DECO_MARKER_ID,
    anchor: "right",
    x: 0,
    width: COLS + 5, // wider than the screen → overflows the LEFT edge
    layer: "bottom",
    bg: 0x008f00,
  });
  render();
  const overflowLeft = sample(DECO_ROW, 0);
  const overflowRight = sample(DECO_ROW, COLS - 1);
  lineDecoration.dispose();
  lineDecoration = undefined;
  decorationBuffer = undefined;
  render();
  return { baselineLeft, baselineRight, overflowLeft, overflowRight };
};
window.__searchProbe = (): SearchProbe => {
  // Draw and read in the SAME synchronous turn: without preserveDrawingBuffer
  // the buffer may be cleared after present, so a readPixels in a later task
  // races (transparent black). The #420 theme sample reads right after its own
  // render for the same reason.
  renderer.render();
  const gl = canvas.getContext("webgl2")!;
  const { width: cw, height: ch } = renderer.cellSize(); // device px
  const sample = (row: number, col: number): string => {
    const x = Math.round(col * cw) + 2;
    // readPixels counts rows from the BOTTOM of the buffer.
    const y = gl.drawingBufferHeight - 1 - (Math.round(row * ch) + 2);
    const px = new Uint8Array(4);
    gl.readPixels(x, y, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    return `rgb(${px[0]},${px[1]},${px[2]})`;
  };
  const top = viewTop();
  const active = searchEngine.activeMatchSpans(top, ROWS);
  const all = searchEngine.matchSpans(top, ROWS);
  // The active match is also present in matchSpans (the ranking, not exclusion,
  // resolves the overlap) — skip it to find a plain-match cell to compare.
  let other: number[] | undefined;
  for (let i = 0; i + 2 < all.length; i += 3) {
    if (all[i] !== active[0] || all[i + 1] !== active[1]) {
      other = [all[i]!, all[i + 1]!, all[i + 2]!];
      break;
    }
  }
  return {
    active: active.length >= 3 ? sample(active[0]!, active[1]!) : null,
    other: other ? sample(other[0]!, other[1]!) : null,
    activeSpan: active,
    matchSpans: all,
    selectionSpans: engine.range(),
  };
};

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
  // Measure the VIEWPORT, not the canvas: the JustermRenderer adapter pins the canvas's CSS box to
  // a grid-exact size, so measuring the canvas would feed back its own pinned size and never see the
  // container shrink/grow (the #term box is 100vw/vh, so the viewport IS the available space).
  const dpr = window.devicePixelRatio || 1;
  const cell = renderer.cellSize(); // device px → CSS px per cell (÷ dpr)
  return {
    parentWidth: window.innerWidth,
    parentHeight: window.innerHeight,
    padding: { top: 0, bottom: 0, left: 0, right: 0 },
    cellWidth: cell.width / dpr,
    cellHeight: cell.height / dpr,
    scrollbarWidth: 0,
    scrollback: maxOffset(),
  };
};
const fitPort: ResizePort = {
  resize: (cols, rows) => {
    console.log(`[fit] resize ${cols}x${rows}`);
    // A resize mutates the buffer too (reflow drops engine highlights), so the
    // search re-runs — the same debounced path as output (#429; xterm hooks
    // onResize into its re-find identically). The demo's fake buffer never
    // reflows, so this is convention-modelling here, load-bearing in a real
    // consumer.
    search.onFrame();
  },
};
const fitController = new FitController({ port: fitPort });
// Keep the disposer + controller so a real consumer tears them down on unmount (the
// ResizeObserver + the pending debounce timer). The demo lives for the page lifetime so it
// never calls these, but capturing them models the convention — and Terminal-level fit
// ownership (who calls disposeFit + fitController.dispose) lands with the widget integration
// in S16 (#133), which this demo wiring stands in for.
// Observe the document element (tracks the viewport), not the canvas — the adapter pins the
// canvas size, so a canvas ResizeObserver would never fire on a viewport change.
const disposeFit = observeResize(document.documentElement, readFitInput, fitController);
void disposeFit;
