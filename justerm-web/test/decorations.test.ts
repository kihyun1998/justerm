import { describe, expect, it } from "vitest";
import { DecorationRegistry } from "../src/decorations";
import { MarkerKind } from "../src/markers";

/** A frame stripped to what the ruler join reads: all-marker absolute lines
 * (stride-2 `id, line`) plus the scroll geometry for the `line / total` ratio. */
function rulerFrame(markerLines: number[], scrollbackLen: number, rows: number) {
  return { markerLines, scrollbackLen, rows };
}

/** One stride-5 marker record `(id, row, kind, exitPresent, exitBits)`. Decorations
 * attach to any marker id, so kind/exit are irrelevant here (default Plain). */
function mk(id: number, row: number, kind: MarkerKind = MarkerKind.Plain): number[] {
  return [id, row, kind, 0, 0];
}
/** A frame carrying the given flat marker records (the only field the registry
 * reads — `markerPositions`, projected to on-viewport markers by the wire). */
function frame(...records: number[][]) {
  return { markerPositions: records.flat() };
}

/** A frame with viewport geometry (cols/rows) — needed for right-anchored columns
 * and multi-row (`height`) clipping. */
function frameGeom(cols: number, rows: number, ...records: number[][]) {
  return { cols, rows, markerPositions: records.flat() };
}

/** One stride-2 `markerLines` record `(id, absoluteLine)` — the v11 group, which
 * unlike `markerPositions` includes markers scrolled OFF the viewport. */
function ml(id: number, line: number): number[] {
  return [id, line];
}
/** A frame carrying absolute marker lines + the viewport's scroll position, i.e. what
 * production always sends. Viewport row 0 is absolute line `scrollbackLen - displayOffset`.
 * `markerPositions` is passed too (the real wire sends both) but omits off-viewport markers,
 * which is the whole point of #461. */
function frameAbs(
  opts: { rows: number; scrollbackLen: number; displayOffset?: number; cols?: number },
  lines: number[][],
  positions: number[][] = [],
) {
  return {
    cols: opts.cols ?? 80,
    rows: opts.rows,
    scrollbackLen: opts.scrollbackLen,
    displayOffset: opts.displayOffset ?? 0,
    markerLines: lines.flat(),
    markerPositions: positions.flat(),
  };
}

