# Territory — accessibility (the screen-reader mirror)

## What it is

Making a GPU canvas readable by assistive technology. The renderer's canvas is opaque to AT, so the
widget package drives a hidden DOM beside it: a row tree mirroring the viewport, an `aria-live` region
announcing new output, an on-demand accessible view of the whole buffer, command-outcome announces,
prompt-to-prompt navigation, and a bridge from an AT text selection to the engine's selection. The
whole-buffer text and the command list come from core; *what* AT is told, and when, is consumer policy.

Every piece here is host-wired: `Terminal` constructs none of it. Only the demo wires it today — the one
consumer, PenTerm, builds its Native panes with no `Accessibility`, no accessible view and no command
announce (penterm 7dadc3c95 `src/blocks/terminal/lib/nativeTerminalCache.ts` — `build`). The one piece a
`Terminal` itself owns is the hidden input textarea's accessible labelling.

## Governing decisions

- [ADR-0011 — justerm-web keeps a viewport cell mirror](../../adr/0011-justerm-web-viewport-cell-mirror.md)
  — the `CellMirror` the row tree reads its text through; its #504 amendment made the screen-reader row
  tree the mirror's surviving reason to exist, and made the mirror text-only
- [ADR-0029 — a published coordinate carries its instant or is re-asked](../../adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md)
  — D3's re-ask is why command navigation samples its list once, beside the document (#743)

Adjacent, not governing: [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
explains why the text comes from core while the announce policy does not, but decides nothing about
the announce model itself; the rest of this note's rules (cadence, gating, echo dedup, reset on
reactivate) are recorded by no ADR.

## Design model

Status: beyond the two records above, every rule comes from the source and the issues cited.

- **The controllers are pure; the DOM is injected.** `AccessibilityController`,
  `CommandAnnounceController`, `CommandNavController`, `AccessibleViewController` and
  `a11ySelectionToPort` take sinks and ports and decide *what* AT should see; the DOM glue
  (`Accessibility`, `DomAccessibleView`) is thin and proven in the demo, not in vitest (`node` env).
- **Two surfaces, deliberately different.** A row tree (`role="list"`, one `listitem` per viewport row,
  with `aria-posinset`/`aria-setsize` against the whole buffer) for spatial review, and an on-demand
  **accessible view** of the whole buffer — the VSCode "Accessible Buffer" analog — built from core's
  `Term::accessible_text` over `AccessiblePort`. The view exists because the live region is a firehose:
  an announce longer than `MAX_ROWS_TO_READ` (20) lines collapses to `TOO_MUCH_OUTPUT`, and a user who
  wants to *read* needs the escape hatch. (The old note put the 20-line cap on the row tree; it is on
  the announce.)
