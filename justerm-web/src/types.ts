/**
 * A decoded terminal frame — the unit the renderer consumes.
 *
 * Structure-of-arrays: one column per cell field (`codepoints`/`fg`/`bg`/…) plus
 * a `spans` directory, exactly as `justerm-wasm-decode`'s `DecodedFrame` exposes
 * it. This is a *structural* view — the wasm `DecodedFrame` class satisfies it
 * (its getters return these typed arrays, and it carries extra cursor/scroll
 * getters later slices read), and tests/demos pass plain objects. The shape is
 * source-agnostic: a frame may arrive decoded from a backend wire (frame mode)
 * or be produced by an in-wasm engine (future).
 *
 * Cells are addressed through the span directory, not row-major: walk `spans` in
 * stride-5 chunks and index the columns at each span's `cell_offset`. See
 * {@link import("./render-core").frameToDrawOps}.
 */
export interface DecodedFrame {
  readonly cols: number;
  readonly rows: number;
  /** `0` = Full (whole viewport); `1` = Partial (only the listed spans). */
  readonly kind: number;
  /** Per-cell base codepoint, span order (`0` = blank). */
  readonly codepoints: ArrayLike<number>;
  /** Per-cell fg/bg colour refs (tagged u32; resolve with `resolveRgb`). */
  readonly fg: ArrayLike<number>;
  readonly bg: ArrayLike<number>;
  /** Per-cell `CellFlags` bits. */
  readonly flags: ArrayLike<number>;
  /** Per-cell 1-based grapheme-cluster index (`0` = none → `sideTable[extra-1]`). */
  readonly extra: ArrayLike<number>;
  /** Span directory, stride 5: `[line, left, right, cell_offset, count]`. */
  readonly spans: ArrayLike<number>;
  /** Grapheme clusters referenced by cells' `extra` index (frame-local). */
  readonly sideTable: readonly string[];
  /**
   * Cursor state (screen coords, 0-based). `cursorShape`: 0 = Block, 1 =
   * Underline, 2 = Bar. `cursorBlink` is the *mode* — the blink timing is a
   * web-side policy. Optional — a frame may omit them (treated as no cursor).
   */
  readonly cursorRow?: number;
  readonly cursorCol?: number;
  readonly cursorVisible?: boolean;
  readonly cursorShape?: number;
  readonly cursorBlink?: boolean;
  /**
   * Scroll op (applied before spans): rows `[scrollTop, scrollBottom]` shifted by
   * `scrollCount` (positive = up). Optional — absent/`hasScroll: false` means no
   * shift. The cell mirror applies it; a span-only frame omits it.
   */
  readonly hasScroll?: boolean;
  readonly scrollTop?: number;
  readonly scrollBottom?: number;
  readonly scrollCount?: number;
}

/** Unsubscribe handle returned by {@link FrameSource.subscribe}. */
export type Unsubscribe = () => void;

/**
 * A source of decoded frames, abstract over where they come from.
 *
 * Frame mode wires this to the consumer's IPC channel (decoding the backend's
 * wire frames); the future in-wasm mode wires it to an in-browser engine. The
 * renderer never knows which — it just subscribes.
 */
export interface FrameSource {
  subscribe(listener: (frame: DecodedFrame) => void): Unsubscribe;
}
