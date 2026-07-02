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

/** An AT text selection resolved to tree coordinates. Each endpoint is a viewport
 * `row` + the UTF-16 `offset` into that row's SR text (the row tree's `textContent`,
 * which is `rowCells(row).text`), or `null` when the endpoint is outside the row tree.
 * The DOM glue builds this from `getSelection()` + the listitems' `aria-posinset`. */
export interface TreeSelection {
  readonly anchor: { row: number; offset: number } | null;
  readonly focus: { row: number; offset: number } | null;
  readonly collapsed: boolean;
}

/**
 * Drive `port` from an AT text selection. A collapsed selection *inside the tree*
 * clears the engine selection (a caret is not a selection); a selection whose anchor
 * is outside the tree is ignored (it isn't ours — leave the engine's selection alone,
 * xterm's `_rowContainer.contains` guard). Otherwise `begin` at the anchor boundary and
 * `extend` to the focus boundary, both as `char` selections — the engine tracks
 * anchor→focus, so a backwards (focus-before-anchor) selection works unchanged.
 */
export function a11ySelectionToPort(
  sel: TreeSelection,
  columnsFor: (row: number) => number[],
  port: SelectionPort,
): void {
  if (!sel.anchor) return; // anchor outside the row tree — not ours
  if (sel.collapsed) {
    port.clear();
    return;
  }
  if (!sel.focus) return;
  const a = boundary(sel.anchor.row, sel.anchor.offset, columnsFor(sel.anchor.row));
  const f = boundary(sel.focus.row, sel.focus.offset, columnsFor(sel.focus.row));
  port.begin(a.row, a.col, a.side, "char");
  port.extend(f.row, f.col, f.side);
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
