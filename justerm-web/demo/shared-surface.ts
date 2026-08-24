// Manual harness for Epic #287 S8 (#776) — TWO terminals on ONE canvas, one WebGL2 context, at two
// different font sizes. Run `pnpm demo` (NOT `vite demo`) and open `/shared-surface.html`.
//
// **Why this page exists rather than a widening of `demo/main.ts`.** Every slice of #287 before this
// one is proven by unit tests against a fake backend and by pixel probes inside the renderer's own
// crate, and none of those is the thing a consumer will actually do. More precisely, and measured
// while starting this slice: nothing outside `src/` called `TerminalSurface.open`,
// `JustermRenderer.attach`, `observeViewportRect` or `onDensityChange` — so the whole
// `composedSurface === false` branch of the adapter had never executed in a browser. This page is
// that branch's first execution, which is why it is the epic's proof rather than a demo of it.
//
// `demo/main.ts` is deliberately untouched: it is the harness the rest of the e2e suite is written
// against, and moving it would quietly change what a large number of unrelated assertions mean.
//
// **The shape is deliberately thin.** `main.ts` is ~3.5k lines because it accumulated one slice's
// affordances at a time, and its probes are calibrated against each other (a cursor cell that no
// other probe may sample, rows reserved per feature). Nothing here should be read as an example of
// that: this page holds one arrangement — two panes, two fonts, two cadences — because the
// arrangement *is* the subject.
import {
  JustermRenderer,
  observeViewportRect,
  StubFrameSource,
  Terminal,
  TerminalSurface,
} from "../src/index";
import type { CellGeometry, Theme } from "../src/index";
import type { DecodedFrame } from "../src/types";

// ── the arrangement ───────────────────────────────────────────────────────────────────────────

/** The shared canvas, in CSS px. One buffer; both terminals draw into slices of it. */
const CANVAS = { width: 900, height: 340 } as const;

/**
 * Where each terminal's DOM overlay sits, in CSS px relative to the stage.
 *
 * **Neither pane is at the origin and neither fills the canvas**, and both of those are the point.
 * A sole tenant sits at `(0, 0)` and covers the whole buffer, so it exercises no coordinate: the x
 * and y a shared tenant is placed at are the numbers `observeViewportRect` derives and the renderer
 * flips to GL's bottom-origin y, and a page that placed pane B at `y = 0` would leave the flip
 * asserted only at the one value where a sign error is invisible.
 *
 * What is left over is the evidence — see `shared-surface.html`'s note on the checkerboard. The
 * gutter `x ∈ [400, 500)` and the band `y ∈ [0, 40)` above pane B are canvas that no grid was
 * placed over, and `draw()` leaves them at `rgba(0,0,0,0)`.
 */
interface PaneBox {
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
}

const PANES: { readonly a: PaneBox; readonly b: PaneBox } = {
  a: { left: 0, top: 0, width: 400, height: 340 },
  b: { left: 500, top: 40, width: 380, height: 260 },
};

/** The 16 ANSI entries. Identical for both panes: this page is about geometry and tiering, and two
 * different palettes would make a pixel difference ambiguous between "the grids are separate" and
 * "the palettes are". The two `defaultBg`s below carry that difference on purpose, alone. */
const ANSI = [
  0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f,
  0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
];

/**
 * The two backgrounds, and the one constraint they are under: **each must differ from the other and
 * from the page's checkerboard**, so a single sampled pixel names which grid painted it.
 *
 * A page whose background matched a terminal's is the #577 failure, and it went green for six
 * slices. Here the same mistake would be worse than invisible — it would be *wrong*: the gutter
 * sample is what proves the two panes share a canvas, and if it matched a pane's background the
 * proof would read as "the grid extends across the gutter", which is the opposite conclusion.
 */
const BG_A = 0x1b2a4a; // deep blue
const BG_B = 0x123a24; // deep green
const FG_A = 0xd7e3ff;
const FG_B = 0xd6ffe4;

const themeFor = (defaultFg: number, defaultBg: number): Theme => ({
  ansi: ANSI,
  defaultFg,
  defaultBg,
});

