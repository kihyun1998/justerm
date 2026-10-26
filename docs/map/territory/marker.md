# Territory — marker

## What it is

A **persistent line anchor** the engine keeps alive through buffer mutation, so a consumer can attach
something to a line and still find it after scrolling, insertion, deletion and reflow. The engine
owns the anchoring; what gets *drawn* at the anchor is [decoration](decoration.md), and the engine
never knows. Two populations share the primitive: plain anchors a consumer registers
(`add_marker`), and OSC 133 command marks the shell emits. This territory also owns the three read
surfaces built on them — the pulled **marker index** (`marker_index`), the **command marks**
(`command_marks`) and the **command lines** (`command_lines`: text and exit frozen per command).

## Governing decisions

- [**ADR-0015 — marker primitive and the overlay marker group**](../../adr/0015-marker-primitive-and-overlay-marker-group.md)
  — the primitive itself and how it reaches the frame (wire v6→7).
- [ADR-0020 — what qualifies for the frame snapshot](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  — why the absolute-line group left in v16 while the viewport `markers` group stayed, and (its
  #737/#741 amendments) what a birth event must carry.
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  anchoring needs the whole buffer, so it is core; appearance, navigation and earcons are not.
- [**ADR-0029 — a coordinate carries its instant, or is re-asked**](../../adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md)
  — promoted out of this primitive (#490 → #737 → #741 → #742 → #743), because all three outbound
  channels meet here and answer the dating question differently. It is why `marker_index` carries a
  basis and an epoch while `command_marks` and `command_lines` carry neither and are still correct.
- [ADR-0025 D1 — a fact lives with its owner](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  — cited for `CommandRecord` living on the marker rather than in a side table (adjacent: the ADR is
  about row and wide-pair state, not markers).

## Design model

Status: ADR-0015/0020/0029 decide the primitive, the frame groups and the dating; everything else
below comes from the source and its doc-comments (read at 7bb31f7) and from the issues cited.

**The primitive**

- **`MarkerKind` carries a *role*, not a style.** `Plain` plus the four OSC 133 boundaries:
  `PromptStart`, `CommandStart`, `OutputStart`, `CommandFinished(Option<i32>)`. The exit code parses
  to `i32` or becomes `None` — absent, empty and non-numeric collapse to "not reported".
- **`133;A`'s options are read, and are not a mark's** (#1083, `Term::prompt_start`):
  `redraw=0` / `1` / `last` set `Term::prompt_redraw` for the resize rule
  ([reflow](reflow.md), ADR-0036), reset by every `A`; an `A` carrying `k=s` (kitty's secondary prompt
  line) records **no** mark, since as a `PromptStart` it would add a prompt to command navigation.
- **Two populations, `normal_markers` and `alt_markers`, routed by the active buffer**
  (`markers_mut`, #186/#187). An alt-screen region scroll rotates *alt* marks and leaves the primary
  list untouched with no guard at the call site; primary marks survive a `vim` session (#118/#158),
  and alt marks are disposed on alt-leave.
- **Command marks are primary-only by definition** (#192): `add_command_mark` returns early on the
  alt screen (#188), and `command_marks`/`command_lines` read the primary population whatever screen
  is up. That is why the command walks carry **no** alt-screen floor (see Cross-cutting invariants).
- **Death is an event, not an absence.** An off-screen marker is *omitted* from the viewport group
  while alive; the consumer learns of disposal through `TermEvent::MarkerDisposed`. Otherwise
  "scrolled away" and "gone" would be the same observation.
- **A marker's `col` domain is `[0, cols]`, a bound rather than a cell (#562).** A command that
  exactly fills its row ends one past the last column, which `extract_lines` absorbs through its
  exclusive end; storing `cursor.col` alone (held at `cols - 1` with `pending_wrap`) cost such a
  command its last character. The inclusive side steps a `from` of `cells.len()` to the next line.

**Bounds — the stream allocates this population**

- **`MAX_MARKERS` caps each buffer at `u16::MAX`, retiring the oldest** (#721). `add_command_mark`
  appends per OSC 133 sequence with no per-line dedup, and eviction only drops a marker whose line
  reached absolute 0 — so marks piled on one line are unreachable by eviction (measured: 70 000 live in
  a 24-row buffer). Overflow `pop_front`s the oldest and announces it through the ordinary
  `MarkerDisposed`, rather than refusing the newest (which would kill shell integration for the rest
  of the session once an unevictable pile fills the cap). Two consequences: the list is a `VecDeque`
  *because of this* (`push_marker`'s comment: `Vec::remove(0)` would memmove per push), and the cap is
  what makes the `u16` marker wire count safe without widening.
- **Command text: `MAX_COMMAND_TEXT` per record (4096 `char`s) and `MAX_COMMAND_TEXT_TOTAL` (16 MiB)
  across records**, because two bounds on one product bound nothing (#1038): `MAX_MARKERS` ×
  `MAX_COMMAND_TEXT` × 4 bytes is half a gigabyte, and a line-less stream (`CSI H`, `133;B`,
  `CSI 59;1H`, `133;C`, repeated) retained 202 MiB from 0.74 MiB of input on 727865d (19.6 MiB
  after). A capture that would cross the budget is cut at a `char` boundary to what is left —
  possibly nothing — and the record is still made. The maintainer chose a total budget with
  truncation over a lower per-command cap (2026-10-08). **The refund rides `Drop`**: a record's bytes
  are a `TextCharge` holding the pool, so every path that drops a marker returns them without naming
  the budget — a per-site decrement would have been another call list beside the funnel #936 exists to
  have one of.
- **`CommandRecord` is one boxed pointer on the marker, not a side table (#750).** Three marks in four
  never carry it; living on the marker, it dies with it — a side table keyed by `MarkerId` would need
  its own purge at every disposal site, the missing-destruction-funnel defect #750 is about.

**The pulled index and its validity**

- **A marker index is *pulled*, and three things keep it valid (#490).** `marker_index` answers once
  with every live marker of the **active** buffer (`MarkerEntry`: id, line, col, kind) plus the basis;
  the consumer then (a) rebases by the `evicted_total` delta, (b) appends on `MarkerCreated` and drops
  on `MarkerDisposed`, (c) re-pulls when `marker_epoch` moves. The split is by *how the buffer moved*:
  eviction shifts every marker identically (one number); birth and death are `O(1)` occurrences
  (events); everything else — reflow, a region rotate that moved a survivor, an alt switch, RIS — is
  non-uniform, so only a re-pull repairs it. `marker_count` in the frame header is the drift check.
- **The birth carries its own instant (#737, #741).** `MarkerCreated.line` is absolute at creation,
  and the same `feed` can evict afterwards, so rebasing against the batch-closing frame misplaces the
  mark (#737). A birth queued when the epoch bumps is an answer about a buffer that no longer exists —
  measured, a mark at absolute 3 reflowed to 5 while the basis stayed 0 (#741). So the event carries
  the pull's whole `(line, evicted_total, epoch)` triple, and the consumer adopts a birth only into the
  generation it names, comparing for **equality** because `bump_marker_epoch` wraps. Placement is
  therefore independent of whether the host drains events before or after reading the frame; only
  *cost* is not (frame-first leaves `marker_count` one ahead and spends a re-pull).
- **`evicted_total` counts only the uniform front-drops** — the scrollback cap, `ED 3` and
  `Term::clear`, all through `lines_left_the_front`. Reflow also drops lines off the front
  (`PaneReflow`'s `evicted`, installed by replacing the deque) and does not come through there: it
  moves survivors non-uniformly, so `marker_epoch` signals it. A holder rebasing off `evicted_total`
  without also watching the epoch is wrong across every resize.
- **`resize` bumps the epoch once, on a dimension change with a marker in either population (#490).**
  A reflow rewrites marker lines at several sites; bumping at the one function that runs them all
  beats a per-site obligation. The dimension gate matters because `resize` has no early return and
  `justerm-web`'s fit re-sends an unchanged grid when only the cell moved — ungated, 100 no-op
  resizes measured 100 bumps.
- **The per-line bump is a correctness trigger as well as a cost (#741).** A marker *below* a bottom
  margin shifts on every output line (`markers_shift_below_margin`, primary-only), bumping the epoch
  per line — 1 000 bumps over 1 000 region scrolls. The same bump is what moves markers non-uniformly
  *inside a single `feed`*: a mark in a DECSTBM footer born at line 4 is answered at 6 two accruals
  later with `evicted_total` unmoved. #737's completeness pass had recorded the opposite, which is why
  every earlier fixture reached the epoch through `resize`. This degrades to the pre-#490 cost
  (`O(M)` per frame) **only if the consumer re-pulls at most once per frame** — a stated obligation of
  the contract. The reach is narrow: it needs DECSTBM leaving a static footer **and** a marker inside
  it; ordinary scrolling bumps 0 times over 1 000 lines, and a marker *inside* the region bumps 0
  times too (with `scroll_top == 0` the scroll accrues into scrollback and the absolute line does not
  move). `markers_rotate_region` bumps only when a *surviving* marker moved.
- **Two corrections measurement forced on the consumer half (#738, #746).** The once-per-frame cap
  bounds requests, not availability: under per-line churn every pull lands stale, so the consumer holds
  no answer for the duration — a blank overview ruler and absent above-the-top anchors, not a bill.
  And the outage was not bounded by the churn: a consumer starting a pull only on an epoch *change*
  left the index unusable after churn stopped, with the count check silent (8 of 40 drag-resizes at
  RTT ≈ 100 ms). Fixed consumer-side by asking a *state* — "what I hold does not describe the newest
  frame, and nothing is in flight" — rather than an edge. Core unchanged.
- **The column is published on both incremental channels, `MarkerCreated.col` and
  `MarkerEntry::col` (#973)** — not on the frame's `MarkerPosition`, on `command_marks`, or in
  `justerm-web`'s `MarkerIndexEntry` mirror (a structural subset). The forcing case is a consumer
  reading the command *being typed*, between `B` and `C`, after the same batch echoed past `B`: PenTerm
  autocomplete takes the latest non-plain mark by `(line, col)` from `marker_index` (penterm
  7dadc3c95 `src-tauri/src/engine/prompt.rs` — `read_input`). It needs no dating of its own: only the
  two reflow branches of `resize` write `col`, and they bump the epoch. Pinned per branch
  (`a_reflow_moves_the_column_under_a_new_generation`,
  `a_reflow_under_the_alt_screen_moves_the_primary_column_too`); before the second existed, disabling
  the on-alt branch's write of the **primary** column left the whole core suite green.
- **Both resize branches carry `(line, col)`, not `(line, 0)`** (#166) — the alt branch once dropped
  the column, truncating the recorded command for any resize taken under a full-screen app. On the alt
  half the column is unpinned (`add_marker` passes 0 and no OSC-133 mark reaches `alt_markers`), but it
  is carried so the two halves state one invariant. **The alt half has no history, so its reflow limit
  is `0`**: passing the primary's limit kept rows a shrink pushed off and reported a marker four lines
  past the end; a marker whose row went is **disposed**, not relocated to row 0.

**Repair through motion, and through destruction in place**

- **Three movers maintain anchors through buffer motion** — `markers_shift_below_margin`,
  `markers_evict_oldest`, `markers_rotate_region` — called from the write path beside their selection
  and tracked-point counterparts (`selection_*`, `tracked_*`).
- **A fourth is not a mover: `dispose_markers_on_row` (#750).** An in-place erase leaves the row
  where it is while everything the mark was about stops existing; before #750 nothing fired, and
  `command_lines` reported commands at document lines that revealed onto blank rows — and after a
  `clear` the shell's redrawn prompt doubled every later command (measured `n=4` for two real ones).
  **`ED` retires, `EL` and `ECH` do not**, for a reason of its own: a line editor redraws the input
  line with `\r ESC[K` on every keystroke after `B`, so an `EL` that retired marks would delete the
  `CommandStart` being typed. The references converge on the same split. **The known residue** is the
  cursor's own row: both references route it through their partial-erase helper, so an erase can blank
  a row to full width and leave every mark on it — followed rather than widened, because the only
  argument for widening is symmetry (sizes under Known holes).
- **`Engine::clear` keeps the marks on what it keeps (#936)**, where xterm.js's `clear()` disposes
  every marker: what it keeps is the prompt being typed on. It keeps the cursor's whole **logical**
  line, because a wrapped command puts `B` above the cursor row (`a_wrapped_command_line_is_kept_whole`).
  Every other mark goes — history and the rows above through `erase_history` → `markers_evict_oldest`,
  the blanked rows below through `dispose_markers_on_row` — and `ED 3` retires history's marks the same
  way, which erases the pre-`clear` commands a real `clear` leaves in scrollback
  (`command_lines_capture.rs`). Pinned by `clear.rs` — `the_command_being_typed_is_still_reported`.
- **What is not in the buffer is frozen on the mark when the stream reveals it (#750).** The command
  text is captured at `C` (`capture_command_text`) and the exit written at `D` (`attach_exit`).
  Re-reading text through the `[b_col, c_col)` clip names whatever now occupies those cells — measured
  for overwrite, `ICH`, `DCH` and erase, so no lifetime rule closes it. Resolving the exit at *query*
  time paired over survivors and slid a code onto the previous command once a disposal broke the run
  (measured, `a0` wearing `a1`'s `Some(2)`). Only `line` stays derived — the half the movers maintain.

**The command lines**

- **`CommandLine::line` is a document line**, the logical-line index of its `CommandStart` within
  `Term::accessible_text` — core's analog of VS Code's `bufferToEditorLineMapping`, because the
  frame-mode web side has no wrap information to map an absolute line itself. It is meaningful only
  with the document of the same instant on the primary screen; on the alt screen `accessible_text`
  returns the other document. Re-ask, never rebase (ADR-0029; `Engine::command_lines` doc; pinned in
  `tests/command_lines_document.rs`).
- **Outside input.** OSC 133 sequences allocate marks and capture text; both are bounded above
  (`MAX_MARKERS` per buffer, `MAX_COMMAND_TEXT`, `MAX_COMMAND_TEXT_TOTAL`). `add_marker` is consumer
  input, bounded by the same cap.

## Code

- `justerm-core/src/term/markers.rs` — `Term::add_marker`, `remove_marker`, `push_marker`, `add_command_mark`, `capture_command_text`, `open_command_start`, `attach_exit`, `dispose_markers_on_row`, `command_marks`, `command_lines`, `command_start`, `doc_line_of`, `primary_grid`, `markers_mut`, `marker_index`, `bump_marker_epoch`, `markers_shift_below_margin`, `markers_evict_oldest`, `markers_rotate_region`, `marker_positions`
- `justerm-core/src/term.rs` — `Marker`, `CommandRecord`, `TextCharge`, `MarkerEntry`, `MarkerIndex`, `CommandLine`, `MAX_MARKERS`, `MAX_COMMAND_TEXT`, `MAX_COMMAND_TEXT_TOTAL`, `Term::lines_left_the_front`, `Term::clear`
- `justerm-core/src/serialize.rs` — `MarkerId`, `MarkerKind`, `MarkerPosition`
- `justerm-core/src/event.rs` — `TermEvent::MarkerDisposed` and its MarkerCreated mirror (a struct variant)
- `justerm-core/src/lib.rs` — `Engine::marker_index`, `Engine::command_marks`, `Engine::command_lines` (the docs.rs statement of each dating contract)
- `justerm-core/tests/marker_index.rs` · `marker_cap.rs` · `osc133.rs` · `command_lines*.rs` · `command_marks_instant.rs` · `clear.rs` · `alt_markers.rs` · `alt_resize_markers.rs`
- `justerm-web/src/marker-index.ts` — `MarkerIndexEntry`, the consumer-side index

## Reference behaviour

- [What retires a line-anchored mark when the line's content is destroyed in place](../../agents/reference-facts.md#what-retires-a-line-anchored-mark-when-the-lines-content-is-destroyed-in-place-750-verified-2026-08-07)
  — `ED` retires, `EL`/`ECH` do not, and the cursor row's partial-erase residue.
- [What the only system that recovers command text does](../../agents/reference-facts.md#what-the-only-system-that-recovers-command-text-does-750)
  — the freeze-at-`C` model.
- [Dating an anchor across a non-uniform move](../../agents/reference-facts.md#dating-an-anchor-across-a-non-uniform-move--the-one-reference-with-a-generation-and-why-it-may-compare-with--741-verified-2026-08-06)
  — the epoch.
- [Relating a coordinate to the document it names](../../agents/reference-facts.md#relating-a-coordinate-to-the-document-it-names--how-the-references-make-which-buffer-unaskable-743-verified-2026-08-07)
  — the `command_lines` document line.
- [The overview ruler](../../agents/reference-facts.md#the-overview-ruler--who-has-one-how-a-mark-is-merged-and-how-big-it-is-500-verified-2026-08-10)
  — what consumes the index.
- [The marker's clear discipline](../../agents/reference-facts.md#the-markers-clear-discipline-534-verified-2026-07-27)
  — a **different** artifact (the wide-spacer marker) that shares the word; linked so a search for
  "marker" lands on the distinction.

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — markers are the
  **third category** there: `doc_line_of` and `command_start` walk absolutely and must **not** get a
  floor, because `command_lines` threads `primary_grid()` through them so all run in one coherent
  `[scrollback ++ primary]` buffer. The failure mode is the mirror of the usual one — someone *adding*
  a floor and breaking command navigation on the alt screen.
- [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  — all three outbound channels meet on this primitive and answer differently: `marker_positions`
  rides the frame and gets coherence free; `MarkerCreated` had to be given a basis (#737) and then the
  generation (#741); `command_marks` is declared instantaneous (#742), available to it because its
  scope is primary whatever screen is up, while `marker_index` follows the active buffer.
  `command_lines` reached the same shape from the other end (#743): a document line had no scalar it
  *could* carry, and it also names a **document**, which on the alt screen is not the one
  `accessible_text` returns.
- [the write path funnels motion and does not funnel destruction](../invariant/no-funnel-for-destruction-in-place.md)
  — this territory found the gap and answers **retire**: the movers read as the whole job, so nothing
  repaired a mark whose row was blanked where it stood (#750).
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — the viewport marker group's count is still `u16` after #621 widened its siblings, and the viewport
  does not bound it (several marks share a line). Past 65 535 the count wrapped while every record was
  still written, and `decode` returned `Ok` with every later group garbage-derived. Closed by #721 at
  the producer (`MAX_MARKERS`), deliberately not by widening, which would entrench the ADR-0020 R3
  violation (the payload is still `O(M)`; M is bounded).
- [RIS keeps configuration and drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — `Term::full_reset` announces a `MarkerDisposed` for every marker and drops both sets (coordinates),
  but carries `next_marker_id` (a handle's counter the consumer still holds — a reissued id would let
  a stale disposal drop a live post-RIS marker) and carries `marker_epoch` across and then bumps it, so
  a held index re-pulls (#490). Carried or not, `next_marker_id` is a `u32` advanced with `+= 1` in
  `push_marker`, so it still wraps at `u32::MAX` (that note's § A counter carried across RIS).

## Blast radius

- [decoration](decoration.md) — the consumer joins its registry by `MarkerId`; a change to lifetime
  or disposal changes what it must reconcile, and every death must arrive as `MarkerDisposed`.
- [frame](frame.md) — the viewport group and the header basis; a change to when `evicted_total` or
  `marker_epoch` moves silently stales every held index.
- [events & replies](events-and-replies.md) — `MarkerCreated` must carry every scalar `MarkerIndex`
  carries; adding a field to one and not the other breaks the incremental mirror.
- [selection](selection.md) · [tracked points](tracked-points.md) — their movers sit beside these
  at every write-path call site (deliberately not merged into one `anchors.rs`, #584); a new mover owes
  three calls, and an anchor-contract change that breaks one likely breaks all.
- [search](search.md) — the contrast case: highlights are *dropped* on motion where marks are moved,
  because only search has a re-search behind it (#750).
- [accessibility](accessibility.md) — `command_lines` indexes the `accessible_text` document; a
  change to how either collapses wrapped rows desynchronises command navigation.
- [logical lines](logical-lines.md) — `doc_line_of` counts logical lines the same way logical-line
  assembly does; a change to either's wrap rule moves `CommandLine::line`.
- [viewport](viewport.md) — a marker line is absolute and the ruler divides by `scrollback_len +
  rows`, so the header scalars are part of this contract.

## Known holes / open

- **Ghostty's `133;P` (a prompt start without the fresh line) is not read**, `P;k=s` included; only
  the `A` form of the redraw options is (#1083). A shell that sends `P;k=s` for a secondary line
  gets no narrowing.
- **bash re-sends `133;B` on every SIGWINCH redraw**, so a window drag adds one `CommandStart`
  per resize event (measured: 36 resizes, 3 → 39 marks), bounded by `MAX_MARKERS`. Older than #1083,
  which only reads past them.
- **The cursor row keeps its marks even when an erase blanks it whole** — the reference edge,
  pinned rather than fixed. `ESC[H ESC[0J` (or `ESC[H ESC[J`, `tput ed`) leaves **one** phantom over an
  *empty* document, while `ESC[1;16H ESC[1J` at 16 columns leaves **all four with zero disposals**
  (`command_lines_content.rs` — `ed_0_disposes_the_rows_below_and_keeps_the_cursor_row_marks`,
  `ed_1_at_the_last_column_blanks_the_row_and_retires_nothing`). The empty-document half matters to
  the consumer: `AccessibleView` splits `""` into one element, so `reveal(0)` lands focus on a blank
  row and the command is announced anyway. **Reachability measured three times, zero each time**
  (#750): ED 1 appears in none of the 16 recorded captures and ED 0 in one (htop); an inline
  `curses.filter()` program and `whiptail` both went to the alt screen; `fzf --height` stays on the
  primary and emits `ESC[9A ESC[J` but erases only its own region below the prompt (33 marks before and
  after). The reaching shape must stay on the primary **and** erase upward over the prompt.
- **`MarkerDisposed`'s own doc names only two causes** ("evicted past the scrollback cap, or scrolled
  out of an in-screen region") while it also announces `ED` disposal, `MAX_MARKERS` overflow, alt-leave,
  the alt resize drop, `Engine::clear` and RIS. It is the thing a consumer must not get wrong, and the
  published statement of it is incomplete.
- **`MarkerIndexEntry` in `justerm-web` omits `col`** — a structural subset, harmless until a web
  consumer needs the typed-command column the Rust channel already carries.
- **`next_marker_id` is a `u32` incremented with `+= 1`** in `Term::push_marker` and carried across
  RIS, so past `u32::MAX` registrations it panics in a debug build and wraps in release. Live markers
  are capped at `MAX_MARKERS`, so a wrap reissues a dead id rather than duplicating a live one; see
  [RIS keeps configuration](../invariant/ris-keeps-configuration-drops-coordinates.md).

