# Territory — accessibility (the screen-reader mirror)

## What it is

Making a GPU canvas readable by assistive technology. **The renderer's canvas is opaque to AT** —
there is nothing in the DOM for a screen reader to walk — so the widget drives a hidden DOM mirror
beside it and keeps that mirror in step with the frame.

The largest cluster in the web widget, and the only territory in this map whose consumer is not a
programmer.

## Governing decisions

**None.**

- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  why the *text* comes from core (whole-buffer extraction needs the buffer) while the announcing
  policy does not
- [ADR-0011 — the web viewport cell mirror](../../adr/0011-justerm-web-viewport-cell-mirror.md) —
  the `CellMirror` this reads viewport row text through

## Design model

- **The controller is pure; the DOM is injected.** No DOM, no GPU, no IPC in the logic — the consumer
  supplies sinks and the controller decides *what* AT should see. That is what makes announce
  behaviour unit-testable at all, since the alternative is asserting against a screen reader.
- **Two surfaces, deliberately different.** A row-tree mirror (`role="list"`) for the live viewport,
  and an **on-demand accessible view** for the whole buffer — the VSCode "Accessible Buffer" analog.
  The second exists because the first is a firehose: it is capped at 20 lines, and a user who wants
  to *read* rather than *monitor* needs the escape hatch.
- **Selection is bridged in both directions.** When a screen-reader user selects inside the hidden
  row tree the browser fires `selectionchange`; the glue resolves it to tree coordinates and a pure
  bridge maps offsets back to grid `(row, col, side)`. This is the frame-mode analog of xterm's
  `AccessibilityManager._handleSelectionChange`.
- **Command outcomes are announced, not just navigable.** When an OSC 133 `CommandFinished` mark
  first becomes visible, the outcome is spoken and a signal fires. VSCode does this on *every*
  command finish rather than only on navigation, and that is the behaviour followed.
- **Announcing is gated on `alt_screen`.** A full-screen application repainting is not new content to
  read, which is why that flag rides the frame header at all.
- **Re-activation must reset every announce-related piece of state.** `reactivate()` emulates a fresh
  manager; a new debounce or idle field that is not reset there leaks across activations — this has
  already happened once with a flush timestamp.

## Code

- `justerm-web/src/accessibility.ts` — `AccessibilityController`, the pure logic and its sinks
- `justerm-web/src/accessibility-dom.ts` — `Accessibility`, the DOM-backed sinks, and the
  `CellMirror` wiring
- `justerm-web/src/accessible-view.ts` — the on-demand whole-buffer view
- `justerm-web/src/a11y-selection.ts` — the AT-selection ↔ grid-selection bridge
- `justerm-web/src/command-announce.ts` — command outcome announce and signals
- `justerm-web/src/cell-mirror.ts` — viewport row text (ADR-0011)
- `justerm-core/src/term/selection.rs` — `Term::accessible_text`, the whole-buffer document the
  accessible view reads

## Reference behaviour

**None** in `docs/agents/reference-facts.md`, and this territory names **two** references in prose
that the usual comparison set does not even contain — xterm.js's `AccessibilityManager` and VSCode's
terminal a11y (`decorationAddon.ts`). Both are cited by symbol, neither is pinned, and one of them is
an editor rather than a terminal.

## Cross-cutting invariants

- [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  — `CommandLine::line` is a *document* line, so it is the one coordinate on any channel that **no
  published scalar rebases**: soft-wrapped rows collapse, so its motion under eviction equals the absolute
  delta except when a continuation row is evicted, and the receiver cannot tell the cases apart
  (measured: abs 17 → 15 while doc went 16 → 15; isolated to the single eviction responsible, abs
  12 → 11 while the document line stayed at 11). **Settled in #743 as a re-ask (ADR-0029 D3), which
  makes this territory's own pairing the contract**: the line is valid for exactly as long as the
  `accessible_text` sampled beside it, so `load()` is the single sampling point and `jump()` no longer
  takes a list of its own at a later instant and holds it for the session. The second half is this
  territory's alone — `accessible_text` floors to the *active* buffer while the line indexes the
  *primary* document, so on the alt screen the two answers are about different buffers at the same
  instant, and no basis, epoch or validity window addresses that. **The reachable shape resolves
  rather than fails**: with a TUI filling the alt screen the held index is in range and names a TUI
  row (measured, document line 1: `"$ lsout"` on the primary, `"TUI row 1"` on the alt), so the
  reveal succeeds, the reading cursor moves onto unrelated content and the command is announced
  beside it. `NavView.reveal` reports whether a line resolved, which catches the *short*-document
  half and cannot catch this one
- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — `Term::accessible_text`
  walks the concatenated `[scrollback ++ grid]` buffer by absolute index, so on the alt screen it
  must floor at `scrollback.len()`. The failure is silent: the AT reads *plausible* text that is not
  on screen
- [only U+0020 can be a row's padding](../invariant/only-u0020-can-be-padding.md) — this territory
  holds **both ends** of that note: core's `accessible_text` (via `extract_lines`) and `cell-mirror`'s
  own row-tree trim in `justerm-web`. The web one had the rule right years before core did, and
  nothing compared them (#153 vs #685)
- [a pointer coordinate is bounded by the converter that produces it](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — `a11y-selection.ts` converts DOM text offsets rather than pixels, so the arithmetic differs and
  the obligation does not: an out-of-tree endpoint resolves to the tree's edge. Already discharged;
  listed so it is not read as exempt (#667)
- [a decoded frame's columns are getters](../invariant/decoded-columns-are-getters.md) — the cell
  mirror that feeds the accessible text walks every damaged cell, which makes it the one reader
  where a per-cell getter read costs the most; it is where the invariant was found (#657)
- [an IME composition is browser-owned state the engine never sees](../invariant/composition-is-browser-owned-state.md)
  — two facts here derive from it, and neither is about the engine. `AccessibilityController.onKey`
  pushes a committed IME text intent **per code point**, because a commit arrives as one multi-unit
  intent while `dedupTyped` drains one code point per echoed output char (#153 G9) — a whole
  composition entering the echo-dedup as a single entry would mismatch and be announced twice. And the
  hidden textarea is a *labelled accessible input* rather than `aria-hidden` (#248), so it is a real
  focus target in the AT tree — which is what makes *"does an AT tool or magnifier read the position we
  anchor it at?"* an open question with a11y consequences rather than an IME-only one (#640 Q4)

## Blast radius

- [logical lines](logical-lines.md) — the mirror consumes wrap-joined text; a change to trimming or
  off-screen context changes what is announced
- [selection](selection.md) — bridged in both directions, so a coordinate change reaches AT
- [marker](marker.md) — OSC 133 marks drive the command announce
- [frame](frame.md) — `alt_screen` gates the announce policy and exists for that reason
- [input encoding](input-encoding.md) — the hidden textarea is both the input target and part of the
  accessibility surface; focus handling reaches both
- [viewport](viewport.md) — the mirror follows the visible window, and the accessible view
  deliberately does not

## Known holes / open

- **Zero governing records** for the family's entire accessibility contract.
- **Two unpinned references, one of them not a terminal.** The behaviours borrowed from VSCode are
  product decisions dressed as prior art, and nothing records why an editor's terminal is the right
  model.
- **The reset-on-reactivate obligation is a convention.** Every announce-related field must be reset
  in one place, with nothing enforcing it — and it has already been missed once.
- **The invalid-regex search state is not exposed to AT**, so a screen-reader user gets no signal
  that their search cannot run. Tracked: #448.
