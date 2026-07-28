# Territory — cursor

## What it is

Where the next glyph lands, what appearance it lands with, and what the consumer is told to draw as a
caret. Three things share the name and only the first is obviously "the cursor":

1. **Position** — `row` / `col`, plus the deferred-wrap state
2. **Pen** — the current SGR appearance copied into each printed cell
3. **Caret report** — visibility, shape and blink *mode*, reported on the frame; the engine never
   animates

## Governing decisions

**None.**

This is the sharpest vertical hole measured in this repo: `cursor` is **mentioned in 19 of the 25
ADRs and is the subject of none**. Grep says "well covered"; nothing governs it.

Adjacent records that do not govern it:

- [ADR-0003 — damage model](../../adr/0003-damage-model-incremental-bounds.md) decides the damage
  grain, and the cursor's old+new cell fold rides that mechanism — but the *rule* ("fold the last-acked
  cursor cell in, but only when it moved") lives in a code comment, not in 0003
- [ADR-0019 — cell composition model](../../adr/0019-cell-composition-model.md) governs how the
  renderer resolves a cell's ink, which is where a cell-invert caret is realised — renderer-side, and
  it does not decide what the engine reports

## Design model

Read out of the source; there is no record to read instead.

- **The pen is a template cell.** `Pen { fg, bg, flags, underline_color }`, and `Pen::cell(c)` stamps
  it onto a glyph. Modelled after Alacritty deliberately: making erase (ED/EL) fill with `bg` instead
  of `Default` *is* BCE (background colour erase), with no structural change.
- **`underline_color` is in the pen but not in the cell.** The 12-byte cell is full, so the print path
  stamps a non-default value into the row's ucolor map — see
  [row-keyed side maps](../invariant/row-keyed-side-maps.md).
- **`pending_wrap` is the deferred last-column wrap** (xterm's *wrapnext*). A print that fills the last
  column leaves the cursor put and defers the wrap to the *next* print. Eager wrapping here is the
  classic off-by-one that shifts every subsequent line.
- **The engine reports the mode, never the animation.** `visible` (DEC ?25), `shape`
  (DECSCUSR, `Block` default), `blink` (att610 ?12) are state; the blink phase is the consumer's.
- **The cursor is invisible while scrolled up** — the frame reports
  `cursor_visible && display_offset == 0`.
- **Position is clamped on set**, to `rows-1` / `cols-1`.
- **Two cursors exist**: `cursor` and `saved_cursor`, the latter saved on alt-screen enter (DEC 1049)
  and restored on leave.

## Code

- `justerm-core/src/cursor.rs` — `Pen`, `CursorShape`, `Cursor`, `Cursor::point` / `set_point`
- `justerm-core/src/term.rs` — `Term::cursor`, `Term::frame_damage` (the old+new fold),
  `Term::reset_damage` (advances `prev_cursor` — the ack is what defines "old"), `Term::write_glyph`
  (pen → cell, ucolor stamp)

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — `underline_color` reaches the row's
  ucolor map, not the cell

## Blast radius

- [damage & viewport](damage-and-viewport.md) — a *pure* cursor move changes no cell content, so
  `damage()` (content-only) misses it by design and `frame_damage()` folds in the old + current cells.
  Changing either the fold or what `reset_damage` stores as `prev_cursor` produces caret ghosting
- [wide glyph & soft wrap](wide-glyph-and-soft-wrap.md) — `pending_wrap` is the entry condition for
  the wrap path, so wrap-rule changes meet the cursor here
- **frame / wire** *(no note yet)* — visibility, shape and blink mode are frame fields; adding a caret
  property is a wire-version question under ADR-0020
- **renderer** *(no note yet)* — the caret is drawn there. Scalar policies (`setCursorContrast`,
  `setCursorThickness`) are renderer-side and consume only what the frame reports

## Known holes / open

- **Zero governing records**, against the highest mention count in the ADR corpus. The rules most at
  risk of being re-derived are `pending_wrap`'s deferral and the old+new damage fold — both currently
  survive as code comments.
- **"Cursor" names three things** (position / pen / caret report) and no artifact says so. A change
  request phrased as "the cursor" is ambiguous across all three.
- **Blink is split** — the engine reports the mode, the consumer owns the phase. This is the correct
  ADR-0017 split but it is nowhere recorded as a decision, and the consumer side re-derived it once
  already (blink pushed as an `isVisible` phase).
