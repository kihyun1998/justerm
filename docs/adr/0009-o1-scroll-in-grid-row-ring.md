# ADR-0009: Scroll eviction via a move+recycle value-handshake — the row ring was measured as a regression and dropped

Status: accepted (2026-06-23); **amended (2026-06-23) — the in-Grid ring this ADR originally adopted was
implemented, benchmarked, and found to be a net regression; it is dropped. The *value-handshake / row
recycling* is kept; the *ring* (`zero`/`phys`) is not. See the amendment at the end — it supersedes the
"in-Grid row ring" parts of the Decision and Consequences below. The Context, the rejection of the
unified ring (Route B), and the handshake design remain valid; read the original text as the reasoning
that led to the handshake, and the amendment for why the ring half was wrong.**

## Context

`#41` profiled terminal-native render throughput end-to-end (PenTerm, the web consumer, flooding 10 MB
of synthetic output through the real backend → IPC → webview pipeline against justerm 0.3.0). With all
consumer-side coalescing in place (rAF render coalescing + a ~16 ms backend frame cadence), the
dominant cost is `Engine::feed` — ~90% of backend time; `encode` and Channel-`send` are negligible
(IPC was never the bottleneck). The decisive clue: **`ascii` — the *simplest* input — is the *slowest*
feed** (958 ms vs 765 ms ansi / 762 ms cjk per 10 MB). If parsing dominated, ascii would be *fastest*;
it is slowest because its lines are shortest, so a fixed byte volume is the *most newlines* → the *most
scrolls*. So `feed` time tracks **newline count, not parse complexity** — the cost is per-scroll
(~10 MB/s).