/**
 * The two font sizes, and why they are far apart rather than merely different.
 *
 * A font configuration is the key of the renderer's middle resource tier (ADR-0021 D2): two grids on
 * two configurations bake two atlases and draw through two cell geometries in the same frame. This
 * page's job is to make that observable from outside, and the cell is derived from the ink box of
 * the font's `█` (ADR-0022) — so on a font where 16px and 14px happen to round to the same box, a
 * "the cells differ" check would pass vacuously or fail for a reason that has nothing to do with the
 * tier. 20 against 10 cannot collapse that way on any face.
 *
 * The e2e still asserts the *relation* rather than either number: an absolute cell dimension is not
 * portable, because CI's fonts are not this machine's (#578, #578's dpr-2 test pinned `cellW === 18`
 * and met 20).
 */
const FONT_SIZE_A = 20;
const FONT_SIZE_B = 10;
const FONT_FAMILY = "monospace";

// ── the two fake backends ─────────────────────────────────────────────────────────────────────

/**
 * One pane's content, as a real consumer's backend would supply it — except that there is no engine
 * here, so the "backend" is an array of strings and a timer.
 *
 * The two panes tick at **different periods** on purpose. "Both draw" and "each draws its own
 * content" are two different claims, and only the second needs independence; a page where both
 * updated on one timer could satisfy every pixel assertion while the two grids were in fact one.
 */
class Pane {
  readonly source = new StubFrameSource();
  private cols = 0;
  private rows = 0;
  private tick = 0;

  constructor(
    readonly name: string,
    private readonly label: string,
  ) {}

  /** Adopt the grid the renderer actually gave us — never the one we asked for (#339). */
  setGrid(cols: number, rows: number): void {
    this.cols = cols;
    this.rows = rows;
  }

  /** Advance the content by one step. Separate from {@link frame} so a probe can re-push the SAME
   * content and see the same pixels, which is what makes a byte-for-byte comparison meaningful. */
  advance(): void {
    this.tick++;
  }

  get step(): number {
    return this.tick;
  }

  /**
   * Build the viewport frame. Every cell of every row is emitted, padded with U+0020 — what a real
   * core sends, and what keeps blank cells painting `defaultBg` rather than being left unwritten.
   */
  frame(): DecodedFrame {
    const { cols, rows } = this;
    const lines: string[] = [];
    lines.push(this.label);
    lines.push(`step ${this.tick}`);
    for (let i = lines.length; i < rows; i++) {
      lines.push(`${this.name}${i} ${"·".repeat(Math.max(0, (i * 3) % 17))}`);
    }

    const codepoints: number[] = [];
    const spans: number[] = [];
    let offset = 0;
    for (let row = 0; row < rows; row++) {
      const chars = [...(lines[row] ?? "")];
      chars.length = cols;
      spans.push(row, 0, cols - 1, offset, cols);
      for (const c of chars) codepoints.push(c ? c.codePointAt(0)! : 0x20);
      offset += cols;
    }
    const n = codepoints.length;
    return {
      cols,
      rows,
      kind: 0,
      codepoints,
      fg: new Array<number>(n).fill(0),
      bg: new Array<number>(n).fill(0),
      flags: new Array<number>(n).fill(0),
      extra: new Array<number>(n).fill(0),
      spans,
      sideTable: [],
    };
  }
}

// ── composition ───────────────────────────────────────────────────────────────────────────────

const stage = document.querySelector<HTMLDivElement>("#stage")!;
const canvas = document.querySelector<HTMLCanvasElement>("#surface")!;
const overlayA = document.querySelector<HTMLDivElement>("#pane-a")!;
const overlayB = document.querySelector<HTMLDivElement>("#pane-b")!;

stage.style.width = `${CANVAS.width}px`;
stage.style.height = `${CANVAS.height}px`;
for (const [el, box] of [
  [overlayA, PANES.a],
  [overlayB, PANES.b],
] as const) {
  el.style.left = `${box.left}px`;
  el.style.top = `${box.top}px`;
  el.style.width = `${box.width}px`;
  el.style.height = `${box.height}px`;
}

/**
 * **The host composes the surface, and the host sizes the drawing buffer.** Both are this page's
 * job rather than a terminal's, and that is the whole difference from `JustermRenderer.create`: a
 * buffer shared by N grids in M font configurations has no cell it can be an exact multiple of, so
 * the widget that used to derive it cannot (ADR-0021 D3), and only whoever measured the container
 * can.
 *
 * Device px, because that is the space `setViewport` takes — one canvas addressed in one space. The
 * canvas's CSS display box is written by `resizeSurface` from `cssWidth`/`cssHeight`, which is the
 * *granted* buffer rather than the requested one (#339).
 */
