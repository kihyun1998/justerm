import { ContextLossRelay } from "./context-loss";
import { DprWatcher, type ResolutionQuery } from "./dpr-watcher";

/**
 * The surface-scoped half of the renderer backend — **every published call that takes no grid**,
 * plus the two registry operations that mint and retire one (Epic #287 S7, ADR-0021).
 *
 * That criterion is not a judgement this package makes; it is already compiled into the renderer's
 * published signatures since 0.15.0 (#773). A call naming a grid acts on one terminal; a call naming
 * none acts on the thing every terminal shares — the context, the drawing buffer, the display's
 * density, the loss of the context and the present that draws all of them. So the split between this
 * interface and {@link import("./justerm-renderer").RendererBackend}'s per-grid remainder is a fact
 * about the renderer rather than a shape chosen here, and a drift in it is a compile error at the
 * assignment in `justerm-renderer.ts` rather than a divergence nobody notices.
 */
export interface SurfaceBackend {
  /** Register a terminal grid and return its id — since renderer 0.15.0 the **only** way to get one:
   * a renderer arrives holding none, and every per-terminal method names the grid it acts on (#773).
   *
   * The four font selectors are optional and trailing, and this package passes all four. They key
   * the atlas, so naming them here means **one** bake: a grid born at the renderer's defaults and
   * then moved by the four setters would bake an atlas per call and free each one again. */
  addGrid(
    paletteColors: Uint32Array,
    defaultFg: number,
    defaultBg: number,
    fontFamily?: string,
    fontSize?: number,
    letterSpacing?: number,
    lineHeight?: number,
  ): number;
  /** Unregister a grid and release what it owned: its VAO, its instance buffer, and — if it was the
   * last grid standing on its font configuration — that configuration's glyph atlas, rasteriser and
   * cache. This is the only way to give GPU memory back without dropping the whole wasm instance,
   * which a consumer holding this object cannot do. */
  removeGrid(grid: number): void;
  /** Re-bake every live font configuration's atlas at a new display density.
   *
   * **It re-derives no measurement**, and that is deliberate: the drawing buffer and every viewport
   * rect are device-px numbers the *consumer* measured, and the renderer will not convert them
   * through its own copy of the density — a copy that lags, since this very notification is dropped
   * while the context is lost (#773). Re-asking is the consumer's, and here that is the surface. */
  setDevicePixelRatio(dpr: number): void;
  /** Size the shared drawing buffer, in **device px** — the same space `setViewport` takes, so one
   * canvas is addressed in one space. The browser may grant less (#339), which `cssWidth`/`cssHeight`
   * report. */
  resizeSurface(width: number, height: number): void;
  /** The drawing buffer's size in **CSS** pixels — what the canvas display box must be set to. */
  cssWidth(): number;
  cssHeight(): number;
  /** Whether a WebGL context loss has been **reported** (#269). Deliberately the event-driven view
   * rather than `gl.isContextLost()`: a browser destroys a context synchronously and only *queues*
   * `webglcontextlost`, so this answers `false` for a window in which every GL call is already dead.
   * Read it as *"was I told"*, never as *"is the GPU usable"* (ADR-0027 D4). */
  isContextLost(): boolean;
  /** Whether a lost context has missed its restore deadline (#327) — the poll counterpart of
   * `setOnContextLoss`. Cleared by a late `webglcontextrestored`, which also heals the renderer. */
  isRestoreOverdue(): boolean;
  /** Register the single function the renderer calls when a lost context has not come back within
   * the deadline. There is **no unset** — the parameter is a `Function` — which is why
   * {@link TerminalSurface} registers an indirection once rather than the consumer's handler. */
  setOnContextLoss(callback: () => void): void;
  /** The grace period, in ms, before the callback above fires. Consumer policy (ADR-0017): the
   * renderer times, the consumer decides how long a blank terminal is tolerable. */
  setContextRestoreTimeoutMs(ms: number): void;
  /** Present the whole canvas — **every registered grid**, which is why this takes no id and why the
   * loop that calls it belongs to the surface rather than to any one terminal. */
  render(): void;
}

