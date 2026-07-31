# Cross-cutting invariant — a decoded frame's columns are getters, so read each one once

## The fact

`DecodedFrame`'s columns look like properties and are **methods**. wasm-bindgen compiles every
`#[wasm_bindgen(getter)]` into a JS accessor, so each read *builds a new object*:

- a **cell column** (`codepoints` / `fg` / `bg` / `flags` / `extra` / `link` / `spans` / …) returns a
  fresh `Uint32Array`/`Uint16Array` **view** — same `ArrayBuffer`, same `byteOffset`, new wrapper.
  No data is copied, but two reads are two objects and `a === b` is false.
- a **string table** (`sideTable` / `linkTable`) is worse: it rebuilds the entire `string[]`, with
  fresh JS strings, on every read.

So the rule is one line: **read a column into a local before the loop, never inside it.**

Measured on a real decoded frame (#657): 10 000 reads of `frame.sideTable[0]` cost **10.3 ms**
against **0.061 ms** through a local — ~170×, on a table with a *single* entry. The gap grows with
the table, because the cost is the rebuild rather than the index.

Two consequences that are easy to state wrong:

1. **This is not the zero-copy contract, and it does not weaken it.** The decoder documents columns
   as views into WASM memory, invalidated when that memory grows (`justerm_wasm_decode.d.ts:42-47`).
   That is about *lifetime* across a second `decodeFrame`. This is about *allocation* per read, and
   both are true at once: one read hands you a live view that is cheap to hold and expensive to
   re-request.
2. **The identity fast path still works — measured through a single read.** `asU32` returns its
   argument untouched when the width already matches, which is what makes the seam zero-copy at all
   (#627). A test that writes `expect(asU32(frame.extra)).toBe(frame.extra)` reads the getter twice
   and fails against code that is doing exactly the right thing. The identity that matters is
   between what a reader received and what it forwards.

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

**Nothing.** No wrong pixel, no wrong text, no error — the output is identical. It is a pure
allocation cost that scales with the viewport: a per-cell read over a 200×50 grid is 10 000 view
objects per frame per column, at frame cadence, and a `sideTable` read per cluster cell rebuilds the
whole table each time.

Which is exactly why it survives review and testing: every fixture in the repo is a plain object,
where the same code is free.

## Discovery history

| Event | Site | Issue |
|---|---|---|
| Found by the first test to drive the adapter with a real decoded frame | `src/cell-mirror.ts` read `flags`, `extra` and `codepoints` per cell, and `sideTable` per cluster cell — while destructuring `spans` once, three lines above | #657 |

The tell is in that last clause: the same function already had the correct pattern for `spans` and
the wrong one for everything else, three lines apart. Nobody was careless — with a plain-object
fixture the two are indistinguishable.

Swept at the same time, and clean: `src/links.ts` destructures (`const { spans, link, linkTable } =
frame`), `src/justerm-renderer.ts` reads each column once on the `apply_damage` path, and
`src/markers.ts` / `src/overlay.ts` take the column as a *parameter* so the caller reads it. The
parameter shape is the sturdiest of the three, because it makes the mistake unavailable.

## Where it will recur

Any new reader that walks cells. Test: if a loop body mentions `frame.`, it is subject to this — and
the fix is to move the read above the loop, not to memoise it. Taking the column as a function
parameter avoids the question entirely, which is why the two modules that do have never had to think
about it.
