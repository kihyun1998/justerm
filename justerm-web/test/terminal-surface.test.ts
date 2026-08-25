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
    // The renderer **drops** a density notification that arrives while the context is lost, rather
    // than queueing it — `restore()` re-reads the live ratio instead (#325). Modelled rather than
    // ignored, so a test that constructs the mid-loss case is asserting the case its comment
    // describes: without this the `contextLost` line in that test is a dead variable and deleting it
    // leaves the test green.
    this.calls.push(`setDevicePixelRatio(${dpr})${this.contextLost ? "-dropped" : ""}`);
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
  /** Bumped by the fake so a delta is readable; the real one counts atlas bakes. */
  bakeCount = 0;
  /** How many font configurations the fake claims to hold. */
  atlases = 1;
  atlasCount(): number {
    return this.atlases;
  }
  bakes(): number {
    return this.bakeCount;
  }
  packCount = 0;
  packs(): number {
    return this.packCount;
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

function harness(initialDpr = 1): Harness {
  const backend = new FakeBackend();
  const canvas = new FakeCanvas();
  const raf = new Raf();
  const queries: FakeQuery[] = [];
  const dpr = { value: initialDpr };
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

describe("GridLease — a stale id becomes unrepresentable", () => {
  it("hands back a lease that carries its own id", () => {
    // The id still exists: every per-grid renderer call names it, and it crosses the wasm boundary as
    // a number (#770). What changes is that it is no longer the ONLY thing a caller holds, so it is
    // no longer the thing a caller has to keep valid.
    const { surface, backend } = harness();

    const lease = surface.addGrid();

    expect(typeof lease.id).toBe("number");
    expect(backend.calls.filter((c) => c.startsWith("addGrid"))).toEqual(["addGrid->1"]);
    expect(lease.released).toBe(false);
  });

  it("releases exactly once, however many times it is asked", () => {
    // The `Renderer` port requires `Terminal.dispose()` to be silent on a second call, so something
    // must absorb it. The difference from the guard this replaces is WHAT absorbs it: the surface
    // used to swallow an id it could not recognise; a lease declines a second call about ITSELF.
    // One is not knowing, the other is knowing.
    const { surface, backend } = harness();
    const lease = surface.addGrid();

    lease.release();
    lease.release();
    lease.release();

    expect(backend.removed).toEqual([lease.id]);
    expect(lease.released).toBe(true);
  });

  it("stops delivering to a released lease's callbacks", () => {
    // The behaviour the three deleted guards were protecting, now a property of the lease rather
    // than of an id lookup: after release there is nothing to deliver to, and no id anyone can name.
    const { surface, canvas } = harness();
    const fired: string[] = [];
    const a = surface.addGrid();
    const b = surface.addGrid();
    a.onReapply(() => fired.push("a"));
    b.onReapply(() => fired.push("b"));

    a.release();
    canvas.emit("webglcontextrestored");

    expect(fired).toEqual(["b"]);
  });


  it("delivers nothing to a released lease, however late a callback arrives", () => {
    // The race a terminal reaches by registering late against its own teardown. Previously this was
    // "the registry does not hold that id" — a guard. Now it is a property of membership: the lease
    // left the surface's set on release, so nothing can reach it to deliver, and no guard is needed
    // for a call that cannot be observed.
    const { surface, canvas } = harness();
    const fired: string[] = [];
    const a = surface.addGrid();

    a.release();
    a.onReapply(() => fired.push("a"));
    a.onEnd(() => fired.push("end"));
    canvas.emit("webglcontextrestored");
    surface.dispose();

    expect(fired).toEqual([]);
    // The side condition that makes this about REGISTRATION rather than about delivery: a lease that
    // accepted the callback would be holding the consumer's closure for the life of the page, which
    // is what `release`'s contract says it does not do.
    expect(a.released).toBe(true);
  });

  it("ends each live lease when the surface ends, and skips released ones", () => {
    const { surface, backend } = harness();
    const ended: number[] = [];
    const a = surface.addGrid();
    const b = surface.addGrid();
    a.onEnd(() => ended.push(a.id));
    b.onEnd(() => ended.push(b.id));

    a.release();
    surface.dispose();

    expect(ended).toEqual([b.id]);
    expect(backend.removed.sort()).toEqual([a.id, b.id].sort());
  });
});

describe("TerminalSurface — the grid registry", () => {
  it("hands each attached terminal its own grid", () => {
    // The registry is the surface's, and a grid id is the terminal's handle into it. Two terminals
    // sharing one surface must never share the id: every per-grid call in the renderer names it, so
    // a duplicate would make one terminal's frames land on the other's cells.
    const { surface, backend } = harness();

    const a = surface.addGrid();
    const b = surface.addGrid();

    expect(a.id).not.toBe(b.id);
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

    a.release();

    expect(backend.removed).toEqual([a.id]);
    expect(surface.gridCount).toBe(1);
    // The side condition, and the one that would catch an over-eager teardown: the sibling is still
    // there, so a later surface dispose still has something to release.
    expect(backend.removed).not.toContain(b.id);
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

    expect(backend.removed.sort()).toEqual([a.id, b.id].sort());
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
    a.onEnd(() => ended.push(a.id));
    b.onEnd(() => ended.push(b.id));

    surface.dispose();

    expect(ended.sort()).toEqual([a.id, b.id].sort());
    expect(backend.removed.sort()).toEqual([a.id, b.id].sort());
  });

  it("retires the grid of a tenant that registered no end callback", () => {
    // The half-composed case: `addGrid` succeeds and the terminal throws before registering. Without
    // the sweep after the end pass, that grid — a VAO, an instance buffer and an atlas refcount —
    // would outlive the surface with nothing holding it.
    const { surface, backend } = harness();
    const a = surface.addGrid();

    surface.dispose();

    expect(backend.removed).toEqual([a.id]);
  });

  it("survives a tenant whose end callback disposes back into the surface", () => {
    // The real shape: a terminal's own teardown calls `removeGrid`, and one that composed the
    // surface also calls `dispose()`. Both re-enter while the surface is mid-teardown, and the latch
    // is what makes that safe rather than infinite — asserted here because the recursion is
    // invisible at either call site and only shows up when both exist.
    const { surface, backend } = harness();
    let ends = 0;
    const a = surface.addGrid();
    a.onEnd(() => {
      ends++;
      a.release();
      surface.dispose();
    });

    surface.dispose();

    expect(ends).toBe(1);
    expect(backend.removed).toEqual([a.id]);
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
    a.onReapply(() => reapplied.push(a.id));
    b.onReapply(() => reapplied.push(b.id));

    canvas.emit("webglcontextrestored");

    expect(reapplied.sort()).toEqual([a.id, b.id].sort());
    // Order is load-bearing: the renderer rebuilds inside `render()`, not when the event fires, so
    // re-deriving before it would read the PRE-restore cell. Present, re-derive, present (#325).
    const renders = backend.calls.filter((c) => c === "render").length;
    expect(renders).toBe(2);
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
    a.onReapply(() => reapplied.push(a.id));

    dpr.value = 2;
    for (const l of [...(queries.at(-1)?.listeners ?? [])]) l();

    expect(backend.calls).toContain("setDevicePixelRatio(2)");
    expect(reapplied).toEqual([a.id]);
  });

  it("tells the host last, after every terminal has re-derived", () => {
    // The order this method's doc-comment claims and nothing pinned: a host re-measuring inside the
    // handler must be measuring against the new cell rather than racing it. Recorded through the
    // backend's own call log so the claim is about SEQUENCE, not about the handler having fired.
    const { surface, backend, queries, dpr } = harness();
    const a = surface.addGrid();
    a.onReapply(() => backend.calls.push("reapply"));
    surface.onDensityChange((d) => backend.calls.push(`density(${d})`));

    dpr.value = 2;
    for (const l of [...(queries.at(-1)?.listeners ?? [])]) l();

    expect(backend.calls.slice(-4)).toEqual([
      "setDevicePixelRatio(2)",
      "reapply",
      "render",
      "density(2)",
    ]);
  });

  it("tells the host when a restore adopts a density that moved while the context was dead", () => {
    // #808. `restore()` re-reads the LIVE ratio and re-bakes at it, because a density notification
    // arriving during a loss is dropped rather than queued — so a restore is the one density
    // adoption with no setter behind it. A SOLE tenant repairs itself (`reapplySurface` re-derives
    // its own buffer, #325); a SHARED one cannot, because the buffer and every viewport rect are
    // the host's, in device px at the ratio that just stopped being true (ADR-0021 D3).
    const { surface, canvas, backend, dpr } = harness();
    const a = surface.addGrid();
    a.onReapply(() => backend.calls.push("reapply"));
    surface.onDensityChange((d) => backend.calls.push(`density(${d})`));

    dpr.value = 2;
    canvas.emit("webglcontextrestored");

    // Told, once, with the live ratio…
    expect(backend.calls.filter((c) => c.startsWith("density("))).toEqual(["density(2)"]);
    // …and told LAST, for the same reason the setter tells last: the renderer rebuilds inside the
    // first `render()`, so anything the host re-measures before that reads the PRE-restore cell.
    expect(backend.calls.slice(-4)).toEqual(["render", "reapply", "render", "density(2)"]);
  });

  it("says nothing when a restore adopts the density the host already has", () => {
    // The half that makes this "the ratio we last announced" rather than "announce on every
    // restore". A spurious notification is not free: the host's handler re-sizes the drawing
    // buffer, and a resized buffer is a cleared one — so an unconditional announce would blank
    // every terminal on the surface once per context loss.
    const { surface, canvas, backend } = harness();
    surface.onDensityChange((d) => backend.calls.push(`density(${d})`));

    canvas.emit("webglcontextrestored");

    expect(backend.calls.filter((c) => c.startsWith("density("))).toEqual([]);
    // The side condition that keeps the assertion above from passing vacuously: the restore ran.
    expect(backend.calls.filter((c) => c === "render")).toHaveLength(2);
  });

  it("seeds the announced ratio from the live one, not from 1", () => {
    // A surface opened on a Retina display starts at 2, and the host sized its buffer at 2. Seeding
    // the field with a literal would make that surface's first restore announce a change nobody
    // made — the mutation this test exists to redden.
    const { surface, canvas, backend } = harness(2);
    surface.onDensityChange((d) => backend.calls.push(`density(${d})`));

    canvas.emit("webglcontextrestored");

    expect(backend.calls.filter((c) => c.startsWith("density("))).toEqual([]);
  });

  it("schedules a present after the host's handler, on both paths", () => {
    // The handler's documented obligations include a fresh `resizeSurface`, which re-creates the
    // drawing buffer and therefore CLEARS it. Both callers present before the handler — the cell is
    // not readable until a render has run — so without a present after it, the last thing to touch
    // the buffer is the clear and the canvas stays blank until something else drives a frame.
    //
    // Asserted as SCHEDULED-then-flushed rather than as an immediate `render`, because coalescing is
    // the claim: a host re-placing N terminals inside one handler owes one frame, not N.
    const { surface, canvas, backend, raf, queries, dpr } = harness();
    surface.addGrid();
    surface.onDensityChange(() => {
      // exactly what the README asks of a host, and nothing more — no frame pushed.
      surface.resizeSurface(1800, 680);
    });

    dpr.value = 2;
    for (const l of [...(queries.at(-1)?.listeners ?? [])]) l();
    expect(raf.scheduled).toBe(1);
    const beforeFlush = backend.calls.filter((c) => c === "render").length;
    raf.flush();
    expect(backend.calls.filter((c) => c === "render").length).toBe(beforeFlush + 1);
    // …and the resize really did land before it, so the present is repainting a cleared buffer
    // rather than merely being one more frame.
    expect(backend.calls.indexOf("resizeSurface(1800,680)")).toBeLessThan(
      backend.calls.lastIndexOf("render"),
    );

    // The restore path owes the same thing and gets it from the same place.
    dpr.value = 4;
    canvas.emit("webglcontextrestored");
    expect(raf.scheduled).toBe(1);
  });

  it("schedules nothing when there was nothing to announce", () => {
    // The side condition that keeps the pair above from being "present on every restore". A restore
    // that adopted no new density asks the host for nothing, so nothing cleared the buffer and the
    // two presents the handler already did are the whole of it.
    const { surface, canvas, raf } = harness();
    surface.addGrid();
    surface.onDensityChange(() => surface.resizeSurface(1800, 680));

    canvas.emit("webglcontextrestored");

    expect(raf.scheduled).toBe(0);
  });

  it("keeps owing the host a move that landed before it registered", () => {
    // The field is *"the density this surface last told the host about"*, so it may only advance when
    // somebody was in fact told. Recording unconditionally would let a move that landed in the window
    // between `open()` and the host's registration read afterwards as agreement — and there is no
    // second `webglcontextrestored` for one loss, so the debt would never be collected.
    //
    // The window is production shape, not a contrivance: `demo/shared-surface.ts` constructs the
    // surface and registers the handler either side of two awaited `attach()` calls, each awaiting a
    // wasm decoder init.
    const { surface, canvas, backend, queries, dpr } = harness();

    dpr.value = 2;
    for (const l of [...(queries.at(-1)?.listeners ?? [])]) l();
    surface.onDensityChange((d) => backend.calls.push(`density(${d})`));
    canvas.emit("webglcontextrestored");

    expect(backend.calls.filter((c) => c.startsWith("density("))).toEqual(["density(2)"]);
  });

  it("says nothing on a SECOND restore at a ratio the first one already announced", () => {
    // The half of the restore path the announce-side tests cannot see: notifying without RECORDING
    // passes every one of them, because they each fire one restore. Two losses in a row is an
    // ordinary case — it is what the renderer's restore deadline exists for.
    const { surface, canvas, backend, dpr } = harness();
    surface.onDensityChange((d) => backend.calls.push(`density(${d})`));

    dpr.value = 2;
    canvas.emit("webglcontextrestored");
    canvas.emit("webglcontextrestored");

    expect(backend.calls.filter((c) => c.startsWith("density("))).toEqual(["density(2)"]);
  });

  it("records the ratio it was GIVEN, not the one the display is at", () => {
    // `setDevicePixelRatio` is public, so a host may adopt a density the display is not at — and what
    // the field holds is what the host was *told*, which is the argument. Recording the live ratio
    // instead would leave the field describing a display nobody was told about, and the restore that
    // then adopts the live ratio would stay silent about a real move.
    //
    // This is also the only test here that drives the setter AS a public setter: the others reach it
    // through `DprWatcher`, where the argument and the live ratio are equal by construction.
    const { surface, canvas, backend, dpr } = harness();
    surface.onDensityChange((d) => backend.calls.push(`density(${d})`));

    surface.setDevicePixelRatio(3);
    expect(dpr.value).toBe(1);
    canvas.emit("webglcontextrestored");

    expect(backend.calls.filter((c) => c.startsWith("density("))).toEqual([
      "density(3)",
      "density(1)",
    ]);
  });

  it("does not re-announce a density the setter already announced during the loss", () => {
    // The interaction, and the reason this is a REMEMBERED value rather than a comparison against
    // the backend. A watcher change delivered while the context is dead reaches `setDevicePixelRatio`
    // — the renderer drops it, but the host is told and re-supplies at the new ratio. `restore()`
    // then adopts that same live ratio. Announcing again would make the host re-measure and re-size
    // the buffer a second time, for a move it has already paid for.
    const { surface, canvas, backend, queries, dpr } = harness();
    surface.onDensityChange((d) => backend.calls.push(`density(${d})`));

    backend.contextLost = true;
    dpr.value = 2;
    for (const l of [...(queries.at(-1)?.listeners ?? [])]) l();
    backend.contextLost = false;
    canvas.emit("webglcontextrestored");

    // The renderer really did drop it — without this the test is "setter, then restore at the same
    // ratio", which is a weaker claim wearing this one's comment.
    expect(backend.calls).toContain("setDevicePixelRatio(2)-dropped");
    expect(backend.calls.filter((c) => c.startsWith("density("))).toEqual(["density(2)"]);
  });
});

/**
 * **The "sole tenant" block that stood here is deleted, not disabled (#802).**
 *
 * It covered a surface refusing a second grid once one tenant had claimed to size the drawing
 * buffer. That state cannot be constructed: the only surface a terminal auto-sizes is the one
 * `JustermRenderer.create` composed, and `create` keeps it in a private field with no accessor, so
 * no second tenant can reach it to attach. The guard defended an unreachable state and the option
 * existed to feed the guard.
 *
 * What replaces the coverage is a **type-level** assertion rather than a runtime one, because the
 * guarantee is structural: `test/published-seam.types.ts` §3 reddens the moment any member of
 * `JustermRenderer` hands its surface back. Mutation-verified four ways — a getter, a method, a
 * public field, and an escape widened to `SurfaceBackend`.
 */

describe("viewportOrigin — where an overlay sits on the shared buffer", () => {
  it("is the overlay's offset from the canvas, in device px", () => {
    // Not the overlay's page position: a viewport addresses the drawing buffer, whose origin is the
    // canvas. Using the raw client rect would place every terminal by however far the page happens
    // to have been scrolled.
    const at = viewportOrigin(
      { overlay: { left: 500, top: 300, width: 400, height: 300 }, canvas: { left: 100, top: 100 } },
      2,
    );
    expect(at).toEqual({ x: 800, y: 400 });
  });

  it("rounds to a whole device pixel", () => {
    // A GL rect cannot be fractional. At dpr 1.5 a 33.5 CSS-px offset is 50.25 device px, and the
    // fractional part shows up as blur rather than as displacement — the failure mode #337/#352 are
    // about, which is why this is asserted rather than left to the caller.
    const at = viewportOrigin(
      { overlay: { left: 33.5, top: 10.5, width: 400, height: 300 }, canvas: { left: 0, top: 0 } },
      1.5,
    );
    expect(at).toEqual({ x: 50, y: 16 });
    expect(at && Number.isInteger(at.x) && Number.isInteger(at.y)).toBe(true);
  });

  it("clamps an overlay scrolled off the top of the canvas to the origin", () => {
    // A negative viewport origin is not a smaller rect — it is a GL error or a silently dropped
    // draw. Reachable the moment a terminal is inside a scroll container.
    const at = viewportOrigin(
      { overlay: { left: -40, top: -120, width: 400, height: 300 }, canvas: { left: 0, top: 0 } },
      2,
    );
    expect(at).toEqual({ x: 0, y: 0 });
  });

  it("answers undefined for an overlay with no box, not the origin (#801)", () => {
    // The defect this replaced, measured in a real browser: a `display: none` overlay reports every
    // field as 0, the clamp turned that into `{0,0}`, and the grid — whose extent is derived from
    // `cols * cell` and never from this box — was re-placed at FULL SIZE on the canvas corner, over
    // its sibling.
    //
    // The side condition is what makes this a test rather than a restatement: the SAME left/top with
    // a box still answers an origin, so the union is keyed on the extent and not on the position.
    const at = { left: 0, top: 0 };
    expect(viewportOrigin({ overlay: { ...at, width: 0, height: 0 }, canvas: at }, 2)).toBeUndefined();
    expect(viewportOrigin({ overlay: { ...at, width: 400, height: 300 }, canvas: at }, 2)).toEqual({
      x: 0,
      y: 0,
    });
  });

  it("answers undefined when EITHER dimension is gone, not only when both are", () => {
    // Mutating the predicate rather than its placement: `width <= 0 && height <= 0` is the plausible
    // differently-wrong version, and it is green on the test above — a `display: none` box zeroes
    // both. These two are the window where the two predicates disagree, and a collapsed pane
    // (`height: 0` inside a flex row, a `width: 0` split) reaches it without anything being hidden.
    const canvas = { left: 0, top: 0 };
    expect(
      viewportOrigin({ overlay: { left: 10, top: 10, width: 0, height: 300 }, canvas }, 1),
    ).toBeUndefined();
    expect(
      viewportOrigin({ overlay: { left: 10, top: 10, width: 400, height: 0 }, canvas }, 1),
    ).toBeUndefined();
  });

  it("treats a negative extent as no box, since a rect cannot have one", () => {
    // `<= 0` rather than `=== 0`. Not defensive: a `getBoundingClientRect` under a CSS transform can
    // report a negative dimension, and `-1` would otherwise pass a `=== 0` guard and place a grid.
    const canvas = { left: 0, top: 0 };
    expect(
      viewportOrigin({ overlay: { left: 10, top: 10, width: -1, height: 300 }, canvas }, 1),
    ).toBeUndefined();
  });

  it("moves with the density, because the origin is device px", () => {
    // The same overlay at two ratios is two different buffer positions — which is why a density
    // change invalidates a rect (ADR-0021 D3) and why this takes the ratio rather than caching one.
    const boxes = {
      overlay: { left: 200, top: 50, width: 400, height: 300 },
      canvas: { left: 0, top: 0 },
    };
    expect(viewportOrigin(boxes, 1)).toEqual({ x: 200, y: 50 });
    expect(viewportOrigin(boxes, 2)).toEqual({ x: 400, y: 100 });
  });
});
