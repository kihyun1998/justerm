import { describe, expect, it, vi } from "vitest";
import { TerminalSurface, viewportOrigin } from "../src/terminal-surface";
import type { ResolutionQuery } from "../src/dpr-watcher";
import type { SurfaceBackend, SurfaceDeps } from "../src/terminal-surface";

/**
 * A fake for the surface-scoped half of the renderer backend — every call that takes no grid, plus
 * the two registry operations that mint and retire one.
 *
 * Deliberately a record of calls rather than a simulation: what this file asserts is *who calls
 * what, and how many times*, which is the whole content of a composition root. The pixels those
 * calls produce are the browser proofs' job (#776).
 */
class FakeBackend implements SurfaceBackend {
  readonly calls: string[] = [];
  /** Grids handed out, in order. `addGrid` returns a fresh id each time, like the real registry. */
  private nextGrid = 1;
  readonly removed: number[] = [];
  lossHandler: (() => void) | undefined;
  /** What `cssWidth`/`cssHeight` report — the granted drawing buffer, in CSS px. */
  css = { width: 800, height: 600 };
  contextLost = false;
  restoreOverdue = false;

  addGrid(): number {
    const id = this.nextGrid++;
    this.calls.push(`addGrid->${id}`);
    return id;
  }
  removeGrid(grid: number): void {
    this.calls.push(`removeGrid(${grid})`);
    this.removed.push(grid);
  }
  setDevicePixelRatio(dpr: number): void {
    this.calls.push(`setDevicePixelRatio(${dpr})`);
  }
  resizeSurface(width: number, height: number): void {
    this.calls.push(`resizeSurface(${width},${height})`);
  }
  cssWidth(): number {
    return this.css.width;
  }
  cssHeight(): number {
    return this.css.height;
  }
  isContextLost(): boolean {
    return this.contextLost;
  }
  isRestoreOverdue(): boolean {
    return this.restoreOverdue;
  }
  setOnContextLoss(callback: () => void): void {
    this.calls.push("setOnContextLoss");
    this.lossHandler = callback;
  }
  setContextRestoreTimeoutMs(ms: number): void {
    this.calls.push(`setContextRestoreTimeoutMs(${ms})`);
  }
  render(): void {
    this.calls.push("render");
  }
}

/** A canvas stand-in: the surface only ever adds/removes listeners and writes the display box. */
class FakeCanvas {
  readonly listeners = new Map<string, Set<() => void>>();
  readonly style = { width: "", height: "" };
  addEventListener(type: string, listener: () => void): void {
    let set = this.listeners.get(type);
    if (!set) {
      set = new Set();
      this.listeners.set(type, set);
    }
    set.add(listener);
  }
  removeEventListener(type: string, listener: () => void): void {
    this.listeners.get(type)?.delete(listener);
  }
  /** Fire a real event the browser would deliver — `webglcontextrestored`. */
  emit(type: string): void {
    for (const l of [...(this.listeners.get(type) ?? [])]) l();
  }
  countOf(type: string): number {
    return this.listeners.get(type)?.size ?? 0;
  }
}

/** A hand-driven `requestAnimationFrame`: nothing runs until {@link Raf.flush}. */
class Raf {
  private pending = new Map<number, () => void>();
  private next = 1;
  cancelled: number[] = [];
  readonly raf = (cb: () => void): number => {
    const id = this.next++;
    this.pending.set(id, cb);
    return id;
  };
  readonly caf = (id: number): void => {
    this.cancelled.push(id);
    this.pending.delete(id);
  };
  /** Run every callback scheduled *so far*; ones scheduled during the flush wait for the next. */
  flush(): void {
    const due = [...this.pending];
    this.pending.clear();
    for (const [, cb] of due) cb();
  }
  get scheduled(): number {
    return this.pending.size;
  }
}

class FakeQuery implements ResolutionQuery {
  listeners = new Set<() => void>();
  addEventListener(_type: "change", listener: () => void): void {
    this.listeners.add(listener);
  }
  removeEventListener(_type: "change", listener: () => void): void {
    this.listeners.delete(listener);
  }
}

