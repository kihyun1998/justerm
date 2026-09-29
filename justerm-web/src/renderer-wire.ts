import type { DecodedFrame } from "./types";

/** Assemble the flat `apply_damage` header from a decoded frame: scroll presence, the
 * `scrollCount` (a negative one rides the `u32` slot as two's complement), and the blink flag.
 * `blinkOn` gates SGR-blink cells (#282); the default `true` shows them. Why:
 * `docs/map/territory/frame-adapter.md` § The widget's wire encoders. */
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
 * blink phase (#576), so the renderer re-packs its retained grid at that phase. `kind` is
 * **Partial**, never Full, and `cols`/`rows` must be the grid the renderer currently holds —
 * either mistake blanks the terminal. Why: `docs/map/territory/frame-adapter.md` § The widget's
 * wire encoders.
 */
export function blinkPhaseHeader(cols: number, rows: number, blinkOn: boolean): Uint32Array {
  const h = new Uint32Array(8);
  h[0] = cols;
  h[1] = rows;
  h[2] = 1; // Partial — never Full; see above
  h[7] = blinkOn ? 1 : 0;
  return h;
}

/** Whether any cell in a frame's flag column carries `blinkBit` (#576) — the gate on the phase
 * re-pack. */
export function carriesBlink(flags: ArrayLike<number>, blinkBit: number): boolean {
  for (let i = 0; i < flags.length; i++) {
    if (((flags[i] ?? 0) & blinkBit) !== 0) return true;
  }
  return false;
}

/** Coerce a decoder array to the exact typed array wasm-bindgen's `&[u32]`/`&[u16]` expect: a
 * real `Uint32Array` passes through by reference; a plain array (a test or demo fixture) is
 * converted. The conversion REINTERPRETS an out-of-range value rather than rejecting it (#467) — a
 * span producer feeding this must clip to valid u32 range itself. Not re-exported from `index.ts`.
 * Why: `docs/map/invariant/decoded-columns-are-getters.md` § The coercion does not validate. */
export const asU32 = (a: ArrayLike<number>): Uint32Array =>
  a instanceof Uint32Array ? a : Uint32Array.from(a);
/** The u16 sibling of {@link asU32} (feeds `flags`), with the same contract (#467). */
export const asU16 = (a: ArrayLike<number>): Uint16Array =>
  a instanceof Uint16Array ? a : Uint16Array.from(a);
/**
 * Like {@link asU32}, but for a column this object **keeps past the current frame** — always a copy,
 * never the argument, since a decoded column is a view the next large decode detaches (#657). Why:
 * `docs/map/invariant/decoded-columns-are-getters.md`.
 */
export const retainU32 = (a: ArrayLike<number>): Uint32Array => Uint32Array.from(a);
