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
  /**
   * The renderer's CSS cell size, px — `cellSize()` divided by `devicePixelRatio`, or
   * `cssCellWidth()`/`cssCellHeight()` where the backend offers them.
   *
   * **Pass it live and unrounded.** Since #632 this is not only a divisor: {@link FitController}
   * carries it in the key that decides whether a flush is redundant, so it is also how the
   * controller learns that a cell-size change happened. A `readInput` that rounds it (or caches it
   * across a spacing change) silently defeats that — the fit still *looks* right while a real
   * resize can be deduped away, which is the failure #632 exists to close.
   */
  cellWidth: number;
  cellHeight: number;
  /** Px a *layout* scrollbar occupies (subtracted from width). Pass `0` for an overlay
   * scrollbar like justerm's #112 (which floats over the canvas and reserves no width). */
  scrollbarWidth: number;
  /** Scrollback lines; `0` means no scrollbar can show, so its width isn't reserved. */
  scrollback: number;
}

/** A proposed terminal grid size. */
export interface Dimensions {
  cols: number;
  rows: number;
}

/**
 * The smallest grid the engine can be in — the mirror of `justerm_core::MIN_COLUMNS` (#547) and
 * of its row floor, which happen to be the same values xterm's `FitAddon` uses
 * (`MINIMUM_COLS`/`MINIMUM_ROWS`).
 *
 * Exported because this package has **two** paths from a pixel box to a grid — this one and
 * {@link import("./justerm-renderer").gridForBox} — and they must not disagree. They did: a
 * 1-column proposal is a grid the engine can never adopt, since `Engine::resize(1, r)` silently
 * yields 2. A consumer driving the engine at 1 while it holds 2 puts every span of the frame
 * outside the renderer's grid, and the surface stops updating.
 *
 * **That was one axis of the agreement; the other was found later (#632).** This path also *refuses*
 * (`undefined`) for an unmeasured cell or a non-finite box, and `gridForBox` did not — so the path
 * that actually reaches the renderer turned an unlaid-out container into a 1×1 terminal while the
 * guarded path was the one nothing calls. Both now refuse. When adding a third box→grid path, check
 * **both** axes: the floor and the refusal.
 *
 * Hand-mirrored, like the wasm getters in `types.ts`: the core constant is Rust and does not
 * cross the wire. If core ever raises its floor, this is the line that must follow.
 */
export const MINIMUM_COLS = 2;
export const MINIMUM_ROWS = 1;

/**
 * Propose the `cols`/`rows` that fill the available box (xterm `FitAddon`).
 */
