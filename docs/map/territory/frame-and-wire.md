# Aggregate — frame & wire

**This note owns no detail.** Two concepts, one boundary crossing.

| Concept | Note |
|---|---|
| What the engine hands the consumer per cycle | [frame](frame.md) |
| The bytes it travels in | [wire format](wire-format.md) |

## Why they sit together

They are the **same crossing at two moments**: what may leave the engine, and how it is packed. A
change to one usually forces the other — a new overlay group is both a `Frame` field and a
`WIRE_VERSION` bump — which is exactly why they read as one thing and why keeping them apart matters.

The distinction that only survives when they are separate: **a dependency on the format is not a
dependency on the snapshot.** The renderer needs the byte layout; the a11y policy needs
`alt_screen`. Those are different edges, and a merged note cannot express either without implying the
other.

## The asymmetry worth knowing before you touch either

Together these two are the most heavily recorded area in the map — six ADRs and a qualification gate
(ADR-0020) — while [selection](selection.md), [caret report](caret-report.md) and
[search](search.md) have none between them.

That is not because the frame is harder. It is because **a change here forces a version bump**, and a
version bump is an event that demands a record. Areas whose mistakes ship silently accumulate no
records at all; the ones that force a ceremony accumulate them automatically.

Read that as a warning about the empty sections elsewhere in this map, not as reassurance about this
one.
