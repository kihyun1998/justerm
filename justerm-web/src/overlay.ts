import type { DecodedFrame } from "./types";

/** One highlighted run on a single viewport row: columns `left..=right`
 * (both inclusive), matching core's `SelectionSpan`. Positions only — the
 * blend colour (focused/inactive) is the renderer's policy (#115). */
export interface HighlightSpan {
  row: number;
  left: number;
  right: number;
}

/** Which overlay a highlight belongs to — the renderer picks a blend colour per
 * kind (#115): the live selection, a search-match, or the ACTIVE (current)
 * search match (#429). */
export type HighlightKind = "selection" | "match" | "active";

/** A {@link HighlightSpan} tagged with its overlay kind, ready for the renderer
 * to paint. */
export interface HighlightRect extends HighlightSpan {
  kind: HighlightKind;
}

/** All overlay groups as one kind-tagged list for the renderer — selection
 * rects, then search-match rects, then the active match (#429). The blend
 * colour per kind is the renderer's policy (#115); this is positions + kind
 * only. The active match is *also* present in the match group (the wire keeps
 * it there, #428) — {@link highlightAt}'s ranking, not exclusion, resolves the
 * overlap, mirroring the family renderer (#427). */
export function highlightRects(frame: DecodedFrame): HighlightRect[] {
  return [
    ...selectionHighlights(frame).map((s): HighlightRect => ({ ...s, kind: "selection" })),
    ...matchHighlights(frame).map((s): HighlightRect => ({ ...s, kind: "match" })),
    ...activeMatchHighlights(frame).map((s): HighlightRect => ({ ...s, kind: "active" })),
  ];
}

/** Rank per kind: active > selection > match (#427/#429 — the current search
 * match paints over a selection covering it; a plain match stays under). */
const KIND_RANK: Record<HighlightKind, number> = { active: 3, selection: 2, match: 1 };

/** The highlight kind covering viewport cell `(col, row)`, or `null` if none.
 * Columns are inclusive (`left..=right`). The renderer calls this per painted
 * cell to decide whether to blend its background. Overlapping kinds resolve by
 * {@link KIND_RANK} regardless of listing order. */
export function highlightAt(rects: HighlightRect[], col: number, row: number): HighlightKind | null {
  let hit: HighlightKind | null = null;
  for (const r of rects) {
    if (r.row !== row || col < r.left || col > r.right) continue;
    if (hit === null || KIND_RANK[r.kind] > KIND_RANK[hit]) hit = r.kind;
  }
  return hit;
}

/** `(row, left, right)` viewport triple, the decoder's overlay stride. */
const OVERLAY_STRIDE = 3;

/** Project a frame's live-selection overlay (#108) onto viewport highlight
 * rects. Pure: positions only, no cells, no colour. A frame without a
 * selection yields none. */
export function selectionHighlights(frame: DecodedFrame): HighlightSpan[] {
  return readOverlay(frame.selectionSpans);
}

/** Project a frame's search-match overlay (#108) onto viewport highlight rects
 * — the same stride-3 walk as {@link selectionHighlights}, a separate wire
 * group. Consumed by search (#110/S9). */
export function matchHighlights(frame: DecodedFrame): HighlightSpan[] {
  return readOverlay(frame.matchSpans);
}

/** Project a frame's ACTIVE-match overlay (#428, wire v12) onto viewport
 * highlight rects — the current search match the consumer designated, same
 * stride-3 walk, its own wire group (#429). */
export function activeMatchHighlights(frame: DecodedFrame): HighlightSpan[] {
  return readOverlay(frame.activeMatchSpans);
}

/** Walk a flat `(row, left, right)` overlay directory into highlight spans. */
function readOverlay(flat: ArrayLike<number> | undefined): HighlightSpan[] {
  const spans: HighlightSpan[] = [];
  if (!flat) return spans;
  for (let i = 0; i < flat.length; i += OVERLAY_STRIDE) {
    spans.push({ row: flat[i]!, left: flat[i + 1]!, right: flat[i + 2]! });
  }
  return spans;
}
