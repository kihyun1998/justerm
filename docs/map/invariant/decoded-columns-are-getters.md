# Invariant — a decoded frame's columns are getters over borrowed memory

## The fact

`DecodedFrame`'s columns look like properties and are methods. wasm-bindgen compiles every
`#[wasm_bindgen(getter)]` into a JS accessor (`justerm-wasm-decode/src/lib.rs — DecodedFrame`), so
each read builds a new object:

- a **cell column** (`codepoints` / `fg` / `bg` / `flags` / `extra` / `link` / `spans` / …) returns a
  fresh `Uint32Array` / `Uint16Array` view over the same `ArrayBuffer` and `byteOffset` — no copy, but
  two reads are two objects and `a === b` is false.
- a **string table** (`sideTable` / `linkTable`) rebuilds the whole `string[]` with fresh strings on
  every read.

Two rules, distinguished by the **lifetime** of the reference, not the number of reads:

1. **Read a column into a local before the loop, never inside it.**
2. **A column kept past the current frame must be copied, never held as the view.** The getter's doc
   (`DecodedFrame::codepoints`, "applies to every column + `spans`") says the view is invalidated if
   WASM memory grows, e.g. on the next `decodeFrame`.

Measured on a real decoded frame (#657): 10 000 reads of `frame.sideTable[0]` cost 10.3 ms against
0.061 ms through a local (~170×) on a single-entry table; the gap grows with the table. A held view
survived 20 000 decodes of the same frame (the allocator reuses the space) and detached after one
decode of a 300x220 frame, or after 109 small ones held open at once — so it arrives with a bigger
frame, i.e. a viewport resize, and passing the detached array on throws (`TypeError: … on a detached or
out-of-bounds ArrayBuffer`).

Three consequences easy to state wrong:

- **The identity fast path works through a single read.** `justerm-web/src/renderer-wire.ts — asU32`
  returns its argument untouched when the width matches, which makes the seam zero-copy (#627). A test
  writing `expect(asU32(frame.extra)).toBe(frame.extra)` reads the getter twice and fails against
  correct code; the identity that matters is between what a reader received and what it forwards.
- **A retained copy is cheap where needed.** `renderer-wire.ts — retainU32` is always a copy; overlay
  spans are `(row, left, right)` triples for highlighted rows only, copied once per frame.
- **The coercion does not validate** (#467). `asU32` / `asU16` pass a real typed array through and
  convert a plain one (a fixture such as `demo/fake-search.ts`) by reinterpreting: a negative wraps to
  two's complement, `NaN` / ±`Infinity` become `0`, past-range wraps mod 2³² (2¹⁶) — pinned in
  `justerm-web/test/justerm-renderer.test.ts` ("asU32 span coercion (#467)"), the class of the #457
  decoration wire. So a span source (`selectionSpans` / `matchSpans` / `activeMatchSpans`) clips itself,
  as `decorationsForFrame` and the demo's producers do. `asU16` feeds `flags` only; `extra` widened to
  `u32` at #621 / #627.

## Why it is cross-cutting

Every consumer of a decoded frame is subject to it, and nothing in the type system says so:
`justerm-web/src/types.ts` declares each column `ArrayLike<number>`, deliberately, so a plain object
satisfies it — and a plain object's property read is free and returns the same array. The in-repo test
corpus is written against the shape where the invariant cannot be violated; production runs on the
shape where it can. The readers (`cell-mirror.ts`, `justerm-renderer.ts`, `links.ts`, `overlay.ts`,
`markers.ts`) never call each other, and each gets it right or wrong on its own.

## Territories it holds in

- [wasm decoder](../territory/wasm-decoder.md) — the producer: `justerm-wasm-decode/src/lib.rs —
  DecodedFrame` getters and their lifetime doc. Added here (new territory).
- [published surface](../territory/published-surface.md) — the seam; #646 gated its types, and this is
  a value-level fact that type gating cannot see.
- [frame](../territory/frame.md) — the columns are the frame's payload; the getter shape is how a
  consumer meets them.
- [grapheme clusters](../territory/grapheme-clusters.md) — `sideTable`, the worst case, the only column
  read behind a per-cell condition.
- [hyperlinks](../territory/hyperlinks.md) — `link` / `linkTable`; `justerm-web/src/links.ts`
  destructures them once (`const { spans, link, linkTable } = frame`).
- [accessibility](../territory/accessibility.md) — `justerm-web/src/cell-mirror.ts` feeds it and is
  where rule 1 was violated; at HEAD 7bb31f7 it destructures every column once
  (`const { spans, flags, extra, codepoints, sideTable, link, linkTable } = frame`).

## What a violation looks like

**Rule 1 — nothing.** No wrong pixel, text or error; a pure allocation cost scaling with the viewport (a
per-cell read over 200×50 is 10 000 view objects per frame per column; a `sideTable` read per cluster
cell rebuilds the table each time). It survives review and tests because every fixture is a plain
object.

**Rule 2 — a `TypeError` out of an event handler, long after the cause.** The retained view detaches
when a later frame grows WASM memory, and the throw lands where the reference is next used — for overlay
spans, a focus flip, unconnected in time or code to the resize that caused it. A plain-object fixture
cannot exhibit it.

## Discovery history

| Event | Site | Issue |
|---|---|---|
| Found by the first test to drive the adapter with a real decoded frame | `cell-mirror.ts` read `flags`, `extra`, `codepoints` per cell and `sideTable` per cluster cell, while destructuring `spans` once three lines above | #657 |
| Rule 2, one probe later | `justerm-renderer.ts` retained the three overlay span columns as views (`lastSelectionSpans` and siblings) and re-read them from `issueOverlay`, which runs on a focus flip with no new frame — a click away from a terminal with a live selection would throw once the viewport had grown | #657 |

Swept at the same time: `links.ts` destructures; `overlay.ts` reads each column once and passes it to
`readOverlay`, and `markers.ts` decodes `frame.markerPositions` once — taking the column as a parameter
makes the mistake unavailable. `justerm-renderer.ts` was clean under rule 1 and the violator of rule 2,
which is the evidence the two rules are worth stating separately.

## Where it will recur

Any new reader that walks cells, and any new field that keeps one. If a loop body mentions `frame.`,
move the read above the loop. If a column is assigned to anything that outlives the call — a field, a
closure, a queue — copy it with `retainU32`. Not guarded: that the three retention sites
(`lastSelectionSpans`, `lastMatchSpans`, `lastActiveMatchSpans` in `JustermRenderer`) keep calling it,
because `JustermRenderer` is constructible only through `create()` (canvas plus dynamic wasm imports)
and no test reaches it behind a fake. The naming is the guard. A new getter added to `DecodedFrame` in
`justerm-wasm-decode` inherits both rules.
