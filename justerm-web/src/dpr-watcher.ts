/**
 * The device-pixel-ratio change signal (#325, consumer half of #322).
 *
 * A display's density can change **with no call from anyone** — dragging a window to another-density
 * monitor, or an OS display-scale change — and at an unchanged CSS size. Nothing else in the widget
 * observes that: `ResizeObserver` watches the content box, which does not move, so the renderer keeps
 * rasterising at the density it was built at and the terminal is blurry until it is recreated.
 *
 * **What makes this its own object rather than four lines beside the `prefers-reduced-motion`
 * listener** (`justerm-renderer.ts`) is the re-arm below: a resolution query is bound to the ratio it
 * was *created* with, so the obvious implementation works exactly once and then goes quiet forever.
 * That is a rule, it fails silently, and `JustermRenderer` cannot be built in a unit test (its
 * constructor reads `window.matchMedia`), so it is extracted for the same reason `ContextLossRelay`
 * and `FrameLoop` are (#696) — and it leaves the same hole: the *composition* is proven in a browser,
 * not here.
 *
 * **Named prior art, and it is close enough to be a port.** xterm.js does exactly this in
 * `CoreBrowserService` — `matchMedia("screen and (resolution: ${devicePixelRatio}dppx)")`, and on
 * every change it removes the old listener, re-reads `devicePixelRatio`, builds a *new* query at that
 * ratio and attaches to it (`src/browser/services/CoreBrowserService.ts:118-137` @ `699f553`). The one
 * deliberate difference is the DOM API: it uses the deprecated `addListener`/`removeListener` pair,
 * this uses `addEventListener`/`removeEventListener`.
 */
export interface ResolutionQuery {
  addEventListener(type: "change", listener: () => void): void;
  removeEventListener(type: "change", listener: () => void): void;
}

export class DprWatcher {
  /** The query currently attached to, or `undefined` while stopped. */
  private query: ResolutionQuery | undefined;
  /** Latched by {@link stop}: the widget's end of life is not an unmount (#606). */
  private ended = false;

  /**
   * One listener identity for the object's whole life, re-attached across queries. An arrow property
   * rather than a bound method so `removeEventListener` is guaranteed to match what was added —
   * a re-bound method silently fails to detach and this class re-attaches on every change.
   */
  private readonly onChangeEvent = (): void => {
    // A resolution query says only *"you have left this ratio"*; it carries no new value. Read the
    // live one at delivery time — which is also why re-arming needs no argument.
    const dpr = this.currentDpr();
    this.arm(dpr);
    this.notify(dpr);
  };

  /**
   * @param matchResolution builds a query for a given ratio — `window.matchMedia` in production
   * @param currentDpr reads the live ratio — `() => window.devicePixelRatio`
   * @param notify what to do about it; called **after** the re-arm, so a handler that throws cannot
   *   leave the watcher detached and silent
   */
  constructor(
    private readonly matchResolution: (dpr: number) => ResolutionQuery,
    private readonly currentDpr: () => number,
    private readonly notify: (dpr: number) => void,
  ) {}

  /** Begin watching, at the ratio in force now. A no-op if already watching, or after {@link stop}. */
  start(): void {
    if (this.ended || this.query) return;
    this.arm(this.currentDpr());
  }

  /** Stop for good, detaching whichever query is current — which after a change is *not* the one
   * {@link start} attached to. Idempotent, as every teardown on this port must be. */
  stop(): void {
    this.ended = true;
    this.detach();
  }

  private arm(dpr: number): void {
    this.detach();
    if (this.ended) return;
    const q = this.matchResolution(dpr);
    q.addEventListener("change", this.onChangeEvent);
    this.query = q;
  }

  private detach(): void {
    this.query?.removeEventListener("change", this.onChangeEvent);
    this.query = undefined;
  }
}
