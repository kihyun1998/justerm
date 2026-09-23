# Territory — selection

## What it is

The user grabs a range spanning screen and scrollback, and that range becomes ① the **highlight** the
renderer paints and ② the **text** that reaches the clipboard. Engine-owned — the consumer only does
pixel→cell conversion and clipboard transport.

## Governing decisions

- [ADR-0026 — a coordinate that arrives from outside is bounded once](../../adr/0026-outside-coordinates-are-bounded-once.md)
  governs **one axis only**: what happens to an out-of-range coordinate handed in through
  `selection_begin` / `selection_extend`, where the bound goes (the producer, here), and that a reader
  may not bound one end of a pair. It says nothing about the selection *model* below — the blank that
  follows is still the state for everything else

The rest of this section is the actual state. Two more decisions sit adjacent, and neither governs the
selection *model* either:

- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  decides that the selection **highlight rides the frame** → *delivery* only, not the model
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  supplies the reason selection lives in core ("needs the whole buffer") → *routing* only

So the anchor coordinate space, `Side`, the four types, the wrap join and the artefact-drop rule are
governed by **no record at all**. The four lines under `docs/architecture.md` §Selection are the only
prose, and prose is not a decision record.

## Design model

With no record, everything below was **read out of the code** — which is itself this territory's
status.

