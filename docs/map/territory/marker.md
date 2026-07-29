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
  — why `marker_lines` (every live marker, absolute) is allowed alongside `markers` (viewport-only)
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  anchoring needs the whole buffer, so it is core; appearance does not, so it is not

## Design model

- **`MarkerKind` carries a *role*, not a style.** `Plain` (a bare anchor) plus the four OSC 133
  boundaries: `PromptStart`, `CommandStart`, `OutputStart`, `CommandFinished(Option<i32>)`. The exit
  code parses to `i32` or becomes `None` — absent, empty and non-numeric all collapse to the same
  "not reported".
- **Death is an event, not an absence.** An off-screen marker is *omitted* from the viewport group
  while still alive, so the consumer learns of disposal through `MarkerDisposed` rather than by
  noticing a gap. Without that, "scrolled away" and "gone" would be the same observation.
- **Two projections, two frames of reference.** `MarkerPosition` is viewport rows and carries the
  kind; `MarkerLine` is the **absolute** buffer line for *every* live marker and carries no kind or
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
  `markers_rotate_region`, `marker_positions`, `all_marker_lines`, and the private
  `primary_grid` / `command_start` / `doc_line_of`. Extracted from `term.rs` in #588
- `justerm-core/src/serialize.rs` — `MarkerId`, `MarkerKind`, `MarkerPosition`, `MarkerLine`
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

## Blast radius

- [decoration](decoration.md) — the consumer joins its registry by `MarkerId`; a change to lifetime
  or disposal changes what the consumer must reconcile
- [frame](frame.md) — two of the five overlay groups, at two different frames of reference
- [selection](selection.md) — the anchor-maintenance trio sits line-for-line beside selection's at
  every call site in the write path. They were deliberately **not** merged into one `anchors.rs`
  (#584); if the anchor contract ever breaks in both at once, revisit that
- [viewport](viewport.md) — `MarkerLine` is absolute and the ruler divides by
  `scrollback_len + rows`, so the header scalars are part of this contract

## Known holes / open

- **The alt-guard is a rule applied per verb.** `if !self.on_alt` around `markers_rotate_region` in
  `linefeed` / `reverse_index` is exactly the "remember the rule at each site" shape ADR-0025 D2
  rejects for row state — but markers have no equivalent record saying so.
- **`MarkerDisposed` as the death channel is documented in a field comment**, and it is the thing a
  consumer must not get wrong; nothing states it as a contract.