interface Harness {
  surface: TerminalSurface;
  backend: FakeBackend;
  canvas: FakeCanvas;
  raf: Raf;
  queries: FakeQuery[];
  dpr: { value: number };
}

function harness(): Harness {
  const backend = new FakeBackend();
  const canvas = new FakeCanvas();
  const raf = new Raf();
  const queries: FakeQuery[] = [];
  const dpr = { value: 1 };
  const deps: SurfaceDeps = {
    backend,
    canvas,
    raf: raf.raf,
    caf: raf.caf,
    matchResolution: () => {
      const q = new FakeQuery();
      queries.push(q);
      return q;
    },
    currentDpr: () => dpr.value,
  };
  return { surface: new TerminalSurface(deps), backend, canvas, raf, queries, dpr };
}

describe("TerminalSurface — the grid registry", () => {
  it("hands each attached terminal its own grid", () => {
    // The registry is the surface's, and a grid id is the terminal's handle into it. Two terminals
    // sharing one surface must never share the id: every per-grid call in the renderer names it, so
    // a duplicate would make one terminal's frames land on the other's cells.
    const { surface, backend } = harness();

    const a = surface.addGrid();
    const b = surface.addGrid();

    expect(a).not.toBe(b);
    // Filtered rather than compared whole: the constructor registers the loss relay before any of
    // this, which its own test asserts. Pinning the full call list here would make every unrelated
    // constructor change fail in the registry's tests.
    expect(backend.calls.filter((c) => c.startsWith("addGrid"))).toEqual(["addGrid->1", "addGrid->2"]);
    expect(surface.gridCount).toBe(2);
  });

  it("releases only the grid handed back, leaving its sibling registered", () => {
    // AC: "ending one terminal releases only its grid". The reference does exactly this — ghostty's
    // `Surface.deinit` derefs its OWN key out of the app's shared set (`src/Surface.zig:833` @
    // `e6e26e1`) rather than the app releasing it on the surface's behalf.
    const { surface, backend } = harness();
    const a = surface.addGrid();
    const b = surface.addGrid();

    surface.removeGrid(a);

    expect(backend.removed).toEqual([a]);
    expect(surface.gridCount).toBe(1);
    // The side condition, and the one that would catch an over-eager teardown: the sibling is still
    // there, so a later surface dispose still has something to release.
    expect(backend.removed).not.toContain(b);
  });

  it("ignores a second release of the same grid", () => {
    // `removeGrid` throws on an id the renderer does not know, and `Terminal.dispose()` is required
    // to be idempotent — so the guard has to be here, where the registry knows what it still holds.
    const { surface, backend } = harness();
    const a = surface.addGrid();

    surface.removeGrid(a);
    surface.removeGrid(a);

    expect(backend.removed).toEqual([a]);
  });
});

describe("TerminalSurface — one animation loop", () => {
  it("coalesces many render requests into a single present", () => {
    // The reason the surface owns the loop rather than each terminal. `render()` takes no grid: one
    // call presents the WHOLE canvas, every registered grid included. N terminals each presenting on
    // their own frame would therefore redraw the surface N times per frame — the cost grows with the
    // number of terminals while the pixels do not.
    const { surface, backend, raf } = harness();

    surface.requestRender();
    surface.requestRender();
    surface.requestRender();
    expect(backend.calls).not.toContain("render");

    raf.flush();

    expect(backend.calls.filter((c) => c === "render")).toHaveLength(1);
  });

  it("schedules the next frame for a request made during a present", () => {
    // The coalescer clears its handle BEFORE presenting (`FrameLoop`'s rule, #696), so a request
    // arriving from inside the present is not swallowed by a stale handle. Without this a frame
    // driven by a listener that itself requests a render would be the last one ever drawn.
    const { surface, backend, raf } = harness();
    let reentered = false;
    const original = backend.render.bind(backend);
    backend.render = (): void => {
      original();
      if (!reentered) {
        reentered = true;
        surface.requestRender();
      }
    };

    surface.requestRender();
    raf.flush();
    expect(backend.calls.filter((c) => c === "render")).toHaveLength(1);
    expect(raf.scheduled).toBe(1);

    raf.flush();
    expect(backend.calls.filter((c) => c === "render")).toHaveLength(2);
  });
});