const surface = await TerminalSurface.open("#surface");
surface.resizeSurface(
  Math.round(CANVAS.width * window.devicePixelRatio),
  Math.round(CANVAS.height * window.devicePixelRatio),
);

interface Attached {
  readonly pane: Pane;
  readonly renderer: JustermRenderer;
  readonly term: Terminal;
  readonly overlay: HTMLDivElement;
  readonly box: PaneBox;
  readonly stopRect: () => void;
  /**
   * The last origin this page **gave** the renderer, device px — not one re-derived at read time.
   *
   * Recorded here because this page is the origin's producer: it measures the DOM box, so it is the
   * site the fact is first true at, and a reader that recomputes from `window.devicePixelRatio` is
   * reading a *different* fact that merely usually agrees. Measured while driving this page: after a
   * viewport change that moved the density without firing `matchMedia` (which is how CDP moves it —
   * a negative result `e2e/demo.spec.ts` already records), the re-derived origin was `500` while the
   * renderer was still drawing at the `750` it had been given, and the probe's rect covered a region
   * no grid was in. The grid was where it was told to be; the reader was wrong.
   */
  rect: { x: number; y: number };
  ended: boolean;
}

async function attach(
  pane: Pane,
  overlay: HTMLDivElement,
  box: PaneBox,
  fontSize: number,
  theme: Theme,
): Promise<Attached> {
  const renderer = await JustermRenderer.attach(surface, {
    fontFamily: FONT_FAMILY,
    fontSize,
    theme,
  });

  // Place the grid on the shared buffer, and keep it placed. `observeViewportRect` computes the
  // origin from the overlay's offset within the canvas, in device px, and re-computes it on a box
  // change AND on a scroll of any ancestor — the second is what a bare `ResizeObserver` misses,
  // since an element that MOVES at an unchanged size fires nothing.
  //
  // It syncs once on the way in, so the grid is placed before the first fit below. That first call
  // lands while the grid is still 0x0 and is a no-op inside the adapter; the rect it stores is what
  // the fit then re-issues.
  const rect = { x: 0, y: 0 };
  const stopRect = observeViewportRect(overlay, canvas, (x, y) => {
    rect.x = x;
    rect.y = y;
    renderer.setViewportRect(x, y);
  });

  // How many cells fit in THIS pane — not in the canvas. The adapter's #339 grant read-back compares
  // this grid's columns against the whole drawing buffer, so for a pane smaller than the surface it
  // can never fire; the grant on a shared surface is the host's to read from `surface.cssSize()`.
  renderer.resize(box.width, box.height);
  const { cols, rows } = renderer.terminalSize();
  pane.setGrid(cols, rows);

  const term = new Terminal(pane.source, renderer, {
    element: overlay,
    // The pane's own input target. Each terminal mounts its own hidden textarea inside its own
    // overlay, which is what "one stacking plane, N overlays" means in practice — and the reason
    // the widget's real keyboard/IME target has never been the canvas (#116). Logged rather than
    // encoded: this page has no engine, so an intent has nowhere real to go.
    input: { send: (intent) => console.log(`[${pane.name}] intent ${intent.kind}`) },
    // The origin is the **overlay's**, not the canvas's, because the overlay is where this grid is
    // drawn — on a shared surface the two differ by exactly the offset `observeViewportRect` feeds
    // the renderer, and handing over the canvas's origin would put every pointer event in pane A's
    // coordinate space.
    //
    // The cell is `cellSize()` (device px) divided by the ratio, because {@link CellGeometry} takes
    // CSS px. That division un-rounds a number the renderer rounded, so it can be off by a fraction
    // of a CSS pixel at a fractional density; harmless for a pointer→cell mapping, and named here
    // because it is exactly the kind of silent conversion this package refuses to make on the
    // renderer's behalf elsewhere.
    getGeometry: (): CellGeometry => {
      const r = overlay.getBoundingClientRect();
      const cell = renderer.cellSize();
      const dpr = window.devicePixelRatio;
      const { cols, rows } = renderer.terminalSize();
      return {
        originX: r.left,
        originY: r.top,
        cellWidth: cell.width / dpr,
        cellHeight: cell.height / dpr,
        cols,
        rows,
      };
    },
  });
  term.mount();
  pane.source.push(pane.frame());

  return { pane, renderer, term, overlay, box, stopRect, rect, ended: false };
}

