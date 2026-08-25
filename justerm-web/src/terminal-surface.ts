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
 * about the renderer rather than a shape chosen here, and a drift in it is a compile error rather than
 * a divergence nobody notices. **Where that error lands moved with #775**: it used to be a
 * `const backend: RendererBackend = new renderer.JustermRenderer(…)` assignment, and it is now the
 * `TerminalSurface<PublishedRenderer>` that {@link TerminalSurface.open} returns having to satisfy
 * `JustermRenderer.build`'s `TerminalSurface<RendererBackend>` parameter.
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
  /** How many distinct font configurations the renderer holds resources for — i.e. how many glyph
   * atlases exist (#772). Surface-scoped because an atlas is shared across grids: it takes no id,
   * and no one terminal can answer it. */
  atlasCount(): number;
  /** Atlas bakes run so far (#772) — every configuration built from nothing, plus every in-place
   * rebuild of one (a density change, a context restore). Read as a **delta** across an operation,
   * never as an absolute: it is what separates *a grid was placed* from *a grid was rebuilt*, which
   * is the claim ADR-0021's middle tier exists to make and the one no pixel can settle. */
  bakes(): number;
  /** Instance-buffer packs run so far (#421). Read as a delta, like {@link bakes}. Surface-scoped
   * because `render` presents every grid: one call packs each *dirty drawn* grid once, and — the
   * property this makes checkable — packs a grid with no viewport not at all (#771). */
  packs(): number;
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
}

/**
 * A terminal's claim on a surface — what {@link TerminalSurface.addGrid} hands back (#805).
 *
 * **The id is still here and still a number**, because a grid handle crosses the wasm boundary as
 * one and every per-grid renderer call names it (recorded in `docs/agents/reference-facts.md`,
 * #770). What changed is that the id is no longer the *only* thing a caller holds, and therefore no
 * longer the thing a caller has to keep valid.
 *
 * **Why that matters rather than being a style preference.** The same record states the condition
 * under which number ids are safe at all — *"a stale handle in JS has to fail loudly rather than
 * address whichever grid landed there"* — and the renderer implements it (`registry.rs` raises
 * `UnknownGrid`, with its two causes deliberately one error). This package used to swallow that:
 * three registry methods took an id and silently ignored one they no longer held. A lease removes
 * the question instead of answering it quietly — there is no id to go stale, because there is no
 * id-keyed call.
 *
 * Every other registration in this package already works this way: `FrameSource.subscribe` returns
 * an `Unsubscribe`, `observeResize` / `captureInput` / {@link observeViewportRect} return disposers,
 * and `DecorationRegistry` hands out a `Decoration` with its own `dispose()`.
 */
export interface GridLease {
  /** The renderer's handle for this grid. Pass it to per-grid renderer calls; do not store it as a
   * substitute for this lease, which is the thing that knows whether it is still valid. */
  readonly id: number;
  /** Whether {@link release} has run. */
  readonly released: boolean;
  /** Run this when the surface's geometry basis moves under this grid — a context restore or a
   * density change, the two events that move every grid's cell with no consumer call behind them.
   * The surface knows *when*; only the terminal knows *what*. A no-op once released. */
  onReapply(reapply: () => void): void;
  /** Run this to END the terminal holding this grid, so {@link TerminalSurface.dispose} can end a
   * tenant rather than merely retire its grid. A no-op once released. */
  onEnd(end: () => void): void;
  /**
   * Hand the grid back, releasing its VAO, its instance buffer and — if it was the last grid on its
   * font configuration — that configuration's atlas.
   *
   * **Idempotent, and that is not the softening this replaced.** The `Renderer` port requires
   * `Terminal.dispose()` to be silent on a second call, so something must absorb it. What differs is
   * what does: the surface used to swallow an id it could not recognise, and a lease declines a
   * second call *about itself*. One is not knowing; the other is knowing.
   */
  release(): void;
}

/** The registry's record of one attached terminal, and the lease handed out for it. */
class Lease implements GridLease {
  reapply: (() => void) | undefined;
  end: (() => void) | undefined;
  private done = false;

