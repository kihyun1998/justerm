# Invariant — an IME composition is browser-owned state the engine never sees

## The fact

`justerm-core` learns about a composition only when it ends: the committed text arrives as a raw `text`
intent (#116), and the in-progress preedit — content, length, its own caret — never reaches the engine.
That is not a gap: there is no VT sequence for a preedit and no frame field carrying one, and the
boundary in [CLAUDE.md](../../../CLAUDE.md) puts the browser on the consumer's side.

Two consequences, the second the one that gets missed:

1. Every behaviour that reacts to a composition is driven by browser events alone —
   `compositionstart` / `compositionupdate` / `compositionend` on the hidden textarea. There is no frame
   field, engine state or `DecodedFrame` field to consult.
2. The frame stream keeps arriving during a composition and describes a cursor that knows nothing of
   it, so a frame-driven surface is reacting to a superseded description of where the user types.

The rules built on this fact are [ADR-0028](../../adr/0028-composition-surfaces-have-one-writer-each.md)
D1–D5 (spine #640 promoted to it and closed, 2026-08-04); new questions here are conformance items
against that record. This note holds the fact.

**Not part of the fact:** that the browser owns the state does not settle what the terminal shows.
#592 considered ghostty's rule — a preedit outranks DECTCEM, so a hidden cursor shows as a solid block
while composing — and rejected it, because revealing a DECTCEM-hidden caret would invert
`cursorCommand`'s contract for a rare case. A maintainer product decision, not a derivation.

## Why it is cross-cutting

Several behaviours with no shared call path each derived this fact locally: the caret's blink policy
(`cursor.ts`), the anchor's placement (`terminal.ts`), the committed text's shape (`composition.ts`),
the a11y echo's granularity (`accessibility.ts`), the suggestion's visibility (`suggestion.ts`). They
arrived issue by issue and share only that a composition has no representation on this side of the
boundary. Caret drawing said "(none identified yet)" until 2026-07-30 while #592 had already shipped
such a behaviour into it, and accessibility was silent the same way; each was found by writing this
note.

## Territories it holds in

Symbols re-verified at HEAD 7bb31f7.

- [input encoding](../territory/input-encoding.md) — owns the mechanism: `justerm-web/src/composition.ts`
  (`CompositionController`), the hidden textarea as the input target (a canvas cannot receive
  composition events), and a confirmation as a raw `text` intent rather than a paste. The consumer key
  veto (`TerminalOptions.beforeKey`, #901) is asked only after the composition gate, so a claim cannot
  take a key the composition owns.
- [caret drawing](../territory/caret-drawing.md) — the caret stops blinking while composing (#592),
  driven by a `setComposing` notification off a browser event.
- [widget lifecycle](../territory/widget-lifecycle.md) — the anchor is re-read where something reads
  it (composition start #631, at the latched origin since #917; focus, at the retained cursor cell) and
  frozen for every writer while composing (#637 frame stream, #649 forced re-sync), while the retained
  cell is kept on every frame in `Terminal.track` (#921). `terminal.ts — Terminal.focus`,
  `syncTextareaAnchor`, `positionTextarea`, `track`, `showPreedit`, `paintPreedit`, `repaintPreedit`;
  `composition.ts — textareaMove`, `viewportCell`.
- [accessibility](../territory/accessibility.md) — `AccessibilityController.onKey` pushes a committed
  IME intent per code point, because one commit arrives as one multi-unit intent while `dedupTyped`
  drains one code point per echoed char (#153 G9). The textarea is a labelled accessible input, not
  `aria-hidden` (#248), which puts the anchor in the accessibility tree (#640 Q4).
- [cell compositing](../territory/cell-compositing.md) — the consumer's suggestion (#972) is hidden from
  `compositionstart` to `compositionend`; the renderer alone could hide it only while a preedit run is
  drawn (missing the start before the first update, and an update to `""`), so the widget gates it on
  `composing` (`suggestion.ts — suggestionCell`) and the renderer's check is the second line.
- [viewport](../territory/viewport.md) — a preedit is the one thing drawn from a coordinate the engine
  gave for something else (the cursor's **grid** row), so the grid → viewport mapping
  (`viewportCell`, using the frame's `displayOffset`) is where a composition meets scrolling (#921,
  #917). Added here; this territory already linked back.

## What a violation looks like

Always silent, never in the layer that would notice: the engine cannot be wrong about a composition it
never saw.

- **A frame-driven surface acting on a superseded cursor (#637).** `positionTextarea` ran from the frame
  subscription with no gate, so output moved the IME anchor out from under the candidate window.
  Measured with the Windows Korean IME: the Hanja window followed the anchor down the screen.
- **Gating one writer and calling the state closed (#649).** #637's guard covered only the unforced
  path; any caller overriding the cache (`element` mousedown → `Terminal.focus()`, public, so also a
  consumer restoring focus after a dialog) delivered the superseded cell. Both entrances are closed at
  their shared seam, `textareaMove`, where `composing` suppresses every path. `force` now means only
  "override the coordinate cache"; its old exemption for #631's re-sync was never needed (`onStart`
  re-anchors before telling the controller). A suppressed move answers `undefined`, a decided no-op that
  leaves the cache alone, so the anchor catches up on the first frame after the composition. A guard on
  one writer is not a rule about the state.
- **Acting on the widget's own unsent state (#911).** At `compositionstart` in continuous CJK the
  previous commit has not left the widget (it goes out one deferred read later, #116), so `cursorAnchor`
  is known to describe the cell that text will take; latching the origin from it drew every Korean
  syllable over the previous one. The fix takes the previous composition's run end while a commit is in
  flight. A consumer holding unsent input cannot read its own echo for a coordinate.
- **A latch turning any source staleness into a composition-long wrong (#921).** The retained cursor
  cell was gated on `cursorVisible`, a drawing bit core sets as `cursor.visible && display_offset == 0`
  (#48), so it froze while scrolled up; D4 latches once, so a one-instant staleness became a preedit at
  the wrong cell for the whole composition. Every writer of the anchor is a composition surface,
  whatever it is about — the fourth latch failure (#631 geometry, #637 frame stream, #911 unsent input,
  #921 frozen source), the first with no composition in its causal chain. Measured in a browser: cursor
  moved four cells during a scroll excursion, caret landed where the view left. Retention is now
  unconditional in `Terminal.track`, the DOM write is not, and `positionTextarea` builds its cell from
  the frame, so it has no ordering condition against `track`.
- **Re-deriving the caret column per frame** (`JustermRenderer.setPreedit`, `preeditCaret`). ADR-0028 D5
  is a rule about every frame: without the retained column the caret snaps back under the run on the
  next output frame. It re-pushes at the current blink phase: re-pushing the loop's last value landed a
  cleared cursor for the whole composition when it began on an off phase (caught by #592's probe).
- **A grid coordinate where a viewport one is required (#921).** `setPreedit`'s `row` is a viewport row;
  passing the grid row drew the run on row 33 while row 35 showed its cell, with the view up 2. At
  offset `d` grid row `r` is on screen only while `r < rows - d`, at row `r + d`. The mapping uses the
  frame's offset, not the requested one (#913 advances that to 0 when an IME-swallowed key asks for the
  bottom while the screen still shows the scrolled frame). Off the bottom the run is cleared and
  re-asserted by `repaintPreedit` on the next frame that can place it. Neither reference suppresses
  permanently: xterm.js skips positioning while `!isCursorInViewport` and re-runs
  `updateCompositionElements` every render; alacritty draws the preedit only while its line is on
  screen ([reference facts](../../agents/reference-facts.md#drawing-the-preedit-two-draw-into-the-grid-one-draws-a-dom-box-249-verified-2026-08-03)).
  The run's end stays a grid row and is recorded only when drawn, so a composition that never comes on
  screen hands on no end (ADR-0028 D4, by a second route needing a consumer that did not wire
  `onScroll`).
- **Two surfaces answering "where is this composition" from two sources (#917).** The `compositionstart`
  re-sync wrote the anchor from `cursorAnchor` before latching the origin, so until the first update the
  anchor named a different cell from the run. Measured with `__preeditViewportProbe` (cursor grid row 33
  of 37, view up 2): anchor 33 after `compositionstart`, 35 after the first update. Now the origin is
  latched first and written through `viewportCell`, withheld off the bottom; `focus()` maps the retained
  cursor the same way. Whether a Japanese / pinyin IME reads the anchor before the first update is
  unmeasured, and could not change the fix's direction. The cache key is the viewport cell written:
  with a grid key a scrolled `focus()` left the anchor `d` rows low on return (pinned by
  `focusReturned`; a grid-keyed mutation reddens it).
- **Reading a two-question predicate as one.** The guard keys on `CompositionController.composing`
  (`isComposing`), not `active` (`isComposing || isSendingComposition`); `active` outlives the candidate
  window by one deferred commit read, exactly where a continuous-CJK `compositionstart` lands. Swapping
  them left all 398 unit tests green at #649, because the wiring needs a DOM; only the e2e control
  discriminates.
- **A reference read as authoritative before its contradiction is resolved.** xterm.js's
  `_syncTextArea` bails while composing, yet `CompositionHelper.updateCompositionElements` rewrites the
  position while composing. #637 resolved it: the OS re-reads the anchor, so whoever writes it owns the
  candidate window — xterm suppresses the involuntary writer and keeps the voluntary one.
- **One reference generalised into a rule.** The shared rule of all three references is "the anchor
  tracks the composition, never where output moved the cursor"; ghostty and alacritty re-aim with no
  gate. justerm-web froze because the preedit had no representation. Since #249 (ADR-0028, 2026-08-03)
  it re-aims too: the preedit writer (`showPreedit`) knows where the composition is and never consults
  the guard, which stays a rule about the involuntary writers. It leaves `textareaCell` alone and never
  re-reads the origin from the frame stream (a per-update re-read jumped the anchor from row 5 to row 9
  under unsolicited output).
- **A measurement destroyed by observing it.** Screen-capturing a live composition moves focus, which
  commits it; #637 once concluded "Hanja conversion happens after the composition ends" from such a
  capture before the maintainer corrected it. Watch, do not screenshot.

## Discovery history

Every behaviour derived the fact locally, and none recorded it.

- **#116** — the mechanism: hidden textarea, composition gate on keys, committed text as raw intent.
- **#592** — first behaviour to need it (caret stops blinking); the fact lived as a call-site comment
  until decanted into this note (#993).
- **#631** — the anchor must be right when the OS reads it at composition start; found by a pass about
  the cell, not compositions.
- **#637** — whether a frame may move the anchor mid-composition was unanswerable from the earlier
  sites; settled by measuring a real IME. The demo could not even produce the precondition (constant
  cursor, cursor-keyed cache), the same property that lets it reproduce #631.
- **#649, #911, #917, #921, #972** — the instances above.

The instance roster lived on spine #640 until ADR-0028; it is kept out of this note on #552's measured
result ([theflow.md](../../agents/theflow.md): a hand-copied roster in ADR-0025 went stale in five
places in three days), the same pairing as [cell size](cell-size-is-derived-state.md) with spine #630.

## Where it will recur

Test: if a behaviour's rule contains "while composing" and its only possible input is a browser event,
it is subject to this invariant.

- **A new predicate on composition state.** `active` and `composing` read identically at a call site,
  and the wiring is invisible to `pnpm test` (`environment: "node"`); a new consumer of composition
  state owes an e2e control, not a unit test (see
  [an awaited in-page promise needs an anchor](an-awaited-in-page-promise-needs-an-anchor.md)).
- **A second reader of the anchor's position.** The OS IME was assumed the only one until #649 measured
  the browser's focus steps; an AT tool or magnifier is an unmeasured candidate for a third.
- **Any new writer of the anchor or of something drawn at the cursor** — it is a composition surface,
  and it must take its cell from the latched origin through `viewportCell`, not from the frame stream.
- #249's predicted inversion did not happen: the preedit still reaches no frame or wire (consumer →
  renderer directly), so the renderer holds one piece of state with no engine counterpart.