export function proposeDimensions(input: FitInput): Dimensions | undefined {
  // The renderer hasn't measured a cell yet → can't fit (xterm's `cell.width === 0` guard).
  if (input.cellWidth === 0 || input.cellHeight === 0) return undefined;
  // **A box with no area is "not measured", and the floor below is what hid that** (#810). An element
  // with no box — `display: none`, detached, not yet laid out — reports every field as `0`, and `0`
  // is finite, so the non-finite refusal at the bottom of this function never sees it while the
  // `Math.max` floor turns it into a plausible `2`x`1`. That is the same sentence the refusal already
  // makes ("not measured, exactly when we should NOT shrink"), applied to the second way a browser
  // says it.
  //
  // **All three references floor here instead, and the divergence is ours on purpose.** xterm.js
  // (`addon-fit/src/FitAddon.ts`), alacritty (`alacritty/src/display/mod.rs:249`) and ghostty
  // (`src/renderer/size.zig:260`) all clamp up to a minimum with no zero-box branch. Two of them own
  // an OS window, which cannot be `display: none`, so a zero box is a degenerate state and flooring
  // is a sane recovery. xterm.js is in the same world as us and still floors — but its `FitAddon`
  // registers **no listeners**, so a hidden terminal is simply never fitted. Ours is automatic
  // (`observeResize` drives this from a `ResizeObserver`, which `display: none` fires), so we receive
  // an input they do not. We automated what they left manual, and this is the cost of that.
  //
  // **The guard is on the PARENT box, not on the space left after padding.** A parent that exists but
  // whose padding exceeds it is a layout the host measured and chose; an absent parent is not a
  // layout at all. Flooring stays right for the first.
  if (input.parentWidth <= 0 || input.parentHeight <= 0) return undefined;
  // A *layout* scrollbar (one that occupies width) only reserves it when there is scrollback
  // to scroll — matches xterm `FitAddon` (`scrollback === 0 ? 0 : ...`). NB: justerm's own
  // #112 scrollbar is an OVERLAY (`position:absolute`, no layout width) — content sits under
  // it — so a consumer using that scrollbar must pass `scrollbarWidth: 0` (as the demo does);
  // this path exists for xterm parity / a hypothetical layout scrollbar, not the #112 overlay.
  const scrollbarWidth = input.scrollback === 0 ? 0 : input.scrollbarWidth;
  const availWidth = input.parentWidth - (input.padding.left + input.padding.right) - scrollbarWidth;
  const availHeight = input.parentHeight - (input.padding.top + input.padding.bottom);
  const cols = Math.max(MINIMUM_COLS, Math.floor(availWidth / input.cellWidth));
  const rows = Math.max(MINIMUM_ROWS, Math.floor(availHeight / input.cellHeight));
  // Non-finite box metrics (NaN from a detached/unmeasured element, or Infinity from a
  // degenerate input) propose a non-finite grid — skip (widening xterm's `isNaN(dims.cols)`
  // guard to all non-finite values). Deliberately more conservative than xterm, which
  // clamps a NaN parent to 0 and resizes to the 2×1 minimum: a non-finite box means "not
  // measured", exactly when we should NOT shrink the terminal.
  if (!Number.isFinite(cols) || !Number.isFinite(rows)) return undefined;
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
  /**
   * The last intent actually emitted — the grid **and the cell it was derived from** (#632).
   *
   * `cols`/`rows` alone is half of what {@link proposeDimensions} consumes: the same pair can come
   * from different cells, so a `cols`/`rows`-only memory cannot express *"the cell moved but the
   * grid did not"*. Two silent failures followed from that, and both are in this one field:
   *
   * 1. A **cell change re-derives the grid outside this controller** — per #578 the consumer calls
   *    `renderer.resize()` directly — so the remembered pair kept describing the pre-change grid. A
   *    later genuine container resize that happened to propose exactly that pair was deduped and
   *    never reached the {@link ResizePort}: the engine holds a grid the box no longer wants, which
   *    is the desync #547 describes (spans land outside the grid and the surface stops updating).
   * 2. It is also why routing a cell change *through* `fit()` used to be unsafe at all — the cell
   *    can move while the grid stays identical, guaranteed once the {@link MINIMUM_COLS} /
   *    {@link MINIMUM_ROWS} floors bind.
   *
   * **Validity condition, because the cell is a proxy rather than the thing itself.** What actually
   * invalidates this memory is *any* write to the grid that bypasses this controller; the cell is
   * merely the one such write that exists today (the #578 setters). A consumer that called
   * `renderer.resize()` out of band with an **unchanged** cell would still leave this stale. The
   * structural cure is to hold no memory at all and let the port dedupe against its own live state,
   * which is what xterm.js does (`FitAddon.fit()` has no dedupe; `Terminal.resize` compares against
   * `this.cols`/`this.rows`) — rejected here because {@link ResizePort} is write-only and published,
   * so it would hand every consumer a new idempotency obligation over `Engine::resize` + SIGWINCH.
   * Revisit the day `ResizePort` can be read, or a second out-of-band writer appears.
   *
   * **Named prior art for the shape actually taken: alacritty**, which coalesces both input families
   * and then runs *two* dedupes over the result — a grid-keyed one that drives the PTY and the
   * terminal, and a whole-`SizeInfo` one (its `PartialEq` covers `cell_width`/`cell_height`) that
   * drives the renderer. Its memory is a live-updated snapshot, exactly like this field, not state
   * read back from a sink. ghostty splits the two obligations instead: it dedupes the *box* path and
   * leaves its cell path (`setCellSize`) undeduped entirely. So the three references do not agree on
   * one shape, and this is the one whose constraints match ours.
   *
   * **Two known holes this key does not close** — both recorded rather than papered over:
   * 1. `cols`/`rows` here is what was *proposed*, and **less may be adopted** (the #339
   *    drawing-buffer clamp; `terminalSize()` is the documented truth). A clamped resize leaves this
   *    describing a grid nobody holds — the same defect one axis over.
   *
   *    *Who* adopts it moved at renderer 0.15.0 (#773): the renderer clamps the shared drawing
   *    buffer and leaves the grid saying what it was told, so `JustermRenderer.resize` reads the
   *    grant back and shrinks the grid itself. This hole is unchanged — the key still remembers the
   *    proposal — but the thing it can disagree with now lives one layer nearer.
   * 2. {@link FitController.latest} is a **snapshot** taken when the observer fired, so a cell change
   *    landing inside the debounce window makes the flush propose against the *pre-change* cell and
   *    store it. It self-heals on the next observer fire, and there may not be one. The cure is to
   *    read the geometry at flush time rather than replay a snapshot, which is what xterm's `fit()`
   *    does — a change to this class's shape, not to this key.
   */
  private last: { cols: number; rows: number; cellWidth: number; cellHeight: number } | undefined;

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
    const { cellWidth, cellHeight } = this.latest;
    const prev = this.last;
    // Redundant only if BOTH the proposal and the cell it came from are unchanged (#632).
    if (
      prev !== undefined &&
      dims.cols === prev.cols &&
      dims.rows === prev.rows &&
      cellWidth === prev.cellWidth &&
      cellHeight === prev.cellHeight
    ) {
      return;
    }
    this.last = { cols: dims.cols, rows: dims.rows, cellWidth, cellHeight };
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