describe("TerminalSurface — teardown", () => {
  it("ends every grid it still holds, then its own ambient work", () => {
    // AC: "ending the surface ends everything it composed". Same order as the reference's
    // composition root: ghostty's `App.deinit` ends every surface it holds and only then its own
    // shared font set, asserting that set is empty by the time it does (`src/App.zig:107`, `:115`).
    const { surface, backend, canvas, raf, queries } = harness();
    const a = surface.addGrid();
    const b = surface.addGrid();
    surface.requestRender();
    expect(canvas.countOf("webglcontextrestored")).toBe(1);
    expect(queries).toHaveLength(1);

    surface.dispose();

    expect(backend.removed.sort()).toEqual([a, b].sort());
    // Ambient work: the pending present is cancelled, the restore listener detached, the density
    // watcher stopped. Each of these DRAWS, so leaving one attached lets an ended surface repaint.
    expect(raf.scheduled).toBe(0);
    expect(canvas.countOf("webglcontextrestored")).toBe(0);
    expect(queries[0]?.listeners.size).toBe(0);
  });

  it("ends each terminal rather than retiring its grid underneath it", () => {
    // The distinction the reference's composition root keeps and this one nearly lost: releasing a
    // grid is not ending a terminal. A surface holding ids alone can only do the first, which leaves
    // each widget running — blink loop, listeners, frame subscription — around an id the renderer has
    // retired, so every per-grid call throws `UnknownGrid` on a timer.
    const { surface, backend } = harness();
    const ended: number[] = [];
    const a = surface.addGrid();
    const b = surface.addGrid();
    surface.onEnd(a, () => ended.push(a));
    surface.onEnd(b, () => ended.push(b));

    surface.dispose();

    expect(ended.sort()).toEqual([a, b].sort());
    expect(backend.removed.sort()).toEqual([a, b].sort());
  });

  it("retires the grid of a tenant that registered no end callback", () => {
    // The half-composed case: `addGrid` succeeds and the terminal throws before registering. Without
    // the sweep after the end pass, that grid — a VAO, an instance buffer and an atlas refcount —
    // would outlive the surface with nothing holding it.
    const { surface, backend } = harness();
    const a = surface.addGrid();

    surface.dispose();

    expect(backend.removed).toEqual([a]);
  });

  it("survives a tenant whose end callback disposes back into the surface", () => {
    // The real shape: a terminal's own teardown calls `removeGrid`, and a sole tenant's also calls
    // `dispose()`. Both re-enter while the surface is mid-teardown, and the latch is what makes that
    // safe rather than infinite — asserted here because the recursion is invisible at either call
    // site and only shows up when both exist.
    const { surface, backend } = harness();
    let ends = 0;
    const a = surface.addGrid({ ownsExtent: true });
    surface.onEnd(a, () => {
      ends++;
      surface.removeGrid(a);
      surface.dispose();
    });

    surface.dispose();

    expect(ends).toBe(1);
    expect(backend.removed).toEqual([a]);
  });

  it("closes the context-loss channel so a deadline armed before disposal delivers nothing", () => {
    // The renderer clears its own callback slot in `Drop`, i.e. at `free()`, which nothing here
    // calls — so the gate lives on this side. Same contract `ContextLossRelay` keeps for one
    // terminal (#579), moved to the surface because `setOnContextLoss` takes no grid.
    const { surface, backend } = harness();
    const handler = vi.fn();
    surface.setOnContextLoss(handler);

    surface.dispose();
    backend.lossHandler?.();

    expect(handler).not.toHaveBeenCalled();
  });

  it("is idempotent", () => {
    const { surface, backend } = harness();
    surface.addGrid();

    surface.dispose();
    surface.dispose();

    expect(backend.removed).toHaveLength(1);
  });

  it("refuses to hand out a grid after disposal", () => {
    // End of life, not unmount — the same one-shot contract `Terminal.dispose()` declares (#606).
    // Silently registering a grid on an ended surface would leak it: nothing is left to release it.
    const { surface } = harness();
    surface.dispose();

    expect(() => surface.addGrid()).toThrow(/disposed/);
  });
});