const a = await attach(new Pane("a", "PANE A — 20px"), overlayA, PANES.a, FONT_SIZE_A, themeFor(FG_A, BG_A));
const b = await attach(new Pane("b", "pane b — 10px"), overlayB, PANES.b, FONT_SIZE_B, themeFor(FG_B, BG_B));
const attached: Attached[] = [a, b];

/**
 * The one event after which every device-px number this page gave the surface is wrong (ADR-0021
 * D3). A pane at CSS x=500 replayed unchanged across dpr 1 → 2 lands at half its correct offset, at
 * twice its width, over its left sibling — no error, and every pixel plausible.
 *
 * Nothing below this layer can pay it: the renderer refuses to scale a stored device-px quantity by
 * its own copy of the density (that copy lags — a density notification arriving during a context
 * loss is dropped), and `TerminalSurface` refuses for the same reason. So the host re-measures. The
 * rects come back on their own, because `observeViewportRect` re-derives from `getBoundingClientRect`
 * at the live ratio; what only this page can re-supply is the buffer.
 */
surface.onDensityChange((dpr) => {
  surface.resizeSurface(Math.round(CANVAS.width * dpr), Math.round(CANVAS.height * dpr));
  for (const t of attached) {
    if (t.ended) continue;
    t.renderer.resize(t.box.width, t.box.height);
    const { cols, rows } = t.renderer.terminalSize();
    t.pane.setGrid(cols, rows);
    t.pane.source.push(t.pane.frame());
  }
  console.log(`[surface] density ${dpr}`);
});

// Two cadences, deliberately coprime-ish so a sample can never catch them in lockstep.
const TICK_A = 300;
const TICK_B = 700;
let timerA = window.setInterval(() => {
  a.pane.advance();
  a.pane.source.push(a.pane.frame());
}, TICK_A);
let timerB = window.setInterval(() => {
  b.pane.advance();
  b.pane.source.push(b.pane.frame());
}, TICK_B);

/** Stop both content timers. Every probe below calls this first: a page that keeps pushing frames
 * turns "did the restore repaint" into "did a timer fire", which is the mistake `__disposeProbe`
 * and `__contextLossProbe` on the other page each had to learn once. */
function stopTimers(): void {
  window.clearInterval(timerA);
  window.clearInterval(timerB);
  timerA = 0;
  timerB = 0;
}

document.querySelector<HTMLButtonElement>("[data-testid='lose-context']")!.addEventListener(
  "click",
  () => {
    const gl = canvas.getContext("webgl2");
    const ext = gl?.getExtension("WEBGL_lose_context");
    if (!ext) return;
    ext.loseContext();
    setTimeout(() => ext.restoreContext(), 100);
  },
);

document.querySelector<HTMLButtonElement>("[data-testid='end-a']")!.addEventListener("click", () => {
  if (a.ended) return;
  a.ended = true;
  a.stopRect();
  a.term.dispose();
  console.log("[a] ended");
});

// ── probes ────────────────────────────────────────────────────────────────────────────────────

/** One pane's placement and the pixels inside it. */
export interface PaneSnapshot {
  /** The grid actually adopted — `terminalSize()`, not what `resize` was asked for. */
  cols: number;
  rows: number;
  /** The cell, in **device** px. Never compared to a literal in a spec: the cell is the ink box of
   * the font's `█` (ADR-0022) and CI's fonts are not this machine's (#578). */
  cellW: number;
  cellH: number;
  /** Where the renderer was told to put this grid — device px, top-origin, as `observeViewportRect`
   * computed it from the overlay's offset within the canvas and this page then recorded. */
  rectX: number;
  rectY: number;
  /** The centre pixel of the pane, `r,g,b,a`. Names which grid painted it. */
  centre: string;
  /** FNV-1a over the pane's whole rect. Two captures of one unchanged grid agree exactly; any
   * repaint difference moves it. Cheap enough to take on both panes around every operation. */
  hash: number;
  /** How many times this pane's content has advanced — so "nothing was re-fed" is a number rather
   * than a claim about what the page did not do. */
  step: number;
}

