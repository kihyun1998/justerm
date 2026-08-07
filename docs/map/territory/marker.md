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
- [**ADR-0029 — a coordinate carries its instant, or is re-asked**](../../adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md)
  — promoted out of this primitive (#490 → #737 → #741 → #742), because all three outbound channels
  meet here and they answer the dating question differently. It is why `marker_index` carries a basis
  and an epoch while `command_marks` carries neither and is still correct

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
  scrolls. **It is a correctness trigger as well as a cost, and reading it as only a cost cost a
  premise (#741).** The same bump is what moves markers non-uniformly *inside a single `feed`*, with
  no resize anywhere — measured, a mark in a DECSTBM footer is born at line 4 and answered at 6 two
  accruals later, with `evicted_total` never moving. #737's completeness pass had recorded the
  opposite (*"no non-uniform move escapes the epoch inside a `feed`"*), which is why every fixture
  written against this area reached the epoch through `resize` and none of them covered the path an
  ordinary `tmux` status line takes. That degrades to the pre-#490 cost (`O(M)` per frame) **only if the consumer re-pulls at
  most once per frame**, which is therefore a stated obligation of the contract rather than an
  assumption about how a consumer happens to be written.
  **Two corrections measurement forced (#738), and a third that falsified the second (#746).**
  *The cap bounds requests, not availability* — a consumer holding to that obligation has every
  pull land stale under per-line churn, so it holds no answer at all for the duration: the
  degradation is a blank overview ruler and absent above-the-top anchors, not a bill.
  **And "the outage is bounded by the churn" was false too**: the consumer started a pull only when
  the epoch *changed*, so a pull landing one generation behind the newest frame left the index
  unusable after the churn had stopped — permanently, with the population unchanged so the count
  check stayed silent. Reached by an ordinary drag-resize once the query round trip approaches the
  resize cadence (8 of 40 drags at RTT ≈ 100 ms). Fixed consumer-side by asking a *state* — *what I
  hold does not describe the newest frame, and nothing is in flight* — rather than an edge. Core is
  unchanged; this is a consumer-half correction to a claim the core-side design had been credited
  with. And *the reach is narrow*: this is the only per-line bump, and
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
- **And by a fourth that is not a mover at all: `dispose_markers_on_row` (#750).** The three
  above repair a *coordinate*, and they read as the whole job because for a long time every way a
  mark could stop describing its content moved the buffer. An in-place erase does not — the row
  stays where it is while everything the mark was about stops existing — so nothing fired, nothing
  was announced, and `command_lines` reported commands that were not there, at document lines that
  reveal onto blank rows. Worse than absent: after a `clear` the shell redraws its prompt onto the
  dead marks' columns, so every later command was reported twice (measured, `n=4` for two real
  commands).
  **`ED` retires, `EL` and `ECH` do not**, and that split is not the reference's helper-identity
  accident being copied — it has its own reason. A line editor redraws the input line with
  `
 ESC[K` on every keystroke, and `B` was emitted before the user began typing, so an `EL` that
  retired marks would delete the `CommandStart` of the command being typed and no command would
  ever be reported. The references converge on the same split anyway (see below). **The known
  residue** is the cursor's own row: both references route it through their partial-erase helper,
  so `ESC[H ESC[0J` leaves exactly one phantom. Followed rather than widened, because the only
  argument for widening is symmetry — the tell ADR-0019's retracted first amendment was caught by.
- **What is *not* in the buffer is frozen on the mark when the stream reveals it (#750).** The
  command text is captured at `C` and the exit code written down at `D`. Re-reading text through
  the recorded `[b_col, c_col)` clip names whatever now occupies those cells — measured for a plain
  overwrite, `ICH`, `DCH` **and** an erase, so no lifetime rule closes it; only the erase is a verb
  disposal could ever reach. And resolving the exit at *query* time meant pairing over survivors
  (`out.last_mut()`), which slid a code onto the previous command the moment a disposal broke the
  run — measured, `a0` wearing `a1`'s `Some(2)`. Only `line` stays derived, because it is the half
  the movers above already maintain. The capture is bounded at `MAX_COMMAND_TEXT`, for the reason
  `MAX_MARKERS` exists: the *stream* chooses the distance between `B` and `C`.
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
  Since #750 the same file owns `dispose_markers_on_row`, `capture_command_text`,
  `open_command_start` and `attach_exit`
- `justerm-core/src/serialize.rs` — `MarkerId`, `MarkerKind`, `MarkerPosition`
- `justerm-core/src/term.rs` — `CommandLine`, `CommandRecord`, `MAX_COMMAND_TEXT`

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [The marker's clear discipline](../../agents/reference-facts.md#the-markers-clear-discipline-534-verified-2026-07-27)
  — note this is the *wide-spacer* marker, a different artifact that shares the word
- [What retires a line-anchored mark when the line's content is destroyed in place](../../agents/reference-facts.md#what-retires-a-line-anchored-mark-when-the-lines-content-is-destroyed-in-place-750-verified-2026-08-07)

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
  `command_marks` has nowhere to put either **and does not need one** (#742) — it is declared
  instantaneous, which the note's own derivation says is available to it and not to `marker_index`,
  because its scope is primary whatever screen is up while the pull's follows the active buffer. Two
  queries over one primitive, opposite shapes, and the difference is derived rather than chosen.
  **`command_lines` is the third, and it reached the same shape from the opposite end (#743)**: it is
  a *document* line, so there was never a scalar it *could* have carried — dating that space would take
  a line-end counter and a generation of its own, where `command_marks` merely declines the pair that
  already exists. It is also the only outbound coordinate that has to name a
  **document** as well as an instant, and on the alt screen the document it names is not the one
  `accessible_text` returns. Both halves are stated on `Engine::command_lines` and pinned in
  `justerm-core/tests/command_lines_document.rs`
- [the write path funnels motion and does not funnel destruction](../invariant/no-funnel-for-destruction-in-place.md)
  — this territory is where the gap was found and the one that answers **retire**: the three
  movers repair a coordinate and read as the whole job, so nothing repaired a mark whose row was
  blanked where it stood (#750). The note's value is the other two answers, which are *not* the
  same and are not reachable from here
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

- **`ESC[H ESC[0J` leaves one phantom**, on the cursor's own row — the reference edge above,
  pinned by `ed_0_disposes_the_rows_below_and_keeps_the_cursor_row_marks` rather than fixed.
- **The alt-guard is a rule applied per verb.** `if !self.on_alt` around `markers_rotate_region` in
  `linefeed` / `reverse_index` is exactly the "remember the rule at each site" shape ADR-0025 D2
  rejects for row state — but markers have no equivalent record saying so.
- **`MarkerDisposed` as the death channel is documented in a field comment**, and it is the thing a
  consumer must not get wrong; nothing states it as a contract.
