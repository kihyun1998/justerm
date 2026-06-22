// Types for the justerm-wasm colour helpers (#36). See colors.js.

/** Role for `Default` resolution: foreground. */
export const FG: 0;
/** Role for `Default` resolution: background. */
export const BG: 1;
export type Role = typeof FG | typeof BG;

/** A resolved palette: 256 packed-RGB indices + the theme's default fg/bg. */
export interface Palette {
  /** `0..15` the theme's ANSI colours, `16..255` the xterm cube/grayscale. */
  colors: Uint32Array;
  defaultFg: number;
  defaultBg: number;
}

/** A decoded colour reference. */
export type ColorRef =
  | { kind: "default" }
  | { kind: "indexed"; index: number }
  | { kind: "rgb"; r: number; g: number; b: number };

/** Resolve a colour ref to a packed `0xRRGGBB` number (alloc-free; per cell). */
export function resolveRgb(ref: number, palette: Palette, role: Role): number;

/** Unpack a colour ref into a tagged object (inspection; not the hot loop). */
export function decodeColorRef(ref: number): ColorRef;
