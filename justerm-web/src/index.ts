// Public API of justerm-web.
export type {
  DecodedFrame,
  FlagBits,
  FrameSource,
  // #862: the underline style is a 3-bit field, so it rides beside FlagBits rather than in it.
  // Types only — the enum's *values* come from `JustermRenderer.underlineStyles`, because a
  // value re-export here would make the decoder a static runtime import.
  UnderlineStyle,
  UnderlineStyles,
  Unsubscribe,
} from "./types";
export type { Renderer } from "./renderer";
export { StubFrameSource } from "./frame-source";
// Consumer events (#117) — fire-and-forget title/bell/cwd notifications from core's
// drain_events, delivered out-of-band via FrameSource.subscribeEvents and routed to
// EventHandlers. onLinkActivate stays with the link controller (#113).
export { dispatchTermEvent } from "./events";
export type { EventHandlers, TermEvent } from "./events";
// Terminal — the frame→renderer pump; with TerminalOptions it also captures input,
// routes the wheel (app / alt-cursor-keys / scrollback, #129 mask), restarts the
// cursor blink on typing, and tracks focus (S16 #133). The routing/notify decisions
// are pure + exported for reuse.
export { rendererNotifyingSink, routeWheel, Terminal, wheelGoesToApp, wheelScrollTarget } from "./terminal";
export type { TerminalOptions, WheelAction } from "./terminal";
export { JustermRenderer } from "./justerm-renderer";
export type { AttachedRendererOptions, JustermRendererOptions, Theme } from "./justerm-renderer";
// TerminalSurface (#775, Epic #287 S7) — one canvas, one WebGL2 context, N attached terminals.
// The only new noun: `JustermRenderer.create` composes one internally for the single-terminal
// arrangement, so a consumer that wants one terminal never names it. A host that wants several
// opens the surface itself and attaches with `JustermRenderer.attach`.
//
// `SurfaceDeps` is exported alongside because the class is constructible — the instantiation seam
// #696 and #579 each declined to build. Unlike `ContextLossRelay`/`FrameLoop`, which stay unexported
// for being uninjectable, this one is the composition root and a host may want its own seams.
export { observeViewportRect, TerminalSurface, viewportOrigin } from "./terminal-surface";
export type {
  AddGridOptions,
  GridLease,
  OverlayBoxes,
  SurfaceBackend,
  SurfaceCanvas,
  SurfaceDeps,
} from "./terminal-surface";
// Context loss (#579) — `ContextLossRelay` is deliberately NOT exported, matching `FrameLoop`
// (#696), the extraction it copies: both exist because `JustermRenderer` cannot be constructed under
// vitest, and neither is injectable, so publishing one would add a semver liability a consumer has
// no way to use. The consumer surface is `JustermRendererOptions.onContextLoss` +
// `JustermRenderer.setOnContextLoss` / `isContextLost` / `isRestoreOverdue`.
// Scroll intent — wheel events → scrollback line delta (xterm consumeWheelEvent).
export { WheelScroller } from "./scroll-control";
export type { ScrollOptions, WheelContext, WheelLike } from "./scroll-control";
// Viewport cell mirror — the a11y text mirror; applies scroll-op damage so the
// screen-reader row tree stays correct across scroll (ADR-0011). Text-only since
// #504 (the renderer composites colour in wasm, #273).
export { CellMirror } from "./cell-mirror";
// Cursor — blink state (web policy). The renderer draws the cursor natively (#270).
export { BLINK_IDLE_TIMEOUT, BLINK_INTERVAL, CursorBlink } from "./cursor";
// SGR 5 text blink phase (#576) — the caret's sibling policy, on its own clock and off by default.
export { TextBlink } from "./text-blink";
// Scrollbar — custom DOM slider over the canvas (thumb math + drag → offset).
export { dragToDisplayOffset, dragTrackRatio, Scrollbar, scrollbarMetrics } from "./scrollbar";
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
// is by index. The active match rides its own overlay channel, ranked above the
// selection (#429) — it is never selected, so a user selection coexists.
export { SearchController, StubSearchPort } from "./search";
export type { SearchOptions, SearchPort, SearchResult } from "./search";
// Fit (#114) — container px → cols/rows (xterm FitAddon parity: padding + scrollbar
// subtract, floor, min 2×1, guards) → a debounced resize intent (ResizePort) the backend
// applies as Engine::resize + PTY SIGWINCH. `observeResize` wires the ResizeObserver.
export {
  FitController,
  MINIMUM_COLS,
  MINIMUM_ROWS,
  observeResize,
  proposeDimensions,
  StubResizePort,
} from "./fit";
export type { Dimensions, FitInput, FitPadding, ResizePort } from "./fit";
// Links — two sources: OSC8 explicit (frame link/linkTable) + plain-URL regex
// over the engine's logical lines (ADR-0017: core assembles, web matches). The
// controller drives hover/leave/activate, OSC8 winning over regex on a cell.
export { computeLinks, LinkController, osc8Links, URL_REGEX } from "./links";
export type { Link, LogicalLine } from "./links";
// Accessibility (#119) — screen-reader mirror: hidden row tree (review) +
// aria-live announce (cursor-anchored viewport diff, typed-echo dedup, alt-screen
// suppress). Pure logic; the consumer injects the DOM sinks.
export { AccessibilityController, TOO_MUCH_OUTPUT } from "./accessibility";
export type { A11yFrame, A11yTreeSink, LiveRegionSink } from "./accessibility";
// Accessible view (#150) — on-demand whole-buffer document (VSCode AccessibleView
// analog): summon → query core (AccessiblePort) → navigable doc, close → restore
// focus. Sibling of the row-tree mirror; the scrollback escape hatch.
export {
  AccessibleViewController,
  DomAccessibleView,
  StubAccessiblePort,
} from "./accessible-view";
export type { AccessiblePort, AccessibleView } from "./accessible-view";
// Command announce (#160) — OSC 133 CommandFinished marks → screen-reader
// announce + exit-driven success/fail signal (VSCode terminalCommand* analog).
// Pure logic; the consumer injects the aria-live + signal sinks. Prompt-to-prompt
// navigation is a separate slice (#166).
export {
  CommandAnnounceController,
  DEFAULT_ANNOUNCE_POLICY,
  TERSE_ANNOUNCE_TEXT,
  VERBOSE_ANNOUNCE_TEXT,
} from "./command-announce";
export type {
  AnnouncePolicy,
  AnnounceText,
  Enablement,
  OutcomePolicy,
  SignalSink,
} from "./command-announce";
// Command navigation (#166) — prompt-to-prompt walk over the whole command
// history (core `command_lines` query) in the accessible view: reveal + announce
// each command + reuse #160's success/fail signal (VSCode navigateToCommand).
export { CommandNavController, StubCommandNavPort } from "./command-nav";
export type { CommandInfo, CommandNavPort, NavView } from "./command-nav";
// Markers (#118/#159) — decode a frame's stride-5 markerPositions into typed
// Markers (id/row/kind/exit). Shared by command announce, decorations, nav.
export { MarkerKind, readMarkers } from "./markers";
export type { Marker } from "./markers";
// Marker-anchored decorations (#120) — a registry that projects per-frame
// decoration rects (positions + colour refs) from markers; colour/render is the
// consumer's (#115). S1: model + lifecycle + auto-dispose; render is S2/S3.
export { DecorationRegistry } from "./decorations";
// #490: the pulled marker index. `MarkerIndexCache` is what a consumer maintains and
// hands to `DecorationRegistry.setMarkerIndex`; `MarkerPort` is the query seam it pulls
// through (sibling of `CommandNavPort`), wired by the host over its own transport.
// `MarkerLineSource` is all the registry actually requires, so a consumer already holding
// marker lines can feed the projection without adopting the cache.
export { MarkerIndexCache } from "./marker-index";
export type { MarkerIndexEntry, MarkerIndexSnapshot, MarkerPort } from "./marker-index";
export type {
  MarkerLineSource,
  Decoration,
  DecorationLayer,
  DecorationOptions,
  DecorationRect,
  OverviewRulerOptions,
  RulerMark,
  RulerPosition,
  SearchRulerOptions,
} from "./decorations";
// The overview ruler's second mark source (#440) and the join that keeps ADR-0024 R3's total order
// in ONE place. A host composes the two sources with `composeRulerMarks` — concatenating them
// itself would re-derive the class partition per host, and no unit test can observe a violation
// (vitest has no layout); only the demo's `__rulerLayerProbe` e2e can.
export { composeRulerMarks, searchRulerMarks } from "./decorations";
// Screen-reader-active gate (#161) — the host injects SR presence (a browser
// can't detect it); while inactive, the a11y announce/signal sinks no-op. Share
// one instance across #119 + #160 so a single toggle governs both.
export { ScreenReaderState } from "./screen-reader";
// DOM glue: hidden row tree + aria-live sinks + a CellMirror-backed adapter the
// consumer mounts beside the canvas and feeds frames (verified in the demo).
export { Accessibility } from "./accessibility-dom";
// #152: bridge an AT text selection in the row tree back to the engine selection,
// reusing the mouse SelectionPort seam. `Accessibility` wires this when given a port.
export { a11ySelectionToPort } from "./a11y-selection";
export type { TreeSelection } from "./a11y-selection";
// Input — DOM events → intent (the backend encodes); outbound seam.
//
// `geometryViolations` is exported because #672 chose **diagnosis over correction** for a violated
// `CellGeometry` precondition, and a diagnosis nobody can reach is not one: without this a consumer
// could neither self-check the value it is about to return nor assert on it in its own tests, and
// the entire mechanism was reduced to a `console.warn` it cannot route. `GeometryViolation` was
// already an exported interface with no way to obtain one.
//
// `checkGeometry` (the warn-and-answer-anyway wrapper) and `resetGeometryWarnings` stay internal on
// purpose: the first is the widget's own per-event call and a consumer calling it would double the
// warning, and the second is test-only for this package, like `clampTo`.
export {
  captureInput,
  geometryViolations,
  keyFromDom,
  Mod,
  MouseEvents,
  mouseFromDom,
  StubInputSink,
  wheelMouseFromDom,
} from "./input";
export type { GeometryViolation, TextareaLike } from "./input";
// IME composition (#116) — a hidden textarea's composition events → committed text
// (read from the textarea value, never the unreliable event data; Korean jongseong
// migration is why). Emits raw `text` intents on the InputSink. Pure logic; the DOM
// textarea + its listeners are the consumer's glue.
export { CompositionController } from "./composition";
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