describe("TerminalSurface — context loss", () => {
  it("registers one relay with the backend and swaps the consumer's handler behind it", () => {
    // `setOnContextLoss` offers no unset, so the relay is registered once at construction and the
    // handler swaps behind it — otherwise `setOnContextLoss(undefined)` is inexpressible.
    const { surface, backend } = harness();
    expect(backend.calls.filter((c) => c === "setOnContextLoss")).toHaveLength(1);

    const first = vi.fn();
    const second = vi.fn();
    surface.setOnContextLoss(first);
    surface.setOnContextLoss(second);
    backend.lossHandler?.();

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
    expect(backend.calls.filter((c) => c === "setOnContextLoss")).toHaveLength(1);
  });

  it("asks every attached terminal to re-derive after a restore, not just one", () => {
    // The half that only exists once a surface holds more than one grid. A restore re-bakes at the
    // live density and moves every grid's CELL, and the drawing buffer belongs to no grid — so each
    // terminal has to re-ask for its own geometry at its own new cell. A handler that re-derived for
    // one grid would leave every sibling drawing at a cell that no longer exists.
    const { surface, canvas, backend } = harness();
    const reapplied: number[] = [];
    const a = surface.addGrid();
    const b = surface.addGrid();
    surface.onReapply(a, () => reapplied.push(a));
    surface.onReapply(b, () => reapplied.push(b));

    canvas.emit("webglcontextrestored");

    expect(reapplied.sort()).toEqual([a, b].sort());
    // Order is load-bearing: the renderer rebuilds inside `render()`, not when the event fires, so
    // re-deriving before it would read the PRE-restore cell. Present, re-derive, present (#325).
    const renders = backend.calls.filter((c) => c === "render").length;
    expect(renders).toBe(2);
  });

  it("stops re-deriving for a terminal that has been released", () => {
    // The mirror of the registry rule: a released grid throws on every per-grid call, so a restore
    // that still reached it would turn a recovery into an exception for its siblings too.
    const { surface, canvas } = harness();
    const reapplied: number[] = [];
    const a = surface.addGrid();
    surface.onReapply(a, () => reapplied.push(a));

    surface.removeGrid(a);
    canvas.emit("webglcontextrestored");

    expect(reapplied).toEqual([]);
  });

  it("does not resurrect a tenant when handed a grid the registry no longer holds", () => {
    // The sibling of the test above, and NOT implied by it — found by mutation: making `onReapply`
    // register unconditionally left that one green, because there the grid was still registered when
    // the callback was handed over. This is the other order, which a `Terminal` reaches by racing its
    // own teardown against a late registration.
    //
    // What it would cost is worse than a stale callback: an entry re-inserted here is one the surface
    // believes it holds, so `gridCount` over-reports and `dispose` calls `removeGrid` on an id the
    // renderer has already retired — which throws, taking every sibling's teardown down with it.
    const { surface, backend, canvas } = harness();
    const reapplied: number[] = [];
    const a = surface.addGrid();
    surface.removeGrid(a);

    surface.onReapply(a, () => reapplied.push(a));
    // `onEnd` has to hold the same line, and it is the more expensive of the two to get wrong: a
    // resurrected tenant here would be *ended* by a later `dispose()`, so a widget that had already
    // torn itself down would be torn down a second time.
    const ends: number[] = [];
    surface.onEnd(a, () => ends.push(a));
    canvas.emit("webglcontextrestored");

    expect(reapplied).toEqual([]);
    expect(surface.gridCount).toBe(0);
    surface.dispose();
    expect(ends).toEqual([]);
    expect(backend.removed).toEqual([a]);
  });
});

