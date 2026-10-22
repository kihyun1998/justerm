# Invariant — a coordinate leaving core carries the instant it is true at, or is answered by a query a re-ask always answers

## The fact

Every buffer coordinate core hands out — an absolute `[scrollback ++ grid]` line, a document line over
`accessible_text` — is true at one instant and false afterwards, because the buffer's origin moves
under it. Whichever channel it leaves by, either the instant leaves with it, or the surface is one where
a re-ask always answers and its doc says so. The derivation of which discharge a surface owes is
[ADR-0029](../../adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md) (D1–D6); this
note states the fact and where it holds.

In the absolute space the instant is **two** scalars, and taking the first alone is the recurring
mistake (#737, #741):

- `evicted_total` (`u64`) counts lines popped since RIS and dates a **uniform** move — eviction shifts
  every live mark by the same −1.
- `marker_epoch` (`u32`, moved by `wrapping_add` in `term/markers.rs — Term::bump_marker_epoch`) dates
  the moves that are not uniform — reflow, a region rotate that moved a survivor, an alt switch, RIS —
  where nothing smaller than a re-pull repairs anything. Because it wraps, a receiver compares it for
  **equality, never order**.

Core hands coordinates out on three channels (re-verified at HEAD 7bb31f7):

| channel | how the instant reaches the receiver | example |
|---|---|---|
| the **frame** | by construction — the header carries `evicted_total` and `marker_epoch`, and every scalar of a `Frame` is sampled in one `Term::frame` body | `display_offset`, `scrollback_len`, `marker_count` (`serialize.rs — Frame`) |
| an **event** | only if the variant carries it; the queue detaches the payload from its instant, and the frame's basis does not reach it | `TermEvent::MarkerCreated { id, line, col, kind, evicted_total, epoch }` |
| a **query answer** | only if the return type carries it, or the answer is declared instantaneous and a re-ask always works | `Engine::marker_index` → `MarkerIndex { markers, evicted_total, epoch }` carries it; `Engine::command_marks` (#742) and `Engine::command_lines` (#743) are re-ask |

**Re-ask is available only when both hold** (#742): the receiver is consumed on a clock that can
re-ask (a user action can; the frame clock cannot, since re-pulling per frame is the `O(M)` payload
ADR-0020 R3 forbids — which is why `marker_index` is held and must carry), and the answer's frame of
reference is constant, so absence has one meaning. `command_marks` is primary-scoped whatever screen is
up, so empty means disposed; `marker_index` reports the *active* buffer, so its silence is ambiguous
between disposed and other-screen. An event has neither discharge: it can only carry.

**The document space has no scalar on any published surface, and would take two.** A document line
indexes `accessible_text`, where soft-wrapped rows collapse, and it moves on two axes (#743, measured
2026-08-07):

- eviction, non-uniformly against the absolute space: an evicted row that soft-wraps into the next
  moves the absolute space and not the document space. Measured over `evicted_total` 0 → 2: absolute
  17 → 15, document 16 → 15; isolated to the one eviction, absolute `[12, 12, 12, 13]` →
  `[11, 11, 11, 12]` with the document line staying `11`. A line-end counter beside `evicted_total`
  would date this exactly.
- a wrap bit flipped in place, which the absolute space does not have: rewriting a short row to
  overflow joins two document lines. Measured: document 2 → 1, absolute `[2, 2, 2, 3]` unchanged,
  `evicted_total` 0, `marker_epoch` 0. Only a generation dates this.

So carrying is buildable and expensive — a counter and an epoch, ADR-0029 D5 duplicated for a second
space — deferred on that cost (ADR-0029 alternative D); re-ask is taken because D3 grants it.

**A document line also names a document.** `accessible_text` returns the active buffer's document (it
floors at `abs_floor()`), while `command_lines` reads `primary_grid()` because OSC 133 marks are
primary-only (#192). On the alt screen they answer about different buffers at one instant. Measured
(#743): with 17 lines of primary scrollback and a five-row TUI up, held document line `1` named
`"$ lsout"` on the primary and `"TUI row 1"` on the alt — in range, onto unrelated content, so no
consumer bounds check catches it. Validity condition: `accessible_text` is the only document core hands
out and `CommandLine::line` the only index into one (public `Engine` surface grepped, #743).

## Why it is cross-cutting

The channels belong to different territories and were decided by different records — the frame by
ADR-0020, the query seams by ADR-0017's mechanism/policy split, the event channel by none. Each site
that met it re-derived the argument: the frame half was settled in #490 by admitting two header
scalars, and the event half re-decided from scratch in #737 on the same mechanism. The frame encoder,
the event queue and the query methods do not call each other; they share only the fact that the
buffer origin moves after the value is sampled. ADR-0029 (#742) now derives the rule; a new dating
question is a conformance item against its D1–D6.

It is invisible from inside one territory: a reader in marker sees a basis on the pull and concludes
markers are handled; a reader in accessibility sees a document line with nothing to compare. And a
wrong answer is silent: a stale coordinate is a plausible line number that decodes, projects and paints
on content it no longer names.

## Territories it holds in

- [events & replies](../territory/events-and-replies.md) — the channel with no governing record, where
  the payload outliving its instant is structural. `event.rs — TermEvent::MarkerCreated` carries the
  same triple as its pull-side sibling since #741; `col` joined under the same basis in #973.
- [marker](../territory/marker.md) — all three channels on one primitive: `marker_positions` (frame),
  `MarkerCreated` / `MarkerDisposed` (events), `Engine::marker_index` and `Engine::command_marks`
  (queries). `command_marks` carries neither scalar and is correct without them (#742): the two queries
  on one primitive with opposite shapes are what make the derivation visible.
- [frame](../territory/frame.md) — the channel that gets coherence free (`term.rs — Term::frame`),
  and so the one that makes the other two look solved.
- [accessibility](../territory/accessibility.md) — `CommandLine::line` is a document line: the one
  coordinate no published scalar rebases, and the one that must say which document it indexes. Settled
  as re-ask in #743: valid exactly as long as the `accessible_text` sampled beside it, and the consumer
  samples the two at one point.
- [logical lines](../territory/logical-lines.md) — the soft-wrap collapse that makes the document
  space diverge from the absolute one.
- [settled-cell paint](../territory/settled-cell-paint.md) — `paint.rs — LineRef` carries
  `evicted_total` and the `alt` identity out through `Engine::changed_logical_lines`; on return
  `term/paint.rs — Term::paint_logical_line` rebases by eviction and covers the non-uniform moves by
  refusing unless the line still reads the reported `text` (#967; read at HEAD 7bb31f7).

Consumer half, outside core: `justerm-web/src/marker-index.ts — MarkerIndexCache` rebases at read time
and adopts an entry only into the generation it names, compared for equality (#741, #746).

## What a violation looks like

A coordinate crossing the boundary while the value that dates it stays behind:

- a type that pairs lines with nothing **and is meant to be held** — a bare
  `Vec<(MarkerId, usize, MarkerKind)>` beside a sibling returning `MarkerIndex`. The undated type alone
  is not the violation (#742 corrected that reading): it is correct where re-ask always answers and the
  doc says so; it is a violation when the receiver has no way back to the engine.
- an event variant gaining a `line` / `row` / `col` / `index` that carries fewer scalars than its
  pull-side sibling — the subset is the axis whichever bug forced the field (#741 was #737's own
  missing half).
- a doc comment telling the reader to rebase one surface's answer by another surface's delta.
- a consumer storing an answer past the frame it was asked in, with no recorded validity window.

The review tell is a sentence like "it is absolute at the moment of X" with no field named X;
`events-and-replies.md` once carried "`line` is absolute on the same basis the frame header reports",
and the consumer was written against it. The runtime symptom is a mark or paint landing on the wrong
line after eviction or reflow, with no error.

## Discovery history

- **#490** (2026-08-04, wire v15/v16) — frame: `evicted_total` and `marker_epoch` admitted to the
  header. Derived the split: a uniform move is one scalar, a non-uniform move an epoch.
- **#737** (2026-08-06) — event channel re-decided from scratch. Two single-`feed` batches differing
  only in whether an OSC 133 mark preceded or followed three evictions agreed on event line, both frame
  bases, epoch and `marker_count`, while their true lines were three apart. Its verify pass found the
  query channel holding the same shape twice. The carried basis fixed only the eviction axis: a mark at
  absolute 3 reflowed to 5 was still answered as 3.
- **#741** (2026-08-06) — the event now carries the full `MarkerIndex` triple, so the next variant is
  checked against a sibling, not a list of axes. Lesson: a subset chosen by the reproducing case reads
  exactly like the whole rule. Consumer half: adopt an entry only into the generation it names,
  equality because `bump_marker_epoch` wraps — at both the queued replay and the live path.
- **#742** (2026-08-06) — query channel, first re-ask discharge. Carry was measured sound but
  expensive: a differential probe over 400 seeds × 60 ops found 0 violations over 20 529 epoch-stable
  transitions and 63 367 per-mark checks; disabling both `bump_marker_epoch` sites gave 7 317, disabling
  the `evicted_total` increment gave 99. Expensive: a top-anchored `DECSTBM` footer with a mark inside
  bumped the epoch on 200 of 200 output lines. Rejected on cost, never correctness. Two premises of its
  issue body were false: `command_lines` does not call `command_marks` (independent walks of
  `normal_markers`), and sampling the basis beside the marks is not a second instant (both `&self`;
  `feed` / `resize` are `&mut self`).
- **#743** (2026-08-07) — document space. Corrected this page's earlier "no scalar can date a document
  line" (false; carry is expensive, not impossible — the claim had spread to six surfaces). Isolated
  the eviction divergence to the single responsible eviction; found the wrap-bit axis by refutation;
  showed the which-buffer axis is constrained by D3.2: returning `[]` on alt, as xterm.js does for
  `Terminal.markers` (`typings/xterm.d.ts:962-963` @ `699f553`), would make absence mean wrong-screen.
- Spine #744 anchored the cluster and closed on promotion to ADR-0029 (2026-08-06). The roster stays
  out of this page on #552's measured result (a hand-copied roster in ADR-0025 went stale in five places
  in three days).

## Where it will recur

- **A new `TermEvent` variant carrying a position.** `MarkerCreated` is the only one at HEAD 7bb31f7
  (read the enum in `event.rs`). Test: name the **pull** that answers the same question and carry every
  scalar it carries. A variant with no such pull is the harder case and has not happened yet.
- **`Engine::command_marks`** — settled re-ask (#742); the answer moves on both axes, so noticing one is
  not grounds to re-derive. Measured: eviction `[7, 7, 7, 8]` → `[5, 5, 5, 6]` with the epoch still; a
  top-anchored `DECSTBM` footer `[7]` → `[10]` in one `feed` with `evicted_total` still. Pinned in
  `justerm-core/tests/command_marks_instant.rs`, with constant scope and absence-means-disposal.
- **`CommandLine::line`** — settled re-ask (#743). Rebasing by `MarkerIndex::evicted_total` is wrong on
  evictions that pop a continuation row; emptying on alt fails D3.2. Pinned in
  `justerm-core/tests/command_lines_document.rs`, whose fixture needs a soft-wrapped line above the
  command and real scrollback (two earlier drafts passed against a deliberately broken engine).
- **A second document handed out beside an index into it** — a second whole-buffer document, or a
  `LogicalLine` batch indexed by number. That is where the pairing above becomes a rule.
- **A consumer cache beside `MarkerIndexCache`** — the only class in `justerm-web/src` holding a buffer
  coordinate across frames (old note's reading; not re-grepped exhaustively), so the second will be
  written without its rebase-at-read discipline unless someone says so.
- The reference cannot settle the design: xterm.js keeps a marker valid by mutating a live object
  (`DecorationService.ts:24-26` @ `699f553`), ghostty marks a pin `garbage` (`PageList.zig:1039`,
  `:3593` @ `e6e26e1`); neither serializes a coordinate across a boundary. It can settle a mechanism:
  ghostty compares its generation with `<` only because it is a `u64` stated never to wrap
  (`PageList.zig:372`, `:379`, `:392`), so equality is forced here —
  [reference-facts](../../agents/reference-facts.md#dating-an-anchor-across-a-non-uniform-move--the-one-reference-with-a-generation-and-why-it-may-compare-with--741-verified-2026-08-06).