/**
 * What the surface needs from the canvas: two listener methods and the display box.
 *
 * Structural, and an `HTMLCanvasElement` satisfies it as-is — the same shape the rest of this
 * package uses for DOM seams, so the production path passes the real element and nothing casts.
 */
export interface SurfaceCanvas {
  addEventListener(type: string, listener: () => void): void;
  removeEventListener(type: string, listener: () => void): void;
  style: { width: string; height: string };
}

/**
 * The collaborators a {@link TerminalSurface} is built from.
 *
 * **This is the instantiation seam `docs/map/territory/widget-lifecycle.md` asked the third slice to
 * price**, and the price turned out to be this interface. `JustermRenderer` cannot be constructed
 * under vitest (its constructor reads `window.matchMedia`), so #696 and #579 each extracted the one
 * piece carrying a rule — `FrameLoop`, `ContextLossRelay` — and left the *composition* provable only
 * in a browser. A composition root whose whole content is composition cannot take that trade a third
 * time: there would be nothing left over to extract.
 */
export interface SurfaceDeps<B extends SurfaceBackend = SurfaceBackend> {
  backend: B;
  canvas: SurfaceCanvas;
  /** `requestAnimationFrame`, injected for the same reason `FrameLoop`'s is (#696). */
  raf: (cb: () => void) => number;
  caf: (id: number) => void;
  /** Builds a resolution query for a ratio — `window.matchMedia` in production ({@link DprWatcher}). */
  matchResolution: (dpr: number) => ResolutionQuery;
  currentDpr: () => number;
}

/** How a grid is registered. */
export interface AddGridOptions {
  paletteColors?: Uint32Array;
  defaultFg?: number;
  defaultBg?: number;
  fontFamily?: string;
  fontSize?: number;
  letterSpacing?: number;
  lineHeight?: number;
  /**
   * Claim **sole tenancy**: this terminal sizes the shared drawing buffer to its own grid extent.
   *
   * The single-terminal arrangement asks for `cols * cell_width(grid)` device pixels, which is
   * #331's exactness — both numbers are integers the renderer hands back, so nothing rounds between
   * the grid the shader lays out and the buffer that has to hold it. That is available only while
   * one grid fills the canvas. A second tenant sizing the same buffer to *its* extent would clobber
   * the first, and silently: no error, the sibling simply drawn into a buffer of the wrong size.
   *
   * So the claim is exclusive and enforced here ({@link TerminalSurface.addGrid} throws either way
   * round). A host wanting two terminals sizes the surface itself with
   * {@link TerminalSurface.resizeSurface} and claims none.
   */
  ownsExtent?: boolean;
}

/** The registry's record of one attached terminal. */
interface Tenant {
  grid: number;
  ownsExtent: boolean;
  /** What to run when the surface's geometry basis moves under this grid. See {@link TerminalSurface.onReapply}. */
  reapply: (() => void) | undefined;
  /** How to END this terminal. See {@link TerminalSurface.onEnd} for why a grid id is not enough. */
  end: (() => void) | undefined;
}

const EMPTY_PALETTE = new Uint32Array(256);

/**
 * The published renderer class, as a type — what {@link TerminalSurface.open} hands back so that a
 * terminal attaching to that surface gets the per-grid half without a cast, and so a drift in the
 * published signatures is a compile error at the adapter's assignment rather than a runtime
 * `undefined`.
 */
type PublishedRenderer = InstanceType<typeof import("justerm-renderer").JustermRenderer>;

