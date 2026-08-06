# Territory — marker

## What it is

A **persistent line anchor** the engine keeps alive through buffer mutation, so a consumer can attach
something to a line and still find it after scrolling, insertion, deletion and reflow. The engine
owns the anchoring; what gets *drawn* at the anchor is [decoration](decoration.md), and the engine
never knows.

Two populations share the primitive: plain anchors a consumer registers, and OSC 133 command marks
the shell emits.

## Governing decisions

- [**ADR-0015 — marker primitive and the overlay marker group**](../../adr/0015-marker-primitive-and-overlay-marker-group.md)
  — the primitive itself and how it reaches the frame (wire v6→7)
- [ADR-0020 — what qualifies for the frame snapshot](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  — why the absolute-line group was allowed alongside `markers` (viewport-only), and, since its
  2026-08-05 amendment, why it left in v16 while `markers` stayed
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  anchoring needs the whole buffer, so it is core; appearance does not, so it is not

## Design model

- **`MarkerKind` carries a *role*, not a style.** `Plain` (a bare anchor) plus the four OSC 133
  boundaries: `PromptStart`, `CommandStart`, `OutputStart`, `CommandFinished(Option<i32>)`. The exit
  code parses to `i32` or becomes `None` — absent, empty and non-numeric all collapse to the same
  "not reported".
- **A marker index is *pulled*, and three things keep it valid (#490).** `Term::marker_index`
  answers once with every live marker's absolute line; after that the consumer (a) rebases by the
  `evicted_total` delta, (b) appends on `MarkerCreated` and drops on `MarkerDisposed`, and (c)
  re-pulls when `marker_epoch` moves. The split is by *how the buffer moved*: eviction shifts every
  marker identically so it is one number; birth and death are `O(1)` occurrences so they are events;
  everything else (reflow, a region rotate that moved a survivor, an alt switch, RIS) is
  non-uniform, so nothing but a re-pull repairs it.
  **The birth carries its own instant, and (a) does not supply it (#737, #741).**
  `MarkerCreated.line` is absolute at the *instant of creation*, and the same `feed` can evict
  afterwards — so the frame that closes the batch reports a third origin, and rebasing the event
  against it misplaces the mark by whatever the batch evicted after the birth (#737). That instant is
  not one number: (c)'s epoch dates the moves (a)'s delta cannot express, and a birth queued when the
  epoch bumps is an answer about a buffer that no longer exists — measured, a mark at absolute 3
  reflowed to 5 while the basis stayed 0 (#741). So the event carries the pull's whole
  `(line, evicted_total, epoch)` triple, and the consumer adopts a birth only into the generation it
  names, comparing for equality because `bump_marker_epoch` wraps. Order-independence is the point:
  placement does not depend on whether the host drains events before or after reading the frame, on
  either axis. Only *cost* does — drain-first keeps a birth `O(1)`, while frame-first leaves
  `marker_count` one ahead of an index that has not been told yet, and a consumer comparing the two
  spends a re-pull reconciling it.
  **The known cost, measured**: a marker sitting *below* a bottom margin shifts on every output line
  (`markers_shift_below_margin`), so it bumps the epoch per line — 1 000 bumps over 1 000 region
  scrolls. That degrades to the pre-#490 cost (`O(M)` per frame) **only if the consumer re-pulls at
  most once per frame**, which is therefore a stated obligation of the contract rather than an
  assumption about how a consumer happens to be written.
  **Two corrections measurement forced (#738).** *The cap bounds requests, not availability* — a
  consumer holding to that obligation has every pull land stale under per-line churn, so it holds
  no answer at all for the duration: the degradation is a blank overview ruler and absent
  above-the-top anchors, not a bill. And *the reach is narrow*: this is the only per-line bump, and
  `markers_shift_below_margin` is primary-only, so it needs DECSTBM leaving a static footer **and**
  a marker inside it. Ordinary scrolling bumps 0 times over 1 000 lines; a marker *inside* the
  region bumps 0 times as well, because with `scroll_top == 0` the scroll accrues into scrollback
  and the marker's absolute line correctly does not move.
- **Death is an event, not an absence.** An off-screen marker is *omitted* from the viewport group
  while still alive, so the consumer learns of disposal through `MarkerDisposed` rather than by
  noticing a gap. Without that, "scrolled away" and "gone" would be the same observation.
- **The population is bounded, because the *stream* allocates it.** `add_command_mark` appends per
  OSC 133 sequence with no per-line dedup, and eviction only drops a marker whose line reached
  absolute 0 — so marks piled on one line are unreachable by eviction and grow with the stream
  (measured: 70 000 live in a 24-row buffer, #721). `MAX_MARKERS` caps a buffer's population at
  `u16::MAX`, retiring the **oldest** through the ordinary disposal event. Two things follow that a
  reader will otherwise re-derive: the marker list is a `VecDeque` *because of this* (`Vec::remove(0)`
  would memmove the whole population per push once the cap is reached), and the cap is why both
  `u16` wire counts are safe without being widened.
- **Two projections, two frames of reference.** `MarkerPosition` is viewport rows and carries the
  kind; the absolute buffer line for *every* live marker rode its own group until v16, and carried no kind or
  exit code — because the ruler mark's colour is the consumer's, so nothing themeable rides there.
- **Anchors are maintained through buffer motion by three verbs** — `markers_shift_below_margin`,
  `markers_evict_oldest`, `markers_rotate_region` — called from the write path beside their
  selection-side counterparts.
- **Primary markers survive an alt-screen excursion.** That is the contract (#118/#158): a mark must
  outlive a `vim` session. `normal_markers` and `alt_markers` are separate populations, and an
  alt-screen scroll must **not** rotate primary markers or it silently disposes them — the alt grid
  occupies the same absolute-line range.
- **Command marks are primary-only by definition** (#192), which is why the command walks
  deliberately carry **no** alt-screen floor — see the cross-cutting note below.

## Code

- `justerm-core/src/term/markers.rs` — `Term::add_marker`, `remove_marker`, `command_marks`,
  `command_lines`, `add_command_mark`, `markers_shift_below_margin`, `markers_evict_oldest`,
  `markers_rotate_region`, `marker_positions`, `marker_index`,
  `bump_marker_epoch`, and the private
  `primary_grid` / `command_start` / `doc_line_of`. Extracted from `term.rs` in #588
- `justerm-core/src/serialize.rs` — `MarkerId`, `MarkerKind`, `MarkerPosition`
- `justerm-core/src/term.rs` — `CommandLine`

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [The marker's clear discipline](../../agents/reference-facts.md#the-markers-clear-discipline-534-verified-2026-07-27)

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — markers are the
  **third category** in that note: `doc_line_of` and `command_start` walk absolutely and must **not**
  get a floor, because `command_lines` threads `primary_grid()` through them so all three run in one
  coherent `[scrollback ++ primary]` buffer. The failure mode here is the mirror of the usual one —
  someone *adding* a floor and silently breaking command navigation on the alt screen
- [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  — all three of core's outbound channels meet on this one primitive, and they answer the instant
  question differently: `marker_positions` rides the frame and gets coherence free, `MarkerCreated`
  had to be given a basis (#737) and then the generation that basis cannot express (#741), and
  `command_marks` still has nowhere to put either
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — the two marker group counts are still `u16` after #621 widened its siblings, and **nothing about
  the viewport bounded either of them**: the absolute-line group reported every live marker, and `markers`,
  though `marker_positions` filters it to visible rows, is unbounded because several marks
  legitimately share one line. Past 65 535 the declared count wrapped while every record was still
  written, and `decode` read the following group's count out of the middle of a marker record — `Ok`,
  with every later group garbage-derived. **Closed by #721 at the producer**: `MAX_MARKERS` bounds a
  buffer's live population at `u16::MAX`, so the input that wraps a count can no longer exist. The
  counts were deliberately *not* widened — that would entrench ADR-0020's stated R3 violation, which
  is what #490 exists to remove, and #490 still owns that half (the payload is still `O(M)`; M is
  merely bounded now)

## Blast radius

- [decoration](decoration.md) — the consumer joins its registry by `MarkerId`; a change to lifetime
  or disposal changes what the consumer must reconcile
- [frame](frame.md) — two of the five overlay groups, at two different frames of reference
- [selection](selection.md) — the anchor-maintenance trio sits line-for-line beside selection's at
  every call site in the write path. They were deliberately **not** merged into one `anchors.rs`
  (#584); if the anchor contract ever breaks in both at once, revisit that
- [search](search.md) — since #691 the write path calls a **third** set of fixups on those same
  lines, for tracked points (`justerm-core/src/term/tracked.rs`), whose forcing case is a search
  anchor. Same machinery, two deliberate differences: a tracked point carries a column, and nothing
  about it reaches a frame — so "the anchor pair" is now a triple, and a new mover owes three calls,
  not two
- [viewport](viewport.md) — a marker line is absolute and the ruler divides by
  `scrollback_len + rows`, so the header scalars are part of this contract

## Known holes / open

- **The alt-guard is a rule applied per verb.** `if !self.on_alt` around `markers_rotate_region` in
  `linefeed` / `reverse_index` is exactly the "remember the rule at each site" shape ADR-0025 D2
  rejects for row state — but markers have no equivalent record saying so.
- **`MarkerDisposed` as the death channel is documented in a field comment**, and it is the thing a
  consumer must not get wrong; nothing states it as a contract.
