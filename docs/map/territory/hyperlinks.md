# Territory — hyperlinks

## What it is

Two unrelated ways a cell becomes clickable, deliberately kept apart:

- **OSC 8** — the application *declares* a link, so the engine stores which cells carry which URI
- **Plain-text URL detection** — nobody declared anything, so the consumer runs a regex over
  assembled text and decides

The first is per-cell state that survives in the buffer; the second is a policy applied to a snapshot.
They share a name and almost nothing else.

## Governing decisions

**None.**

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  the split above *is* this record applied twice: storing declared links needs the buffer, so it is
  core; deciding what counts as a URL is policy, so it is not. Core has no regex dependency as a
  result
- [ADR-0020 — what qualifies for the frame snapshot](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  — why OSC 8 is **not** a consumer event: a hyperlink is per-cell state, not a point-in-time
  notification. The clearest worked example of its state-versus-event rule

## Design model

- **A declared link is an index, not a string.** The cell carries `LINK_PRESENT` and the row's link
  map holds a column-keyed index into a buffer-wide `hyperlink_pool` — so the same URI repeated
  across a thousand cells costs one string.
- **`LINK_PRESENT` occupies xterm's `HAS_EXTENDED` slot**, which is the sibling of the combining-mark
  bit: both are presence flags gating a row-keyed side map.
- **The wire re-indexes per frame.** `hyperlink_pool` indices are buffer-global; a frame carries only
  the URIs it needs, remapped to frame-local `link_table` positions. A consumer never sees the pool.
- **Detection is the inverse arrangement.** The engine assembles the viewport's logical-line text plus
  a per-character cell map — it has the whole buffer and the consumer does not — and the consumer runs
  the regex and `new URL()` validation over that text, mapping matches back through the cells.
- **Neither path knows about the other.** A detected URL is not stored, and a declared link is not
  re-validated. Whether a cell can be both is answered by nothing.

## Code

- `justerm-core/src/cell.rs` — `LINK_PRESENT`
- `justerm-core/src/grid.rs` — `Links`, the row's column-keyed link map
- `justerm-core/src/term.rs` — `hyperlink_pool`, `Term::hyperlink`, and the per-frame remap into
  `link_table`
- `justerm-core/src/serialize.rs` — `Frame`'s `link_table`
- `justerm-web/src/links.ts` — the URL regex and validation policy, over
  [logical lines](logical-lines.md)

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. `LINK_PRESENT` is described as occupying xterm's
`HAS_EXTENDED` slot — a concrete claim about another implementation's bit layout, in a comment, with
no pinned row.

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — the link map is one of the three, under
  the same presence-bit discipline: read only through the gate, ride with the row, and a write that
  clears the cell owes the bit rather than the map

## Blast radius

- [logical lines](logical-lines.md) — detection consumes that shape, including its off-screen context
  rows, so a change to trimming changes which URLs match
- [wire format](wire-format.md) — `link_table` is a per-frame side table, and the remap is what keeps
  the cell record fixed-width
- [events & replies](events-and-replies.md) — the boundary partner: OSC 8 is the worked example of
  what is *not* an event
- [reflow](reflow.md) · [soft wrap](soft-wrap.md) — the link map rides the row through both, and a URL
  detected across a wrap join spans rows the consumer must highlight separately

## Known holes / open

- **Zero governing records** for either path, including the pool-and-index design that makes repeated
  URIs cheap.
- **Nothing states whether the two paths may overlap.** A cell inside a declared OSC 8 link is also
  text that the URL regex will match; which one a click follows is undefined by every artifact.
- **The `HAS_EXTENDED` claim is unpinned**, and it is a statement about another project's bit layout.
- **Detection is viewport-only by construction.** A URL entirely in scrollback is never detected,
  because the consumer only ever sees assembled *viewport* lines — a limitation no document states.