  constructor(
    readonly id: number,
    private readonly onRelease: (lease: Lease) => void,
  ) {}

  get released(): boolean {
    return this.done;
  }

  /**
   * **No gate here, deliberately — leaving the registry is what stops delivery.**
   *
   * Three mechanisms could enforce *"a released lease delivers nothing"*: a guard on these two
   * registrations, {@link release} clearing what it holds, and the lease leaving the surface's set.
   * Only the third can be observed: once it is gone from the set, neither `reapplyAll` nor `dispose`
   * can reach it, so the other two are unfalsifiable by construction — each masked by the one below.
   *
   * That is not a tidiness point. A branch no test can redden is a branch nothing is checking, and
   * the first version of this class had all three, with a mutation passing on two of them. The same
   * call is on record next door for the same reason (`ContextLossRelay`, #579), and it is the reason
   * a registration after release is simply harmless here rather than defended against: the lease is
   * out of the set, and the object itself is garbage as soon as its terminal drops it.
   */
  onReapply(reapply: () => void): void {
    this.reapply = reapply;
  }

  onEnd(end: () => void): void {
    this.end = end;
  }

  release(): void {
    if (this.done) return;
    this.done = true;
    this.onRelease(this);
  }
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
 * **The ownership rule.** It is written down once, as a cross-cutting invariant —
 * `docs/map/invariant/a-layer-ends-what-it-exclusively-holds.md` — because it holds in three
 * territories and is invisible from each of them. Restated here only in the form this class
 * implements, with the note as the authority:
 *
 * > **A layer ends what it exclusively holds, and never what it shares.** So a terminal releases its
 * > own grid; the surface ends every terminal it still holds, and then its own ambient work.
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
  private readonly leases = new Set<Lease>();
  /**
   * The consumer's never-restored-context handler, behind an indirection held for the surface's
   * life. Surface-scoped because `setOnContextLoss` is: one context, one loss, one notification —
   * a per-terminal channel would deliver the same event N times.
   */
  private readonly contextLoss = new ContextLossRelay();
  private readonly dprWatcher: DprWatcher;
  /** Reads the live display density, for {@link announcedDpr} to be compared against. */
  private readonly currentDpr: () => number;
  /**
   * The density this surface last **told the host about** — not the one the renderer holds, and not
   * the live one (#808).
   *
   * It is the surface's own fact, because the surface is where an announcement is first true: the
   * renderer's copy answers *"what am I baking at"* and `currentDpr()` answers *"what is the display
   * at"*, and neither of those is *"what does the host believe"*. Every device-px quantity the host
   * gave — the drawing buffer's size and every viewport rect — was measured at this ratio, so the
   * moment it stops matching the live one those numbers are stale and only an announcement can say so
   * (ADR-0021 D3).
   *
   * Seeded from the live ratio rather than from `1`: a surface opened on a Retina display is already
   * in agreement with a host that sized its buffer there, and nothing is owed.
   *
   * Named prior art: xterm.js's WebGL renderer keeps `_devicePixelRatio` for exactly this comparison
   * — `if (this._devicePixelRatio !== this._coreBrowserService.dpr)`
   * (`addons/addon-webgl/src/WebglRenderer.ts:186` @ `699f553`). What it does *not* do is consult it
   * on a restore, and it does not have to: its restore rebuilds at the ratio it stored, while
   * `justerm-renderer`'s re-reads the live one (#325). The mechanism is the reference's; the site is
   * ours because the adoption is.
   */
  private announcedDpr: number;
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
    this.currentDpr = deps.currentDpr;
    this.announcedDpr = deps.currentDpr();

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
      // …and only now is it knowable WHICH density was adopted, because the rebuild happened inside
      // the first `present()`. A sole tenant needs nothing further — `reapplyAll` re-derives its own
      // buffer from its own grid (#325). A SHARED tenant cannot: the buffer and every viewport rect
      // are the host's, in device px at a ratio that has just stopped being true, and no `change`
      // event was ever dispatched for the host to have noticed on its own (#808).
      const live = this.currentDpr();
      if (live !== this.announcedDpr) this.announceDensity(live);
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
   * a surface opened on the published renderer hands back the published renderer, which is what lets
   * `JustermRenderer.build` take a `TerminalSurface<RendererBackend>` and turn a signature drift in
   * that package into a compile error here.
   */
  rendererBackend(): B {
    return this.backend;
  }