Root cause is in `grid.rs`: `scroll_up_region` / `scroll_down_region` do
`self.lines[top..=bottom].rotate_left(1)` — an **O(rows) row-handle move per scrolled line**. `Row` is
`Vec<Cell>`, so `rotate_left` moves the 24-byte `Vec` handles (not the cell bytes), but a 10 MB ascii
flood is ~160k newlines × O(rows) ≈ millions of handle moves ≈ the ~960 ms. A second, smaller per-scroll
cost rides along: `linefeed` does `self.grid.row(0).to_vec()` to copy the evicted top row into
scrollback — a per-newline allocation. The `grid.rs` module comment already anticipated the fix ("the
ring … can move whole rows in/out cheaply"); this ADR settles *which* ring, before building it.

Three reference implementations were read at the source level and converge:

- **alacritty `grid/storage.rs` + `grid/mod.rs`.** `Storage<T> { inner: Vec<Row>, zero, visible_lines,
  len }` holds **screen + scrollback in one ring**. `compute_index` maps a logical `Line` to a physical
  slot by adding `zero` and wrapping with a *conditional subtraction* (`if zeroed >= inner.len() { zeroed
  - inner.len() } else { zeroed }`) — faster than `%`. Full-screen scroll is `raw.rotate(-(positions))`
  (advance `zero`, O(1)); the rotated-out rows are **recycled in place** and `reset(&template)`-cleared,
  no allocation. The **region** case (`region.start != 0`) falls back to explicit
  `raw.swap(i, i + positions)` within the bounded region — O(region), touching neither history nor the
  display offset.
- **xterm.js `common/CircularList.ts`.** `{ _array, _startIndex, _length, _maxLength }`;
  `_getCyclicIndex(i) = (_startIndex + i) % _maxLength`. `push` when full overwrites the oldest and
  advances `_startIndex`; `recycle()` returns the displaced element for reuse — the steady-state
  zero-allocation pattern.
- **wezterm** keeps an equivalent scrollback ring.

The convergence — *(a) full-screen = advance an offset O(1); (b) region = swap O(region) fallback;
(c) recycle the scrolled-out row instead of allocating* — is a non-arbitrariness signal (CLAUDE.md:
1-principle + named prior-art cross-check). What the references do *not* settle for justerm is the
**boundary question**: alacritty/xterm.js unify screen + scrollback in **one** ring; justerm already
**splits** them — `Grid.lines: Vec<Row>` (screen only) and `Term.scrollback: VecDeque<Row>` (history).

### Non-goal — fidelity to alacritty's unified ring is *not* the rationale

The tempting reading is "alacritty unifies screen+scrollback in one `Storage`, so justerm should too"
(**Route B**). Before adopting that analogy, verify its *shared cause* (the discipline that says surface
similarity ≠ same principle). alacritty unifies for two concrete reasons: its renderer reads a **single
viewport coordinate space that straddles the screen↔history boundary**, and it wants **cross-boundary
row recycling** (a scrolled-out screen row literally becomes a history row in the same physical slot).
Neither cause is present in justerm:

- justerm's renderer (beamterm) consumes a **viewport snapshot + damage** (ADR-0003/0005/0008) — the
  wire format already erases the screen-vs-history distinction; the consumer never sees the boundary.
- justerm already has a **unified absolute coordinate** for the consumers that need one (selection,
  search): the `scrollback.len() + screen_row` mapping, computed on demand. The benefit alacritty buys
  with physical unification, justerm already holds in its mapping layer.

So the analogy's shared cause is **absent** → it does not compel Route B. The O(1) win is *orthogonal*
to unification: it comes entirely from not `rotate_left`-ing the screen `Vec`, which is local to `Grid`.

### Why the screen↔scrollback split is the correct grain, not an accident

The two structures have **different access patterns and lifetimes**, a real domain seam:

| | screen (`Grid.lines`) | scrollback (`Term.scrollback`) |
| --- | --- | --- |
| access | random read/write 2D (goto, ICH/DCH, ED/EL, BCE, wide-char) | append-only FIFO (push back, evict front) |
| mutated by | nearly every VT op | scroll-out + cap eviction only |
| identity | fixed `rows × cols` | unbounded-until-cap, read-only history |

Unifying them (Route B) would merge a random-write grid with an append-only FIFO into one structure and
force `Grid` to know the history limit + eviction policy — a single-responsibility regression — and would
require rewriting every absolute-coordinate consumer (selection / search / `viewport_row` / damage). The
alt screen has **no scrollback** (an established invariant, Hidden VT state §Alt-screen), so a unified
ring imposes an abstraction the alt grid does not need. Reflow already joins `scrollback ++ screen` into
one stream **only where it must** (`reflow_pane`) — the one place unification pays, without paying for it
everywhere. "Perfect = the *correct* grain, not the *maximal* one" (CLAUDE.md).

## Decision

Adopt **Route A — ring the screen `Grid` only, keep `Term.scrollback` a separate `VecDeque`, and cross
the boundary with an explicit value-handshake.**

- **In-Grid row ring.** `Grid` gains a `zero: usize`; `lines` is a ring of exactly `rows` rows. Every
  accessor maps a logical row through `phys(r) = zero + r` wrapped by **conditional subtraction**
  (alacritty's trick, not `%`, since `zero + r < 2·rows` always): `cell`, `cell_mut`, `row`, `row_mut`,
  `clear`, and the reflow bridge.
- **Full-screen scroll is O(1) via a value-handshake.** The hot path (`linefeed` with `scroll_top == 0`,
  primary screen) calls one Grid method that advances `zero` and exchanges rows by *move*, never copy:

  ```rust
  // Grid: advance the ring one line; install `blank` at the new bottom,
  // return the evicted top row. Grid knows nothing of scrollback/limits/policy.
  fn scroll_up_recycle(&mut self, blank: Row) -> Row {
      let evicted = std::mem::replace(&mut self.lines[self.phys(0)], blank);
      self.zero = self.wrap(self.zero + 1); // O(1)
      evicted
  }
  ```

  `Term` owns the **policy**: it supplies a cleared `blank` (recycled from the `pop_front`ed cap-eviction
  row when scrollback is at its limit, else a fresh alloc while below the cap — xterm.js `recycle`), and
  decides that the returned `evicted` row enters scrollback. `Grid` stays a pure `rows × cols` grid; the
  boundary is crossed by a one-way value exchange, not a callback or an internals-reach. This also
  deletes the per-newline `row(0).to_vec()` copy — eviction becomes a move.
- **Region scroll stays O(region) via swap.** When `scroll_top > 0`, on the alt screen, or for any
  DECSTBM sub-region, scrolling shifts rows *within* the region (it does **not** enter scrollback —
  Hidden VT state §"Scrollback accrues only on a top-anchored, primary-screen scroll"). These keep the
  current row-`rotate`/swap within `[top..=bottom]` — alacritty's own `region.start != 0` fallback, and
  explicitly permitted by #41 ("region scrolls … can stay O(region)"). They are not the hot path.
- **RI / `scroll_down_region` stays the region path.** Reverse-index never accrues scrollback, so it
  needs no handshake; the existing in-region shift is kept (O(region), rare).

## Consequences

- **Hot path is O(1) per line.** A feed-heavy bench (10 MB short lines) becomes parser-bound, not
  scroll-bound; `feed` time stops correlating with newline count (ascii ≈ ansi ≈ cjk per MB) — #41's
  acceptance. Steady-state is **zero-allocation** (the cap-evicted row recycles into the new blank).
- **Wire format and `DecodedFrame` unchanged.** `record_scroll(top, bottom, n)` and damage are emitted
  in **logical** coordinates, which the ring preserves exactly; the consumer sees an identical scroll op
  and identical cells for identical input (ADR-0003/0005). No `WIRE_VERSION` bump.
- **Core purity preserved.** `Grid` gains no knowledge of history, limits, or eviction; that policy stays
  in `Term`. The two modules remain independently reasoned-about — the test of the right boundary.
- **The hidden cost is the `phys()` mapping, and it must be total.** *Every* Grid accessor maps through
  the offset; one un-mapped path is silent corruption. Specific obligations (recorded in
  `docs/architecture.md` §Hidden VT state, [#41]): `take_lines`/`set_screen` (reflow/resize) must
  **linearize** the ring (rotate to `zero = 0`) before/after handing rows to `grid::reflow`, which
  assumes logical order; `clear` (alt entry) resets `zero = 0`; the alt grid and primary grid each carry
  their **own** `zero` across `mem::swap` (it is a `Grid` field); a region scroll with `scroll_top > 0`
  must **not** advance `zero` (it would desync rows outside the region).
- **Reversible toward Route B only on a real feature need.** If justerm ever needs **stable row identity
  across the screen↔history boundary** (e.g. an image/sixel anchored into scrollback, or boundary-blind
  reflow), the unified `Storage` ring becomes the right model and this ADR is revisited. Nothing in #41's
  scope needs it; per CLAUDE.md ("bones correct from day one, tail grows by dogfood") the correct
  boundary to commit *today* is the split with a clean handshake.

## Alternatives considered

- **Unified screen + scrollback ring (Route B, alacritty `Storage` 1:1).** Rejected — it merges two
  structures with different access patterns/lifetimes, regresses `Grid`'s single responsibility, imposes
  a scrollback abstraction on the scrollback-less alt screen, and forces a rewrite of every
  absolute-coordinate consumer. The unification's payoff (cross-boundary viewport + recycling) is already
  held by justerm's on-demand `scrollback.len() + row` mapping, so the analogy to alacritty lacks a
  shared cause. Faithfulness to alacritty is a non-goal (above); fitness to justerm's seam is the goal.
- **Keep `rotate_left`, shrink the per-row cost.** Rejected — it is O(rows) by construction; no constant
  factor turns ~10 MB/s into parser-bound. The algorithm, not the constant, is the cost.
- **Flatten the screen to one contiguous `Vec<Cell>` and `memmove` on scroll.** Rejected — still O(rows ×
  cols) bytes moved per scroll (worse than moving 24-byte handles), and it forfeits the cheap whole-row
  move the ring and reflow both rely on (`Row` as a movable unit).
- **Callback from `Grid` into `Term` for eviction.** Rejected — it leaks the boundary the value-handshake
  keeps clean; `Grid` must not call back into history policy.

## Amendment (2026-06-23) — the ring was measured as a regression; recycle without it

The Decision above was implemented in two commits (the ring in `af15a87`, the handshake in `8e370d5`)
and then benchmarked. **The ring is a net regression and has been dropped** (`1fa3b14`); only the
move+recycle handshake survives. What the original reasoning got wrong:

### What the benchmark showed

`benches/throughput.rs`, 24 rows, criterion `--baseline` (paired, same session):

- **Default cap, ring vs pre-`#41` baseline:** `ascii` **−21 %** (CI −26 %…−17 %), `cjk` **−6 %**
  (CI −6.8 %…−5.0 %) — confident *regressions*; `ansi`/`scrolling` within noise.
- **Cap-hitting (`scrollback_limit = 100`, so eviction churns every line — what a real flood does),
  three variants:** the ring was the *slowest* of the three on most inputs (e.g. `scrolling` 107 vs the
  pre-`#41` baseline's 112 MiB/s); the **no-ring move+recycle was fastest** (`scrolling` ~140, `ascii`
  ~144 MiB/s), beating both the ring and the original `to_vec` path.

### Why the ring lost — two errors in the original analysis

1. **The O(rows) it removed was already free.** `lines: Vec<Row>` with `Row = Vec<Cell>`, so
   `rotate_left` moves 24-byte `Vec` *handles*, **not** cell data — and `rows` is small *and bounded*
   (a screen is ~24–100 rows; scrollback is a separate `VecDeque`, never thousands of in-grid rows).
   Big-O describes growth as N→∞; with N small and fixed, the constant dominates, and a 24-handle
   `rotate_left` is sub-microsecond. The "Keep `rotate_left`" alternative was rejected with *"the
   algorithm, not the constant, is the cost"* — **that was exactly backwards.** This is precisely the
   under-reach the unified-ring analogy was checked against (CLAUDE.md "shared cause"): alacritty's
   `Storage` ring earns its keep because *its* buffer holds screen **+ thousands of scrollback rows**, so
   *its* rotate really is O(thousands). justerm split them, so the ring's premise never transferred —
   and the original ADR stopped one step short of concluding "therefore the ring buys nothing here."
2. **The ring taxed the actually-hot path.** Making `lines` a ring forces every cell access through
   `phys()`. Cell writes (printing) outnumber scrolls by orders of magnitude, so the ring moved cost
   *onto* the hot path to shave a rare, already-free one — a net loss, worst on print-heavy `ascii`.

### The real cost, and the real fix

`#41`'s ~960 ms was **not** `rotate_left`. It was the per-newline eviction: `grid.row(0).to_vec()`
(copy ~2 KB + allocate) every line, plus an alloc/free **pair** every line once scrollback sits at its
cap (a 10 MB / ~160 k-line flood is at cap the whole time). That is a *constant-factor / allocator*
problem, not an algorithmic one. The fix is to **recycle the row buffer**, which needs no ring:

```rust
// Grid: keep the cheap rotate_left; move row 0 out, swap a recycled blank into the bottom slot.
pub(crate) fn scroll_up_recycle(&mut self, mut blank: Row) -> Row {
    blank.clear();
    blank.resize(self.cols, Cell::default());
    self.lines.rotate_left(1);                 // 24-byte handle moves — negligible
    let last = self.rows - 1;
    std::mem::replace(&mut self.lines[last], blank)   // evicted row out by move (no copy)
}
```

`Term` still owns policy (parks the cap-evicted row in a `recycled_row` spare and feeds it back as the
next blank → zero per-line alloc/copy in steady state). The **value-handshake and the
full-screen-vs-top-anchored-sub-region routing are unchanged** — they were always ring-agnostic, which is
why dropping the ring touched only `grid.rs` (−114/+37 lines: no `zero`, no `phys`, accessors back to
direct indexing, region scrolls back to plain `rotate`, `take_lines` back to a plain `take`).

### What stands and what doesn't

- **Stands:** rejecting the unified ring (Route B) — that reasoning is independent of the in-Grid ring
  and still correct; the screen↔scrollback split is right. The handshake's purity (Grid stays a pure
  `rows × cols` grid; Term owns history policy) also stands.
- **Superseded:** every "ring / `zero` / `phys` / O(1) scroll / linearize `take_lines`" claim in the
  Decision and Consequences. There is no per-cell mapping, so the "hidden cost is `phys()`" obligation
  list is moot. `record_scroll`/damage stay in logical coordinates trivially (rows never leave logical
  order). `DecodedFrame` output is identical; 260 tests pass.
- **Method lesson (for the next perf slice):** profile the *kind* of cost (allocator vs memory-bandwidth
  vs CPU) before assigning a Big-O label. "Slow scroll" was allocator churn, and the bound N was small —
  neither implies an algorithmic fix. Measure the change, don't infer it from the complexity class.
