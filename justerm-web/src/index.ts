// Public API of justerm-web.
export type { DecodedFrame, FlagBits, FrameSource, Unsubscribe } from "./types";
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
export type { JustermRendererOptions, Theme } from "./justerm-renderer";
// Scroll intent — wheel events → scrollback line delta (xterm consumeWheelEvent).
export { WheelScroller } from "./scroll-control";
export type { ScrollOptions, WheelContext, WheelLike } from "./scroll-control";
// Viewport cell mirror — the a11y text mirror; applies scroll-op damage so the
// screen-reader row tree stays correct across scroll (ADR-0011). Text-only since
// #504 (the renderer composites colour in wasm, #273).
export { CellMirror } from "./cell-mirror";
// Cursor — blink state (web policy). The renderer draws the cursor natively (#270).
export { BLINK_INTERVAL, CursorBlink } from "./cursor";
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
export type {
  Decoration,
  DecorationLayer,
  DecorationOptions,
  DecorationRect,
  OverviewRulerOptions,
  RulerMark,
  RulerPosition,
} from "./decorations";
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
export {
  captureInput,
  keyFromDom,
  Mod,
  MouseEvents,
  mouseFromDom,
  StubInputSink,
  wheelMouseFromDom,
} from "./input";
export type { TextareaLike } from "./input";
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