describe("TerminalSurface — density", () => {
  it("pushes a density change to the backend and re-derives every terminal", () => {
    // `setDevicePixelRatio` takes no grid — it is the surface's, and it moves EVERY grid's cell.
    // This is the call whose per-terminal placement was the concrete defect: with the watcher on the
    // terminal, disposing one terminal stopped density tracking for its siblings.
    const { surface, backend, queries, dpr } = harness();
    const reapplied: number[] = [];
    const a = surface.addGrid();
    surface.onReapply(a, () => reapplied.push(a));

    dpr.value = 2;
    for (const l of [...(queries.at(-1)?.listeners ?? [])]) l();

    expect(backend.calls).toContain("setDevicePixelRatio(2)");
    expect(reapplied).toEqual([a]);
  });
});

describe("TerminalSurface — the sole tenant", () => {
  it("refuses a second grid on a surface whose extent one terminal owns", () => {
    // The single-terminal arrangement sizes the drawing buffer to `cols * cell` — #331's exactness,
    // where nothing rounds between the grid the shader lays out and the buffer holding it. That is
    // only available while ONE grid fills the canvas: a second tenant sizing the same buffer to its
    // own extent would clobber the first, and the failure is the silent kind — no error, the sibling
    // simply drawn into a buffer that is the wrong size.
    //
    // So it is refused loudly here rather than left to whoever measures the pixels afterwards. A
    // host that wants two terminals sizes the surface itself and claims no sole tenancy.
    const { surface } = harness();
    surface.addGrid({ ownsExtent: true });

    expect(() => surface.addGrid()).toThrow(/sole tenant/);
  });

  it("allows a second grid once the sole tenant has gone", () => {
    // The claim is a property of the registry's current membership, not a latch: a surface whose one
    // terminal ended is an ordinary empty surface again.
    const { surface } = harness();
    const a = surface.addGrid({ ownsExtent: true });
    surface.removeGrid(a);

    expect(() => surface.addGrid()).not.toThrow();
  });

  it("refuses sole tenancy on a surface that already has a terminal", () => {
    // The mirror direction, and it is not implied by the first: without it the guard would depend on
    // which order the two attaches happened in.
    const { surface } = harness();
    surface.addGrid();

    expect(() => surface.addGrid({ ownsExtent: true })).toThrow(/sole tenant/);
  });
});

describe("viewportOrigin — where an overlay sits on the shared buffer", () => {
  it("is the overlay's offset from the canvas, in device px", () => {
    // Not the overlay's page position: a viewport addresses the drawing buffer, whose origin is the
    // canvas. Using the raw client rect would place every terminal by however far the page happens
    // to have been scrolled.
    const at = viewportOrigin(
      { overlay: { left: 500, top: 300 }, canvas: { left: 100, top: 100 } },
      2,
    );
    expect(at).toEqual({ x: 800, y: 400 });
  });

  it("rounds to a whole device pixel", () => {
    // A GL rect cannot be fractional. At dpr 1.5 a 33.5 CSS-px offset is 50.25 device px, and the
    // fractional part shows up as blur rather than as displacement — the failure mode #337/#352 are
    // about, which is why this is asserted rather than left to the caller.
    const at = viewportOrigin(
      { overlay: { left: 33.5, top: 10.5 }, canvas: { left: 0, top: 0 } },
      1.5,
    );
    expect(at).toEqual({ x: 50, y: 16 });
    expect(Number.isInteger(at.x) && Number.isInteger(at.y)).toBe(true);
  });

  it("clamps an overlay scrolled off the top of the canvas to the origin", () => {
    // A negative viewport origin is not a smaller rect — it is a GL error or a silently dropped
    // draw. Reachable the moment a terminal is inside a scroll container.
    const at = viewportOrigin(
      { overlay: { left: -40, top: -120 }, canvas: { left: 0, top: 0 } },
      2,
    );
    expect(at).toEqual({ x: 0, y: 0 });
  });

  it("moves with the density, because the origin is device px", () => {
    // The same overlay at two ratios is two different buffer positions — which is why a density
    // change invalidates a rect (ADR-0021 D3) and why this takes the ratio rather than caching one.
    const boxes = { overlay: { left: 200, top: 50 }, canvas: { left: 0, top: 0 } };
    expect(viewportOrigin(boxes, 1)).toEqual({ x: 200, y: 50 });
    expect(viewportOrigin(boxes, 2)).toEqual({ x: 400, y: 100 });
  });
});