/**
 * One canvas, one WebGL2 context, N attached terminals (Epic #287 S7, ADR-0021).
 *
 * **What it owns**, and each of these because the renderer scopes it to the surface rather than to a
 * grid: the canvas and the context behind it, the grid registry, the single animation loop that
 * presents, the display density, and context-loss recovery. A `Terminal` attaches to a surface,
 * holds the grid it was handed, and owns its own DOM overlay.
 *
 * **Why this exists rather than one context per terminal.** WebGL binds a context to exactly one
 * canvas, and browsers cap live contexts at around sixteen; a tiling or tabbed host hits the cap and
 * loses contexts, and every re-attach re-bakes an atlas. One context drawing N viewports removes the
 * cap and makes showing a terminal a placement rather than a rebuild. The forced consequence is
 * accepted knowingly: one canvas means every terminal shares one stacking plane, so arbitrary DOM
 * cannot be interleaved between two of them, and each overlay must track its rect or it drifts away
 * from the GL viewport it is supposed to sit over.
 *
 * **The ownership rule, stated once and nowhere else** (the AC's "in exactly one place"):
 *
 * > **A terminal releases its own grid; the surface ends every terminal it still holds, and then its
 * > own ambient work.**
 *
 * Both halves are the reference's, which is the only reference that has ever had to answer this —
 * ghostty is the one prior art sharing font machinery between terminals. Its `Surface.deinit` derefs
 * **its own** key out of the app's shared set (`src/Surface.zig:833`), rather than the app releasing
 * it on the surface's behalf; and its `App.deinit` ends every surface it holds and only then its own
 * shared set (`src/App.zig:107`), asserting that set is empty by the time it does (`:115`) — i.e.
 * the root does not release grids for terminals, it relies on each having released its own. Both at
 * SHA `e6e26e1`. Read ghostty's "surface" as justerm's *grid*: its noun is one per terminal and this
 * one is one per app, which is the single easiest way to misread the whole territory.
 *
 * xterm.js is the other reference and it is a **negative** result worth stating, because it also
 * confirms the placement: its WebGL addon creates a canvas and a context **per `Terminal`**
 * (`addons/addon-webgl/src/WebglRenderer.ts:91`, `:97` @ `699f553`) and registers the context-loss
 * listeners and the device-pixel-dimension observer on *that* canvas (`:125`, `:137`, `:148`). It has
 * no surface concept to copy — it is the architecture this epic replaces — but its placement of
 * those three is the same one derived here: they belong to whoever owns the canvas, and here that is
 * one object for the whole app rather than one per terminal.
 */
export class TerminalSurface<B extends SurfaceBackend = SurfaceBackend> {
  private readonly backend: B;
  private readonly canvas: SurfaceCanvas;
  private readonly raf: (cb: () => void) => number;
  private readonly caf: (id: number) => void;
  /** The attached terminals, keyed by the grid id each was handed. */
  private readonly tenants = new Map<number, Tenant>();
  /**
   * The consumer's never-restored-context handler, behind an indirection held for the surface's
   * life. Surface-scoped because `setOnContextLoss` is: one context, one loss, one notification —
   * a per-terminal channel would deliver the same event N times.
   */
  private readonly contextLoss = new ContextLossRelay();
  private readonly dprWatcher: DprWatcher;
  private readonly onContextRestored: () => void;
  /** The pending present's rAF id, or `undefined` when none is scheduled. */
  private presentId: number | undefined;
  /** The host's density-change handler — see {@link onDensityChange}. */
  private densityHandler: ((dpr: number) => void) | undefined;
  /** Latched by {@link dispose}: a surface's end of life is not an unmount, matching `Terminal`. */
  private disposed = false;

