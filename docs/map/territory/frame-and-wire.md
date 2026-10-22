# Aggregate — frame & wire

**This note owns no detail.** One boundary crossing, seen at three moments.

| Concept | Note |
|---|---|
| What the engine hands the consumer per cycle, and what may ride it | [frame](frame.md) |
| The bytes it travels in, and what `decode` rejects | [wire format](wire-format.md) |
| Who reads those bytes on the JS side, and on which published version | [WASM decoder](wasm-decoder.md) |

## Why they sit together

They are the **same crossing at three moments**: what may leave the engine, how it is packed, and
how the far side reads it. A change to one usually forces the others — a new overlay group is a
`Frame` field, a `WIRE_VERSION` bump and a new getter — which is why they read as one thing and why
keeping them apart matters.

The distinction that only survives when they are separate: **a dependency on the format is not a
dependency on the snapshot.** The renderer needs the byte layout; the a11y policy needs
`alt_screen`; the web widget needs a *published* decoder that already has the getter. Those are
different edges, and a merged note cannot express one without implying the others.

## The asymmetry worth knowing before you touch any of them

Together these are the most heavily recorded area in the map — seven decision records
(ADR-0005, 0008, 0013, 0014, 0015, 0016, and the qualification gate ADR-0020) — while
[selection](selection.md) and [search](search.md) have one between them that governs a single axis
(ADR-0026, outside coordinates), and [caret report](caret-report.md) has none. (Until 2026-10-22
this note said those three had none; ADR-0026 has since reached the first two.)

That is not because the frame is harder. It is because **a change here forces a version bump**, and a
version bump is an event that demands a record. Areas whose mistakes ship silently accumulate no
records; the ones that force a ceremony accumulate them automatically. Even here the ceremony lapsed:
v9–v18 were admitted with no ADR of their own, recorded only in the comment on `VERSION`.

Read that as a warning about the empty sections elsewhere in this map, not as reassurance about this
one.