describe("DecorationRegistry (#120 S1)", () => {
  // The core join: a decoration registered against a marker id projects onto that
  // marker's CURRENT viewport row each frame, with its x-range/layer/colour refs.
  // Colours pass through as opaque refs (resolution is the renderer/#115).
  it("projects a registered decoration onto its marker's current row", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, x: 2, width: 3, layer: "top", bg: 0x112233 });

    expect(reg.decorationsForFrame(frame(mk(7, 4)))).toEqual([
      { row: 4, left: 2, right: 4, layer: "top", bg: 0x112233, fg: undefined },
    ]);
  });

  // Defaults mirror xterm's IDecorationOptions: x=0, width=1 (single cell),
  // layer='bottom' (under the text).
  it("defaults x=0, width=1, layer=bottom", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1 });

    expect(reg.decorationsForFrame(frame(mk(1, 0)))).toEqual([
      { row: 0, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // Disposing the handle removes the decoration — it stops projecting.
  it("disposing the handle stops projection", () => {
    const reg = new DecorationRegistry();
    const d = reg.register({ markerId: 3 });
    expect(reg.decorationsForFrame(frame(mk(3, 1)))).toHaveLength(1);

    d.dispose();

    expect(d.disposed).toBe(true);
    expect(reg.decorationsForFrame(frame(mk(3, 1)))).toEqual([]);
  });

  // Auto-dispose: a MarkerDisposed event (out-of-band, like #160's) disposes every
  // decoration anchored to that marker — xterm's `marker.onDispose(() =>
  // decoration.dispose())`. Even if the id is later reissued (RIS) and reappears
  // in a frame, the disposed decoration never projects again.
  it("onMarkerDisposed disposes every decoration on that marker", () => {
    const reg = new DecorationRegistry();
    const a = reg.register({ markerId: 5, x: 0 });
    const b = reg.register({ markerId: 5, x: 1, layer: "top" });
    reg.register({ markerId: 6 }); // a different marker, untouched

    reg.onMarkerDisposed(5);

    expect(a.disposed).toBe(true);
    expect(b.disposed).toBe(true);
    // marker 5 reappearing yields nothing; marker 6 still projects.
    expect(reg.decorationsForFrame(frame(mk(5, 2), mk(6, 3)))).toEqual([
      { row: 3, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // The projection is frame-driven: a marker the frame does not mention at all yields no
  // rect, and reappears once it does (mirrors `overlay.ts` highlight projection).
  //
  // NOTE this frame carries `markerPositions` only, so it exercises the viewport-relative
  // half of the join. Do NOT read it as "a marker scrolled off the viewport yields nothing"
  // — that WAS its rationale and #461 made it false: a real frame also carries the absolute
  // `markerLines`, which includes markers scrolled above the top, and those now project their
  // visible rows. See the `frameAbs` tests below for that path.
  it("yields no rect for a marker absent from the frame; it reappears when present", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 9, x: 1 });

    expect(reg.decorationsForFrame(frame(mk(2, 0)))).toEqual([]); // marker 9 off-viewport

    expect(reg.decorationsForFrame(frame(mk(9, 5)))).toEqual([
      { row: 5, left: 1, right: 1, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // Registering against a marker id that never appears in a frame is a harmless
  // no-op: a live handle that simply never projects (justerm has no marker object
  // to guard on `isDisposed`, and id reuse forbids a permanent reject-set).
  it("registering against a marker never seen in a frame is a harmless no-op", () => {
    const reg = new DecorationRegistry();
    const d = reg.register({ markerId: 999 });

    expect(d.disposed).toBe(false);
    expect(reg.decorationsForFrame(frame(mk(1, 0)))).toEqual([]);
  });

  // Multiple decorations per marker (different layers/columns) and across markers
  // each project, following their marker's row — in registration order, whichever
  // marker each anchors to (#458). Here the two rules happen to agree, so this
  // fixture pins multiplicity, not ordering; the ordering tests are below.
  it("projects multiple decorations per marker and across markers", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 0, layer: "bottom" });
    reg.register({ markerId: 1, x: 5, layer: "top" });
    reg.register({ markerId: 2, x: 2 });

    expect(reg.decorationsForFrame(frame(mk(1, 3), mk(2, 8)))).toEqual([
      { row: 3, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined },
      { row: 3, left: 5, right: 5, layer: "top", bg: undefined, fg: undefined },
      { row: 8, left: 2, right: 2, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // #202: a multi-row decoration (`height` > 1) projects one single-row rect per
  // row it spans, starting at the marker's row and extending DOWN (xterm `top =
  // marker.line`, `height` rows). The rect shape stays single-row so the S2 render
  // is untouched.
  it("projects one rect per row for a multi-row height", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 0, width: 2, height: 3, bg: 0x111111 });

    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 4)))).toEqual([
      { row: 4, left: 0, right: 1, layer: "bottom", bg: 0x111111, fg: undefined },
      { row: 5, left: 0, right: 1, layer: "bottom", bg: 0x111111, fg: undefined },
      { row: 6, left: 0, right: 1, layer: "bottom", bg: 0x111111, fg: undefined },
    ]);
  });

  // A multi-row decoration whose span runs past the viewport bottom is clipped to
  // the visible rows (no rects for rows that don't exist).
  it("clips a multi-row height to the viewport bottom", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, height: 5 }); // marker at row 3, only rows 3..4 visible

    expect(reg.decorationsForFrame(frameGeom(20, 5, mk(1, 3)))).toEqual([
      { row: 3, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined },
      { row: 4, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // #202: a right-anchored decoration counts columns from the RIGHT edge. x=0,
  // width=1 → the rightmost cell (cols-1); the span extends leftward by width.
  it("anchors columns to the right edge", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, anchor: "right", x: 0, width: 1 });

    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 2)))).toEqual([
      { row: 2, left: 19, right: 19, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // Right anchor with an x offset and width: x cells in from the right, extending
  // leftward. cols=20, x=1, width=3 → right = 20-1-1 = 18, left = 20-1-3 = 16.
  it("offsets a right-anchored span inward by x", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, anchor: "right", x: 1, width: 3 });

    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 0)))).toEqual([
      { row: 0, left: 16, right: 18, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // #461: a multi-row decoration whose marker scrolled ABOVE the viewport top must still
  // paint the rows of it that are visible — the vertical mirror of #457's horizontal clip.
  // Core drops an above-top marker from `markerPositions` (`m.line.checked_sub(top)?`), so
  // joining on that alone makes the whole decoration vanish; the absolute `markerLines`
  // group carries it. xterm has no such gap: it keys colour lookup to the absolute buffer
  // line (`WebglRenderer` `row = y + buffer.ydisp`) and buckets every line the height
  // covers (`DecorationService._addToLineBuckets`).
  it("paints the visible rows of a decoration whose marker is above the viewport top", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 0, width: 2, height: 5, bg: 0x00ff00 });

    // Viewport row 0 == absolute line 13. The marker sits at 10, so the decoration spans
    // absolute 10..14 — rows -3..1 in viewport terms, of which 0 and 1 are on screen.
    const rects = reg.decorationsForFrame(
      frameAbs({ rows: 10, scrollbackLen: 13 }, [ml(1, 10)], []),
    );

    expect(rects.map((r) => r.row)).toEqual([0, 1]);
    expect(rects.every((r) => r.left === 0 && r.right === 1 && r.bg === 0x00ff00)).toBe(true);
  });

  // #461 (2-lens): the two marker groups are merged PER MARKER, not switched between. A
  // consumer whose `markerLines` omits a marker that `markerPositions` carries (the demo does
  // exactly this — its markerLines holds only the decoration marker, while markerPositions also
  // holds command marks) must not see that decoration silently vanish.
  it("resolves a marker carried only by markerPositions, alongside absolute ones", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 0, width: 1, bg: 0x111111 }); // absolute-only
    reg.register({ markerId: 2, x: 1, width: 1, bg: 0x222222 }); // markerPositions-only

    const rects = reg.decorationsForFrame(
      frameAbs({ rows: 10, scrollbackLen: 13 }, [ml(1, 15)], [mk(2, 4)]),
    );

    expect(rects).toEqual([
      { row: 2, left: 0, right: 0, layer: "bottom", bg: 0x111111, fg: undefined },
      { row: 4, left: 1, right: 1, layer: "bottom", bg: 0x222222, fg: undefined },
    ]);
  });

  // #461: the absolute line WINS for a marker in both groups — it is the only one that can
  // express an anchor above the viewport top, so a stale/derived viewport row must not mask it.
  it("prefers the absolute line when a marker is in both groups", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 0, width: 1 });

    const rects = reg.decorationsForFrame(
      frameAbs({ rows: 10, scrollbackLen: 13 }, [ml(1, 16)], [mk(1, 9)]),
    );

    expect(rects.map((r) => r.row)).toEqual([3]); // 16 - 13, not the markerPositions row 9
  });

  // #482: the wire carries EVERY live marker (M, unbounded with scrollback), but only markers with
  // a registered decoration project. Filtering to those keeps per-frame allocation/iteration
  // O(decorations), not O(markers). The invariant this pins is that a decorated marker buried among
  // many non-decorated ones is NOT dropped, and that the 6 undecorated markers contribute nothing.
  //
  // This test used to also pin cross-marker precedence as CORE's marker order — #458 decided the
  // opposite (registration order; see the precedence test above), so the expected order below is
  // reversed from what #482 wrote. That was never #482's own finding: it recorded the order it
  // happened to observe while leaving the choice open as "#458". Only the order changed here; the
  // O(D) property #482 exists to guard is untouched, since the projection loop is still sized by
  // decorations, not by the 8 markers on the wire.
  it("projects only decorated markers from a large marker set (#482)", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 50, x: 0, width: 1, bg: 0xaa0000 }); // registered FIRST
    reg.register({ markerId: 20, x: 1, width: 1, bg: 0x00aa00 }); // registered SECOND

    // 8 live markers on the wire in core marker order (id == absolute line); only 20 and 50 are
    // decorated. top == 0, so row == line == id.
    const lines = [10, 20, 30, 40, 50, 60, 70, 80].map((id) => ml(id, id));
    const rects = reg.decorationsForFrame(frameAbs({ rows: 100, scrollbackLen: 0 }, lines, []));

    // Registration order (50 before 20 — the reverse of core's marker order, which is the point),
    // and the 6 undecorated markers contribute nothing.
    expect(rects).toEqual([
      { row: 50, left: 0, right: 0, layer: "bottom", bg: 0xaa0000, fg: undefined },
      { row: 20, left: 1, right: 1, layer: "bottom", bg: 0x00aa00, fg: undefined },
    ]);
  });

  // #458: where two decorations on DIFFERENT markers cover the same cell, the winner is the one
  // registered LAST — xterm's documented contract (`typings/xterm.d.ts`: "When 2 decorations both
  // set the ... color the last registered decoration will be used"). Precedence is the consumer's
  // policy (ADR-0017), so it must follow the consumer's own input, not core's marker emission order
  // — which is decided by where the anchors happen to sit in the buffer and is unchangeable from
  // here. The renderer resolves per-property last-in-wire-order (#452), so "wins" == "emitted
  // later".
  it("resolves cross-marker precedence by registration order, not marker order (#458)", () => {
    const reg = new DecorationRegistry();
    // Marker 10 is ABOVE marker 20 in the buffer, so core emits 10 first. Register them the other
    // way round; the two decorations overlap on row 3, column 0.
    reg.register({ markerId: 20, x: 0, width: 1, bg: 0x0000aa }); // registered FIRST
    reg.register({ markerId: 10, x: 0, width: 1, height: 3, bg: 0xaa0000 }); // registered SECOND
    const rects = reg.decorationsForFrame(
      frameAbs({ rows: 10, scrollbackLen: 0 }, [ml(10, 1), ml(20, 3)], []),
    );

    const onTheSharedCell = rects.filter((r) => r.row === 3 && r.left <= 0 && r.right >= 0);
    expect(onTheSharedCell).toHaveLength(2);
    expect(onTheSharedCell.at(-1)!.bg).toBe(0xaa0000); // the LAST registered wins the cell
    // Whole-array order, so this cannot pass on a coincidence of the filter: every rect of the
    // first-registered decoration precedes every rect of the second.
    expect(rects.map((r) => [r.row, r.bg])).toEqual([
      [3, 0x0000aa],
      [1, 0xaa0000],
      [2, 0xaa0000],
      [3, 0xaa0000],
    ]);
  });

  // #458, the claim that outlives a single frame: the winner must not change when the BUFFER moves.
  // Upstream this is not guaranteed — xterm re-appends a decoration that spans an insert/delete
  // point, promoting it to "last" — so it is worth pinning that justerm's order is immune. Same two
  // decorations, two frames whose anchors have swapped relative position; the same one wins both.
  it("keeps the same winner when the anchors swap relative position between frames (#458)", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 0, width: 1, bg: 0x0000aa }); // registered FIRST
    reg.register({ markerId: 2, x: 0, width: 1, bg: 0xaa0000 }); // registered SECOND — must win

    // Frame A: marker 1 above marker 2. Frame B: the buffer moved and marker 2 is now above.
    const a = reg.decorationsForFrame(frameAbs({ rows: 10, scrollbackLen: 0 }, [ml(1, 2), ml(2, 5)]));
    const b = reg.decorationsForFrame(frameAbs({ rows: 10, scrollbackLen: 0 }, [ml(2, 1), ml(1, 6)]));

    // In both frames the last-emitted (= winning) rect is marker 2's, whichever anchor sits higher.
    expect(a.at(-1)!.bg).toBe(0xaa0000);
    expect(b.at(-1)!.bg).toBe(0xaa0000);
    // And the rows really did swap, so the frames genuinely differ (not two identical inputs).
    expect(a.map((r) => r.row)).toEqual([2, 5]);
    expect(b.map((r) => r.row)).toEqual([6, 1]);
  });

  // #458: `register` always mints a NEW decoration — it never updates one in place. That is what
  // makes "register again to take precedence" work at all, since `Set.add` of a member already
  // present is a no-op that does NOT move it to the end. The consequence the docs must not hide:
  // the earlier handle stays LIVE, so registering again leaves two decorations, and disposing the
  // new one hands the cell back to the old one rather than clearing it.
  it("registering the same options again adds a second live decoration, it does not replace (#458)", () => {
    const reg = new DecorationRegistry();
    const first = reg.register({ markerId: 1, x: 0, width: 1, bg: 0x0000aa });
    const second = reg.register({ markerId: 1, x: 0, width: 1, bg: 0xaa0000 });

    const both = reg.decorationsForFrame(frame(mk(1, 0)));
    expect(both.map((r) => r.bg)).toEqual([0x0000aa, 0xaa0000]); // both live; the newer wins
    expect(first.disposed).toBe(false); // …and the first was NOT replaced

    second.dispose();
    expect(reg.decorationsForFrame(frame(mk(1, 0))).map((r) => r.bg)).toEqual([0x0000aa]);
  });

  // #461: entirely above the viewport -> nothing, and no negative row reaches the u32 wire.
  it("emits no rect for a decoration whose span ends above the viewport top", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 0, width: 2, height: 2 }); // absolute 10..11

    expect(
      reg.decorationsForFrame(frameAbs({ rows: 10, scrollbackLen: 13 }, [ml(1, 10)], [])),
    ).toEqual([]);
  });

  // #462: WITHOUT viewport geometry (`rows`) the bottom clip has no viewport to clip to.
  // It used to fall back to +Infinity, so a multi-row `height` walked UNBOUNDED — a large
  // height looped up to ~1e9 times (hang / OOM) and wrote rows far past any viewport, which
  // the u32 wire (`decorationWire`) then wraps. With no `rows` we cannot show a row BELOW the
  // anchor, so the span caps to the anchor row: the loop is bounded (structurally 1 row, for
  // ANY height) and every emitted row is a finite, top-clamped integer. Production never takes
  // this path — `DecodedFrame.rows` is required — but the type seam (`DecorationFrame.rows?`)
  // and consumer/demo-built frames make it reachable, the same class #457 hardened for columns.
  it("bounds a multi-row height to the anchor row when the frame has no rows", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, height: 100_000 }); // huge height, no geometry to clip it

    // frame() carries no `rows` — the old +Infinity path. The loop must terminate at 1 row.
    const rects = reg.decorationsForFrame(frame(mk(1, 4)));
    expect(rects).toHaveLength(1);
    expect(rects[0]).toEqual({ row: 4, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined });
  });

  // #462: a non-finite anchor row (a consumer-supplied +Infinity marker) must emit nothing
  // rather than spin. `Math.max(0, Infinity)` is `Infinity`, and `Infinity <= Infinity` is
  // TRUE while `Infinity + 1` stays `Infinity`, so the row loop would never terminate. #461's
  // `Math.max(0, startRow)` clamp covers a NEGATIVE anchor; a non-finite one is the residue it
  // does not — a separate guard, not the same one.
  it("emits no rect for a non-finite anchor row (no unbounded loop)", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, height: 3 });

    expect(reg.decorationsForFrame(frame(mk(1, Number.POSITIVE_INFINITY)))).toEqual([]);
  });

  // #462 (2-lens): the anchor row has TWO entry points (#461) — the viewport-relative
  // `markerPositions` (above) and the ABSOLUTE `markerLines` (`startRow = line - top`). A
  // non-finite absolute line reaches the SAME `firstRow` guard, so it too emits nothing rather
  // than spinning. Pins the second entry point, not just the first.
  it("emits no rect for a non-finite ABSOLUTE marker line", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, height: 3 });

    expect(
      reg.decorationsForFrame(
        frameAbs({ rows: 10, scrollbackLen: 13 }, [ml(1, Number.POSITIVE_INFINITY)], []),
      ),
    ).toEqual([]);
  });

  // #462 (2-lens): the PRODUCTION path is safe from the u32 wrap by construction. An absolute
  // line at/above 2**32 makes `startRow` (= line - top) enormous, but with `rows` present (which
  // production always sends — `DecodedFrame.rows` is required) the bottom clip drops every row
  // past the viewport, so `firstRow > rows-1` yields NOTHING and no wrapped row ever reaches the
  // wire. The single-wrapped-row residual exists ONLY on the geometry-less seam (frame() with no
  // rows), the same "absent geometry we cannot clip" residual #457 accepted for columns.
  it("drops an absurd absolute line entirely when the frame carries rows", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 0, width: 1 });

    expect(
      reg.decorationsForFrame(frameAbs({ rows: 10, scrollbackLen: 0 }, [ml(1, 2 ** 32 + 5)], [])),
    ).toEqual([]);
  });

  // #457: a right-anchored span wider than the screen overflows the LEFT edge. It is
  // CLIPPED here, not passed through: the wire carries u32 columns, so a negative
  // `left` would wrap to ~4.29e9 and the renderer's `col >= left` would match NOTHING —
  // the decoration would vanish instead of being clipped. xterm has no stored span to
  // wrap (`forEachDecorationAtCell` tests `x >= xmin && x < xmax` per cell), so an
  // out-of-range span simply never matches there. NOTE the reference only covers the
  // LEFT-anchored form: xterm's colour path ignores `anchor` entirely (only its DOM
  // element honours it), so a right-anchored span is justerm's own design (#459) —
  // clipping it is consistent with that design, not a reproduction of xterm.
  it("clips a right-anchored overflow to the viewport", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, anchor: "right", x: 0, width: 25 }); // wider than 20 cols

    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 0)))).toEqual([
      { row: 0, left: 0, right: 19, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // #457 (2-lens): the clip must reject NON-FINITE columns too, or the invariant it
  // states is false. `Math.max(0, NaN)` is NaN and `right < NaN` is false, so a NaN
  // span slipped through the visibility check — and `Uint32Array[NaN]` is 0, so it
  // landed as a spurious paint on column 0. Verified numerically before fixing.
  it("emits no rect for a non-finite x or width", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: Number.NaN, width: 3 });
    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 0)))).toEqual([]);

    const inf = new DecorationRegistry();
    inf.register({ markerId: 1, x: Number.POSITIVE_INFINITY, width: 3 });
    expect(inf.decorationsForFrame(frameGeom(20, 10, mk(1, 0)))).toEqual([]);
  });

  // #457 (2-lens) the HIGH end, the mirror of the fixed low-end bug: a column at or
  // past 2**32 also wraps in the u32 wire (2**32 -> 0), so an absurd x would paint
  // column 0. Clipping `right` to the last visible column makes such a span empty.
  it("emits no rect for a span that starts past the last column", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 2 ** 32, width: 3 });
    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 0)))).toEqual([]);

    const offRight = new DecorationRegistry();
    offRight.register({ markerId: 1, x: 25, width: 3 }); // starts right of a 20-col view
    expect(offRight.decorationsForFrame(frameGeom(20, 10, mk(1, 0)))).toEqual([]);
  });

  // #457 the dangerous inverse: if the span is entirely off-screen its `right` is also
  // negative, and a wrapped `right` makes `col <= right` true for EVERY column — the
  // decoration would paint the whole row. Emit nothing instead.
  it("emits no rect for a span that is entirely off-screen to the left", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: -10, width: 5 }); // covers -10..-6, no visible cell

    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 0)))).toEqual([]);
  });

  // Defaults are unchanged (height 1, anchor left) — a plain decoration still
  // projects exactly one left-anchored rect (guards against the additive fields
  // shifting existing behaviour).
  it("defaults to height 1, anchor left (single left rect)", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 2, width: 3 });

    expect(reg.decorationsForFrame(frameGeom(20, 10, mk(1, 1)))).toEqual([
      { row: 1, left: 2, right: 4, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // Dispose is idempotent — a double dispose neither throws nor corrupts the store.
  it("dispose is idempotent", () => {
    const reg = new DecorationRegistry();
    const d = reg.register({ markerId: 4 });

    d.dispose();
    d.dispose();

    expect(reg.decorationsForFrame(frame(mk(4, 0)))).toEqual([]);
  });
});

describe("DecorationRegistry.rulerMarksForFrame (#120 S3)", () => {
  // A ruler mark sits at the marker's buffer-relative position: line / (scrollback
  // + rows). Here total = 90 + 10 = 100, line 25 → topRatio 0.25.
  it("places a ruler mark at the marker's buffer-relative position", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, overviewRulerOptions: { color: 0xff0000 } });

    expect(reg.rulerMarksForFrame(rulerFrame([7, 25], 90, 10))).toEqual([
      { topRatio: 0.25, color: 0xff0000, position: "full" },
    ]);
  });

  // #458: ruler marks are emitted in REGISTRATION order too — one rule for the file. The scrollbar
  // paints them in array order (`setMarks` appends one div per mark, no z-index), so two marks close
  // enough to overlap on the track resolve the same way a shared cell does: last registered on top.
  //
  // The fixture INTERLEAVES registration across two markers, and it has to. The old loop iterated
  // `byMarker`, whose Map key order is when each MARKER was first registered — so with one
  // decoration per marker the old and new orders coincide and a two-decoration fixture pins
  // nothing. With A→7, B→25, A'→7 the two rules genuinely differ: grouping by marker yields
  // A, A', B; registration order yields A, B, A'.
  it("emits ruler marks in registration order, not grouped by marker (#458)", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, overviewRulerOptions: { color: 0xaa0000 } }); // A
    reg.register({ markerId: 25, overviewRulerOptions: { color: 0x00aa00 } }); // B — other marker
    reg.register({ markerId: 7, overviewRulerOptions: { color: 0x0000aa } }); // A' — back to 7

    // total = 90 + 10 = 100 → line 10 is 0.1, line 50 is 0.5.
    expect(reg.rulerMarksForFrame(rulerFrame([7, 10, 25, 50], 90, 10))).toEqual([
      { topRatio: 0.1, color: 0xaa0000, position: "full" },
      { topRatio: 0.5, color: 0x00aa00, position: "full" },
      { topRatio: 0.1, color: 0x0000aa, position: "full" },
    ]);
  });

  // The whole point of the ruler: an OFF-viewport marker (absent from the viewport
  // marker group, present in the all-marker `markerLines`) still gets a mark, so a
  // user sees anchors they'd have to scroll to reach.
  it("shows a mark for a marker off the current viewport", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 3, overviewRulerOptions: { color: 0x00ff00 } });

    // marker at buffer line 4, viewport far below — markerLines still carries it.
    expect(reg.rulerMarksForFrame(rulerFrame([3, 4], 196, 4))).toEqual([
      { topRatio: 0.02, color: 0x00ff00, position: "full" },
    ]);
  });

  // A decoration with no overviewRulerOptions is a cell-only decoration (#198) —
  // it never contributes a ruler mark.
  it("ignores decorations without overviewRulerOptions", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, bg: 0x111111 }); // cell decoration, no ruler

    expect(reg.rulerMarksForFrame(rulerFrame([7, 25], 90, 10))).toEqual([]);
  });

  // A ruler decoration whose marker isn't in `markerLines` (disposed, or a stale
  // id) yields no mark — the join is inner.
  it("ignores a ruler decoration whose marker is absent from markerLines", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 9, overviewRulerOptions: { color: 0xff0000 } });

    expect(reg.rulerMarksForFrame(rulerFrame([1, 5], 90, 10))).toEqual([]);
  });

  // The position option rides through (default "full"); an explicit position is
  // honoured so a consumer can put marks in a gutter column.
  it("honours the position option", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 2, overviewRulerOptions: { color: 0x0000ff, position: "right" } });

    expect(reg.rulerMarksForFrame(rulerFrame([2, 50], 90, 10))).toEqual([
      { topRatio: 0.5, color: 0x0000ff, position: "right" },
    ]);
  });

  // No content (total 0) → no marks, no divide-by-zero.
  it("yields no marks when there is no content", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, overviewRulerOptions: { color: 0xff0000 } });

    expect(reg.rulerMarksForFrame(rulerFrame([1, 0], 0, 0))).toEqual([]);
  });

  // #463: a non-finite `scrollbackLen` must yield NO mark, not invalid CSS. The `total <= 0`
  // guard is a size comparison, and `NaN <= 0` is FALSE, so a NaN total slipped straight
  // through to `topRatio = line / NaN = NaN`, which `scrollbar.ts` writes as `top: NaN%` — an
  // invalid rule the browser drops, silently stacking the mark at the track default. Rejecting
  // non-finite `total` (not a comparison) is the fix; `Number.isFinite` is exactly the check the
  // `NaN <= 0` slip needs.
  it("yields no marks when scrollbackLen is non-finite", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, overviewRulerOptions: { color: 0xff0000 } });

    // NaN slips the old `total <= 0` size guard (NaN <= 0 is false). +Infinity is the case ONLY
    // the total guard catches: `line / Infinity` is `0` — finite — so the per-line finite guard
    // and the clamp both pass it through as a mark at the track top; only rejecting a non-finite
    // `total` drops it. Both must yield no mark.
    expect(reg.rulerMarksForFrame(rulerFrame([7, 25], Number.NaN, 10))).toEqual([]);
    expect(reg.rulerMarksForFrame(rulerFrame([7, 25], Number.POSITIVE_INFINITY, 10))).toEqual([]);
  });

  // #463: a non-finite marker LINE (a consumer-built markerLines carrying Infinity/NaN) makes
  // `topRatio` non-finite even when `total` is fine — `Infinity / 100` is `Infinity`, written as
  // `top: Infinity%`. The mark has no placeable position, so it is skipped (clamping cannot
  // rescue it: `Math.max(0, NaN)` is `NaN`, so a clamp alone still emits invalid CSS).
  it("yields no mark for a non-finite marker line", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, overviewRulerOptions: { color: 0xff0000 } });

    expect(reg.rulerMarksForFrame(rulerFrame([7, Number.POSITIVE_INFINITY], 90, 10))).toEqual([]);
  });

  // #463: a marker line PAST the content end (`scrollbackLen + rows`) — a frame lag/mismatch
  // between the absolute markerLines and the scroll geometry — gives `topRatio > 1`, placing the
  // mark below the track where it is invisible. Clamp it to the track bottom (1) so it stays
  // visible at the nearest valid edge.
  it("clamps a mark past the content end to the track bottom", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, overviewRulerOptions: { color: 0xff0000 } });

    // total = 100, line = 150 → raw ratio 1.5, clamped to 1.
    expect(reg.rulerMarksForFrame(rulerFrame([7, 150], 90, 10))).toEqual([
      { topRatio: 1, color: 0xff0000, position: "full" },
    ]);
  });

  // #463: the mirror — a negative absolute line gives `topRatio < 0` (above the track); clamp to
  // the top (0). Both directions of the clamp are pinned so a one-sided fix cannot pass.
  it("clamps a negative marker line to the track top", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, overviewRulerOptions: { color: 0xff0000 } });

    expect(reg.rulerMarksForFrame(rulerFrame([7, -20], 90, 10))).toEqual([
      { topRatio: 0, color: 0xff0000, position: "full" },
    ]);
  });

  // The ruler is a scrollback navigator, so it's suppressed on the alt screen
  // (xterm hides its ruler canvas on the alt buffer) — even when markerLines and
  // content are present.
  it("yields no marks on the alt screen", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, overviewRulerOptions: { color: 0xff0000 } });

    expect(
      reg.rulerMarksForFrame({ markerLines: [7, 25], scrollbackLen: 90, rows: 10, altScreen: true }),
    ).toEqual([]);
  });

  // Completeness pass (lens 1): the registry does NOT validate x/width (mirroring
  // xterm, which only defaults them) — a width of 0 is invisible, not a crash. Since
  // #457 that invisibility is realised by emitting NO rect rather than a degenerate
  // one (right < left): the same visible result, but the wire now carries only spans
  // with at least one visible cell, which is what keeps a negative column from ever
  // reaching a u32 lane. xterm agrees — its per-cell test `x >= xmin && x < xmax` is
  // false everywhere when width is 0.
  it("emits no rect for a degenerate (zero-width) span", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: 3, width: 0 });

    expect(reg.decorationsForFrame(frame(mk(1, 2)))).toEqual([]);
  });

  // #457: a negative x is clipped for the same reason as the right-anchored overflow —
  // the u32 wire cannot carry a negative column, so the partly-visible span must be
  // clipped to its visible part here rather than "left to the renderer" (which would
  // receive a wrapped value and paint nothing).
  it("clips a negative x to the first visible column", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 1, x: -2, width: 3 });

    expect(reg.decorationsForFrame(frame(mk(1, 0)))).toEqual([
      { row: 0, left: 0, right: 0, layer: "bottom", bg: undefined, fg: undefined },
    ]);
  });

  // Completeness pass (lens 1 + command-announce parity): disposal is purely
  // event-driven with NO permanent reject-set, so a marker id reissued by a full
  // reset (RIS) can be registered against afresh and projects normally. Mirrors
  // `CommandAnnounceController` pruning `seen` on disposal so a reused id re-fires.
  it("allows registering a new decoration on a reused marker id after disposal", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 5, x: 0 });
    reg.onMarkerDisposed(5); // marker 5 disposed; its decoration gone

    const fresh = reg.register({ markerId: 5, x: 4, layer: "top" }); // id reissued

    expect(fresh.disposed).toBe(false);
    expect(reg.decorationsForFrame(frame(mk(5, 7)))).toEqual([
      { row: 7, left: 4, right: 4, layer: "top", bg: undefined, fg: undefined },
    ]);
  });
});