  constructor(deps: SurfaceDeps<B>) {
    this.backend = deps.backend;
    this.canvas = deps.canvas;
    this.raf = deps.raf;
    this.caf = deps.caf;

    // Registered unconditionally and exactly once, whether or not a consumer opted in: the renderer's
    // `setOnContextLoss` takes a `Function` with no unset, so a later handler has nothing to register
    // *with* unless the indirection is already in place — and a swap behind it is what makes
    // `setOnContextLoss(undefined)` expressible at all (#579).
    this.backend.setOnContextLoss(this.contextLoss.notify);

    // A density change moves every grid's cell and arrives with no call from anyone — another-density
    // monitor, an OS scale change — at an unchanged CSS size, so no `ResizeObserver` sees it (#325).
    // It is the surface's because `setDevicePixelRatio` takes no grid, and that placement is the
    // concrete defect this class fixes: while the watcher sat on the terminal, disposing one terminal
    // stopped density tracking for every sibling sharing its canvas.
    this.dprWatcher = new DprWatcher(deps.matchResolution, deps.currentDpr, (dpr) =>
      this.setDevicePixelRatio(dpr),
    );
    this.dprWatcher.start();

    // A restore is the one buffer change with no consumer call behind it (#325): `restore()` re-reads
    // the LIVE density and re-bakes at it, because a density notification arriving during a loss is
    // dropped rather than queued. So a density that moved while the context was dead is adopted here,
    // with no setter behind it and no other notice that it happened.
    //
    // The order is load-bearing and was measured, not reasoned: the renderer rebuilds inside its next
    // `render()`, not when this event fires, so re-deriving first would use the PRE-restore cell. The
    // second present is because re-deriving re-sizes the drawing buffer and a resized buffer is a
    // cleared one — without it every terminal is blank until the next frame someone happens to drive.
    this.onContextRestored = (): void => {
      this.present();
      this.reapplyAll();
      this.present();
    };
    this.canvas.addEventListener("webglcontextrestored", this.onContextRestored);
  }

  /**
   * Open a surface on a canvas: construct the renderer backend (which is what binds the WebGL2
   * context to that canvas) and take the scheduling and density seams from `window`.
   *
   * The **production entry point** for a host composing more than one terminal —
   * `JustermRenderer.create` calls it too, which is what makes the single-terminal path a use of
   * this one rather than a parallel one.
   *
   * The wasm module is loaded with a dynamic `import()`: two top-level wasm-bindgen "bundler"
   * imports race their init and the second fails (`__wbindgen_externrefs` undefined), so deferring
   * to runtime lets the bundler instantiate each cleanly.
   */
  static async open(canvasSelector: string): Promise<TerminalSurface<PublishedRenderer>> {
    const renderer = await import("justerm-renderer");
    const canvas = document.querySelector<HTMLCanvasElement>(canvasSelector);
    if (!canvas) throw new Error(`justerm-web: canvas ${canvasSelector} not found`);
    return TerminalSurface.forBackend(new renderer.JustermRenderer(canvasSelector), canvas);
  }

  /**
   * Build a surface over a backend that already exists and the canvas it was constructed on, taking
   * the scheduling and density seams from `window`.
   *
   * The assembly point {@link open} uses, split out because it is also the seam a host with its own
   * backend needs. It is the only place in this class that names a browser global, which is what
   * keeps the class itself constructible under vitest (`node` environment, no `window`) — construct
   * it directly with {@link SurfaceDeps} to supply your own.
   */
  static forBackend<B extends SurfaceBackend>(
    backend: B,
    canvas: SurfaceCanvas,
  ): TerminalSurface<B> {
    return new TerminalSurface({
      backend,
      canvas,
      raf: (cb) => requestAnimationFrame(cb),
      caf: (id) => cancelAnimationFrame(id),
      // xterm.js's `CoreBrowserService` builds exactly this query and re-arms it on every change
      // (`src/browser/services/CoreBrowserService.ts:118-137` @ `699f553`) — see `DprWatcher`, which
      // is a port of it and carries the one deliberate difference (the listener API).
      matchResolution: (dpr) => window.matchMedia(`screen and (resolution: ${dpr}dppx)`),
      currentDpr: () => window.devicePixelRatio,
    });
  }

  /**
   * The backend this surface drives, for the per-grid half a terminal needs
   * ({@link import("./justerm-renderer").JustermRenderer.attach}).
   *
   * Typed as the parameter rather than as {@link SurfaceBackend} so nothing is cast on the way out:
   * a surface opened on the published renderer hands back the published renderer, and the adapter's
   * `const backend: RendererBackend = …` assignment is what turns a signature drift in that package
   * into a compile error here.
   */
  rendererBackend(): B {
    return this.backend;
  }

