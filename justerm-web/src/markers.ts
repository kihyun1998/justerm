/**
 * Reading engine markers off a decoded frame (#118/#159). The wire carries them
 * as a flat stride-5 `Uint32Array` (`justerm-wasm-decode`'s `markerPositions`);
 * this decodes that into typed {@link Marker}s. Shared by command announce (#160)
 * and — later — decorations (#120) and prompt navigation (#166).
 */

/** A marker's kind, matching the wire discriminant (#159 `MarkerKind`). */
export enum MarkerKind {
  /** A `add_marker` decoration anchor (#118) — no OSC-133 semantics. */
  Plain = 0,
  /** OSC `133;A` — the shell prompt begins here. */
  PromptStart = 1,
  /** OSC `133;B` — the typed command begins here. */
  CommandStart = 2,
  /** OSC `133;C` — the command was submitted; output begins here. */
  OutputStart = 3,
  /** OSC `133;D[;exit]` — the command finished (with its exit code, if any). */
  CommandFinished = 4,
}

/** A marker projected onto the viewport: its id, row, kind, and — for a finished
 * command — its exit code (absent when the shell reported none). */
export interface Marker {
  readonly id: number;
  readonly row: number;
  readonly kind: MarkerKind;
  /** Present only for {@link MarkerKind.CommandFinished} with a reported code. */
  readonly exit?: number;
}

/** u32 lanes per marker in `frame.markerPositions`: `id, row, kind, exitPresent,
 * exitBits` — see the wasm `MARKER_STRIDE`. */
const MARKER_STRIDE = 5;

/** Decode `frame.markerPositions` (a flat stride-5 array) into {@link Marker}s.
 * `exitBits` is a raw u32; `| 0` reinterprets it as the signed i32 the engine
 * sent. An absent array (no markers) yields `[]`. */
export function readMarkers(flat?: ArrayLike<number>): Marker[] {
  const out: Marker[] = [];
  if (!flat) return out;
  for (let i = 0; i + MARKER_STRIDE <= flat.length; i += MARKER_STRIDE) {
    // The loop bound guarantees these five lanes exist (`!` over the strict
    // indexed-access `number | undefined`).
    const exitPresent = flat[i + 3]! !== 0;
    out.push({
      id: flat[i]!,
      row: flat[i + 1]!,
      kind: flat[i + 2]! as MarkerKind,
      exit: exitPresent ? flat[i + 4]! | 0 : undefined,
    });
  }
  return out;
}
