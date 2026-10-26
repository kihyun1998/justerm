# Territory — reflow

## What it is

`Term::resize` as a whole: re-splitting soft-wrapped logical lines when the width changes, carrying
every engine-held coordinate through the move, and deciding the fate of each piece of state a
geometry change touches. On the primary screen a resize re-lays the whole buffer, history included,
so a long line keeps its tail instead of losing it at the old margin; the alt screen is only re-fit.
The re-split itself is `grid.rs` — `reflow`; the pane bookkeeping is `term.rs` — `reflow_pane`; no
public method is named for either — the only entry is `Engine::resize`.

## Governing decisions

- [ADR-0035 — a column resize does not reflow the logical line holding the cursor](../../adr/0035-the-cursors-logical-line-is-not-reflowed.md)
  (#1082): D1 is the maintainer's judgement and reverses #1076's choice; D2 (the cursor's clamp and
  park), D3 (a line with no content reflows), D4 (the primary's cursor on alt) and D5 (a stranded
  wide lead on a continuing row) are derivations.
- [ADR-0025 — row and wide-pair cell state ownership](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  bounds it from one side: D4 governs the verbs that *edit*, and a resize is a **reallocation**, so a
  wide lead left without its spacer by `Row::resize` is a legal state rather than a violation (#529).
- `docs/architecture.md` §Hidden VT state carries two entries that are this territory's contract:
  *"the alt screen resizes but does not reflow"* and *"soft-wrap vs a hard line-end must be
  distinguished for reflow"*.

## Design model

From the source, `docs/architecture.md` §Hidden VT state, and the `Engine::resize` doc-comment, which
is the consumer-facing contract. No record governs it.

**The re-split.**
- **Primary reflows; the alt screen does not** (#567). `ReflowDims::reflow` is `false` for the alt
  pane: rows are dropped or added to reach the new size and nothing re-wraps, because a full-screen
  application places its own lines. Measured on #567 on an `htop` recording across a live
  `SIGWINCH`: re-splitting left debris in cells htop does not overwrite, because it repaints without
  clearing. All three references take the same position with one flag on the same resize function
  (`docs/architecture.md` §Hidden VT state).
- **Scrollback and screen reflow as one stream** (`reflow_pane` joins them into `all`), which is what
  keeps the concatenated coordinate space coherent across a resize.
- **Reflow is not gated on DECAWM**, deliberately — ghostty gates its equivalent (`.reflow =
  self.modes.get(.wraparound)` in `terminal/Terminal.zig`'s `resize`; unpinned). With DECAWM off a full
  6-column row still re-splits into two at width 3, where ghostty truncates. Three grounds: the wrap
  flag means "continues into the next", still true after a re-split, and dropping it would extract
  `"abcdef"` as `"abc\ndef"`; the mode is global and momentary while the buffer is neither, so reading
  it at resize time decides the fate of history written under the opposite setting; and not
  reflowing truncates content. No per-row signal exists to be finer with. `Term::resize` passes
  `reflow: true` as a constant; the first two grounds are restated in the `Engine::resize` rustdoc,
  the ghostty comparison only here.
- **A wide pair straddling the new boundary is not split**: the re-split emits a short row and marks
  the vacated column as the wrap artefact (#533). Trailing blanks on a hard-ended row are trimmed by
  content, so a BCE-coloured tail does not re-split into a phantom row (#530). Both are documented
  on `reflow`.

**Coordinates through the move.**
- `reflow` takes a `points` slice and returns where each landed, index-aligned. `Term::resize`
  assembles it by an offset idiom: on the primary, the selection's two endpoints, then markers, then
  tracked points, then the settled-cell-paint change record (`ChangeWatch::points`, #967); the cursor rides in
  `reflow_pane` as point 0, and its row is `reflow`'s `keep` (below). On the alt screen the primary pane carries markers, tracked points and
  the watch, the alt pane carries alt markers and alt tracked points, converted to alt-local rows and
  re-anchored on the new base because the primary history may rewrap underneath them.
- **A returned point is a position in the logical line, not necessarily a cell** (#562, closed
  2026-07-27 after five designs that made `reflow` itself materialise a row were measured and
  rejected — a cursor at column 59 resized to width 4 emptied the buffer; a blank-line exemption
  turned 22 alt lines into 21). `col` may equal `new_cols`, and each holder reads it its own way: the
  cursor as the start of the next row (only the re-fit alt pane still hands it one, below); an OSC 133 mark as an exclusive bound; a selection endpoint is
  clamped to `(cols - 1, Side::Right)` in `Term::selection_reflowed` (#1032 — keeping its old side
  dropped or gained the row's last character).
- **The cursor's own logical line is not reflowed** (#1082, [ADR-0035](../../adr/0035-the-cursors-logical-line-is-not-reflowed.md)).
  `reflow_pane` passes the cursor's row as `reflow`'s `keep`; `reflow` walks out to the line's whole
  wrapped run and emits its rows as they are, cut or padded by `Row::resize`, wrap flags kept. A
  point on the run keeps its row with `col` clamped to `new_cols`. The cursor's column comes from its
  own point, not the re-split, clamped to `cols - 1`, and its deferred wrap is left as it arrived — so
  the content trim cannot pull it left over trailing blanks, and a cursor that was not parked is
  never armed (that glued its row to the next line, #562 constraint 2). The reason is the shells:
  bash and zsh answer SIGWINCH by climbing `CUU` once per row the prompt took **at the old width**
  (recorded in `fixtures/resize_shell_redraw.raw`), so a re-split cursor line sent the redraw to the
  wrong row — a piece of the prompt left above it on a narrowing, a line of output erased by `EL` on a
  widening. This replaced #1076's narrower rule (keep a parked cursor that lands exactly past a full
  row), which a narrowing past the exact width in one call did not meet. **A line with no content is
  reflowed as before**: it reads the same either way, and keeping it would spend a row on a pane
  with no history. **A wide lead the cut strands on a kept row that continues becomes the wrap
  artefact** (`set_leading_spacer`), as the re-split leaves it (#533): left a lead, a later reflow
  joined it mid-line and an overwrite took its neighbour for its spacer. On a hard-ended row it stays
  a lead, ADR-0025's last-column state (#529). **While alt is up, the line kept is
  `primary_cursor_on_alt`'s** ([alt screen](alt-screen.md)), not the `saved_cursor` slot's.
- **The seam, not `reflow`, buys the cursor's row** (#567, PR #569): a cursor on a blank line the join
  absorbed comes back past the last row emitted, and `reflow_pane` pushes blank rows up to it so the
  pane scrolls into history, gated on `limit > 0` (the alt pane passes `0`). Without it the next byte
  destroyed a character (a prompt at the bottom of a full screen). Tracked lines are bounded at
  `split + dims.rows - 1`, where the final geometry is known.
- **What leaves the buffer differs by holder.** `PaneReflow::evicted` counts rows dropped off the
  front by the cap (primary) or by the shrink (alt). Tracked points on such a row are released;
  alt markers are disposed with `MarkerDisposed`; primary markers saturate to line 0
  ([marker](marker.md) owns why). Reflow does not advance `evicted_total`; it bumps the marker epoch
  once per dimension change when markers exist (#490).
- **Query-derived state is invalidated, user-authored state is re-anchored.** Search highlights are
  dropped and the consumer re-searches; the selection is carried — except on the alt screen, where
  any geometry change drops it (#660), because the alt pane does not reflow.

**State not anchored to a line.** The axis that decides each is whether the value's *meaning*
survives the geometry change.
- The DECSTBM scroll region is a range over the current screen, so it is **reset** — only when the
  geometry actually moved; `resize` has no early return and a same-size call redefines nothing.
- The deferred wrap is cursor state, so it is **carried** (#848); where the cursor ends short of the
  last column the one-past position is representable, so the flag clears and the cursor takes it.
- The tab-stop table is **extended, never rebuilt or trimmed** (#849): new columns take the default
  ladder at their absolute index (`is_default_tab_stop`), so `tabs.len()` is the widest the terminal
  has been and the walks need `len() >= cols`. The line it replaced rebuilt the table on every call,
  so a one-row-taller window replaced an application's stops. A stop a narrowing pushed outside
  returns on widening. The references split 2–2 on that half (xterm and xterm.js keep, alacritty and
  ghostty drop; SHAs on #849), so it is the maintainer's call, recorded on #849. RIS restores the
  default ladder, since `full_reset` rebuilds from the constructor.
- `repeat_anchor` is cleared (#825: a reflow moves the cell `REP` would read back);
  `display_offset` is clamped to the new history length; `line_damage` is re-sized, the pending scroll
  op dropped and the whole screen damaged.

**Bounds on outside input.** `cols` is clamped to `MIN_COLUMNS..=MAX_COLUMNS` (#547, #621) and `rows`
to `1..=MAX_ROWS` (= `u16::MAX`), silently — the `Engine::resize` rustdoc tells the consumer to read the
width back. The work is O(history + screen) cells plus O(points); the point count is the marker
population plus the tracked-point registry, the latter with no cap ([tracked points](tracked-points.md)).

## Code

- `justerm-core/src/grid.rs` — `reflow` (the join and re-split, taking and returning `points`),
  `Row::resize`, `Grid::take_lines`, `Grid::set_screen`
- `justerm-core/src/term.rs` — `Term::resize`, `ReflowDims`, `PaneReflow`, `reflow_pane`,
  `is_default_tab_stop`, `default_tabs`
- `justerm-core/src/term/selection.rs` — `Term::selection_reflowed`
- `justerm-core/src/term/paint.rs` — `ChangeWatch::points`, `ChangeWatch::reflowed`
- `justerm-core/src/lib.rs` — `Engine::resize`
- Tests: `justerm-core/tests/resize.rs`, `alt_no_reflow.rs`, `reflow_past_end_points.rs`,
  `reflow_point_mapping.rs`, `reflow_row_budget.rs`, `reflow_trim.rs`, `reflow_wide_pair.rs`,
  `resize_parked_prompt.rs`, `resize_cursor_line.rs` (with `fixtures/resize_shell_redraw.raw`),
  `alt_resize_markers.rs`, `alt_selection_resize.rs`, `resize_cursor_and_marker_state.rs`

## Reference behaviour

- [Mapping a tracked point through reflow](../../agents/reference-facts.md#mapping-a-tracked-point-through-reflow-549-verified-2026-07-27)
  — the point mapping that divided by `new_cols` was wrong once rows can be short (#549).
- [Relocating a cluster that grew to width 2](../../agents/reference-facts.md#relocating-a-cluster-that-grew-to-width-2-529-verified-2026-07-28)
  — the pair-straddle handling.
- [The tab-stop table across a resize](../../agents/reference-facts.md#the-tab-stop-table-across-a-resize--four-mechanisms-no-convergence-on-the-column-axis-849-verified-2026-09-03)
  — the 2–2 split behind "extend, never trim".
- [The deferred wrap across the rest of the verbs](../../agents/reference-facts.md#the-deferred-wrap-across-the-rest-of-the-verbs-848-verified-2026-09-03)
  — carrying the flag across a resize.
- [A parked cursor on a trailing blank across a narrowing](../../agents/reference-facts.md#a-parked-cursor-on-a-trailing-blank-across-a-narrowing-1076-verified-2026-10-26)
  — where the cursor lands: xterm.js does not reflow its line, alacritty and ghostty move it down
  (#1076, #1082).
- [A selection when the screen changes under it](../../agents/reference-facts.md#a-selection-when-the-screen-changes-under-it-660-verified-2026-07-31)
  — dropping the selection on an alt-screen resize.
- The DECAWM divergence is argued against ghostty's `Terminal.zig` with no pinned row.

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) — here
  that fact stops being cosmetic and becomes irreversible, which settled its repair in #810: a hidden
  element measures `0x0`, the fit paths floored that to a `2x1` grid, and on the alt screen a resize
  is a re-fit — rows dropped, nothing re-wrapped — so there is nothing to restore from on re-widening.

## Blast radius

Everything anchored to a line, because reflow is the one operation that moves content **between**
rows rather than within one.

- [selection](selection.md) — its anchors ride the `points` slice, and a `col == cols` answer is not a
  cell, so the endpoint's side has to be chosen when clamping.
- [marker](marker.md) — marks ride the same slice and read `col == cols` as an exclusive bound; alt
  markers re-anchor on a base that shifts when primary history rewraps beneath them.
- [tracked points](tracked-points.md) — released, not clamped, when their line is evicted; a set left
  out of the slice is silently misplaced.
- [search](search.md) · [active match](active-match.md) — highlights are invalidated rather than
  moved, so the consumer's re-search is the only thing that restores them.
- [soft wrap](soft-wrap.md) — the wrap bits are reflow's input; a re-split writes new ones.
- [wide glyph](wide-glyph.md) — `Row::resize` cuts through pairs and D4 stops at this boundary.
- [cursor position](cursor-position.md) — the cursor is point 0, and `pending_wrap` is carried here.
- [viewport](viewport.md) · [damage](damage.md) — `display_offset` is clamped and the whole screen is
  damaged with the scroll op dropped, because the op's rows no longer exist.
- [settled-cell paint](settled-cell-paint.md) — its change record rides the slice last
  (`ChangeWatch::reflowed`), and a reflow folds the answered lines back into the changed ones, so the
  consumer is asked to re-paint lines it already painted.
- [vt-interpretation](vt-interpretation.md) — the axis the list above cannot reach: the tab-stop
  table and the scroll margins are indexed by columns and rows, not lines. That absence is why the
  tab table was rebuilt on a rows-only resize through #826 and #848 without anyone standing in this
  note seeing it.
- [alt screen](alt-screen.md) — both grids take the new size on every resize, and only the primary
  re-splits.

## Known holes / open

- **One governing record, for one line of it.** ADR-0035 decides the cursor's line; the rest of the
  operation with the widest blast radius in the engine has none.
- **The DECAWM divergence is deliberate and unrecorded in a decision.** It diverges from a named
  reference with three stated reasons, held by this note and a rustdoc paragraph.
- **Nothing states which point sets must be passed.** The `points` slice is an offset convention in
  `Term::resize`: forget an anchor set and it is silently misplaced, with no compiler or test naming
  the omission. Four sets ride it on the primary today.
- **Which fields a resize may reset has no record**, though the three that raised it are each
  decided (#848 the deferred wrap, #849 the tab table, the margins here). The counterpart for the
  other reset — [RIS keeps configuration, drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — does not transfer: every row there reasons from *"RIS wipes every cell"*, and a resize wipes none.
  The axis that derives all of them is *whether the value's meaning survives the geometry change*
  (not who wrote it — that reading fails on the margins, which the application wrote and which reset
  anyway). It is deliberately not promoted: the trigger is a **fourth** field whose answer the
  references do not already give.
- **The cursor's line loses what is past the new width, also after widening back** (ADR-0035's
  accepted cost, xterm.js's too, measured 2026-10-26): a program that does not redraw — an unfinished
  `progress:` line — keeps only the cut row. On a line of several rows each row is cut, so the loss
  is from the middle (a 40-cell line at 30 → 10 → 30 keeps its first and last ten). A cursor that was
  not parked lands on the last kept cell, so its next glyph overwrites that cell: `read -p` with
  input typed, narrowed, then typed on, reads `Name of td` / `ef`. Which side of this trade-off a
  pane takes is policy, and is decided with #1083 (ADR-0035's amendment).
- **A prompt split over two logical lines still leaves a piece** (measured 2026-10-26): zsh with a
  two-line `PS1` climbs one `ESC[A` for its top line's old row count, but the top line is its own
  logical line and re-splits, so `top line h` stays above the redraw. #1083 is the fix that covers it.
- **The space cut from under a parked cursor is gone for whatever follows the resize** (#1076,
  measured 2026-10-26): the park sits on the prompt's last glyph, so a combining mark fed straight
  after the resize attaches to `$` rather than the space, and under `?7l` the next glyph overwrites
  `$`. A shell's redraw rewrites the row first.
- **A widening leaves the cursor's line padded inside** (ADR-0035): its rows keep their wrap flags,
  so `abcdef` + `gh` widened to 8 reads `abcdef  gh` in text and search until something rewrites it,
  and a later reflow joins the blanks in. xterm.js reads the same.
- **An OSC 133 mark on the prompt line is not moved to where the redraw puts its text** (#1082): it
  keeps its row with its column clamped, while the shell redraws the prompt at the new width from
  the prompt's first row. bash with the marks in `PS1` re-emits them in that redraw (measured
  2026-10-26), which places fresh ones; marks emitted any other way were not measured.
- **The alt pane reads a cursor on the last column as the next row on a narrowing** (measured
  2026-10-26): it does not reflow, so the cursor's column is the old one, and at `col == cols`
  `reflow_pane`'s next-row reading moves it — `(0, 12)` at 13 columns becomes `(1, 0)` at 12, parked
  or not. xterm.js and alacritty clamp the column instead, as the primary's cursor line now does
  (ADR-0035 D2); the alt pane is the only one still handed that reading.
- **"The seam may create rows" narrows ADR-0025 and the ADR does not say so.** PR #569 (#567) let
  `reflow_pane` push a row for the cursor; ADR-0025's text still reads as if a resize creates none.
