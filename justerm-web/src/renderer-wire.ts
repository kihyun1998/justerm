import type { DecodedFrame } from "./types";

/** Assemble the flat `apply_damage` header from a decoded frame. Pure (no backend), so the
 * wire assembly — scroll presence, the negative `scrollCount` that rides a `u32` slot as
 * two's complement, the blink flag — is unit-testable. `blinkOn` gates SGR-blink cells (#282):
 * the adapter passes {@link TextBlink}'s current phase (#576), and the `true` default keeps a
 * caller that has no phase — a test, a hand-built fixture — showing blinking text rather than
 * hiding it. */
export function damageHeader(frame: DecodedFrame, blinkOn = true): Uint32Array {
  const hasScroll =
    frame.scrollTop !== undefined &&
    frame.scrollBottom !== undefined &&
    frame.scrollCount !== undefined &&
    frame.scrollCount !== 0;
  const h = new Uint32Array(8);
  h[0] = frame.cols;
  h[1] = frame.rows;
  h[2] = frame.kind;
  h[3] = hasScroll ? 1 : 0;
  h[4] = frame.scrollTop ?? 0;
  h[5] = frame.scrollBottom ?? 0;
  h[6] = frame.scrollCount ?? 0; // a negative shift wraps to u32; the renderer reads it `as i32 as i16`.
  h[7] = blinkOn ? 1 : 0;
  return h;
}

/**
 * The header for a **phase-only** re-issue: an empty damage that carries nothing but the new SGR-5
 * blink phase (#576).
 *
 * The renderer takes `blink_on` in the damage header and keeps it (`webgl.rs` `last_blink_on`), so
 * this is how a consumer flips the phase between frames — scatter no cells, re-pack the retained
 * grid at the new phase. No renderer or wire change is needed for text blink, which is why the
 * whole feature lands in the widget.
 *
 * `kind` is **Partial**, and that is the load-bearing value: a Full header wipes the grid *before*
 * scattering, and this damage scatters nothing, so a Full flip would blank the terminal instead of
 * re-drawing it. `cols`/`rows` must be the grid the renderer currently holds — a mismatch makes it
 * allocate a fresh (empty) grid, with the same result.
 */
export function blinkPhaseHeader(cols: number, rows: number, blinkOn: boolean): Uint32Array {
  const h = new Uint32Array(8);
  h[0] = cols;
  h[1] = rows;
  h[2] = 1; // Partial — never Full; see above
  h[7] = blinkOn ? 1 : 0;
  return h;
}

/** Whether any cell in a frame's flag column carries `blinkBit` (#576). Pure, so the gate on the
 * phase re-pack is unit-testable without a backend — and separate from the frame it came from, so
 * a `number[]` fixture and the decoder's `Uint16Array` are the same code path. */
export function carriesBlink(flags: ArrayLike<number>, blinkBit: number): boolean {
  for (let i = 0; i < flags.length; i++) {
    if (((flags[i] ?? 0) & blinkBit) !== 0) return true;
  }
  return false;
}

/** Coerce a decoder array to the exact typed array wasm-bindgen's `&[u32]`/`&[u16]` expect.
 * The decoder's getters already return the right typed array (fast path: identity — a real
 * `Uint32Array` passes through by reference, not copied); the fallback covers a plain-array
 * frame (test/demo fixtures, e.g. `demo/fake-search.ts`).
 *
 * The fallback `Uint32Array.from` REINTERPRETS an out-of-range value, it does not reject it:
 * a negative wraps to its two's-complement, `NaN`/±`Infinity` land as `0`, and `>= 2**32` wraps
 * mod 2**32 (#467, pinned in the renderer test — the same class as the #457 decoration wire). A
 * span source feeding this (`selectionSpans` / `matchSpans` / `activeMatchSpans`) MUST clip to
 * valid u32 range itself, as `decorationsForFrame` and the demo's span producers do; this
 * coercion knows nothing of a value's meaning or geometry and so cannot validate — the producer
 * owns validity. Deliberately not rejected here (#467): a per-frame coercion is the wrong layer.
 *
 * Exported for the seam test only; not re-exported from the package `index.ts`. */
export const asU32 = (a: ArrayLike<number>): Uint32Array =>
  a instanceof Uint32Array ? a : Uint32Array.from(a);
/** The u16 sibling of {@link asU32} (feeds `flags` — and no longer `extra`, which widened to u32
 * at #621/#627), with the same contract: the
 * fallback `Uint16Array.from` REINTERPRETS an out-of-range value (a negative or `>= 2**16` wraps
 * mod 2**16, `NaN`/±`Infinity` → `0`), it does not reject — the producer must clip, this cannot
 * validate (#467). Exported for the seam test only; not re-exported from `index.ts`. */
export const asU16 = (a: ArrayLike<number>): Uint16Array =>
  a instanceof Uint16Array ? a : Uint16Array.from(a);
/**
 * Like {@link asU32}, but for a column this object **keeps past the current frame** — always a copy,
 * never the argument.
 *
 * The opposite rule from {@link asU32}, and not a contradiction: a column that is forwarded and
 * forgotten wants the zero-copy view (#627), while a column that is *retained* cannot have one. A
 * decoded frame's columns view WASM memory directly and are invalidated when that memory grows —
 * the decoder states it as a contract — so a retained view survives exactly until the next decode
 * large enough to reallocate. Measured (#657): a held view detaches after **one** decode of a
 * 300x220 frame, or 109 small ones held at once, and passing the detached array to any wasm entry
 * point throws `TypeError: … on a detached or out-of-bounds ArrayBuffer` rather than degrading.
 *
 * That throw would land in {@link JustermRenderer.issueOverlay}, which by design runs on a **focus
 * flip with no new frame** — so the visible failure is that clicking away from a terminal with a
 * live selection raises, after the viewport has grown at some earlier point.
 *
 * Cheap: overlay spans are `(row, left, right)` triples for the highlighted rows only, copied once
 * per frame — not a cell column.
 */
export const retainU32 = (a: ArrayLike<number>): Uint32Array => Uint32Array.from(a);
