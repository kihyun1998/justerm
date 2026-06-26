/**
 * A decoded terminal frame — the unit the renderer consumes.
 *
 * S1 models only the dimensions + kind; the structure-of-arrays cell columns,
 * spans, cursor and scroll come from `justerm-wasm-decode` in S2 (#105). The
 * shape is intentionally source-agnostic: a frame may arrive decoded from a
 * backend wire (frame mode) or produced by an in-wasm engine (future).
 */
export interface DecodedFrame {
  readonly cols: number;
  readonly rows: number;
  /** `full` = whole viewport; `partial` = damage-bounded (spans, in S2). */
  readonly kind: "full" | "partial";
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
