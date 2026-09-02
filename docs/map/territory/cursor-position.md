# Territory — cursor position

## What it is

Where the next glyph lands: `row`, `col`, and the deferred-wrap state that makes the last column
behave. Purely engine-internal — a consumer never sets it, and what it *sees* of the cursor is a
different territory ([caret report](caret-report.md)).

## Governing decisions

**None.**

The position rules — clamping, the deferred wrap, the alt-screen save/restore pairing — are governed
by no record. `docs/architecture.md` §"Hidden VT state" describes the deferred wrap as a hazard to
model, which is a warning rather than a decision.

## Design model

Read out of the source; there is no record to read instead.

- **`pending_wrap` is the deferred last-column wrap** (xterm's *wrapnext*). A print that fills the
  last column leaves the cursor where it is and defers the wrap to the *next* print. **Eager wrapping
  here is the classic off-by-one that shifts every subsequent line**, which is why the flag exists at
  all rather than the cursor simply advancing.
- **Position is clamped on set**, to `rows-1` / `cols-1` — so an out-of-range addressing sequence
  yields a degenerate position rather than a panic or an out-of-bounds write.
- **Two cursors exist.** `cursor` and `saved_cursor`, the latter written on alt-screen enter
  (DEC 1049) and restored on leave. The pairing is what makes a full-screen application's excursion
  transparent to the shell underneath it.
- **Origin mode (DECOM) makes addressing relative** to the scroll region's top margin, and clamps to
  it — so the same escape sequence means different absolute rows depending on a mode set earlier.
- Reverse wraparound (DEC ?45) lets a **backspace** at column 0 of a soft-wrapped row move back to the
  end of the previous row — BS only, soft wraps only.

## Code

- `justerm-core/src/cursor.rs` — `Cursor` (`row`, `col`, `pending_wrap`), `Cursor::point` /
  `Cursor::set_point`
- `justerm-core/src/term.rs` — `Term::write_glyph` (sets `pending_wrap`), `Term::backspace`,
  the DECOM / DECAWM / reverse-wraparound mode flags, alt-screen enter/leave save-restore

## Reference behaviour

**One axis, measured; the rest still prose.** `architecture.md` describes the deferred-wrap model as
matching xterm's behaviour, and for most of it that is still prose that was never grepped against a
pinned tree — the single most consequential positional rule here.

- [Backward tabulation, and who clears the deferred wrap](../../agents/reference-facts.md#backward-tabulation-and-who-clears-the-deferred-wrap-826-verified-2026-09-02)
  — **which verbs reset the flag**, measured across all four references by #826. The answer is that
  justerm is the outlier: no reference clears it in CBT, and xterm does not normally clear it in the
  *forward* tab either — its one `ResetWrap` there is gated on the `curses` resource, off by default.
  So the "matches xterm" prose is now known to be wrong on at least this axis, in justerm's favour
  by its own coherence argument and not by the reference's.

Everything else about the model — when `write_glyph` arms it, what consumes it, how it survives a
resize — remains unpinned.

## Cross-cutting invariants

- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md)
  — the caret is a span, and an application can park the cursor on a wide glyph's trailing spacer
  with an ordinary `CUB` / `CHA`, so the position this territory owns can name half a glyph (#454)

## Blast radius

- [soft wrap](soft-wrap.md) — `pending_wrap` is the **entry condition** for the wrap path. Changing
  when it is set changes which rows carry a wrap link
- [damage](damage.md) — a pure cursor move changes no cell content, so the content-only damage model
  misses it by design and the frame producer folds the old and current cells in
- [pen](pen.md) — they travel together in the same struct and are written by the same verbs, but the
  coupling is only that: a position change does not change appearance

## Known holes / open

- **Zero governing records**, for rules whose failure mode is a silently shifted screen.
- **The deferred-wrap rule survives only as a field comment.** It is the kind of thing a reader
  "simplifies" — advancing the cursor eagerly looks equivalent and is not.
- **DECOM's interaction with the clamp is unspecified** in any artifact: origin mode clamps to the
  region, `set_point` clamps to the screen, and no document states which applies when both do.
