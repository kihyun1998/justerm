// Public API of justerm-web.
export type { DecodedFrame, FrameSource, Unsubscribe } from "./types";
export type { Renderer } from "./renderer";
export { StubFrameSource } from "./frame-source";
export { Terminal } from "./terminal";
export { BeamtermRenderer } from "./beamterm-renderer";
export type { BeamtermOptions, Theme } from "./beamterm-renderer";
// Render core — the pure DecodedFrame → draw-op mapping (testable, no GL/wasm).
// Exposed so alternate renderers (or #115's render policy) can reuse it.
export { frameToDrawOps, identityPolicy } from "./render-core";
export type { DrawOp, FlagBits, RenderPolicy } from "./render-core";