// #189 (S3): the user-visible payoff of the per-buffer marker refactor (#186/#187).
// core now carries alt-scoped markers on the alt frame's `marker_positions` and
// disposes them on alt-leave; the registry is buffer-agnostic (it reads
// `markerPositions` with NO alt gate — unlike `rulerMarksForFrame`, which suppresses
// on alt), so those alt markers' decorations already project on the alt screen's
// visible rows. There is no new registry logic in this slice — these tests LOCK the
// existing correct behavior against a future alt-gate regression, mirroring core's
// `alt_markers.rs` on the web side.
describe("DecorationRegistry — alt-screen decorations (#189)", () => {
  // #461 (2-lens GAP): every test below sends `markerPositions` only, so they exercise the
  // viewport-relative half of the join — but PRODUCTION alt frames also carry `markerLines`
  // and take the absolute half. This locks #189 on the path that actually runs.
  //
  // No bleed is possible there either, and for a stronger reason than a gate: core's
  // `markers()` returns `alt_markers` when `on_alt`, so a primary decoration's marker is not
  // in an alt frame's `markerLines` AT ALL (term.rs). On alt, `set_display_offset` early-returns
  // so `displayOffset === 0`, while `scrollbackLen` still reports the primary's length — so an
  // alt marker's absolute line is `scrollbackLen + row` and `line - top` is exactly `row`.
  // VALIDITY: this holds while `markers()` stays per-buffer; a change there reopens it.
  it("projects an alt-scoped decoration through the ABSOLUTE path, with no primary bleed", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 77, x: 2, width: 1, bg: 0x00ff00 }); // alt-scoped marker
    reg.register({ markerId: 1, x: 0, width: 1 }); // a PRIMARY marker, absent from an alt frame

    // scrollbackLen is the primary's length; displayOffset is 0 on alt; the alt marker at
    // viewport row 3 therefore has absolute line 13 + 3.
    // Built as a variable, not an inline literal: `altScreen` is deliberately NOT part of
    // `DecorationFrame` — decorations are not suppressed on alt (that is #189's point, unlike
    // `rulerMarksForFrame`) — so an inline literal would trip the excess-property check. It is
    // set here only so the scenario reads as the alt buffer.
    const altAbsFrame = {
      cols: 80,
      rows: 10,
      scrollbackLen: 13,
      displayOffset: 0,
      markerLines: [77, 16],
      markerPositions: [],
      altScreen: true,
    };
    const rects = reg.decorationsForFrame(altAbsFrame);

    expect(rects).toEqual([
      { row: 3, left: 2, right: 2, layer: "bottom", bg: 0x00ff00, fg: undefined },
    ]);
  });

  /** An alt frame: alt-scoped marker records plus `altScreen: true`. The registry
   * does NOT read `altScreen` (that's the point — decorations are not suppressed on
   * alt); it's set so the scenario reads as the alt buffer. */
  const altFrame = (...records: number[][]) => ({
    markerPositions: records.flat(),
    altScreen: true,
  });
  /** An alt frame WITH viewport geometry — real alt frames carry `cols`/`rows`, so
   * pin the geometry-dependent paths (multi-row `height` clip, right-anchor columns)
   * on the alt buffer too, not just via the primary `frameGeom` tests. */
  const altFrameGeom = (cols: number, rows: number, ...records: number[][]) => ({
    cols,
    rows,
    markerPositions: records.flat(),
    altScreen: true,
  });

  // Alt-frame markers drive decoration rendering on the alt screen's visible rows —
  // highlighting a line inside a full-screen TUI (vim/htop). The alt screen has no
  // scrollback, so an alt marker is always on-viewport when carried.
  it("projects a decoration whose marker rides an alt frame (no alt suppression)", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 42, x: 0, width: 4, bg: 0x008f00 });

    expect(reg.decorationsForFrame(altFrame(mk(42, 3)))).toEqual([
      { row: 3, left: 0, right: 3, layer: "bottom", bg: 0x008f00, fg: undefined },
    ]);
  });

  // No cross-buffer bleed: core omits primary markers from the alt frame (and marker
  // ids are engine-global — a single `next_marker_id` counter — so a primary id never
  // collides with an alt id). A decoration on a primary-only marker, absent from the
  // alt frame's `markerPositions`, yields nothing on alt; it reappears on the primary
  // frame that carries it. The join is purely marker-id, so isolation is structural.
  it("does not bleed a primary-only decoration onto the alt screen", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 100, x: 0, width: 4, bg: 0x008f00 }); // primary anchor

    // On alt, only the alt marker (7) is carried; 100 is absent → no rect for it.
    expect(reg.decorationsForFrame(altFrame(mk(7, 1)))).toEqual([]);

    // Back on a primary frame carrying marker 100, the primary decoration projects.
    expect(reg.decorationsForFrame(frame(mk(100, 2)))).toEqual([
      { row: 2, left: 0, right: 3, layer: "bottom", bg: 0x008f00, fg: undefined },
    ]);
  });

  // Clear on alt-leave: core disposes the alt-scoped marker on `?1049l` and fires
  // `MarkerDisposed`, which the consumer forwards to `onMarkerDisposed`. The alt
  // decoration then stops projecting (even if its id is later reused), while a
  // primary decoration on a still-live marker is untouched — no cross-buffer teardown.
  it("clears alt decorations on alt-leave (MarkerDisposed) without touching primary", () => {
    const reg = new DecorationRegistry();
    const primary = reg.register({ markerId: 100, bg: 0x001122 });
    const alt = reg.register({ markerId: 7, bg: 0x008f00 });

    reg.onMarkerDisposed(7); // alt-leave disposes the alt-scoped marker

    expect(alt.disposed).toBe(true);
    expect(primary.disposed).toBe(false);
    // The alt id reappearing (id reuse) yields nothing; the primary still projects.
    expect(reg.decorationsForFrame(frame(mk(7, 0), mk(100, 2)))).toEqual([
      { row: 2, left: 0, right: 0, layer: "bottom", bg: 0x001122, fg: undefined },
    ]);
  });

  // Geometry on alt (2-lens completeness, sibling lens): a multi-row `height`
  // decoration clips to the alt viewport bottom using the alt frame's `rows`. The
  // alt screen has no scrollback, so nothing exists below the viewport to spill onto
  // — the demo paints exactly this (a height-3 highlight inside the alt buffer).
  it("clips a multi-row (height) decoration at the alt viewport bottom", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, x: 0, width: 2, height: 4, bg: 0x008f00 });

    // Alt viewport = 5 rows (0..4); the marker sits at row 3, so a height-4 span
    // (rows 3..6) clips to the two on-screen rows 3 and 4.
    expect(reg.decorationsForFrame(altFrameGeom(6, 5, mk(7, 3)))).toEqual([
      { row: 3, left: 0, right: 1, layer: "bottom", bg: 0x008f00, fg: undefined },
      { row: 4, left: 0, right: 1, layer: "bottom", bg: 0x008f00, fg: undefined },
    ]);
  });

  // Geometry on alt (2-lens completeness, sibling lens): a right-anchored span is
  // measured from the alt frame's `cols`, exactly as on the primary — the column
  // math has no buffer-specific path, but pin it on alt so a future regression in
  // alt `cols` handling can't slip past.
  it("right-anchors columns against the alt frame's cols", () => {
    const reg = new DecorationRegistry();
    reg.register({ markerId: 7, x: 0, width: 3, anchor: "right", bg: 0x008f00 });

    // cols=10, right anchor x=0 width=3 → [10-0-3, 10-1-0] = [7, 9].
    expect(reg.decorationsForFrame(altFrameGeom(10, 5, mk(7, 2)))).toEqual([
      { row: 2, left: 7, right: 9, layer: "bottom", bg: 0x008f00, fg: undefined },
    ]);
  });
});
