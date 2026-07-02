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
  /**
   * Live-selection overlay (#108, wire v6): viewport `(row, left, right)`
   * triples, both columns inclusive — `justerm-wasm-decode`'s `selectionSpans`
   * getter. Positions only (the blend colour is web policy, #115). Optional —
   * a frame with no selection omits it (treated as empty).
   */
  readonly selectionSpans?: ArrayLike<number>;
  /**
   * Search-match overlay (#108): same viewport `(row, left, right)` stride-3
   * layout as {@link selectionSpans}, a separate wire group —
   * `justerm-wasm-decode`'s `matchSpans` getter. Set on the backend via
   * `Engine::set_search_highlights`; consumed by search (#110). Optional.
   */
  readonly matchSpans?: ArrayLike<number>;
  /**
   * Decoration/command markers visible in this viewport (#118/#159, wire v10):
   * stride-5 `(id, row, kind, exitPresent, exitBits)` — `justerm-wasm-decode`'s
   * `markerPositions` getter. `kind`: 0 = Plain (#118 decoration), 1 = PromptStart,
   * 2 = CommandStart, 3 = OutputStart, 4 = CommandFinished (OSC 133). For a finished
   * command, `exitPresent` is 1 and `exitBits` is the exit code as a raw u32 —
   * reinterpret as signed with `exitBits | 0`. Off-screen markers are absent (still
   * alive; disposal comes via a `MarkerDisposed` event). Optional — a frame with no
   * markers omits it. Consumed by decorations (#120) + prompt-nav a11y (#160).
   */
  readonly markerPositions?: ArrayLike<number>;
  /**
   * EVERY live marker's absolute buffer line (#120 S3, wire v11): stride-2
   * `(id, line)` — `justerm-wasm-decode`'s `markerLines` getter. A superset of
   * {@link markerPositions} by id, but keyed to the absolute buffer line (in the
   * `scrollbackLen + rows` frame), including OFF-viewport markers — so the overview
   * ruler can place marks a user must scroll to reach. Optional; omitted when there
   * are no markers. Consumed by decorations (#120 `rulerMarksForFrame`).
   */
  readonly markerLines?: ArrayLike<number>;
  /** Grapheme clusters referenced by cells' `extra` index (frame-local). */
  readonly sideTable: readonly string[];
  /**
   * Per-cell OSC 8 hyperlink index (wire v2), span order: `0` = none, else
   * `linkTable[link - 1]` is the URI. Both halves of a wide glyph carry it.
   * `justerm-wasm-decode`'s `link` getter. Optional — a frame with no links omits it.
   */
  readonly link?: ArrayLike<number>;
  /** OSC 8 URIs referenced by cells' `link` index (frame-local) — the decoder's
   * `linkTable` getter. */
  readonly linkTable?: readonly string[];
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
   * Viewport scroll position (#112, wire v5): `displayOffset` lines scrolled up
   * from the bottom (0 = following), `scrollbackLen` history lines. The scrollbar
   * sizes its thumb from these. Optional — a frame may omit them (no scrollback).
   */
  readonly displayOffset?: number;
  readonly scrollbackLen?: number;
  /**
   * Whether the alternate screen (`?1049`/`?47`) is active (#149, wire v9) —
   * `justerm-wasm-decode`'s `altScreen` getter. The a11y announce policy (#119)
   * suppresses output reads when set (a TUI repaint isn't new output). Optional —
   * a frame may omit it (treated as the primary screen).
   */
  readonly altScreen?: boolean;
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