- **New output is a row-text diff, not damage.** `announceNewOutput` shifts the previous rows by the
  frame's scroll op, announces each changed row's changed *suffix*, skips the first frame (baseline),
  skips Full frames (a repaint, not output), and skips the alternate screen — a full-screen TUI
  repainting is not new content, which is why `alt_screen` rides the frame header at all (#149).
- **The announce cadence: 200 ms coalescing debounce, capped at 1 s, with a leading edge** (#153,
  #215) — a deliberate divergence from xterm's one-per-second throttle (`TimeBasedDebouncer`). Between
  200 ms and 1 s gaps it may announce more often than xterm (a responsiveness trade); the cap bounds a
  sub-debounce flood (`yes`, a long build) that would otherwise re-arm forever; the leading edge speaks
  the first output after ≥1 s idle at once, as xterm's synchronous leading refresh does. Only a real
  announce resets the idle clock (`flush` sets `lastFlushAt` below its empty guard).
- **Gated markup needs a repair on the off→on edge; markup with no repair must not be gated.** The
  screen-reader-active check (#161, `ScreenReaderState`, default *active* because a browser cannot
  detect a screen reader) gates the row tree's `setRow` writes too (#169), not only announces. That is
  safe because the bookkeeping keeps running while the gate is closed and `reactivate()` ends with
  `syncTree()`, re-writing every row from the cached frame — so `reactivate()` is **not** an
  announce-only reset, and reading it as one is how the next gated field arrives without its repair
  (the #215 shape, one axis over). The demo's search box `aria-invalid` (#448) is ungated for the other
  half of the rule: nothing re-writes it on reactivation, so a gated write would stay stale.
- **While inactive, the whole diff is skipped, not only the flush** (#183) — the gated sink alone would
  still spend the CPU. Echo dedup enqueues keystrokes only while active, because its drain runs only
  inside the gated diff; an ungated push would grow unbounded and swallow the first output after
  reactivation. xterm's disposed manager registers no key listener at all.
- **Re-activation emulates a fresh manager, and that is a convention.** `reactivate()` cancels the
  pending announce, clears `lastFlushAt` (#215) and the echo queue (#183), then re-syncs the tree. A new
  debounce or idle field not reset there leaks across activations — it already happened once, with the
  flush timestamp.
- **Typed-echo dedup** (`onKey` → `dedupTyped`, xterm's `_charsToConsume`): a typed code point is
  queued and dropped when the shell echoes it. Per code point (#153 G9), because an IME commit or paste
  arrives as one multi-unit intent while the drain consumes one code point per echoed char. The queue
  drains at the output rate, so a never-echoed key (`read -s`) cannot linger past later output. The
  host forwards keys; the demo does so for `char` keys and committed text.
- **Command outcomes are announced, not just navigable** (#160, #167). When an OSC 133
  `CommandFinished` mark first becomes *visible* in a frame (`markerPositions`, the viewport marker
  group), `CommandAnnounceController` speaks the outcome and fires a signal, as VSCode does on every
  command finish. The first frame only seeds `seen`; Full frames are not skipped, so a command that
  finished while the user was scrolled up is announced when it scrolls into view. A mark is marked seen
  **before** any enable check, so a suppressed command never replays when a modality is enabled later.
  `seen` is pruned by `onMarkerDisposed`, which also lets a RIS (ids reissued from 0) announce again.
- **`auto` is resolved in the controller, not by wrapping the sink**, as VSCode resolves it inside
  `playSignal` — a blanket sink gate cannot express `on`. Two divergences from VSCode: the announce
  modality admits `on` (VSCode allows `auto | off`), so a host piping the region elsewhere can opt in;
  and the default wording carries the exit code (`VERBOSE_ANNOUNCE_TEXT`, #167 F2 — a non-sighted user
  has no red decoration to read it from). `TERSE_ANNOUNCE_TEXT` is VSCode parity.
- **Command navigation samples its list once, with the document** (#743, ADR-0029 D3).
  `CommandNavController.load` is the single sampling point and must be called when the view is summoned;
  `jump` before `load` does nothing, and a jump whose `NavView.reveal` returns `false` announces nothing.
  `CommandLine::line` is a *document* line, valid only against the `accessible_text` sampled beside it.
- **The AT selection bridges to the engine selection.** `Accessibility` listens for `selectionchange`,
  resolves each endpoint to a row/offset or to the out-of-tree side it sits on, and `a11ySelectionToPort`
  maps offsets to grid `(row, col, side)` through `CellMirror.rowCells` and drives the same
  `SelectionPort` the mouse uses — xterm's `_handleSelectionChange`. An out-of-tree endpoint clamps to
  the tree's edge (#217); a selection wholly on one side is ignored; a collapsed caret inside the tree
  clears.
- **The hidden input textarea is a labelled accessible input, not `aria-hidden`** (#248,
  `makeHiddenTextarea` in `terminal.ts`). It is programmatically focused to type, and focusing an
  `aria-hidden` element violates WCAG 4.1.2 — a screen reader lands on it and announces "blank". The row
  tree stays the separate review surface, with no `aria-owns` between the two, as xterm.js's helper
  textarea and accessibility rows coexist.
- **Outside input and its bounds.** Frames are bounded by the viewport; the announce batch is flushed
  within 1 s and collapsed above 20 lines; `seen` is bounded by live markers (core's `MAX_MARKERS`)
  provided the host forwards disposals. Two inputs have no bound here: the echo queue grows with every
  key typed while active and no output arrives, and the accessible view renders one `div` per document
  line of the whole buffer, so its size is the scrollback limit's.

## Code

- `justerm-web/src/accessibility.ts` — `AccessibilityController` (`onFrame`, `reactivate`, `syncTree`,
  `onKey`, `onBoundaryFocus`), `A11yTreeSink`, `LiveRegionSink`, `TOO_MUCH_OUTPUT`
- `justerm-web/src/accessibility-dom.ts` — `Accessibility` (`setScreenReaderActive`, `dispose`), the
  DOM sinks, and the selection-change glue (`bridgeSelection`)
- `justerm-web/src/screen-reader.ts` — `ScreenReaderState` (`gateLive`)
- `justerm-web/src/accessible-view.ts` — `AccessibleViewController`, `AccessiblePort`,
  `DomAccessibleView`
- `justerm-web/src/command-announce.ts` — `CommandAnnounceController`, `Enablement`,
  `DEFAULT_ANNOUNCE_POLICY`, `VERBOSE_ANNOUNCE_TEXT`, `TERSE_ANNOUNCE_TEXT`
- `justerm-web/src/command-nav.ts` — `CommandNavController`, `CommandNavPort`, `NavView`
- `justerm-web/src/a11y-selection.ts` — `a11ySelectionToPort`, `TreeSelection`
- `justerm-web/src/cell-mirror.ts` — `CellMirror` (`rowText`, `rowCells`), ADR-0011; also read by
  [hyperlinks](hyperlinks.md)' `LinkTracker`
- `justerm-web/src/terminal.ts` — `makeHiddenTextarea`
- `justerm-core/src/term/selection.rs` — `Term::accessible_text`; `justerm-core/src/lib.rs` —
  `Engine::accessible_text`, `Engine::command_lines`

## Reference behaviour

- [Relating a coordinate to the document it names](../../agents/reference-facts.md#relating-a-coordinate-to-the-document-it-names--how-the-references-make-which-buffer-unaskable-743-verified-2026-08-07)
  — both references make "which buffer" unaskable rather than answering it, and neither shape imports
  into frame mode; this is why `CommandLine::line` is re-asked rather than rebased
- [Trimming a line's end](../../agents/reference-facts.md#trimming-a-lines-end-685-verified-2026-08-03)
  — the predicate both of this territory's text extractors follow (core's and `CellMirror`'s)

The announce model itself is compared only in prose: xterm.js's `AccessibilityManager` and VSCode's
terminal a11y (`decorationAddon.ts`, `accessibilitySignalService.ts`) are cited by symbol in the
source, unpinned, and one of them is an editor. One pinned fact is cited by #993 rather than stored:
xterm.js labels its helper textarea (`src/browser/CoreBrowserTerminal.ts:488-489` @ `699f553`) and
sets no `aria-owns` under `src/browser`.

## Cross-cutting invariants

- [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  — `CommandLine::line` is a document line, the one coordinate **no published scalar rebases**: soft
  wraps collapse, so it moves with the absolute delta except when a continuation row is evicted, and the
  receiver cannot tell the cases apart (measured for #743: abs 17 → 15 while doc went 16 → 15). Settled
  as a re-ask, so this territory's pairing is the contract. The alt-screen half is this territory's
  alone: `accessible_text` floors to the *active* buffer while the line indexes the *primary* document,
  and the reachable shape **resolves rather than fails** — with a TUI filling the alt screen the held
  index names a TUI row (measured, document line 1: `"$ lsout"` on the primary, `"TUI row 1"` on the
  alt), so the reading cursor moves onto unrelated content and the command is announced beside it
- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `Term::accessible_text`
  walks `[scrollback ++ grid]` by absolute index and starts at `abs_floor()`; a missing floor is silent,
  AT reading plausible text that is not on screen
- [only U+0020 can be a row's padding](../invariant/only-u0020-can-be-padding.md) — this territory
  holds both ends: core's `accessible_text` (via `extract_lines`) and `CellMirror`'s row text. The web
  one had the rule right years before core did, and nothing compared them (#153 vs #685)
- [a pointer coordinate is bounded by the converter that produces it](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — `a11y-selection.ts` converts DOM text offsets rather than pixels; the obligation is the same and
  discharged (an out-of-tree endpoint resolves to the tree's edge, #667)
- [a decoded frame's columns are getters](../invariant/decoded-columns-are-getters.md) — the cell
  mirror walks every damaged cell, the reader where a per-cell getter costs most; it is where the
  invariant was found (#657)
- [an IME composition is browser-owned state the engine never sees](../invariant/composition-is-browser-owned-state.md)
  — a committed composition enters echo dedup per code point (#153 G9), and the textarea is a real AT
  focus target (#248), which makes *"does an AT or magnifier read the position we anchor it at?"* an
  a11y question as well as an IME one (#640 Q4)
- [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md)
  — `Accessibility.dispose` is the host's to call and no host calls it (that note's § A teardown the host must remember)

## Blast radius

- [logical lines](logical-lines.md) — the accessible view is wrap-joined logical lines; a change to
  joining or trimming changes the document a reader walks and the lines `command_lines` indexes into
- [marker](marker.md) — the announce reads the viewport marker group's kind and exit, and assumes ids
  are pruned through disposal; a marker id reused without a disposal event silences a command
- [selection](selection.md) — the bridge drives the same `SelectionPort` as the mouse and assumes a
  `char` selection at a cell side; a change to sides or snapping changes what an AT selection selects
- [frame](frame.md) — the announce assumes `kind === 0` means a repaint and `altScreen` means a TUI; a
  frame kind that carries output as Full would go unannounced
- [alt-screen](alt-screen.md) — `accessible_text` answers about the active buffer while command lines
  index the primary one; a change to what the alt screen exposes changes which buffer a reader is in
- [input encoding](input-encoding.md) — the hidden textarea is both the input target and an AT focus
  target, and echo dedup assumes keys are forwarded per intent
- [viewport](viewport.md) — the row tree follows the visible window and computes `aria-posinset` from
  `scrollbackLen − displayOffset`; the accessible view deliberately does not follow it
- [search](search.md) — the search box is the one consumer-owned control this territory reaches into
  (#439's live count, #448's rejected-pattern state); since #448 the published `isInvalidRegex` doc
  carries an AT duty
- [widget lifecycle](widget-lifecycle.md) — `Accessibility.dispose` (timer, `selectionchange`) is
  host-ended ambient work the widget never sees

## Known holes / open

- **The one consumer ships no screen-reader support.** PenTerm's Native panes wire none of this
  territory (penterm 7dadc3c95 `nativeTerminalCache.ts` — `build`); the row tree, announces and the
  accessible view have run only in the demo and have never met a real screen reader in a host.
- **Command navigation on the alt screen still lands on the wrong buffer.** `CommandNavController.load`
  does not check the screen, and `DomAccessibleView.reveal` is a bounds check, so an in-range primary
  line resolves onto alt-screen content. Core's `Engine::command_lines` doc says a bounds check does not
  save a caller here; the web docs on `NavView.reveal` and `DomAccessibleView.reveal` claim it does.
- **Zero records** for the announce model (cadence, gating, dedup, reset-on-reactivate); the
  reset-on-reactivate obligation is enforced by nothing and has been missed once.
- **Two unpinned references, one not a terminal.** The behaviours borrowed from VSCode are product
  decisions dressed as prior art, and nothing records why an editor's terminal is the right model.
- **A rejected search pattern is exposed as state and never spoken** (#448): the input carries
  `aria-invalid` and points at the reason, but nothing announces it — #439's decision. Whether that is
  enough is answerable only by a real screen reader, and no one has run one.
- **The echo queue has no bound** while active and output-less (see `## Design model`).