export interface SurfaceSnapshot {
  dpr: number;
  /** The context's own verdict. Headless SwiftShader loses contexts on its own (#580), and every
   * pixel below reads `0,0,0,0` when it has — an environment failure, not a defect. */
  contextLost: boolean;
  /** The shared drawing buffer, device px, and its CSS display box. */
  bufW: number;
  bufH: number;
  cssW: number;
  cssH: number;
  /** A pixel in the gutter between the panes, and one in the band above pane B — canvas that no
   * grid was placed over. `draw()` clears the whole buffer to `rgba(0,0,0,0)` before any grid's
   * scissored clear, so these must be fully transparent. They are the evidence that the two panes
   * are on ONE canvas: if a pane's background reached here, the panes would not be separate rects. */
  gutter: string;
  band: string;
  a: PaneSnapshot;
  b: PaneSnapshot;
}

declare global {
  interface Window {
    __surfaceProbe?: () => SurfaceSnapshot;
    /**
     * `r,g,b,a` at one device-px point of the shared buffer, top-origin, after a present.
     *
     * Exists because a *recorded* rect cannot prove a placement. `SurfaceSnapshot.rectX/rectY`
     * report the origin this page GAVE the renderer, which is the right thing for locating a rect to
     * digest — and it stays exactly as true when the `setViewportRect` call is deleted, so an
     * assertion on it says nothing about whether the renderer was told. Measured: with that one call
     * removed the placement test still passed while three others went red. A pixel at a
     * DOM-derived point is the reading that cannot be satisfied by a number nobody sent.
     */
    __pixelAt?: (x: number, y: number) => string;
    __independenceProbe?: () => Promise<{
      before: SurfaceSnapshot;
      after: SurfaceSnapshot;
    }>;
    __surfaceLossProbe?: () => Promise<{
      before: SurfaceSnapshot;
      /** Read with NO await between `loseContext()` and the two reads — the instant in which the
       * driver and the widget's reported state disagree (#639). Asserting the window exists is what
       * keeps the restore assertions from passing vacuously on a context that never died. */
      raceWindow: { glSaysLost: boolean; widgetSaysLost: boolean };
      /**
       * Read **inside** the `webglcontextrestored` turn, with no present of our own — so the only
       * thing that can have drawn these pixels is {@link TerminalSurface}'s own restore handler.
       *
       * This exists because the obvious version of this probe could not fail. `snapshot()` presents
       * before it reads, and presenting is exactly what makes the renderer run its deferred rebuild
       * — so a probe that presents supplies the thing the handler is there to supply. Measured:
       * with the surface's `webglcontextrestored` listener deleted outright, and again with
       * `reapplyAll` cut to one grid, all five tests stayed green. The reader was proving the
       * renderer's recovery and calling it the surface's.
       *
       * The surface registers its listener in its constructor and this one is added at probe time,
       * so DOM registration order puts the handler's two presents before this read, in the same
       * task — which is also why the drawing buffer is still readable, `preserveDrawingBuffer`
       * being off.
       */
      afterRestoreNoPresent: { a: string; b: string; gutter: string };
      /** After `webglcontextrestored`, one `present()`, and **no frame fed from this page**. */
      after: SurfaceSnapshot;
      /** How many frames this page pushed between `before` and `after`. Must be `0`: recovery here
       * means the renderer repaints from its own retained grids, because a frame-mode consumer has
       * no retained state to be asked again for (ADR-0020 R3). */
      framesPushed: number;
    }>;
  }
}

const gl = (): WebGL2RenderingContext | null => canvas.getContext("webgl2");

/** `r,g,b,a` at a device-px point, counted from the buffer's **bottom** like every `readPixels`. */
function pixelAt(context: WebGL2RenderingContext, x: number, yTop: number): string {
  const px = new Uint8Array(4);
  context.readPixels(
    x,
    context.drawingBufferHeight - 1 - yTop,
    1,
    1,
    context.RGBA,
    context.UNSIGNED_BYTE,
    px,
  );
  return `${px[0]},${px[1]},${px[2]},${px[3]}`;
}

