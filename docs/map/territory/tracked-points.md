# Territory — tracked points

## What it is

A registry of absolute `[scrollback ++ grid]` positions the engine keeps on their **content** for a
holder that lives outside it (GLOSSARY "Tracked point"). The consumer registers a `(line, col)`
(`Engine::track_point` → `TrackedId`), the write path shifts the line through every mover of the
absolute index, and the holder asks for it back (`Engine::tracked_point`) and is told `None` once the
line has left the buffer, then releases it (`Engine::untrack_point`). Nothing about it reaches a frame.
The forcing case is a search backend remembering which occurrence the emphasis was on across a
re-search (#691).

## Governing decisions

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  why it is a point registry and not an engine-owned search anchor: the engine keeps a position valid;
  which position is worth remembering, which occurrence is current and what to do once the point is
  gone stay with the consumer (`term/tracked.rs` module doc).
- [ADR-0026 — outside coordinates are bounded once](../../adr/0026-outside-coordinates-are-bounded-once.md)
  — D2/D3/D4: the coordinate is the consumer's (no engine producer to clamp), so it is bounded at the
  **read**, both ends, the line into the owning buffer's range and the column to the grid width.
- [ADR-0029 — a published coordinate carries its instant or is re-asked](../../adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md)
  — `tracked_point`'s answer carries no basis; it is the re-ask discharge, and the registry is what
  makes re-asking possible for a coordinate the holder must keep between two queries.

## Design model

From the source (`term/tracked.rs` module and method docs, `Engine::track_point` rustdoc),
`docs/architecture.md` §Hidden VT state ("Anchor lifecycle"), and #691.

- **Why it exists** (#691, measured 2026-08-03): a consumer holding an absolute coordinate across two
  queries has a number the engine renumbers in its absence — at four sites, with two signs. Measured
  on justerm-web's search: at the scrollback cap the emphasis walked forward one occurrence per
  evicted line, with the count label unchanged and no user input. The selection and markers each
  carried a fixup per mover; a coordinate held outside carried none.
- **Same machinery as a marker, two deliberate differences.** Like a marker it has a stable id, a line
  the write path keeps, and an observable death. Unlike one, (1) **nothing reaches a frame** — every
  live marker rides frame groups, so registering one to remember a position would paint it on the
  overview ruler; `track_point` does not even raise `frame_pending`; and (2) it is **asked, not
  announced** — a marker's death is a `TermEvent` because a decoration holder is push-driven, while
  the one caller shape asks on every re-search anyway, so an event would be a second channel saying
  what `None` already says. And the holder anchors at a `(line, col)` of its choosing.
- **The fixups — one per mover, beside the selection's and the markers'.**

  | mover | fixup | rule |
  |---|---|---|
  | lines leaving the front (cap, `ED 3`, `Engine::clear`) | `Term::tracked_evict_oldest(n)` via `Term::lines_left_the_front` | shift down `n`; a point on a dropped line is released. Both populations, since `ED 3` on alt drops the primary history beneath alt points |
  | top-anchored sub-region accrual (#449) | `Term::tracked_shift_below_margin` | lines below the margin +1; primary only, as the accrual branch is |
  | in-screen region / RI / SU / SD / IL / DL scroll, and a linefeed inside a region | `Term::tracked_rotate_region` | rotate within the region; a point on the dropped edge is released — the **marker** policy, not the selection's clamp, because one position has no surviving part to keep |
  | reflow (`Term::resize`) | the `points` slice through `reflow_pane` | mapped; released, not saturated, when the reflow evicted its line; alt points converted to alt-local rows and re-based, released when a shrink pushed their row off ([reflow](reflow.md)) |

- **A line, not the characters on it.** Erasing or overwriting the cells under a point leaves it `Some`
  (measured on #750). That is the positional answer, deliberate: the only consumer resolves "which
  occurrence was I on" by nearest position, so a point over rewritten content is still serviceable,
  where a command mark asserts that a command happened there. The **column is carried, not tracked**:
  ICH/DCH shift cells past it without moving it — convergent with xterm (markers carry no column) and
  ghostty (pins untouched by `insertChars` / `deleteChars`).
- **Per buffer, and only the active one resolves.** `normal_tracked` / `alt_tracked`, selected by
  `on_alt` (`Term::tracked`, `Term::tracked_mut`), as markers are. The primary grid and the alt grid
  occupy the same absolute indices, so a bare `(line, col)` cannot say which screen it means
  (measured: a point on primary line 4 and the alt screen's second row both read `4`); a point of the
  inactive buffer answers `None`. The floor alone was wrong here twice in #691's branch — one floor
  for every point, then a per-buffer floor. xterm's markers hang off a `Buffer` and ghostty's pins off
  a per-screen `PageList`, so a cross-screen read is unconstructible there. `None` therefore means any
  of: the line left, the id was released or never issued, the point belongs to the other screen — all
  three say *do not move anything on account of this point*.
- **Alt points die with the alt screen**, unannounced (`Term::switch_to_primary` clears
  `alt_tracked`); primary points survive an alt excursion.
- **Out of range is bounded, not rejected, at the read** (`Term::tracked_point`): the line clamped to
  `[abs_floor, last]`, the column to `[0, cols]` — one past the last cell is a legal bound, which a
  caller pairing it with text extraction needs. A coordinate never in range is also resolved by a
  reflow (it maps to the top), so "bounded once" holds for the site, not the value.
- **RIS drops every point and keeps the id counter** (`Term::full_reset` carries `next_tracked_id`):
  there is no disposal event, so the only thing keeping a stale id honest is that it is never
  reissued.
- **Release is the holder's job** (`Term::untrack_point`, id-based across both lists): the engine
  cannot know when a holder is done, so without it the registry only grows.
- **The one caller.** PenTerm's search backend registers a point at the start of every match it
  designates, untracking the previous one, resolves `anchored_index` as the first match at or after
  the point (wrapping to 0), and untracks on `clear` — `clear_highlights` keeps it
  (`penterm 7dadc3c95 src-tauri/src/engine/search.rs — SearchState`). The justerm-web contract it
  implements is `SearchPort.anchoredIndex` ([active match](active-match.md)); the in-repo demo backend
  is a browser-side fake that does not run the engine.

**Bounds on outside input.** The coordinate is consumer-authored and bounded at the read. The
**number of live points is unbounded**: each `track_point` pushes onto a `Vec` with no cap, and
`tracked_point` / `untrack_point` are linear scans. Ids are a `u32` counter incremented without a
check (Known holes).

## Code

- `justerm-core/src/term/tracked.rs` — `Term::track_point`, `Term::tracked_point`,
  `Term::untrack_point`, `Term::tracked`, `Term::tracked_mut`, `Term::tracked_shift_below_margin`,
  `Term::tracked_rotate_region`, `Term::tracked_evict_oldest`
- `justerm-core/src/term.rs` — `TrackedId`, `TrackedPoint`, the `normal_tracked` / `alt_tracked` /
  `next_tracked_id` fields; the callers `Term::linefeed_inner`, `Term::scroll_region_lines`,
  `Term::lines_left_the_front`, `Term::resize`, `Term::switch_to_primary`, `Term::full_reset`
- `justerm-core/src/lib.rs` — `Engine::track_point`, `Engine::tracked_point`, `Engine::untrack_point`
- `justerm-core/src/term/walk.rs` — `Term::abs_floor`
- Tests: `justerm-core/tests/tracked_points.rs` (every test resolves the point back to text by
  re-searching for its needle, because an ordinal cannot tell "the same occurrence, renumbered" from
  "a different one")

## Reference behaviour

- [What carries the current match across a re-search](../../agents/reference-facts.md#search-what-carries-the-current-match-across-a-re-search-437441-verified-2026-08-03)
  — ghostty holds a tracked pin beside the index; none of the three keeps a bare ordinal.
- [Search on the overview ruler, and how a match position stays valid](../../agents/reference-facts.md#search-on-the-overview-ruler-and-how-a-match-position-stays-valid-440-verified-2026-08-13)
  — 3–0, nobody holds a raw coordinate across a buffer mutation.
- [Relating a coordinate to the document it names](../../agents/reference-facts.md#relating-a-coordinate-to-the-document-it-names--how-the-references-make-which-buffer-unaskable-743-verified-2026-08-07)
  — how the references make "which buffer" unaskable, the shape the active-only read reproduces.
- [Mapping a tracked point through reflow](../../agents/reference-facts.md#mapping-a-tracked-point-through-reflow-549-verified-2026-07-27)
  — the reflow mapping every carried coordinate shares.
- The column convergence (xterm markers carry none; ghostty pins untouched by in-row edits) is cited
  in the `Engine::track_point` rustdoc without a pinned row.

## Cross-cutting invariants

- [the write path funnels motion and does not funnel destruction](../invariant/no-funnel-for-destruction-in-place.md)
  — the **positional** answer: three motion fixups and no destruction hook, by design.
- [RIS keeps configuration and drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — both halves: the point sets die, `next_tracked_id` survives because the id is in the holder's hands
  (`tracked_points.rs` — `ris_drops_every_tracked_point`, `a_tracked_id_is_not_reissued_after_ris`); carrying it does not stop a wrap at `u32::MAX` (that note's
  § A counter carried across RIS).
- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `Term::tracked_point`
  floors at `abs_floor`, and is the site that showed a floor is a bound, not an identity: only scoping
  the read to the active buffer separates the two screens (`an_alt_read_floors_at_the_alt_grid`,
  `a_point_of_the_inactive_buffer_does_not_resolve`).
- [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  — `tracked_point` answers a bare `(line, col)` with no basis, true only until the next mutation; it
  holds as a re-ask surface (the holder asks right before resolving against a fresh result set), and
  the `TrackedId` is what lets a holder re-ask instead of keeping the coordinate.

## Blast radius

- [grid & scrollback](grid-and-scrollback.md) — a new path that drops lines off the front must go
  through `lines_left_the_front`, or points keep naming lines that moved.
- [reflow](reflow.md) — points ride the `points` slice by an offset idiom; a reorder of that slice
  silently maps them onto another holder's positions.
- [vt-interpretation](vt-interpretation.md) — a new line-shifting verb owes a `tracked_*` call beside
  the `selection_*` and `markers_*` ones, and is the one with no frame to make its drift visible.
- [alt screen](alt-screen.md) — the per-buffer split assumes the alt list is cleared on leave and that
  the active screen decides which list a read consults.
- [active match](active-match.md) · [search](search.md) — the consumer's occurrence anchor is a
  tracked point; `None` is what makes `anchoredIndex` fall back to the clamped ordinal.
- [marker](marker.md) — the sibling holder; its fixups sit beside these and share their movers, but
  its eviction policy (saturate on a primary reflow) differs.

## Known holes / open

- **No cap on live points, and no reclamation without the holder.** A holder that forgets
  `untrack_point` grows the registry forever; reads and releases are linear scans. Bounded in practice
  only by PenTerm untracking the previous anchor on each designation.
- **`next_tracked_id` is a `u32` incremented with `+= 1`**: past `u32::MAX` registrations it panics in
  a debug build and wraps in release, reissuing ids — the hazard RIS carries the counter to avoid.
  PenTerm spends one id per designation (every next/prev).
- **The positional answer is unpinned.** That an erase or overwrite under a point leaves it `Some` is
  stated as measured (#750) in `Term::track_point`'s doc-comment; no test in `tracked_points.rs`
  asserts it, so a future "retire on destroy" change would not redden.
- **"Tracked point" names two things.** `grid.rs`'s `reflow` doc and ADR-0025 call every coordinate
  carried through a reflow (cursor, selection anchors, OSC 133 marks) a "tracked point"; the GLOSSARY
  term is this registry. A reader of one can mistake the other.
- **The alt-screen `row < rows` guard in `Term::resize`'s tracked-point loop is unproven.** A
  324-resize sweep never reached it, because the alt fit runs `reflow_pane` with `reflow: false`
  (`PaneReflow` doc; carried from the old marker note, #691).
- **Nothing in this repo runs the wired path end to end**: the only engine-backed caller is PenTerm.
