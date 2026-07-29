# Territory — grid & scrollback

## What it is

Where cells live: a `Vec<Row>` for the visible screen, a `VecDeque<Row>` for history, an inactive
second grid for the alt screen — and the rules for when a line leaves one and enters the other.

The storage the whole engine stands on, and the one whose performance was **measured and
misdiagnosed** before it was understood.

## Governing decisions

- [**ADR-0009 — O(1) scroll in a grid row ring**](../../adr/0009-o1-scroll-in-grid-row-ring.md), and
  its amendment matters more than the original: the in-`Grid` ring it first adopted was **removed**
  once the cost was profiled properly

## Design model

- **Scrollback is not in `Grid`.** `Grid { cols, rows, lines: Vec<Row> }` holds the screen; `Term`
  holds `scrollback: VecDeque<Row>` beside it, plus `scrollback_limit`. The concatenated
  `[scrollback ++ grid]` coordinate space every reader walks is an **indexing convention**, not a
  storage layout — which is why the alt-screen floor has to be reproduced at every walk site.
- **Accrual has a precise condition, and "the screen scrolled" is not it.** A line enters history
  *only* when `scroll_top == 0` **and** the alt screen is inactive. A top-anchored sub-region
  `[0..k]` still accrues; a region with `scroll_top > 0`, the alt screen, and reverse-index never do.
- **The explicit line-editing verbs never accrue.** Even a full-screen `SU` with `scroll_top == 0`
  **drops** its top line rather than pushing it to history. justerm matches xterm.js here — which
  carries a `FIXME` to change it — and *trails* real xterm and alacritty. A deliberate, recorded
  divergence in the direction of the weaker reference.
- **The row buffer is recycled; there is no ring.** The eviction's allocate-and-copy is the
  per-newline cost, not the row shift.
- **The original diagnosis was wrong, and the correction is the useful part.** A flood profile blamed
  `rotate_left`; measured, that moves 24-byte `Vec` *handles* over a bounded screen height —
  sub-microsecond, never the bottleneck. The real cost was `linefeed`'s eviction copying ~2 KB and
  allocating every line, plus an allocate/free **pair** once scrollback is at its cap — which a flood
  is, throughout.

## Code

- `justerm-core/src/grid.rs` — `Grid`, `Row`, `Grid::new`, `set_screen`, `take_lines`, and the
  region-scroll primitives
- `justerm-core/src/term.rs` — `scrollback`, `scrollback_limit`, `alt_grid`, `on_alt`,
  `Term::linefeed` (the eviction), `Term::scroll_region_lines`, `Term::scrollback_len`
- `justerm-core/src/term/walk.rs` — the readers that treat the two as one buffer

## Reference behaviour

**None** in `docs/agents/reference-facts.md`, although two claims here are explicitly comparative:
the accrual condition is *"verified against alacritty `region.start == 0`"*, and the SU divergence
names xterm.js's `FIXME`. Both are exactly the shape this map treats as fragile — a comparison made
once, in prose, about upstream code that moves.

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — the whole reason that
  invariant exists is the storage decision above: one concatenated coordinate space over two logical
  buffers

## Blast radius

- [viewport](viewport.md) — `scrollback_len + rows` is the addressable height, and eviction changes it
  under a consumer's feet
- [selection](selection.md) · [marker](marker.md) — anchors are absolute-line coordinates, so
  eviction is one of the things that move them — and so, less obviously, is a top-anchored
  sub-region scroll that grows history without moving anything on screen ([selection](selection.md)
  carries the set)
- [reflow](reflow.md) — screen and scrollback re-lay as one stream
- [soft wrap](soft-wrap.md) — a wrap link can point from the last scrollback row into the grid, which
  is why the artefact clear has to couple across that seam
- [damage](damage.md) — a scroll is a recorded op, and eviction is what makes it more than a shift
- [vt-interpretation](vt-interpretation.md) — the accrual condition is read by every vertical-motion
  verb

## Known holes / open

- **The SU/SD/IL/DL non-accrual is a known divergence from the stronger references**, matching the
  one that itself carries a `FIXME`. It is recorded in a spec section and in no decision.
- **No pinned reference rows** for either comparative claim (§Reference behaviour), in the territory
  where "what counts as history" is a user-visible contract.
- **`scrollback_limit` is not exposed.** A consumer sees `scrollback_len` and cannot tell whether
  history is being evicted or simply has not grown.
- **ADR-0009's amendment is the live content and the title is not.** A reader arriving at "O(1)
  scroll in a grid row ring" finds a ring that no longer exists.
