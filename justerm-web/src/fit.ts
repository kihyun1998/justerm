/**
 * Fit: container pixel size → terminal `cols`/`rows` (#114), the frame-mode analog
 * of xterm.js `FitAddon.proposeDimensions`. Pure geometry — no DOM: the caller reads
 * the parent box, element padding, cell size, and scrollbar width, and this proposes
 * the grid that fills the available space. The resize *intent* (drive the backend's
 * `Engine::resize` + the PTY SIGWINCH) is the caller's, via a {@link ResizePort}.
 */

/** Element padding (CSS px) subtracted from the parent box before fitting. */
export interface FitPadding {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

/** Everything {@link proposeDimensions} needs — read by the DOM adapter from the
 * container box, the terminal element's padding, the renderer's cell size, and the
 * scrollbar. Structural, so the adapter (and tests) build it without a real DOM. */
export interface FitInput {
  /** The container (parent) content box, CSS px. */
  parentWidth: number;
  parentHeight: number;
  /** The terminal element's padding, CSS px. */
  padding: FitPadding;
  /** The renderer's CSS cell size, px (beamterm cell). */
  cellWidth: number;
  cellHeight: number;
  /** Px reserved for the scrollbar when it shows. */
  scrollbarWidth: number;
  /** Scrollback lines; `0` reserves no scrollbar width (#112 coupling). */
  scrollback: number;
}

/** A proposed terminal grid size. */
export interface Dimensions {
  cols: number;
  rows: number;
}

/** The smallest usable grid (xterm `FitAddon` `MINIMUM_COLS`/`MINIMUM_ROWS`). */
const MINIMUM_COLS = 2;
const MINIMUM_ROWS = 1;

/**
 * Propose the `cols`/`rows` that fill the available box (xterm `FitAddon`).
 */
export function proposeDimensions(input: FitInput): Dimensions | undefined {
  // The renderer hasn't measured a cell yet → can't fit (xterm's `cell.width === 0` guard).
  if (input.cellWidth === 0 || input.cellHeight === 0) return undefined;
  // The scrollbar only shows (and only reserves width) when there is scrollback — matches
  // xterm `FitAddon` (`scrollback === 0 ? 0 : ...`) and the #112 scrollbar's hide-at-0.
  const scrollbarWidth = input.scrollback === 0 ? 0 : input.scrollbarWidth;
  const availWidth = input.parentWidth - (input.padding.left + input.padding.right) - scrollbarWidth;
  const availHeight = input.parentHeight - (input.padding.top + input.padding.bottom);
  const cols = Math.max(MINIMUM_COLS, Math.floor(availWidth / input.cellWidth));
  const rows = Math.max(MINIMUM_ROWS, Math.floor(availHeight / input.cellHeight));
  // NaN box metrics (a detached/unmeasured element) propose a NaN grid — skip (xterm's
  // `isNaN(dims.cols) || isNaN(dims.rows)` guard).
  if (Number.isNaN(cols) || Number.isNaN(rows)) return undefined;
  return { cols, rows };
}

/** The write-side resize intent: the backend applies `Engine::resize(cols, rows)` and
 * resizes the PTY window (SIGWINCH). Mirrors the sibling `SelectionPort`/`SearchPort`. */
export interface ResizePort {
  resize(cols: number, rows: number): void;
}

/** A recording {@link ResizePort} for tests. */
export class StubResizePort implements ResizePort {
  readonly calls: Dimensions[] = [];
  resize(cols: number, rows: number): void {
    this.calls.push({ cols, rows });
  }
}

/** Default debounce for coalescing a burst of resize events (our design choice —
 * xterm's `FitAddon` is manual, no debounce). */
const DEFAULT_DEBOUNCE_MS = 100;

/**
 * Debounces container-resize events into a single backend resize intent (#114). The DOM
 * adapter feeds each observed geometry to {@link fit}; the controller coalesces a burst,
 * proposes the grid, and drives the {@link ResizePort}. Pure logic — the debounce clock is
 * injected (defaults to `setTimeout`), so it's unit-tested without real time or a DOM.
 */
export class FitController {
  private readonly port: ResizePort;
  private readonly debounceMs: number;
  private readonly setTimer: (fn: () => void, ms: number) => number;
  private readonly clearTimer: (handle: number) => void;
  private latest: FitInput | undefined;
  private timer: number | undefined;
  /** The last grid actually emitted, to skip a resize that doesn't change it. */
  private lastCols: number | undefined;
  private lastRows: number | undefined;

  constructor(opts: {
    port: ResizePort;
    debounceMs?: number;
    setTimer?: (fn: () => void, ms: number) => number;
    clearTimer?: (handle: number) => void;
  }) {
    this.port = opts.port;
    this.debounceMs = opts.debounceMs ?? DEFAULT_DEBOUNCE_MS;
    this.setTimer = opts.setTimer ?? ((fn, ms) => setTimeout(fn, ms) as unknown as number);
    this.clearTimer = opts.clearTimer ?? ((h) => clearTimeout(h));
  }

  /** A container-resize was observed with this geometry. Coalesced on the debounce timer. */
  fit(input: FitInput): void {
    this.latest = input;
    if (this.timer !== undefined) this.clearTimer(this.timer);
    this.timer = this.setTimer(() => this.flush(), this.debounceMs);
  }

  /** Cancel a pending debounced fit (the sibling controllers' dispose pattern). */
  dispose(): void {
    if (this.timer !== undefined) {
      this.clearTimer(this.timer);
      this.timer = undefined;
    }
  }

  private flush(): void {
    this.timer = undefined;
    if (!this.latest) return;
    const dims = proposeDimensions(this.latest);
    if (!dims) return;
    if (dims.cols === this.lastCols && dims.rows === this.lastRows) return; // unchanged → skip
    this.lastCols = dims.cols;
    this.lastRows = dims.rows;
    this.port.resize(dims.cols, dims.rows);
  }
}

/**
 * Auto-track container resizes (justerm-web's design choice — xterm's `FitAddon` is a
 * manual `fit()`). Observes `element` with a `ResizeObserver`; on each resize, reads the
 * current geometry via `readInput` and feeds {@link FitController.fit} (which debounces).
 * Returns a disposer. The only DOM touch in this module, and lazy — referenced only when
 * called, so the pure fit logic stays unit-testable without a DOM.
 */
export function observeResize(
  element: Element,
  readInput: () => FitInput,
  controller: FitController,
): () => void {
  const ro = new ResizeObserver(() => controller.fit(readInput()));
  ro.observe(element);
  return () => ro.disconnect();
}
