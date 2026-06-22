// Colour helpers for the justerm-wasm decoder (#36, ADR-0008).
//
// The single hand-written JS mirror in the package: these decode justerm's
// tagged-u32 colour-ref encoding (high byte = tag — 0 Default, 1 Indexed, 2 Rgb;
// low 24 bits = payload). They live in JS, not WASM, because `resolveRgb` runs
// per cell and a WASM call per cell would defeat the zero-copy design (#34 AC3).
// Kept in lockstep with Rust `encode_color` by a parity test (wasm-pack --node).

/** Role for `Default` resolution: foreground. */
export const FG = 0;
/** Role for `Default` resolution: background. */
export const BG = 1;

/**
 * Resolve a colour reference to a packed `0xRRGGBB` number. Alloc-free — call it
 * per cell. `palette` is `{ colors: Uint32Array(256), defaultFg, defaultBg }`
 * (build `colors` once per scheme with `buildPalette`); `role` is `FG` or `BG`,
 * selecting which default a `Default` ref resolves to.
 *
 * Does NOT apply inverse/dim/hidden/bold→bright — those are render policy the
 * caller applies afterward.
 */
export function resolveRgb(ref, palette, role) {
  switch (ref >>> 24) {
    case 0:
      return role === FG ? palette.defaultFg : palette.defaultBg;
    case 1:
      return palette.colors[ref & 0xff];
    default:
      return ref & 0xffffff;
  }
}

/**
 * Unpack a colour reference into a tagged object — `{ kind: 'default' }`,
 * `{ kind: 'indexed', index }`, or `{ kind: 'rgb', r, g, b }`. For inspection,
 * not the hot loop (it allocates an object); use `resolveRgb` there.
 */
export function decodeColorRef(ref) {
  switch (ref >>> 24) {
    case 0:
      return { kind: "default" };
    case 1:
      return { kind: "indexed", index: ref & 0xff };
    default:
      return { kind: "rgb", r: (ref >>> 16) & 0xff, g: (ref >>> 8) & 0xff, b: ref & 0xff };
  }
}
