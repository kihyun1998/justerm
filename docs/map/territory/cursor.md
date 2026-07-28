# Aggregate — cursor

**This note owns no detail.** It exists because *"the cursor"* names three separate concepts, and a
request phrased that way is ambiguous across all three. Follow the links; the content lives there.

| Concept | Note | One line |
|---|---|---|
| Where the next glyph lands | [cursor position](cursor-position.md) | `row` / `col` + the deferred last-column wrap. Engine-internal |
| What it looks like when it lands | [pen](pen.md) | the SGR template cell stamped into every printed cell |
| What the consumer is told to draw | [caret report](caret-report.md) | visibility, shape, blink **mode** — reported, never animated |

## Why these three sit together

They share a **struct** and a set of **writing verbs**, not a purpose. `Cursor` holds the position
and the `Pen`, and the print path touches both on every glyph — so a change to the print path meets
all three at once even though nothing about their rules is shared.

That is the whole relationship, and stating it is this note's only job. It is also the reason the
three were written as one note first: sharing a struct reads like sharing a concept until you
measure their blast radii, which turn out to have almost no overlap.

## What splits them

Each answers to a different authority.

- **Position** answers to the VT specification — DECOM, DECAWM, reverse wraparound, the deferred
  wrap. Wrong here means a silently shifted screen.
- **Pen** answers to the cell model — it is a template `Cell`, chosen so that enabling BCE later is a
  switch rather than a refactor. Wrong here means wrong colours.
- **Caret report** answers to the **consumer boundary** (ADR-0017) — mode crosses the wire, phase
  does not. Wrong here means the caret ghosts, blinks when it should not, or is drawn on a viewport
  the user is not looking at.

## The trap this note records

*"Cursor"* is a **singular noun hiding a merge**. A territory named with an `&` announces that it
covers two things and gets questioned; this one did not, and it went unnoticed until its three blast
lists were written out separately and barely overlapped. When a note's own §Known holes says *"this
name means three things"*, that is a split waiting to be done, not a documented quirk.