  /** How many terminals are attached. For assertions and for a host deciding whether to tear down. */
  get gridCount(): number {
    return this.tenants.size;
  }

  /**
   * Register a terminal and hand back its grid id — the handle every per-grid renderer call names.
   *
   * The id is the terminal's, and it is never reused: the renderer throws on an id it does not know,
   * which is what makes a stale handle fail loudly instead of addressing whichever grid landed in a
   * freed slot.
   *
   * @throws if the surface has been disposed, or if the sole-tenancy claim
   *   ({@link AddGridOptions.ownsExtent}) cannot be honoured — see that field for why it is exclusive.
   */
  addGrid(opts: AddGridOptions = {}): number {
    if (this.disposed) {
      throw new Error("justerm-web: this TerminalSurface was disposed — build a new one");
    }
    const ownsExtent = opts.ownsExtent ?? false;
    const soleTenant = [...this.tenants.values()].some((t) => t.ownsExtent);
    if (soleTenant || (ownsExtent && this.tenants.size > 0)) {
      throw new Error(
        "justerm-web: this surface already has a sole tenant sizing its drawing buffer — " +
          "size the surface yourself (resizeSurface) and attach terminals that claim no extent",
      );
    }
    const grid = this.backend.addGrid(
      opts.paletteColors ?? EMPTY_PALETTE,
      opts.defaultFg ?? 0,
      opts.defaultBg ?? 0,
      opts.fontFamily,
      opts.fontSize,
      opts.letterSpacing,
      opts.lineHeight,
    );
    this.tenants.set(grid, { grid, ownsExtent, reapply: undefined, end: undefined });
    return grid;
  }

  /**
   * Hand a grid back. **A no-op for a grid this surface no longer holds** — the guard is here rather
   * than at the caller because the registry is the only thing that knows what is still registered,
   * and `removeGrid` throws on an unknown id while `Terminal.dispose()` is required to be idempotent.
   */
  removeGrid(grid: number): void {
    if (!this.tenants.delete(grid)) return;
    this.backend.removeGrid(grid);
  }

  /**
   * Register what to run when the surface's geometry basis moves under `grid` — a context restore or
   * a density change, the two events that move every grid's cell with no consumer call behind them.
   *
   * The terminal supplies this rather than the surface computing it, because what has to be re-asked
   * is per-grid: the grid's own size at its own new cell, its viewport rect, and — for a sole
   * tenant — the drawing buffer derived from both. The surface knows *when*; only the terminal knows
   * *what*.
   */
  onReapply(grid: number, reapply: () => void): void {
    const tenant = this.tenants.get(grid);
    if (tenant) tenant.reapply = reapply;
  }

  /**
   * Register how to **end** the terminal holding `grid`, so {@link dispose} can end a tenant rather
   * than merely retire its grid.
   *
   * **A grid id is not enough, and the gap is not cosmetic.** The registry maps ids, not objects, so
   * without this a surface teardown releases every grid and leaves each terminal running — its blink
   * loop, its reduced-motion listener and its frame subscription intact, now holding an id the
   * renderer has retired. Every per-grid call then throws `UnknownGrid`, on a timer, from a widget the
   * host has no reason to think is still alive.
   *
   * This is the difference between the two sentences the reference's composition root keeps separate:
   * ghostty's `App.deinit` ends every **surface** it holds and only then its own shared set
   * (`src/App.zig:107` @ `e6e26e1`), and it can assert that set is empty by then (`:115`) precisely
   * because ending a terminal is what releases its claim. Releasing the claim without ending the
   * terminal is the half that cannot be asserted.
   *
   * A no-op for a grid the registry does not hold, matching {@link onReapply}.
   */
  onEnd(grid: number, end: () => void): void {
    const tenant = this.tenants.get(grid);
    if (tenant) tenant.end = end;
  }

