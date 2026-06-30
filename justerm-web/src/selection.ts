import type { CellGeometry, MouseEventLike } from "./input";

/** Which half of a cell an anchor sits on (mirrors core `Side`). Left = the
 * cell is included on a rightward drag; Right = excluded. */
export type Side = "left" | "right";

/** What a selection covers (mirrors core `SelectionType`). */
export type SelType = "char" | "word" | "line" | "block";

/**
 * The control seam from the web widget to the engine's selection state. In
 * frame mode the consumer wires this to the backend, which calls core's
 * `selection_begin` / `selection_extend` / `selection_clear`; the resulting
 * overlay spans return on the next frame. Sibling to {@link FrameSource}: the
 * frame channel is read-only, this is the matching write channel.
 */
export interface SelectionPort {
  /** Anchor a new selection of `ty` at viewport `(row, col)`, `side`. */
  begin(row: number, col: number, side: Side, ty: SelType): void;
  /** Move the live selection's focus to viewport `(row, col)`, `side`. */
  extend(row: number, col: number, side: Side): void;
  /** Drop the selection. */
  clear(): void;
  /** The engine's selection text (core `selection_text`, computed on the backend
   * across scrollback), or `null` when nothing is selected. Async: in frame
   * mode it round-trips to the backend. */
  text(): Promise<string | null>;
}

/** A recording {@link SelectionPort} for tests/demos — the simplest concrete
 * sink behind the seam (mirrors `StubInputSink`). */
export class StubSelectionPort implements SelectionPort {
  readonly calls: SelCall[] = [];
  begin(row: number, col: number, side: Side, ty: SelType): void {
    this.calls.push({ kind: "begin", row, col, side, ty });
  }
  extend(row: number, col: number, side: Side): void {
    this.calls.push({ kind: "extend", row, col, side });
  }
  clear(): void {
    this.calls.push({ kind: "clear" });
  }
  /** The text the next {@link text} query resolves to (set by tests). */
  textValue: string | null = null;
  text(): Promise<string | null> {
    return Promise.resolve(this.textValue);
  }
}

/** One recorded {@link SelectionPort} call. */
export type SelCall =
  | { kind: "begin"; row: number; col: number; side: Side; ty: SelType }
  | { kind: "extend"; row: number; col: number; side: Side }
  | { kind: "clear" };

/**
 * Copy the current selection to the clipboard: query the engine for its text
 * and hand it to `writeClipboard`. Returns whether anything was copied. The
 * text comes from the backend (core `selection_text`); this only ferries it.
 */
export async function copySelection(
  port: Pick<SelectionPort, "text">,
  writeClipboard: (text: string) => Promise<void>,
): Promise<boolean> {
  const text = await port.text();
  // Null (no selection) or empty (collapsed) → leave the clipboard untouched.
  if (!text) return false;
  await writeClipboard(normalizeForCopy(text));
  return true;
}

/** Copy-policy normalization (xterm parity). Non-breaking spaces (U+00A0) become
 * regular spaces so pasted text carries no invisible NBSPs. Done here, not in
 * core's `selection_text`, because justerm never emits NBSP as padding — any in
 * a cell is real content, and core stays a faithful dump. */
function normalizeForCopy(text: string): string {
  return text.split(String.fromCharCode(0xa0)).join(" ");
}

/** Auto-scroll speed (signed lines per tick) for a pointer `py` px below the
 * viewport top, viewport `height` px tall. 0 while inside; outside, scrolls
 * toward the pointer. (xterm `_getMouseEventScrollAmount`.) */
export function dragScrollSpeed(py: number, height: number): number {
  if (py >= 0 && py <= height) return 0;
  // Distance out of the viewport: below the bottom subtracts the height, above
  // the top stays negative.
  let offset = py > height ? py - height : py;
  offset = Math.min(Math.max(offset, -DRAG_SCROLL_MAX_THRESHOLD), DRAG_SCROLL_MAX_THRESHOLD);
  offset /= DRAG_SCROLL_MAX_THRESHOLD; // → [-1, 1]
  // sign keeps a minimum ±1 step; the rounded term ramps to ±(SPEED-1) more.
  return offset / Math.abs(offset) + Math.round(offset * (DRAG_SCROLL_MAX_SPEED - 1));
}

/** Pointer this many px out of the viewport reaches the top scroll speed. */
const DRAG_SCROLL_MAX_THRESHOLD = 50;
/** Top auto-scroll speed in lines per tick. */
const DRAG_SCROLL_MAX_SPEED = 15;

/** DOM click count → selection granularity (xterm: 1 single, 2 double, 3
 * triple). A 4th click and beyond stays line. */
function modeForClick(detail: number): SelType {
  if (detail >= 3) return "line";
  if (detail === 2) return "word";
  return "char";
}

/** Resolve a pointer event to its viewport cell + nearest cell edge. */
function cellAndSide(ev: MouseEventLike, geom: CellGeometry): { row: number; col: number; side: Side } {
  const px = ev.clientX - geom.originX;
  const py = ev.clientY - geom.originY;
  const col = Math.floor(px / geom.cellWidth);
  const row = Math.floor(py / geom.cellHeight);
  const within = px - col * geom.cellWidth;
  return { row, col, side: within >= geom.cellWidth / 2 ? "right" : "left" };
}

/**
 * Translates a DOM mouse drag into {@link SelectionPort} commands. Pure logic —
 * no DOM listeners, no rendering: the widget feeds it normalised events, it
 * decides what the engine selection should be. The highlight comes back via
 * frames ({@link selectionHighlights}); this only drives the model.
 */