/** FNV-1a over a device-px rect. A whole-rect digest rather than a handful of samples, so a
 * "unchanged" claim covers every pixel of the sibling rather than the three someone thought to
 * check — the differential shape `justerm-renderer/demo/per-grid-state.html` established. */
function hashRect(
  context: WebGL2RenderingContext,
  x: number,
  yTop: number,
  w: number,
  h: number,
): number {
  if (w <= 0 || h <= 0) return 0;
  const buf = new Uint8Array(w * h * 4);
  context.readPixels(
    x,
    context.drawingBufferHeight - yTop - h,
    w,
    h,
    context.RGBA,
    context.UNSIGNED_BYTE,
    buf,
  );
  let hash = 0x811c9dc5;
  for (let i = 0; i < buf.length; i++) {
    hash ^= buf[i]!;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

/** Where this pane's viewport is, derived exactly as the page derived it for the renderer — the
 * same function, not a second copy of the arithmetic. */
function rectOf(t: Attached): { x: number; y: number; w: number; h: number } {
  const cell = t.renderer.cellSize();
  const { cols, rows } = t.renderer.terminalSize();
  // The origin this page GAVE (see `Attached.rect`); the extent as the renderer derives it — `cols *
  // cell`, both integers it hands back, which is why `setViewportRect` takes no extent at all.
  return { x: t.rect.x, y: t.rect.y, w: cols * cell.width, h: rows * cell.height };
}

function snapshotPane(t: Attached, context: WebGL2RenderingContext): PaneSnapshot {
  const r = rectOf(t);
  const cell = t.renderer.cellSize();
  const { cols, rows } = t.renderer.terminalSize();
  return {
    cols,
    rows,
    cellW: cell.width,
    cellH: cell.height,
    rectX: r.x,
    rectY: r.y,
    centre: pixelAt(context, r.x + (r.w >> 1), r.y + (r.h >> 1)),
    hash: hashRect(context, r.x, r.y, r.w, r.h),
    step: t.pane.step,
  };
}

/**
 * Present, then read — **in the same turn**, with nothing awaited in between.
 *
 * The context is created without `preserveDrawingBuffer`, so the buffer is undefined once the frame
 * is presented to the compositor. A read that loses the race comes back all zeroes, which reads
 * exactly like "nothing was drawn".
 */
function snapshot(): SurfaceSnapshot {
  surface.present();
  const context = gl()!;
  const css = surface.cssSize();
  const dpr = window.devicePixelRatio;
  // The gutter's horizontal centre, at the vertical centre of the canvas; and the band above pane B,
  // sampled at pane B's own horizontal centre so it is directly above painted pixels.
  const gutterX = Math.round(((PANES.a.left + PANES.a.width + PANES.b.left) / 2) * dpr);
  const bandX = Math.round((PANES.b.left + PANES.b.width / 2) * dpr);
  return {
    dpr,
    contextLost: context.isContextLost(),
    bufW: context.drawingBufferWidth,
    bufH: context.drawingBufferHeight,
    cssW: css.width,
    cssH: css.height,
    gutter: pixelAt(context, gutterX, Math.round((CANVAS.height / 2) * dpr)),
    band: pixelAt(context, bandX, Math.round((PANES.b.top / 2) * dpr)),
    a: snapshotPane(a, context),
    b: snapshotPane(b, context),
  };
}

window.__pixelAt = (x: number, y: number): string => {
  surface.present();
  return pixelAt(gl()!, Math.round(x), Math.round(y));
};

window.__surfaceProbe = (): SurfaceSnapshot => {
  stopTimers();
  return snapshot();
};

window.__independenceProbe = async (): Promise<{ before: SurfaceSnapshot; after: SurfaceSnapshot }> => {
  stopTimers();
  // Settle first: the timers may have pushed a frame whose present is still owed to a rAF, and a
  // `before` taken across that boundary would differ from `after` for a reason that is not the
  // change under test.
  await new Promise<void>((r) => requestAnimationFrame(() => r()));
  const before = snapshot();

  // ONLY pane A advances. Pane B is fed nothing — and `present()` still redraws the whole canvas,
  // which is the point: B's rect must come back byte-identical from B's own retained grid while A's
  // moves.
  a.pane.advance();
  a.pane.source.push(a.pane.frame());
  const after = snapshot();
  return { before, after };
};

window.__surfaceLossProbe = async (): Promise<{
  before: SurfaceSnapshot;
  raceWindow: { glSaysLost: boolean; widgetSaysLost: boolean };
  afterRestoreNoPresent: { a: string; b: string; gutter: string };
  after: SurfaceSnapshot;
  framesPushed: number;
}> => {
  stopTimers();
  const context = gl()!;
  const ext = context.getExtension("WEBGL_lose_context");
  if (!ext) throw new Error("WEBGL_lose_context unavailable — this probe cannot run");

  await new Promise<void>((r) => requestAnimationFrame(() => r()));
  const before = snapshot();

  // Each await is on a browser event that is *permitted* not to arrive, so each names itself on
  // expiry: a probe that hangs tells you only that something did.
  const once = (event: string, budget = 5000): Promise<void> =>
    new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error(`#776 loss probe: no ${event} within ${budget}ms`)),
        budget,
      );
      canvas.addEventListener(
        event,
        () => {
          clearTimeout(timer);
          resolve();
        },
        { once: true },
      );
    });

  const lost = once("webglcontextlost");
  ext.loseContext();
  // No await between the call and these two reads. A browser destroys the context SYNCHRONOUSLY and
  // only queues the event, so this is the window in which the driver says lost and the widget's
  // report — which is event-driven by design (ADR-0027 D4) — does not.
  const raceWindow = { glSaysLost: context.isContextLost(), widgetSaysLost: surface.isContextLost() };
  await lost;

  // A macrotask yield between the loss and the restore. Not a settling fudge: without it Chromium
  // never fires `webglcontextrestored` at all — the same finding the other page's probe and the
  // renderer's own GL proofs both record.
  await new Promise<void>((r) => setTimeout(r, 0));
  // Registered AFTER the surface's own (constructor-time) listener, so this runs once that handler
  // has presented — in the same task, which is what keeps the buffer readable. Nothing here presents.
  const readInRestoreTurn = new Promise<{ a: string; b: string; gutter: string }>((resolve) => {
    canvas.addEventListener(
      "webglcontextrestored",
      () => {
        const c = gl()!;
        const ra = rectOf(a);
        const rb = rectOf(b);
        resolve({
          a: pixelAt(c, ra.x + (ra.w >> 1), ra.y + (ra.h >> 1)),
          b: pixelAt(c, rb.x + (rb.w >> 1), rb.y + (rb.h >> 1)),
          gutter: pixelAt(
            c,
            Math.round(((PANES.a.left + PANES.a.width + PANES.b.left) / 2) * window.devicePixelRatio),
            Math.round((CANVAS.height / 2) * window.devicePixelRatio),
          ),
        });
      },
      { once: true },
    );
  });

  const restored = once("webglcontextrestored");
  ext.restoreContext();
  await restored;
  const afterRestoreNoPresent = await readInRestoreTurn;

  // Nothing is fed. `TerminalSurface`'s restore handler presents, asks every attached terminal to
  // re-derive, and presents again; the renderer rebuilds inside `render()`, from the grids it
  // retained. `snapshot()` presents once more, which is what makes the read safe rather than what
  // makes the recovery happen.
  const after = snapshot();
  return { before, raceWindow, afterRestoreNoPresent, after, framesPushed: 0 };
};

// ── the boot gate ─────────────────────────────────────────────────────────────────────────────

/**
 * Written **last**, after both terminals are mounted and every probe is installed.
 *
 * `demo/index.html`'s suite gates on its control bar, which is a *proxy*: it mounts ~350 lines
 * before the probe assignments, and is sound only because the one `await` between them resolves on
 * the microtask queue. That validity condition is recorded in that spec and it is not one this page
 * has to inherit — here the gate is a node the subject emits once it is genuinely ready, which is
 * also what the one comparable Playwright suite does (`test/playwright/TestUtils.ts:515` waits for
 * `.xterm-rows`, a node the terminal itself renders).
 */
document.querySelector<HTMLParagraphElement>("[data-testid='surface-ready']")!.textContent =
  `ready — A ${a.renderer.terminalSize().cols}x${a.renderer.terminalSize().rows}` +
  ` · B ${b.renderer.terminalSize().cols}x${b.renderer.terminalSize().rows}` +
  ` · dpr ${window.devicePixelRatio}`;