  /**
   * Size the shared drawing buffer, in **device** pixels, and write the canvas display box from what
   * the browser actually granted.
   *
   * Device pixels because the buffer belongs to no grid: a surface drawing N grids in M font
   * configurations has no cell to be a multiple of (ADR-0021 D3). The renderer deliberately will not
   * convert a CSS measurement for you — the only density it holds is its own copy of yours, and that
   * copy lags by construction, since a density notification arriving during a context loss is dropped
   * outright.
   *
   * The display box is written from `cssWidth`/`cssHeight` rather than from what was asked for,
   * because WebGL may grant less (#339) — forget this and a device-px buffer displays at twice its
   * size on a Retina screen.
   */
  resizeSurface(deviceWidth: number, deviceHeight: number): void {
    this.backend.resizeSurface(deviceWidth, deviceHeight);
    this.canvas.style.width = `${this.backend.cssWidth()}px`;
    this.canvas.style.height = `${this.backend.cssHeight()}px`;
  }

  /** The granted drawing buffer in CSS px — what the canvas is displayed at, and the space a fit
   * divides in. */
  cssSize(): { width: number; height: number } {
    return { width: this.backend.cssWidth(), height: this.backend.cssHeight() };
  }

  /**
   * Ask for a present on the next animation frame, coalescing every request in between into **one**.
   *
   * This is why the loop is the surface's. `render()` takes no grid: one call presents the whole
   * canvas, every registered grid included. N terminals each presenting on their own frame would
   * redraw the surface N times per frame — a cost that grows with the number of terminals while the
   * pixels do not.
   *
   * The pending handle is cleared **before** presenting, which is `FrameLoop`'s rule (#696) and
   * xterm.js's `RenderDebouncer._innerRefresh` shape: a request arriving from inside the present then
   * schedules the next frame instead of being swallowed by a handle that no longer cancels anything.
   */
  requestRender(): void {
    if (this.disposed || this.presentId !== undefined) return;
    this.presentId = this.raf(() => {
      this.presentId = undefined;
      this.backend.render();
    });
  }

  /**
   * Present the canvas now, outside the loop — every registered grid, since `render()` takes no id.
   *
   * The two paths that need it rather than {@link requestRender} are the ones that must have drawn
   * before they return: a context restore (the renderer rebuilds *inside* `render`, so the cell is
   * not readable until one has run) and a density change. It is also what a sole tenant's
   * `render()` reaches, which is what keeps the single-terminal timing exactly as it was.
   */
  present(): void {
    this.backend.render();
  }

  /**
   * Adopt a new display density: re-bake every live font configuration's atlas at it, then have
   * every attached terminal re-ask for its geometry at the cell that just moved under it.
   *
   * Surface-scoped because `setDevicePixelRatio` is — one canvas is one drawing buffer at one
   * density — and because the re-derivation is owed by *every* grid, not by whichever terminal's
   * consumer happened to make the call.
   *
   * A no-op at an unchanged ratio and dropped while the context is lost, both inside the renderer,
   * so it is safe to call unconditionally.
   */
  setDevicePixelRatio(dpr: number): void {
    this.backend.setDevicePixelRatio(dpr);
    this.reapplyAll();
    this.present();
    // Told LAST, after every terminal has re-derived what it can, so a host re-measuring inside this
    // handler is measuring against the new cell rather than racing it.
    this.densityHandler?.(dpr);
  }

