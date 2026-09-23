# Territory — reflow

## What it is

Re-splitting soft-wrapped logical lines when the width changes, and carrying every tracked point
through the move. A resize does not merely crop — on the primary screen it re-lays the whole buffer,
scrollback included, so a long line keeps its tail instead of losing it at the old margin.

**This territory was invisible to the first pass of this map.** It has no dedicated module, its code
sits inside `term.rs` and `grid.rs`, and no public method is named for it — you reach it only through
`resize`.

## Governing decisions

**None.**

- [ADR-0025 — row and wide-pair cell state ownership](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  bounds it from one side: D4 governs the verbs that *edit*, and reflow is a **reallocation**, so a
  lead left without its spacer here is a legal state rather than a violation
- `docs/architecture.md` §"Hidden VT state" carries two entries that are this territory's contract —
  *"the alt screen resizes but does not reflow"* and *"soft-wrap vs a hard line-end must be
  distinguished for reflow"*

## Design model

- **Primary reflows; the alt screen does not** (#567). The alt pane is re-fit only — rows dropped or
  added to reach the new size, nothing re-wrapped — because a full-screen application places its own
  lines and re-wrapping them changes what it drew.
- **Reflow is not gated on DECAWM**, deliberately, and ghostty gates its equivalent (`.reflow =
  self.modes.get(.wraparound)` in `terminal/Terminal.zig`'s `resize`) — on the reading that an
  application turning autowrap off is placing lines itself, so its content is a layout rather than a
  flow. Nothing observable is known to break either way: with DECAWM off a full 6-column row still
  re-splits into two at width 3, where ghostty truncates it. Three grounds. **The wrap flag is not a
  lie** — `Row::is_wrapped` means "continues into the next", true after a re-split; DECAWM governs the
  write path, not how stored content is laid out again, and dropping the flag would extract
  `"abcdef"` as `"abc\ndef"`. **The mode is global and momentary; the buffer is neither** — read at
  resize time it would decide the fate of history written under the opposite setting, so a TUI
  turning it off while drawing would leave every wrapped line in scrollback un-reflowed. **It costs
  content** — not reflowing truncates each row. And there is no per-row signal to be finer with: a
  row written under DECAWM off and one that merely ended early are both unwrapped, and a line that
  exactly fills its width carries no wrap flag either.
- **Tracked points travel with the content.** `reflow` takes a `points` slice — the cursor, selection
  anchors, markers — and returns where each landed. Anything anchored to a line has to be in that
  slice or it is silently wrong afterwards.
- **Scrollback and screen reflow as one stream**, which is what makes the concatenated coordinate
  space coherent across a resize rather than only within the visible grid.
- **Query-derived state is invalidated, user-authored state is re-anchored.** Search highlights are
  dropped and the consumer re-searches; the selection is carried. The engine can recompute neither,
  but only one is reproducible by the consumer.
- **A point one past the last cell cannot be expressed** (#562). Five designs were built, measured
  and rejected — that issue is the content, not a pointer to it.
- **`resize` also rewrites configuration that is not anchored to a line at all**, and the axis that
  decides each one is whether the value's *meaning* survives the geometry change. The DECSTBM scroll
  region is a range over the current screen, so it is **reset** — but only when the geometry actually
  moved, since a resize to the size the terminal already has changes no meaning. The deferred wrap is
  cursor state, expressible at any geometry, so it is **carried** (#848). The tab-stop table is a set
  of marks that still mean what they meant, so it is **extended, never rebuilt or trimmed** (#849).
  The line it replaced rebuilt the table from defaults on every call, so a window one row taller — or
  a consumer re-asserting its size — replaced an application's stops with multiples of eight, on an
  axis the table is not indexed by. New columns take the default ladder at their *absolute* index;
  nothing is trimmed, so `tabs.len()` is the widest the terminal has been and the walks need
  `len() >= cols`, not equality. A stop a narrowing pushed outside the grid returns when it widens.
  The corpus splits 2-2 on that half: xterm keeps it structurally (`MAX_TABS` 1024, independent of
  the screen, `ptyx.h:3611` @ `6380a3e`) and xterm.js in a sparse map, while alacritty truncates
  through `Vec::resize_with` (`alacritty_terminal/src/term/mod.rs:2341` @ `852e971`) and ghostty
  rebuilds on a column change (`src/terminal/Terminal.zig:3759` @ `e6e26e1`). No tie-breaker row
  covers the axis, so the call is the maintainer's, recorded on #849. RIS still restores the default
  ladder, since `full_reset` takes the table from the constructor.
  The three were decided one at a time and separately; see *Known holes*.
- **A cursor that reflows to "just after the content" on a full pane buys its row from history**
  (#562). `reflow` may answer `col == cols`; the cursor reads it as the start of the next row, which
  the caller's fit supplies while the pane is shorter than the screen. When the content already fills
  the pane, the pane scrolls one row into history — without it the cursor was pulled back onto the
  last glyph and the next byte destroyed a character (a prompt at the bottom of a full screen). Five
  earlier designs made `reflow` itself materialise the row and were rejected on measurements (a
  cursor at column 59 resized to width 4 emptied the buffer; a blank-line exemption turned 22 alt
  lines into 21): `reflow` cannot see the pane's budget. The gate is `limit > 0`, not "is this the
  alt screen" — since #567 alt panes pass `limit: 0` — which **amends** ADR-0025: `reflow` does not
  create rows, the seam may when the pane can pay. A tracked line is bounded at
  `split + dims.rows - 1` here, where the final geometry is known; bounding against `reflow`'s own
  row count clamped away rows the fit was about to create.
- **The alt pane re-fits without re-splitting** (`reflow: false`, #567): its content is a layout,
  and all three references take the same position with one flag on the same resize function. Measured
  on an `htop` recording across a live `SIGWINCH`, re-splitting left debris in cells htop does not
  overwrite, because it repaints without clearing.

## Code

- `justerm-core/src/grid.rs` — `reflow`, which takes and returns the tracked `points`
- `justerm-core/src/term.rs` — `Term::resize`, `ReflowDims`, `PaneReflow`, `reflow_pane`
- `justerm-core/src/lib.rs` — `Engine::resize`, whose doc comment is the consumer-facing contract

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Mapping a tracked point through reflow](../../agents/reference-facts.md#mapping-a-tracked-point-through-reflow-549-verified-2026-07-27)
- [Relocating a cluster that grew to width 2](../../agents/reference-facts.md#relocating-a-cluster-that-grew-to-width-2-529-verified-2026-07-28)

The DECAWM divergence is argued at the call site against ghostty's `Terminal.zig`, with the
counter-evidence measured — a full row still re-splits with DECAWM off, and ghostty truncates instead.
That reasoning is in a code comment rather than in a record.

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) — this
  territory is where that fact stops being cosmetic and becomes **irreversible**, which is what
  settled its repair in #810. A hidden element measures `0x0`, the fit paths used to floor that to a
  `2x1` grid, and a resize is where a proposed grid becomes a change to the buffer: on the primary
  screen the re-split preserves logical lines, so re-widening restores the content; on the **alt
  screen** a resize is a re-fit — rows dropped, nothing re-wrapped (see the second bullet under
  *Design model*) — so there is nothing to restore from. The invariant is about absence producing a
  *plausible* answer; here the plausible answer also cannot be taken back

## Blast radius

Everything anchored to a line, because reflow is the one operation that moves content **between**
rows rather than within one.

- [selection](selection.md) — anchors are tracked points, and reflow is one of the four things that
  move an absolute coordinate (that note carries the set; a three-item count hides one of them)
- [marker](marker.md) — same, and alt markers additionally re-anchor on a base that shifts when the
  primary scrollback rewraps beneath them
- [search](search.md) — highlights are invalidated rather than moved, which is the asymmetry above
- [soft wrap](soft-wrap.md) — the wrap links are the input; a re-split writes new ones
- [wide glyph](wide-glyph.md) — `Row::resize` cuts through pairs and D4 stops at this boundary
- [cursor position](cursor-position.md) — the cursor is a tracked point
- [viewport](viewport.md) · [damage](damage.md) — a resize marks the whole screen damaged
- [vt interpretation](vt-interpretation.md) — **the axis the list above cannot reach.** Everything
  named so far is anchored to a *line*, so nothing here points at the fields `resize` rewrites that
  are indexed by columns and rows: the tab-stop table and the scroll margins. That absence is the
  structural reason the tab table was rebuilt on a rows-only resize through #826 and #848 without
  anyone standing in this note seeing it. The edge already existed in the other direction — that
  note's blast radius names reflow — and this is the return leg

## Known holes / open

- **Zero governing records** for the operation with the widest blast radius in the engine.
- **The DECAWM divergence is deliberate and unrecorded.** It diverges from a named reference with
  three stated reasons, held only by this note — the exact shape ADR promotion exists for.
- **#562 — a point one past the last cell has no representation.** Five rejected designs; read the
  issue before touching relocation.
- **Nothing states which point sets must be passed.** The `points` slice is a convention: forget to
  include an anchor set and it is silently misplaced, with no compiler or test naming the omission.
- **Which fields a resize may reset at all has no record**, though the three fields that raised it
  are now each decided (#848 the deferred wrap, #849 the tab table, and the margins here). The
  counterpart record exists for the *other* reset —
  [RIS keeps configuration, drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — and does not transfer: every row there reasons from *"RIS wipes every cell"*, and a resize wipes
  none.

  **A formulation that does not break is available and is deliberately not promoted yet.** The axis
  is *not* who wrote the value — that reading derives #848 and #849 and then fails on the margins,
  which the application wrote and which reset anyway. It is whether the value's **meaning survives
  the geometry change**: a range over the screen does not, a set of marks does, cursor state does, a
  coordinate is clamped, derived state is rebuilt. That derives all seven fields this function
  touches. What it does not do is *buy* anything — the one question it predicted (the margins on a
  no-op) was already answered 3/3 by the references and is now fixed — so promoting it would be
  archaeology over closed issues. The trigger to write it is a **fourth** field whose answer the
  references do not already give.
