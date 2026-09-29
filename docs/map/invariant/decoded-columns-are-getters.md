# Cross-cutting invariant — a decoded frame's columns are getters over borrowed memory

## The fact

`DecodedFrame`'s columns look like properties and are **methods**. wasm-bindgen compiles every
`#[wasm_bindgen(getter)]` into a JS accessor, so each read *builds a new object*:

- a **cell column** (`codepoints` / `fg` / `bg` / `flags` / `extra` / `link` / `spans` / …) returns a
  fresh `Uint32Array`/`Uint16Array` **view** — same `ArrayBuffer`, same `byteOffset`, new wrapper.
  No data is copied, but two reads are two objects and `a === b` is false.
- a **string table** (`sideTable` / `linkTable`) is worse: it rebuilds the entire `string[]`, with
  fresh JS strings, on every read.

So the rule is one line: **read a column into a local before the loop, never inside it.**

And a second, which is the same fact seen from the other end: **a column you keep past the current
frame must be copied, never held as the view.** The two look contradictory — one says stop
re-reading, the other says stop holding — and they are not: a column that is forwarded and forgotten
wants the zero-copy view, a column that outlives its frame cannot have one. The test is the
*lifetime* of the reference, not the number of reads.

Measured on a real decoded frame (#657): 10 000 reads of `frame.sideTable[0]` cost **10.3 ms**
against **0.061 ms** through a local — ~170×, on a table with a *single* entry. The gap grows with
the table, because the cost is the rebuild rather than the index.

Three consequences that are easy to state wrong:

1. **The allocation cost and the lifetime contract are different facts, and both bite.** The decoder
   documents columns as views into WASM memory, invalidated when that memory grows
   (`justerm_wasm_decode.d.ts:42-47`). Per-read allocation is what the first rule is about; that
   invalidation is what the second is about. Measured (#657): a held view survives 20 000 decodes of
   the *same* frame — the allocator reuses the space and memory never grows — and detaches after
   **one** decode of a 300x220 frame, or after 109 small ones held open at once. So the failure does
   not creep in gradually; it arrives the moment a bigger frame does, which for a terminal means a
   viewport resize. Passing the detached array on **throws** (`TypeError: … on a detached or
   out-of-bounds ArrayBuffer`) rather than degrading.
   A retained copy is cheap where it is needed: overlay spans are `(row, left, right)` triples for the
   highlighted rows only, copied once per frame (`retainU32`).
2. **The identity fast path still works — measured through a single read.** `asU32` returns its
   argument untouched when the width already matches, which is what makes the seam zero-copy at all
   (#627). A test that writes `expect(asU32(frame.extra)).toBe(frame.extra)` reads the getter twice
   and fails against code that is doing exactly the right thing. The identity that matters is
   between what a reader received and what it forwards.
3. **The coercion does not validate** (#467). `asU32` / `asU16` pass a real typed array through by
   reference and convert a plain one — a test or demo fixture such as `demo/fake-search.ts`. The
   conversion **reinterprets** an out-of-range value rather than rejecting it: a negative wraps to its
   two's complement, `NaN` / ±`Infinity` land as `0`, and a value past the type's range wraps mod
   2³² (or 2¹⁶) — pinned in the renderer test, the same class as the #457 decoration wire. So a span
   source feeding it (`selectionSpans` / `matchSpans` / `activeMatchSpans`) must clip to a valid
   range itself, as `decorationsForFrame` and the demo's span producers do: the coercion knows nothing
   of a value's meaning or geometry, and a per-frame coercion is the wrong layer to validate at. `asU16`
   feeds `flags` only; `extra` widened to `u32` at #621/#627. The wraps are pinned in
   `justerm-web/test/justerm-renderer.test.ts` ("asU32 span coercion (#467)").

## Why it is cross-cutting

**Every consumer of a decoded frame is subject to it, and nothing in the type system says so.**
`src/types.ts` declares each column `ArrayLike<number>` — deliberately, so a plain object satisfies
it — and a plain object's property read is free and returns the same array every time. So the entire
in-repo test corpus is written against a shape where this invariant cannot be violated, while
production runs on the shape where it can.

That is what makes it a *cross-cutting* fact rather than a note on one module: the readers are
independent, they never call each other, and each one gets it right or wrong on its own.

## Territories it holds in

- [published surface](../territory/published-surface.md) — the seam this rides on. #646 gated its
  *types*; this is one of the value-level facts that gate structurally cannot see
- [frame](../territory/frame.md) — the columns are the frame's payload, and the getter shape is how
  a consumer meets them
- [grapheme clusters](../territory/grapheme-clusters.md) — `sideTable` is the worst case, and the
  only column read behind a per-cell condition rather than per cell
- [hyperlinks](../territory/hyperlinks.md) — `link` / `linkTable`, the same pair one feature over
- [accessibility](../territory/accessibility.md) — the cell mirror feeds it, and the mirror is where
  the violation was found

## What a violation looks like

Two symptoms, one per rule, and they could not be less alike.

**Rule 1 — nothing.** No wrong pixel, no wrong text, no error; the output is identical. It is a pure
allocation cost that scales with the viewport: a per-cell read over a 200×50 grid is 10 000 view
objects per frame per column, at frame cadence, and a `sideTable` read per cluster cell rebuilds the
whole table each time.

Which is exactly why it survives review and testing: every fixture in the repo is a plain object,
where the same code is free.

**Rule 2 — a `TypeError` out of an event handler, long after the cause.** The retained view detaches
when some later frame grows WASM memory, and the throw lands wherever the reference is next
*used* — for the overlay spans that is a focus flip, which has no connection in time or in code to
the resize that caused it. A plain-object fixture cannot exhibit this one either: it has no backing
memory to invalidate.

## Discovery history

| Event | Site | Issue |
|---|---|---|
| Found by the first test to drive the adapter with a real decoded frame | `src/cell-mirror.ts` read `flags`, `extra` and `codepoints` per cell, and `sideTable` per cluster cell — while destructuring `spans` once, three lines above | #657 |
| The second rule found the same way, one probe later | `src/justerm-renderer.ts` retained the three overlay span columns as views (`lastSelectionSpans` and siblings) and re-read them from `issueOverlay`, which by design runs on a **focus flip with no new frame** — so a click away from a terminal with a live selection would throw, once the viewport had grown at any earlier point | #657 |

The tell is in that last clause: the same function already had the correct pattern for `spans` and
the wrong one for everything else, three lines apart. Nobody was careless — with a plain-object
fixture the two are indistinguishable.

Swept at the same time: `src/links.ts` destructures (`const { spans, link, linkTable } = frame`) and
`src/markers.ts` / `src/overlay.ts` take the column as a *parameter* so the caller reads it — the
sturdiest of the shapes, because it makes the mistake unavailable. `src/justerm-renderer.ts` was
clean under rule 1 and was the violator of rule 2, on the `apply_damage` path and the retention
fields respectively, which is the clearest evidence that the two rules are worth stating separately:
one file obeyed the one anybody would think to check.

## Where it will recur

Any new reader that walks cells, and any new field that keeps one. Two tests, one per rule: if a
loop body mentions `frame.`, move the read above the loop rather than memoising it; and if a column
is assigned to anything that outlives the call — a field, a closure, a queue — copy it. In this
repo the second is spelled `retainU32` next to `asU32`, so the choice is visible at the assignment;
what is **not** guarded is that the three retention sites keep calling it, because
`JustermRenderer` is only constructible through `create()` (canvas plus dynamic wasm imports) and no
test can reach it behind a fake. The naming is the guard, which holds only as long as someone reads
it. Taking the column as a function
parameter avoids the question entirely, which is why the two modules that do have never had to think
about it.