  /**
   * Be told when the display's density changes — **the one event after which the device-px numbers
   * the host gave this surface are all wrong** (ADR-0021 D3: *"a density change invalidates every
   * device-pixel quantity the consumer gave — the surface's size as well as every viewport rect —
   * since only the consumer can re-measure them"*).
   *
   * A host that placed terminals with `setViewportRect` owes a fresh
   * {@link resizeSurface} and a fresh rect per terminal here. **Nothing else can pay it.** The
   * renderer refuses to convert a stored device-px quantity through its own copy of the density,
   * because that copy lags by construction, and this package refuses for the same reason: a rect
   * scaled by a ratio we hold is a measurement nobody made.
   *
   * A **sole tenant needs none of this** — its rect is the origin, which no density moves, and it
   * re-derives its own buffer. So this exists for the shared arrangement, which is also the only one
   * that can get it wrong: a pane at CSS x=400 replayed unchanged across dpr 1 → 2 lands at half its
   * correct offset, at twice its width, over its left sibling. No error, and every pixel plausible.
   *
   * Fires **after** every attached terminal has re-derived what it can on its own, so the cell is
   * already the new one when the handler runs. Pass `undefined` to detach.
   */
  onDensityChange(handler: ((dpr: number) => void) | undefined): void {
    this.densityHandler = handler;
  }

  private reapplyAll(): void {
    for (const tenant of [...this.tenants.values()]) tenant.reapply?.();
  }

  /**
   * Install (or clear, with `undefined`) the handler called when a lost context has not come back
   * within {@link setContextRestoreTimeout}. Surface-scoped: one context means one loss, so a
   * per-terminal channel would deliver the same event once per terminal.
   *
   * Nothing is re-registered with the renderer — the relay installed at construction stays, and this
   * swaps the handler behind it.
   */
  setOnContextLoss(handler: (() => void) | undefined): void {
    this.contextLoss.set(handler);
  }

  /** Change the restore grace period, in ms. Applies to the **next** loss: a deadline already armed
   * keeps the duration it was armed with. */
  setContextRestoreTimeout(ms: number): void {
    this.backend.setContextRestoreTimeoutMs(ms);
  }

  /** Whether a context loss has been **reported** — for surfacing the state, not for deciding whether
   * drawing is safe (ADR-0027 D4). Keeps answering after {@link dispose}: only the push stops. */
  isContextLost(): boolean {
    return this.backend.isContextLost();
  }

  /** Whether a lost context has missed its restore deadline. Advisory, and it un-sets — a late
   * restore clears it and heals the renderer, so read it each time. */
  isRestoreOverdue(): boolean {
    return this.backend.isRestoreOverdue();
  }

  /**
   * End the surface: release every grid still attached, then stop everything it runs on its own
   * behalf — the pending present, the restore listener, the density watcher, and the context-loss
   * channel.
   *
   * **The order is the rule stated in the class doc, and it is the reference's**: end what you
   * composed, then yourself. Releasing the grids first is what leaves the shared per-config tier
   * empty by the time the surface goes, which is the state ghostty's root asserts rather than
   * assumes (`src/App.zig:115`).
   *
   * A terminal that ends first releases only its own grid; this catches the ones that did not. It
   * does **not** relieve a terminal of its own teardown — a `Terminal` still owns its DOM overlay,
   * its listeners and its blink loop, none of which the surface can see.
   *
   * Idempotent, and end of life rather than unmount: {@link addGrid} throws afterwards. **It stops
   * work, not memory** — the wasm instance, the GL context and the atlases the Rust side owns go
   * with the binding's `free()`, which cannot be called while the consumer still holds the object.
   */
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    // End each TERMINAL, which is what releases its grid — the order the reference's root keeps and
    // the reason it can assert an empty shared tier afterwards. A tenant that registered no `end`
    // (or whose terminal has already gone) still has its grid retired by the sweep below, so a
    // half-composed registration cannot leak one.
    //
    // Re-entrant by design and safe by the latch above: a terminal's own teardown calls back into
    // `removeGrid`, and a sole tenant's also calls `dispose()`, which returns immediately.
    for (const tenant of [...this.tenants.values()]) tenant.end?.();
    for (const grid of [...this.tenants.keys()]) this.removeGrid(grid);
    if (this.presentId !== undefined) {
      this.caf(this.presentId);
      this.presentId = undefined;
    }
    this.canvas.removeEventListener("webglcontextrestored", this.onContextRestored);
    this.dprWatcher.stop();
    this.contextLoss.end();
    this.densityHandler = undefined;
  }
}