export class SelectionController {
  private dragging = false;
  private hasSelection = false;
  /** Signed lines/tick the active drag wants to auto-scroll (0 = in bounds). */
  private dragScrollAmount = 0;
  /** The pointer's last column/side, to re-anchor the focus at an edge on tick. */
  private lastCol = 0;
  private lastSide: Side = "left";
  /** The press timestamp, to measure click duration for the alt-click move. */
  private downTimeStamp = 0;
  /** Whether the pointer moved since the press (a real drag, not a bare click). */
  private dragged = false;
  private readonly onScroll: (lines: number) => void;
  private readonly getRows: () => number;
  private readonly onMoveCursor: (cell: { row: number; col: number }) => void;
  private readonly isAtBottom: () => boolean;
  private readonly onPaste: () => void;
  /** Undefined when no consumer wants primary — then the text query is skipped. */
  private readonly onPrimarySelection: ((text: string) => void) | undefined;

  constructor(
    private readonly port: SelectionPort,
    private readonly getGeometry: () => CellGeometry,
    opts: {
      onScroll?: (lines: number) => void;
      getRows?: () => number;
      onMoveCursor?: (cell: { row: number; col: number }) => void;
      isAtBottom?: () => boolean;
      onPaste?: () => void;
      onPrimarySelection?: (text: string) => void;
    } = {},
  ) {
    this.onScroll = opts.onScroll ?? (() => {});
    // No row count → an infinitely tall viewport → never out of bounds → inert.
    this.getRows = opts.getRows ?? (() => Infinity);
    this.onMoveCursor = opts.onMoveCursor ?? (() => {});
    // Cursor-move only makes sense at the live prompt; assume so unless told.
    this.isAtBottom = opts.isAtBottom ?? (() => true);
    this.onPaste = opts.onPaste ?? (() => {});
    this.onPrimarySelection = opts.onPrimarySelection;
  }

  /** A mouse press. `detail` is the DOM click count (1 = single). */
  mouseDown(ev: MouseEventLike, detail: number): void {
    // Middle-click pastes the X11 primary buffer — a separate gesture, not a
    // selection. Only the left button drives selection; right/other are ignored.
    if (ev.button === 1) {
      this.onPaste();
      return;
    }
    if (ev.button !== 0) return;
    this.downTimeStamp = ev.timeStamp ?? 0;
    this.dragged = false;
    const { row, col, side } = cellAndSide(ev, this.getGeometry());
    if (ev.shiftKey && this.hasSelection) {
      // Shift+click extends the live selection (keep the anchor) — incremental.
      this.port.extend(row, col, side);
    } else {
      // Alt on a single click switches to a rectangular block; multi-clicks keep
      // their word/line granularity (xterm `shouldColumnSelect`).
      const ty = detail === 1 && ev.altKey ? "block" : modeForClick(detail);
      this.port.begin(row, col, side, ty);
      this.hasSelection = true;
    }
    this.dragging = true;
  }

  /** Pointer motion. Extends the focus only while a drag is live. When the
   * pointer is outside the viewport the move records an auto-scroll amount
   * instead of extending — {@link tick} drives the edge from there. */
  mouseMove(ev: MouseEventLike): void {
    if (!this.dragging) return;
    this.dragged = true;
    const geom = this.getGeometry();
    const { row, col, side } = cellAndSide(ev, geom);
    this.lastCol = col;
    this.lastSide = side;
    this.dragScrollAmount = dragScrollSpeed(ev.clientY - geom.originY, this.getRows() * geom.cellHeight);
    if (this.dragScrollAmount === 0) {
      this.port.extend(row, col, side);
    }
  }

  /** One auto-scroll step — the consumer calls this on a timer while the button
   * is down. Scrolls the viewport by the pending amount and pins the focus to
   * the edge row toward the pointer (xterm `_dragScroll`). No-op in bounds. */
  tick(): void {
    if (!this.dragging || this.dragScrollAmount === 0) return;
    this.onScroll(this.dragScrollAmount);
    const edgeRow = this.dragScrollAmount > 0 ? this.getRows() - 1 : 0;
    this.port.extend(edgeRow, this.lastCol, this.lastSide);
  }

  /** A mouse release. Ends the drag; the selection itself stays (for copy).
   * A quick Alt-click that never dragged, at the live prompt, instead asks the
   * shell to move its cursor to the clicked cell — a feature distinct from block
   * selection (xterm `altClickMovesCursor`). */
  mouseUp(ev: MouseEventLike): void {
    this.dragging = false;
    this.dragScrollAmount = 0;
    const elapsed = (ev.timeStamp ?? 0) - this.downTimeStamp;
    if (ev.altKey && !this.dragged && elapsed < ALT_CLICK_MOVE_CURSOR_TIME && this.isAtBottom()) {
      // The empty block selection begun on mousedown is not real — drop it.
      this.port.clear();
      const { row, col } = cellAndSide(ev, this.getGeometry());
      this.onMoveCursor({ row, col });
      return;
    }
    // A real drag selection (not a bare click) feeds the X11 primary buffer.
    // Reuses the copy path → NBSP-normalized, empty selections skipped.
    if (this.onPrimarySelection && this.dragged && this.hasSelection) {
      const sink = this.onPrimarySelection;
      void copySelection(this.port, async (text) => sink(text));
    }
  }
}

/** A click shorter than this (ms) with Alt held moves the cursor instead of
 * selecting — longer is treated as a (possibly empty) selection. */
const ALT_CLICK_MOVE_CURSOR_TIME = 500;