  /** How many terminals are attached. For assertions and for a host deciding whether to tear down. */
  get gridCount(): number {
    return this.leases.size;
  }

  /**
   * Register a terminal and hand back its grid id — the handle every per-grid renderer call names.
   *
   * The id is the terminal's, and it is never reused: the renderer throws on an id it does not know,
   * which is what makes a stale handle fail loudly instead of addressing whichever grid landed in a
   * freed slot.
   *
   * @throws if the surface has been disposed. Nothing else refuses a grid: a surface accepts as
   *   many terminals as a host attaches. (It used to refuse a second one when a first had claimed to
   *   size the drawing buffer — deleted in #802, because the only surface a terminal auto-sizes is
   *   the one `JustermRenderer.create` composed, and that one is unreachable, so the guard defended
   *   a state nobody could construct. `test/published-seam.types.ts` §3 now pins the reachability.)
   */
  addGrid(opts: AddGridOptions = {}): GridLease {
    if (this.disposed) {
      throw new Error("justerm-web: this TerminalSurface was disposed — build a new one");
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
    const lease = new Lease(grid, (l) => {
      this.leases.delete(l);
      this.backend.removeGrid(l.id);
    });
    this.leases.add(lease);
    return lease;
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
   * How many glyph atlases this surface holds — one per distinct font configuration (#772).
   *
   * **Surface-scoped on the same rule the rest of this interface is split by**: it names no grid,
   * because an atlas belongs to none. Two terminals in one font answer `1`; a third in another font
   * answers `2`; the last terminal to leave a configuration releases it.
   *
   * This is what makes sharing **observable rather than asserted** (#801). The published README has
   * claimed since #772 that terminals on one font configuration share one atlas, and until this
   * method existed on the widget's side no consumer could check it — the number was reachable only
   * by casting past the adapter to the raw wasm object, which is what #776 had to do.
   */
  atlasCount(): number {
    return this.backend.atlasCount();
  }

  /**
   * Atlas bakes run so far — read as a **delta across an operation**, never as an absolute.
   *
   * **It is the only thing that separates a placement from a rebuild** (#801). Hiding a terminal and
   * showing it again should cost nothing but a rect: every byte of its grid, its instances and its
   * configuration's atlas stays resident (`clearViewport`, #770). A pixel check cannot say whether
   * that held — the content comes back looking identical either way, which is exactly what a rebuild
   * also produces. A bake count that does not move can.
   *
   * Not a stable API surface; a counter for verification, and it wraps harmlessly.
   */
  bakes(): number {
    return this.backend.bakes();
  }

  /**
   * Instance-buffer packs run so far — a delta, like {@link bakes}.
   *
   * **What it makes checkable is the cost half of hiding** (#801). The renderer's draw loop skips an
   * unplaced grid *before* the re-pack, and until this reached the widget the saving was a number
   * quoted from the renderer's own measurement rather than something a consumer could observe:
   * feeding a hidden terminal moves this by zero, feeding a shown one does not, and the second half
   * is what stops the first from being a vacuous zero.
   */
  packs(): number {
    return this.backend.packs();
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
    //
    // The ratio recorded is the ARGUMENT, not the live one: this is a public setter, so a host may
    // adopt a density the display is not at, and what {@link announcedDpr} holds is what the host was
    // told. It is recorded even when the renderer drops the call for a lost context — the host is
    // still told, still re-supplies at this ratio, and the restore that later adopts the same live
    // ratio then owes it nothing (#808).
    this.announceDensity(dpr);
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
    for (const lease of [...this.leases]) lease.reapply?.();
  }

  /**
   * Hand the ratio to the host, and record it as announced **only if there was a host to hand it
   * to**. The two are one step on purpose: a notification the field does not record is one the next
   * restore repeats, and a record with no notification behind it is a debt nobody collects.
   *
   * The guard is the field's own definition, not a nicety. A move that lands in the window between
   * `open()` and the host's {@link onDensityChange} registration would otherwise read afterwards as
   * agreement — and there is no second `webglcontextrestored` for one loss, so the restore comparison
   * would stay silent for the rest of the page's life. That window is the production shape: a host
   * composes the surface, awaits a terminal per pane, and registers the handler after.
   */
  private announceDensity(dpr: number): void {
    const handler = this.densityHandler;
    if (!handler) return;
    this.announcedDpr = dpr;
    handler(dpr);
    // **And present again, because the handler's own obligations left the canvas blank.** What this
    // asks a host for is a fresh {@link resizeSurface} — which re-creates the drawing buffer, and a
    // resized buffer is a cleared one. Both callers present BEFORE this point, for a reason that has
    // not gone away (the cell is not readable until a render has run), so without this the last thing
    // to touch the buffer is the clear.
    //
    // Coalesced rather than immediate: the host may re-place N terminals inside one handler, and each
    // `setViewportRect` / `resize` already asks its own grid to re-derive. One frame after the whole
    // handler is the cheapest point at which every terminal is placed.
    //
    // Only the SHARED arrangement can reach the blank — a sole tenant's buffer is `reapplyAll`'s own,
    // sized before the present above — but the call is unconditional because the surface cannot tell
    // the two apart, and a redundant coalesced present costs one frame that was going to be presented
    // anyway. (#808; the shape pre-dates it on `setDevicePixelRatio` and is fixed on both paths at
    // once, since two rules for one mechanism is what this area has repeatedly paid for.)
    this.requestRender();
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
    for (const lease of [...this.leases]) lease.end?.();
    // Whatever is left had no `end`, or its terminal did not release on being ended. Either way the
    // grid is GPU memory nothing holds any more, and `release` is what gives it back.
    for (const lease of [...this.leases]) lease.release();
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

/** The two boxes a viewport rect is derived from, both in CSS px and both from the same clock. */
export interface OverlayBoxes {
  /**
   * The terminal's DOM overlay, as `getBoundingClientRect()` reports it.
   *
   * **The size is here because the origin alone cannot answer the question** (#801). A hidden
   * overlay — `display: none`, or a pane not yet in the layout — reports every field as `0`, which
   * is indistinguishable from an overlay legitimately sitting at the canvas's top-left corner once
   * the extent is dropped. It was dropped, and the two states then shared one answer: measured in a
   * real browser, hiding the second pane of `demo/shared-surface.html` moved its viewport from
   * `[500, 40]` to `[0, 0]` and drew that whole grid over its sibling, which went on reporting
   * itself healthy.
   */
  overlay: { left: number; top: number; width: number; height: number };
  /** The surface canvas, from the same call, so the difference is a position *within* the canvas. */
  canvas: { left: number; top: number };
}

/**
 * Compute a terminal's viewport origin on the shared drawing buffer, in **device px**, from where its
 * DOM overlay sits relative to the canvas (#775) — or `undefined` when the overlay has no box at all.
 *
 * Pure and separately testable, which is the point: the arithmetic is where a sign or a unit goes
 * wrong, and the DOM plumbing around it is not worth a test.
 *
 * **Rounded, and to an integer device pixel.** A viewport is a GL rect and cannot be fractional; a
 * fractional CSS offset at a fractional density otherwise lands the grid a subpixel off its overlay,
 * which reads as blur rather than as displacement (the failure #337 and #352 are about).
 *
 * **Clamped at zero.** An overlay scrolled above the canvas has a negative offset, and a negative
 * viewport origin is not a smaller rect — it is a GL error or a silently dropped draw.
 *
 * **`undefined` is a state, not a failure, and it is why this returns a union** (#801). An overlay
 * with no area is not somewhere — it is nowhere, and the clamp above is exactly what used to turn
 * that into the plausible wrong answer `{ x: 0, y: 0 }`. Answering `undefined` makes the caller
 * decide, at the compiler's insistence, rather than placing a full-size grid at the canvas's corner:
 * the extent a viewport is given is derived from `cols * cell` and never from this box, so a zeroed
 * box shrinks nothing and the renderer's own no-area guard is never reached. Making the state
 * unrepresentable rather than guarded is the same move `GridLease` made one issue earlier (#805).
 */
export function viewportOrigin(
  boxes: OverlayBoxes,
  dpr: number,
): { x: number; y: number } | undefined {
  if (boxes.overlay.width <= 0 || boxes.overlay.height <= 0) return undefined;
  return {
    x: Math.max(0, Math.round((boxes.overlay.left - boxes.canvas.left) * dpr)),
    y: Math.max(0, Math.round((boxes.overlay.top - boxes.canvas.top) * dpr)),
  };
}

/**
 * Keep a terminal's GL viewport following its DOM overlay, and hand back the disposer.
 *
 * **Why this exists rather than leaving the host to call `setViewportRect`.** One WebGL context binds
 * to one canvas, so a terminal is a transparent overlay positioned over its slice of that canvas —
 * and the two layers are moved by different things. A missed update is silent: the GL viewport stays
 * where it was while the overlay (the hidden IME textarea, the a11y tree, the scrollbar) moves off
 * it, so the terminal *looks* fine and the caret is in the wrong place.
 *
 * **A `ResizeObserver` alone is not enough, and that is the whole reason this is not two lines.** It
 * fires when the element's *box* changes, never when the element *moves* — a scroll, a sibling
 * appearing above it, a pane drag all relocate the overlay at an unchanged size. So this observes the
 * box **and** listens for scroll on the capture phase, which is the only way to hear a scroll on an
 * ancestor that is not the window.
 *
 * **It does not fire on a density change, and a caller must not expect it to.** Its triggers are the
 * two observed boxes and a scroll; a `ResizeObserver` on the default box reports CSS pixels, and a
 * density change moves no CSS box. So the origin it last sent stays scaled by the old ratio until
 * something else moves the overlay. Re-supplying it is the host's, from
 * {@link TerminalSurface.onDensityChange} — {@link viewportOrigin} is exported for exactly that, so
 * the host recomputes with this function's own arithmetic rather than a second copy of it. (The
 * package README claimed the opposite until #776, which is the first code to depend on the answer.)
 *
 * **It DOES fire when the overlay is hidden, and that is why `place` takes a union** (#801).
 * `display: none` removes the box, which the `ResizeObserver` reports as a size change like any
 * other — measured in a real browser, not inferred. `visibility: hidden` does not fire it, because
 * the box survives; that case is the host's to handle with an explicit `JustermRenderer.hide()`,
 * and it is worth handling, since the pixels live on the shared canvas rather than in the overlay,
 * so hiding the overlay alone leaves the terminal fully drawn and fully paid for.
 *
 * Same shape as `observeResize` in `fit.ts`, deliberately: an observer, a disposer, and no state.
 *
 * @param overlay the terminal's DOM overlay element
 * @param canvas the surface's canvas element
 * @param place what to do with the computed origin — normally `JustermRenderer.setViewportRect`,
 *   with `undefined` meaning the overlay has no box and the terminal should stop being drawn
 *   (`JustermRenderer.hide`). One callback rather than two, because the two are one fact about one
 *   overlay and a second channel is a second writer to it
 * @param currentDpr reads the live density, at each sync. The origin is device px, so its value
 *   depends on the ratio — which is *not* the same as tracking it; see the paragraph above.
 */
export function observeViewportRect(
  overlay: Element,
  canvas: Element,
  place: (origin: { x: number; y: number } | undefined) => void,
  currentDpr: () => number = () => window.devicePixelRatio,
): () => void {
  const sync = (): void => {
    place(
      viewportOrigin(
        { overlay: overlay.getBoundingClientRect(), canvas: canvas.getBoundingClientRect() },
        currentDpr(),
      ),
    );
  };
  const ro = new ResizeObserver(sync);
  ro.observe(overlay);
  ro.observe(canvas);
  // Capture, so a scroll on ANY ancestor is heard: scroll does not bubble from an element, and the
  // container that moves an overlay is usually not the window.
  window.addEventListener("scroll", sync, true);
  sync();
  return () => {
    ro.disconnect();
    window.removeEventListener("scroll", sync, true);
  };
}
