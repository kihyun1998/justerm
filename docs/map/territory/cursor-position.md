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

**None.** No entry in `docs/agents/reference-facts.md`. The deferred-wrap model is described in
`architecture.md` as matching xterm's behaviour, but that is prose — never grepped against a pinned
tree, and it is the single most consequential positional rule here.

## Cross-cutting invariants

*(none identified yet)*

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
