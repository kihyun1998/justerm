# Aggregate — cursor

**This note owns no detail.** *"The cursor"* names three separate concepts, and a request phrased that
way is ambiguous across all three. Follow the links; the content lives there.

| Concept | Note | One line |
|---|---|---|
| Where the next glyph lands | [cursor position](cursor-position.md) | `row` / `col`, the deferred last-column wrap, three save slots. Engine-internal |
| What it looks like when it lands | [pen](pen.md) | the SGR template cell stamped into every printed and erased cell |
| What the consumer is told to draw | [caret report](caret-report.md) | visibility, shape, blink **mode** — reported, never animated |

## Why these three sit together

They share a **struct** and a set of **writing verbs**, not a purpose. `Cursor` (`cursor.rs`) holds the
position, the `Pen`, and the visibility / shape / blink the caret report reads; the print path, DECSC /
DECRC and the alt-screen save touch all of them at once. That is the whole relationship. Sharing a
struct reads like sharing a concept until the blast radii are written out — they barely overlap.

## What splits them

Each answers to a different authority.

- **Position** answers to the VT specification and the measured references — DECOM, DECAWM, reverse
  wraparound, the deferred wrap. Wrong here means a silently shifted screen.
- **Pen** answers to the cell model — a template `Cell`, which is why BCE is a property of the fill
  rather than a separate mechanism. Wrong here means wrong colours.
- **Caret report** answers to the consumer boundary
  ([ADR-0017](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)) — mode crosses the wire,
  phase does not. Wrong here means the caret ghosts, blinks when it should not, or is drawn on a
  viewport the user is not looking at.

## The trap this note records

*"Cursor"* is a singular noun hiding a merge. A territory named with an `&` announces that it covers
two things and gets questioned; this one did not, and the three were written as one note until their
blast lists were written out separately and barely overlapped. A field added to `Cursor` is saved and
restored by the alt-screen save, which copies the whole struct, and not by DECSC's `SavedCursor`, which
copies field by field — whether or not it belongs to the position. Check which of the three concepts
it is before adding it.
