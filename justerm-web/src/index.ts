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
// Scroll intent — wheel events → scrollback line delta (xterm consumeWheelEvent).
export { WheelScroller } from "./scroll-control";
export type { ScrollOptions, WheelContext, WheelLike } from "./scroll-control";
// Viewport cell mirror — applies scroll-op damage for frame mode (ADR-0011).
export { CellMirror } from "./cell-mirror";
export { cellToDrawOp } from "./render-core";
// Cursor — blink state (web policy) + cell-invert/underline overlay.
export { BLINK_INTERVAL, CursorBlink, CursorShape, cursorOp } from "./cursor";
// Scrollbar — custom DOM slider over the canvas (thumb math + drag → offset).
export { dragToDisplayOffset, Scrollbar, scrollbarMetrics } from "./scrollbar";
export type { ScrollbarMetrics, ScrollbarOptions, ScrollPosition } from "./scrollbar";
// Selection — drag → engine selection commands (SelectionPort, the write-side
// sibling of FrameSource), drag-scroll, alt-click cursor move, copy, primary.
export { copySelection, dragScrollSpeed, SelectionController, StubSelectionPort } from "./selection";
export type { SelCall, SelectionPort, SelType, Side } from "./selection";
// Overlay — frame selection/search spans → kinded highlight rects + per-cell
// lookup the renderer blends (colour is #115's policy).
export { highlightAt, highlightRects, matchHighlights, selectionHighlights } from "./overlay";
export type { HighlightKind, HighlightRect, HighlightSpan } from "./overlay";
// Search — query-box state machine (count/index/wrap/debounce) → SearchPort.
// Matches stay backend-side (only their matchSpans cross the wire); navigation
// is by index. Active match = selection (reuses the selection highlight).
export { SearchController, StubSearchPort } from "./search";
export type { SearchPort, SearchResult } from "./search";
// Input — DOM events → intent (the backend encodes); outbound seam.
export { captureInput, keyFromDom, Mod, mouseFromDom, StubInputSink, wheelMouseFromDom } from "./input";
export type {
  CaptureOptions,
  CellGeometry,
  Intent,
  InputSink,
  Key,
  KeyAction,
  KeyboardEventLike,
  KeyEvent,
  MouseAction,
  MouseButton,
  MouseEvent,
  MouseEventLike,
  NamedKey,
} from "./input";
