/**
 * Bridging an AT text selection in the accessibility row tree back to the engine's
 * selection (#152), the frame-mode analog of xterm's
 * `AccessibilityManager._handleSelectionChange`. When a screen-reader user selects
 * text inside the hidden `role="list"` row tree, the browser fires `selectionchange`;
 * the DOM glue resolves that to tree coordinates (which listitem, which UTF-16 offset)
 * and this pure bridge maps the offsets back to grid `(row, col, side)` via each row's
 * column map ({@link CellMirror.rowCells}) and drives the SAME {@link SelectionPort}
 * seam the mouse selection uses (S8/#109) — so an AT selection becomes a real core
 * selection (copyable, highlighted) with no separate path.
 *
 * Pure — no DOM, no `window.getSelection`. The glue produces the {@link TreeSelection}
 * (structural, a `MouseEventLike`-style seam) so this logic is unit-tested.
 */

import type { Side, SelectionPort } from "./selection";

/** One endpoint of an AT text selection, resolved by the DOM glue:
 * - `{ row, offset }` — inside a listitem: viewport `row` + UTF-16 `offset` into that
 *   row's SR text (the row tree's `textContent`, which is `rowCells(row).text`).
 * - `"before"` / `"after"` — *outside* the tree but the selection overlaps it, with the
 *   endpoint sitting above row 0 / below the last row (native Select-All, a text-drag
 *   overshooting an edge, or the sibling live region). Clamped to the tree boundary
 *   (#217), mirroring xterm's `_handleSelectionChange` begin/end clamp.
 * - `null` — no node at all (empty `getSelection`), so there's nothing to map. */
export type TreeEndpoint = { row: number; offset: number } | "before" | "after" | null;

/** An AT text selection resolved to tree coordinates. The DOM glue builds this from
 * `getSelection()` + the listitems' `aria-posinset` (and `compareDocumentPosition` for
 * the out-of-tree `before`/`after` classification). */
export interface TreeSelection {
  readonly anchor: TreeEndpoint;
  readonly focus: TreeEndpoint;
  readonly collapsed: boolean;
}

/** Whether an endpoint resolved *inside* the tree (vs an out-of-tree side / null). */
function inside(ep: TreeEndpoint): ep is { row: number; offset: number } {
  return ep !== null && ep !== "before" && ep !== "after";
}

/**
 * Drive `port` from an AT text selection. A collapsed selection whose caret is *inside
 * the tree* clears the engine selection (a caret is not a selection); a caret outside
 * the tree is left alone (xterm's `_rowContainer.contains` guard). For a range, each
 * out-of-tree endpoint that still *overlaps* the tree is clamped to the nearest boundary
 * — `"before"` → the start of row 0, `"after"` → the end of the last row (`rowCount-1`)
 * — so native Select-All and drag-overshoot select the intersection instead of no-oping
 * (#217, xterm's `_handleSelectionChange` clamp). A selection wholly on one side (both
 * endpoints `"before"`, or both `"after"`) doesn't overlap and is ignored. Otherwise
 * `begin` at the anchor boundary and `extend` to the focus boundary as `char` selections
 * — the engine tracks anchor→focus, so a backwards selection works unchanged.
 */
export function a11ySelectionToPort(
  sel: TreeSelection,
  columnsFor: (row: number) => number[],
  rowCount: number,
  port: SelectionPort,
): void {
  if (sel.collapsed) {
    if (inside(sel.anchor)) port.clear(); // caret in the tree clears; outside is not ours
    return;
  }
  if (sel.anchor === null || sel.focus === null) return; // a missing endpoint — nothing to map
  // Both endpoints on the same outside side → the selection doesn't overlap the tree
  // (xterm bails when `begin` is below the last row or `end` is above the first).
  if ((sel.anchor === "before" || sel.anchor === "after") && sel.anchor === sel.focus) return;
  const a = clamp(sel.anchor, rowCount, columnsFor);
  const f = clamp(sel.focus, rowCount, columnsFor);
  // No invalid-range guard on purpose (unlike xterm's `throw 'invalid range'`): a backwards
  // pair is normal (the engine normalizes anchor→focus, like the mouse backward-drag), and a
  // degenerate begin===end (two UTF-16 offsets collapsing onto one grid column via a wide or
  // combining char) is a harmless zero-width selection (empty text, copy skips it) — safer
  // than throwing. Don't "restore parity" by adding the assert. (#217 2-lens.)
  port.begin(a.row, a.col, a.side, "char");
  port.extend(f.row, f.col, f.side);
}

/** Resolve an endpoint to a grid boundary, clamping an out-of-tree side to the tree
 * edge: `"before"` → the left of row 0's first char, `"after"` → the right of the last
 * row's last char (end-of-text on `rowCount-1`). */
function clamp(
  ep: { row: number; offset: number } | "before" | "after",
  rowCount: number,
  columnsFor: (row: number) => number[],
): { row: number; col: number; side: Side } {
  if (ep === "before") return boundary(0, 0, columnsFor(0));
  if (ep === "after") {
    const last = Math.max(0, rowCount - 1);
    // Offset past the end → the RIGHT of the last char (end-of-line), like xterm clamping
    // `end` to `lastRowElement.textContent.length`.
    return boundary(last, Number.MAX_SAFE_INTEGER, columnsFor(last));
  }
  return boundary(ep.row, ep.offset, columnsFor(ep.row));
}

/** Map a DOM text `offset` in `row` (column map `columns`, one entry per UTF-16 unit)
 * to a grid boundary. Offset `o` is the boundary to the LEFT of char `o` →
 * `(columns[o], "left")`. At end-of-text (`o >= length`) it's the RIGHT of the last
 * char → `(columns[last], "right")`, so selecting to end-of-line includes it. A blank
 * row (empty map) collapses to column 0 (xterm's `[0, 1]` empty-line sentinel). */
function boundary(
  row: number,
  offset: number,
  columns: number[],
): { row: number; col: number; side: Side } {
  if (columns.length === 0) return { row, col: 0, side: "left" };
  if (offset >= columns.length) {
    return { row, col: columns[columns.length - 1]!, side: "right" };
  }
  return { row, col: columns[Math.max(0, offset)]!, side: "left" };
}