- **`SelectionType` is deliberately exhaustive, on convergence rather than on traffic** (#843).
  Traffic cannot decide it: the type appears in one public signature, as a *parameter* to
  `Engine::selection_begin`, and nothing hands one outward, so the attribute would cost a consumer
  nothing (an earlier draft argued the opposite from traffic this API does not have). What keeps it
  closed is that alacritty arrives at the same four modes independently — `Simple` / `Semantic` /
  `Lines` / `Block` (`alacritty_terminal/src/selection.rs:93`), glossing `simple` as "without any
  expansion" and `semantic` as expanding "to the nearest semantic escape char".

- **Typing drops the selection, and it is the widget that decides so** (#913). `LocalPointer` gained
  an **optional** `clear()` — optional so a consumer on the older shape keeps compiling and simply
  does not drop — which `Terminal` calls for any user input. Two things about it are easy to get
  wrong and are the reason it is written down: it is **not** gated on the viewport (the snap that
  ships with it is, and bundling them would leave a selection alive whenever the user is already at
  the bottom, which is most of the time), and the *"is there one?"* guard lives in the controller
  rather than the widget, because the widget cannot see selection state. A live drag is skipped
  deliberately — dropping the anchor mid-gesture leaves the next `mouseMove` extending nothing.
  Both references do this at the same moment
  ([`reference-facts.md` § scroll-on-user-input](../../agents/reference-facts.md#scroll-on-user-input--both-references-snap-and-xtermjs-does-it-at-two-sites-913-verified-2026-09-16)).
- **The widget reports a settled selection on two signals, not one, and they are guarded
  differently** (#914). `onPrimarySelection` **commits** — it fires on the release and carries
  the text; `onSelectionChange` **notifies** — it fires on each selection command and carries
  nothing. Five things about the pair are easy to get wrong and are why it is written down:
  - **The commit is guarded on the selection being empty, never on the pointer having moved.** It
    was the latter until #914, which made a double- or triple-click — a real word/line selection
    that releases without motion — unreportable. The emptiness test is not written at the call site
    at all: `copySelection` skips `null` and `""`, and core resolves a never-extended `begin` to a
    zero-width run, so a bare click stays silent by arithmetic rather than by a flag. The
    `hasSelection` term still standing beside it is presently unreachable-false and reddens no test;
    it is a belt, not the guard.
  - **That arithmetic was false on a wide glyph until the same change fixed core, and the web half
    is unsafe without it.** `resolve` applied the #454 pair widening to a zero-width run whose one
    column sat between a lead and its spacer, so a bare press on either inner half returned the
    glyph — and the Block arm disagreed with itself, `selection_range` refusing the empty rectangle
    while `selection_text` widened it. The motion flag had been hiding that; removing it made the
    click overwrite primary. Core now decides emptiness from the endpoints before widening
    (`same line && from >= to`), as all three references do. **So a web build carrying #914 must
    not ship against a core without it** — see
    [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md).
  - **The change signal carries no text on purpose.** `SelectionPort.text()` round-trips to the
    backend in frame mode, so a text-carrying signal at per-cell firing rate is a round-trip per
    pointer move. Passing the controller's `hasSelection` instead was measured and rejected: it is
    set at `begin`, so it reads `true` after a bare click that selected nothing.
  - **Its de-dup is keyed on the selection — `type | anchor | focus` — never the pointer's cell,
    and `tick()` is outside the key.** A first version keyed on the cell and passed every test while
    silencing every word and line selection: a real triple-click re-anchors *one* cell three times
    with a growing type, and the tests built a fresh controller per case, so they never pressed
    twice. An auto-scroll tick has no stable identity — the pointer is held still outside the
    viewport and every tick extends to the same `(edgeRow, lastCol, lastSide)` while the selection
    grows under it.
  - **It can report a change that altered no text, and a consumer is expected to debounce.** The
    controller sees commands, not the engine's resolved selection: a bare click in a pane with
    nothing selected replaces nothing with nothing and still reports, and a tick reports even when
    the scroll is pinned at a boundary. Both references compare resolved selections and so do not;
    matching them would need the engine's answer, which is the round-trip the signal exists to
    avoid. The one real consumer already debounces (PenTerm's copy-on-select, 150 ms).

  Both references keep the same two-event split
  ([`reference-facts.md` § two selection-out signals](../../agents/reference-facts.md#two-selection-out-signals-and-what-actually-guards-the-committing-one-914-verified-2026-09-16)).
- **Select-all is two ordinary anchors, not a mode** (#935). `Term::select_all` needs no viewport
  coordinate, which is the whole point: a frame-mode consumer holds only the viewport and would
  otherwise have to scroll the user's view to the top and back to anchor there. Four things about it
  are easy to get wrong:
  - **It is a snapshot.** xterm.js keeps a flag (`isSelectAllActive`) and recomputes the extent on
    every read, so its selection grows with output. Here the extent is two absolute anchors and the
    three fixups plus reflow keep them on their content like any other selection's — a flag would
    have needed its own arm in each. This is a *derivation* from the anchor model, and a better one
    can overturn it.
  - **Blank edges are trimmed**, first non-blank cell to last, and an all-blank buffer selects
    nothing (ghostty `Screen.selectAll`). This was the **maintainer's call**, shown against
    xterm.js's whole-buffer `[0,0]..[cols, ybase+rows-1]`, which copies a trailing run of newlines
    from a half-empty screen. "Blank" is U+0020 with no combining mark — a combining carrier on a
    space is content, as [only U+0020 can be padding](../invariant/only-u0020-can-be-padding.md)
    requires, where ghostty's base-code-point test would drop it.
  - **"All" is the active buffer**, floored by `abs_floor` on the alt screen, as xterm.js selects
    its active buffer.
  - **On the web it goes through `SelectionController.selectAll`, not the port directly**, because
    the controller owns the two things the rest of the widget reads: `hasSelection` (so typing drops
    it and a Shift+click extends it) and the change signal, which it fires as xterm.js's
    `selectAll()` fires `onSelectionChange`. It does **not** feed primary: xterm.js feeds primary
    only from a mouse selection, while ghostty's `select_all` copies — the references split.
    `SelectionPort.selectAll` is optional (the #913 precedent), and the controller does nothing
    without it. A select-all made **during a live drag** is extended by that drag's next move to
    "buffer start → pointer"; xterm.js's flag outlives the drag. Unhandled, because it needs the
    shortcut pressed with the button held.
- **Anchors are absolute buffer coordinates** — `BufferPoint { line, col }`, where `line` indexes
  `[scrollback ++ screen]` from the oldest line. Not viewport coordinates.
- **Why absolute**: it is invariant under a top-anchored scroll. A line evicted into scrollback grows
  `scrollback.len()` by exactly the screen shift, so existing content keeps its absolute index.
- **Four things move the coordinate, and they do not line up one-to-one with the fixups.**
  `selection.rs`'s module doc names three *kinds* — cap eviction, in-screen region/RI scroll, reflow —
  and that count is the one worth quoting, but the handlers are:

  | what moves it | handler |
  |---|---|
  | scrollback cap eviction, and `ED 3` / `Engine::clear` (#936) | `Term::selection_evict_oldest(n)` — one line for the cap, all of history at once for the other two, where an endpoint on *any* dropped line clamps and a selection wholly inside them is cleared (`clear` then drops the selection outright). An endpoint on the evicted line clamps to column 0 / Left of the new top line (a Block keeps its columns), the rule the region rotate already applied. Until #935 it kept its column, so an indented top line lost its first cells on every evicted line. Select-all made that routine, because its start sits on the oldest line (xterm.js `handleTrim` resets to `[0,0]`) |
  | an in-screen region / RI scroll moving content | `Term::selection_rotate_region` |
  | a **top-anchored sub-region** scroll growing scrollback while rows below the margin stay put, so their absolute index rises (#449) | `Term::selection_shift_below_margin` |
  | reflow re-splitting logical lines | `grid.rs`'s `reflow`, via tracked points |
  | a **shrinking** resize on the **alt** screen | **none — the selection is dropped** (`Term::resize`, #660). Not for want of machinery: the alt branch tracks points through its own `reflow_pane` call and already rotates *markers* with the returned `evicted`. A selection is two ordered endpoints, so a shrink that destroys the row under one and not the other has no "dispose" answer, and reusing the marker policy would move its ends by different rules. A grow moves nothing and keeps it |

  The third is the one a three-item list hides: nothing on screen moved and no line was evicted, yet
  every absolute index below the margin changed — because `scrollback.len()` grew and the coordinate
  is measured from the oldest line. It is the counter-case to the invariant directly above.
- **`Side` (Left/Right)** — which edge of a cell the anchor sits on. Lets a drag include or exclude the
  cell under the pointer; this is what makes mouse precision possible.
- **Four types** — Char (runs across lines) / Word / Line / Block (rectangular).
- **Two outputs** — `selection_range()` yields **per-viewport-row** `SelectionSpan { row, left, right }`
  and emits nothing for off-screen rows. `selection_text()` yields copy text, applying the wrap join,
  trailing-whitespace trim and scrollback traversal.
- **Split of labour** — `src/selection.rs` holds types only (75 lines). The cell-aware logic (text
  extraction, range clipping) lives in `src/term/selection.rs`, where the cells are reachable —
  moved out of `term.rs` in #587.
- **`set_word_separators` forces `' '` into whatever it is given**, and that is not a convenience.
  A blank cell packs `' '`, so the space ends the walk at the end of a row's written text *and*
  backstops the wide-pair rule: without it, double-clicking next to a wide separator starts the
  highlight on that separator's trailing spacer, bisecting the glyph, and the walk runs to the row's
  end through the padding. Enforcing it at intake rather than in the walk is ghostty's shape — it
  prepends its own blank codepoint to every parsed set (`config/Config.zig`, *"Always include null as
  first boundary"*). The set is the only thing bounding the walk, so one that omits the separators
  present in the buffer makes a double-click walk the whole soft-wrap run (11.7 ms release, 801,920
  chars). **A length bound, if ever wanted, is a field beside this one, not an argument**:
  `word_start` / `word_end` are `pub(super)`, reached through `Term::selection_begin`, so there is
  no call site for a consumer to inject into.
- **An alt-screen resize drops the selection, on any geometry change (#660)** — a policy, not a
  capability limit: the alt branch makes its own `reflow_pane` call with tracked points and uses the
  result to rotate alt markers (`reflow: false` disables the re-split, not point tracking). The first
  comment on it claimed alt had "nothing to re-anchor through", which is the failure #660 is.
  Rotation is the wrong trade because a marker is one point with a binary fate while a selection is
  two ordered endpoints: when a shrink destroys the row under one and not the other, "dispose" means
  nothing and the right behaviour is the clamp-and-overtake policy `selection_rotate_region` applies
  to scrolls — a second policy, not this fix. Both references drop rather than rotate on the axis
  they consider unsafe: alacritty on a width change (`term/mod.rs:680-682`), xterm.js on a height
  change (`SelectionService.ts:156-160`). **Any change, not just a shrink**: keeping it on a grow was
  tried and a randomised sweep refuted it in one run — an alt resize reflows the primary pane, and
  on alt `scrollback` *is* that history, so `scrollback.len()` moves under an anchor even when the
  alt grid does not, and `selection_text` walks off the end. Rebasing by the base delta would be
  unambiguous on a grow but a shrink still needs the two-endpoint policy, and shipping half would
  leave the axes behaving differently. The exact no-op is not a resize, so a consumer that
  re-asserts its size does not make alt selection impossible.
- **`DEFAULT_WORD_SEPARATORS` is alacritty's `SEMANTIC_ESCAPE_CHARS` verbatim plus U+3000**
  (`alacritty_terminal/src/term/mod.rs:45` @ `852e971`): space, tab and a punctuation set that
  omits `.`, `/` and `-` so a path or URL stays one word. Two properties are load-bearing. **It is a
  literal set, not the Unicode `White_Space` property** — a `char::is_whitespace()` predicate would
  end a word at the four `Line_Break=GL` (glue) spaces U+00A0, U+2007, U+202F and U+205F, whose
  purpose is "do not break here"; a locale-formatted `1<NNBSP>234` double-clicked as `1`. All three
  references are literal sets for the same reason. **U+3000 is justerm's one divergence from every
  reference default**: it is the only East-Asian-Wide codepoint `White_Space` accepts (measured over
  U+0000–U+10FFFF), so on alacritty and xterm.js `　abc` is one word. It was once kept on the ground
  that core had no injection point; that ground is gone, so it survives as a default a consumer
  wanting reference-exact behaviour removes.

## Code

- `justerm-core/src/selection.rs` — `SelectionType`, `Side`, `SelectionSpan`, `BufferPoint`, `Anchor`,
  `Selection::ordered`
- `justerm-core/src/term/selection.rs` — `Term::selection_begin` / `selection_extend` /
  `selection_clear` / `select_all` / `selection_range` / `selection_text` / `accessible_text`; the three coordinate
  fixups `selection_shift_below_margin` / `selection_evict_oldest` / `selection_rotate_region`; and
  the private `resolve` / `Resolved` that turn a selection into absolute bounds. Extracted from
  `term.rs` in #587. As with search, the crate now has **two** files named `selection.rs` — the
  types in `src/selection.rs` above, the mechanism here — so a bare `selection.rs:NN` citation is
  ambiguous
- `justerm-core/src/term/walk.rs` — the shared buffer-walk floor the selection reaches cells through:
  `Term::abs_line` / `abs_row`, `prev_pos` / `next_pos` (the logical-line step), `word_start` /
  `word_end`, `is_word_boundary`. Extracted from `term.rs` in #585. Since #545 `is_word_boundary`
  is a `Term` method reading injected policy, not a free function over a fixed set — the set itself
  lives in `term.rs` (`DEFAULT_WORD_SEPARATORS`, `set_word_separators`, and the `full_reset` line
  that carries it across RIS)
- `justerm-web/src/selection.ts` — `SelectionController` (the web gestures: char / word / line /
  block, drag auto-scroll, alt-click) and `SelectionPort`, the write channel to the engine's
  `selection_*` that pairs with the read-only frame channel
- Consumers: justerm-web does pixel→cell and clipboard; justerm-renderer paints the highlight.
  `SelectionController` binds no listeners — since #902 `Terminal` feeds it through
  `TerminalOptions.selection`, only the presses the application did not take (see
  [input encoding](input-encoding.md))

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row is pinned to a `file:line`
at a recorded SHA; a paraphrase drops the pin).

- [Word selection started *on* a separator](../../agents/reference-facts.md#word-selection-started-on-a-separator--the-references-disagree-so-justerm-is-not-an-outlier)
  — justerm's walkers break on the **neighbour** cell's class, never the start cell's own, so
  word-selecting the space in `"ab cd"` returns both words joined. That looks like a defect and is
  not: alacritty does the same and xterm.js does the opposite, so a **split reference makes this a
  product choice, not a correctness fix**. Recorded explicitly so it is not re-litigated
- [Mapping a tracked point through reflow](../../agents/reference-facts.md#mapping-a-tracked-point-through-reflow-549-verified-2026-07-27)
  — how a reference carries an anchor across a re-split, which is what `reflow(points)` does with the
  selection
- [A selection when the screen changes under it](../../agents/reference-facts.md#a-selection-when-the-screen-changes-under-it-660-verified-2026-07-31)
- [What the engine does with a column it was handed anyway](../../agents/reference-facts.md#what-the-engine-does-with-a-column-it-was-handed-anyway-671-verified-2026-07-31)
  — the read-side half. The three references converge on *"an out-of-range column is not
  observable"* and reach it three different ways (clamp / a guaranteed producer plus a total reader /
  a type that cannot express it). justerm now clamps, at the write site, and the section records what
  that choice does **not** buy — no equivalent of alacritty's wrap-to-next-line arm, which it does not
  need (#671)
  — the three available designs, one per reference: clear on a width change and rotate otherwise
  (alacritty), clear on a height change (xterm.js), or make the anchor a tracked pin so it cannot go
  stale (ghostty). justerm's primary pane is the first; the alt pane had none, which is #660

## Cross-cutting invariants

- [an absent element box measures as zero](../invariant/an-absent-box-measures-as-zero.md) —
  `CellGeometry.originX`/`originY` are the only two of its six fields with no stated precondition,
  because a position may legitimately be `0` or negative, so `geometryViolations` structurally cannot
  flag a box that has gone away. **Repaired in #819**: `getGeometry` answers `CellGeometry |
  undefined`, because the consumer took the measurement and is the only party that can tell absence
  from a legitimate `0`. This territory holds the site, and it is the one with **state** to unwind —
  `SelectionController.mouseMove` *and* `tick` both reset `dragScrollAmount`, since a refusal that
  only returned early would latch the last auto-scroll speed and the tick timer fires whether
  or not the pointer moves. Distinct from the product ambiguity #680 settled next door, which this
  note draws the boundary against — and #819 is what showed the two are reachable through the *same*
  symptom: #680's `cellHeight > 0` guard passes when the cell comes from the renderer, so the
  max-speed auto-scroll it fixed returns through the origin
- [the write path funnels motion and does not funnel destruction](../invariant/no-funnel-for-destruction-in-place.md)
  — this territory takes the **positional** answer, and it is the discriminator that keeps the note
  honest: a selection is a region of the screen, so showing what is now under the highlight after
  an in-place erase or overwrite is the semantics rather than staleness (#750)
- [the cell size is derived state](../invariant/cell-size-is-derived-state.md)
  — the pointer-to-cell conversion here divides by a cell that five setters can move, in a unit the
  renderer does not report (#578)
- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `Term::prev_pos` must
  not join down into the primary scrollback while on alt (#207)
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — the selection overlay group shares its record shape *and* its count prefix with search's, so it
  inherits the `u32` widening #621 made there. A selection cannot practically reach the ceiling the
  way a query can, which is why the fix arrived from the other territory
- [RIS keeps configuration and drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — this territory holds **both** halves: the injected `word_separators` must survive `ESC c` (#545)
  while the `selection` anchors beside it must die with the buffer they index
- [only U+0020 can be a row's padding](../invariant/only-u0020-can-be-padding.md) — this territory
  holds **two** of the five implementations: `extract_lines` for the linear arms and the block arm's
  own loop. #685 measured them returning different text for the same cells when only one was fixed
- [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md) — this territory
  holds **both** observables of it: `selection_range` and `selection_text` widen at the same funnel
  (`resolve`) precisely so they cannot disagree (#454)
- [a pointer coordinate is bounded by the converter that produces it](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — `cellAndSide` owes the bound on both axes, and the engine's own clamp (both axes since #671) is a
  backstop rather than a substitute: the alt-click cursor move leaves through the consumer's callback,
  so no core guard is on that path at all (#667)

## Blast radius

Check these after changing this territory:

- [wide glyph & soft wrap](wide-glyph-and-soft-wrap.md) — `selection_text` performs the wrap join and
  drops the wide-wrap artefact, so a change to the pair/wrap rules changes extraction output
- [frame & wire](frame-and-wire.md) — the highlight leaves as an overlay group, so ADR-0014's wire group
  is affected
- [search & active match](search.md) — the active search match is painted **on top of** selection in
  its own colour, so the precedence between the two overlays is a single decision touching both
  (#430 pins the active ∩ selected fg channel). Both also share the absolute coordinate space
- [damage & viewport](damage-and-viewport.md) — the highlight's visibility is gated by the same
  `display_offset` as everything else pushed into the engine
- [logical lines](logical-lines.md) — `accessible_text` is listed under **both** territories' `## Code`
  (it lives in this module, but its contract is a whole-buffer document), so a change to either side
  can invalidate the other's pin. The edge was one-way until #587, and that is exactly how the move
  broke logical-lines' pin without any sweep noticing
- [reflow](reflow.md) — the fourth row of the table above. `grid.rs`'s `reflow` takes selection
  anchors as tracked `points`, and #562 (reflow cannot express a point one past the last cell) surfaced
  right here

## Known holes / open

- **`resolve`'s five `+ 1`s stay unguarded, which is sound only while every stored anchor arrives
  through `Term::viewport_to_abs`** ([viewport](viewport.md)), where both axes are clamped (#660,
  #671). Bounding there rather than at each `+ 1` keeps one site answering "what does a viewport
  coordinate mean" instead of five for one rule. The writers were enumerated when #671 landed: the
  three coordinate fixups move `.line` or write columns in range by construction, `resize`'s primary
  branch re-clamps the reflowed points (#562) and its alt branch drops the selection (#660), and
  `Term::resize` is the only writer of `grid.cols()`. **A fourth writer of `self.selection`** — one
  that builds an `Anchor` without that function — would put `resolve` back in reach of its own
  arithmetic, and nothing checks for one. The clamp is deliberately not `debug_assert`ed: pointer
  input past the grid is ordinary (ADR-0026 D1).
- **`SelectionController` remembers a selection core has dropped.** `hasSelection` is set on `begin`,
  while core clears the selection on a screen swap — so a Shift+click after leaving an
  alt-screen application extends nothing and selects nothing. Seen while working #902, which sidesteps
  it only for the forced press (that one anchors); the ordinary Shift+click still has it. The
  controller sees no frames, so repairing it needs a signal it does not receive today.
  **#913 narrowed this and did not close it**: the sentence used to read *"and never cleared"*, which
  stopped being true when `clear()` arrived — but its one caller is user input, so the field is still
  wrong for the trigger described here, a screen swap the controller never hears about.
  **#914 found a second, much commoner way it lies, and the word "exactly" above had to go**: the
  field is set at `begin`, so a bare click that selected nothing leaves it `true` while the engine
  reports empty text — measured, not reasoned. That is every click, not an alt-screen aftermath.
  It is why the change signal #914 added carries no state (a boolean payload would have published
  this defect), and why that signal never reads the field. #914 did close one smaller way it lied:
  the alt-click cursor move dropped its selection at the port and left the flag `true`, so the next
  keystroke's `clear()` dropped it a second time — that branch now clears the flag too.
  **#935 added a third way**: `SelectionController.selectAll` sets the field unconditionally, while
  core selects nothing on an all-blank buffer, and `SelectionPort.selectAll` returns `void`, so the
  controller cannot know. The visible cost is small — a Shift+click on an empty pane extends nothing.
- **Neither selection-out signal hears about a selection the widget did not make.** Two paths change
  the engine's selection without passing through `SelectionController`, so a consumer listening to
  `onSelectionChange` (#914) is not told:
  - **core drops it** — on a screen swap, or a shrinking resize on the alt screen. Same root as the
    hole above: the controller sees no frames. xterm.js does report this one, because its selection
    service owns the buffer-activate listener.
  - **an assistive-technology text selection** — the widget's accessibility layer calls
    `a11ySelectionToPort`, which drives `port.begin` / `extend` / `clear` directly, and the
    `selectionPort` option tells a real consumer to pass the same port the mouse uses (the demo
    passes a logging stub instead, so it does not show this). xterm.js
    routes the same path through `terminal.select(...)`, which reaches its change event. Found by
    #914's completeness lens, not acted on there: it needs `a11ySelectionToPort` to take a reporter,
    which is a change to a second published function.

- **Zero governing records.** The whole §Design model above is unrecorded. *"Why absolute
  coordinates"* and *"what moves the coordinate"* are the kind of thing that gets
  re-decided, and their grounds exist only in code comments.
- ~~**Block selection over wide characters is unspecified**~~ — **closed by #454**: a rectangle
  widens onto whole pairs **per row** (a rectangle meets a pair at a different column on each), on
  both observables. The rule is
  [a span covers a wide pair whole](../invariant/a-span-covers-a-wide-pair-whole.md); the visible
  consequence is that a row where it fires is one column wider than the rectangle, which is what all
  three references do.
- ~~**Word-selection boundaries** — the set is hardcoded in core.~~ **Closed by #545**: the set is
  consumer policy now (`Term::set_word_separators`, default `DEFAULT_WORD_SEPARATORS`), so the
  routing conforms to ADR-0017. Two things it left behind, both narrower than the hole was:
  - ~~the **trailing trim** is still `char::is_whitespace()`-based~~ **closed by #685**: the trim
    removes `' '` only, at both of this territory's sites — `extract_lines` *and* the block arm's
    own per-row loop, which is a second implementation nothing had noticed. The rule is now
    [only U+0020 can be a row's padding](../invariant/only-u0020-can-be-padding.md). What it did
    **not** close: a *written* trailing ASCII space is still dropped, because `Cell` cannot
    distinguish one from a blank — the references split 2–1 there against 3–0 on the property, so
    it is a separate decision, not a leftover;
  - `' '` is **forced into every injected set** at the setter, because a blank cell packs `' '` and
    the walk uses that both to stop at a row's padding and to backstop the wide-pair rule. That is
    ghostty's shape (it prepends its own blank codepoint at the config intake), and it is the one
    part of this policy the consumer does *not* own.
