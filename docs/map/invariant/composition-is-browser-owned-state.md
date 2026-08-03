# Cross-cutting invariant — an IME composition is browser-owned state the engine never sees

## The fact

**`justerm-core` learns about a composition only when it ends.** The committed text arrives as a raw
`text` intent (#116); the in-progress preedit — its content, its length, its own caret — never reaches
the engine at all. That is not a gap: there is no VT sequence for a preedit and no frame field carrying
one, and the boundary in [CLAUDE.md](../../../CLAUDE.md) puts the browser on the consumer's side of the
line by construction.

Two consequences follow, and the second is the one that gets missed:

1. **Every behaviour that reacts to a composition is driven by browser events alone** —
   `compositionstart` / `compositionupdate` / `compositionend` on the hidden textarea. There is no
   frame to key on, no engine state to read back, and no `DecodedFrame` field to consult.
2. **The frame stream keeps arriving while a composition is open, and it describes a cursor that knows
   nothing about the composition.** So a surface that reacts to frames is, during a composition,
   reacting to a *superseded* description of where the user is typing.

## Why it is cross-cutting

**Four behaviours, no shared call path, and every one of them derived this fact locally.** The caret's
blink policy, the anchor's placement, the shape of the committed text and the granularity of the a11y
echo have nothing to do with each other — they arrived issue by issue, out of different features — and
they share only that a composition is a browser fact with no representation on this side of the
boundary.

The criterion the map uses is *a fact that holds in N territories is invisible from the other N-1 if it
is not written here*. This one is at four, and the count is doing work: **[caret
drawing](../territory/caret-drawing.md) said "(none identified yet)" until 2026-07-30 — while #592 had
already shipped exactly such a behaviour into it** — and [accessibility](../territory/accessibility.md)
was silent in the same way until this note reached four. Each was found by writing this note, not by
reading that one. The same shape as an absolute-index walk being rediscovered three times before
[its floor](alt-screen-buffer-floor.md) got a home.

## Territories it holds in

- [input encoding](../territory/input-encoding.md) — **owns the mechanism**: `composition.ts`, the
  hidden textarea as the real input target (a canvas cannot receive composition events), and the
  decision that a confirmation is a raw `text` intent rather than a paste
- [caret drawing](../territory/caret-drawing.md) — the caret stops blinking while composing (#592),
  driven by a `setComposing` notification off a browser event, because there is no frame field to key
  on. The rejected half is recorded below under *what is not part of this fact*
- [widget lifecycle](../territory/widget-lifecycle.md) — the anchor is re-read at the moments something
  reads it (composition start #631, focus), and frozen for **every** writer while a composition is open
  (#637 the frame stream, #649 the forced re-sync). `Terminal.focus`, `syncTextareaAnchor`,
  `positionTextarea`, `textareaMove` in `justerm-web/src/terminal.ts`
- [accessibility](../territory/accessibility.md) — `AccessibilityController.onKey` must push a
  committed IME text intent **per code point**, because one commit arrives as a single multi-unit
  intent while `dedupTyped` drains one code point per echoed output char (#153 G9). And the textarea is
  a *labelled accessible input* rather than `aria-hidden` (#248), which is what puts the anchor's
  position inside the accessibility tree — the reason *"does an AT tool read the anchor?"* is a live
  question rather than an idle one (#640 Q4)

## What a violation looks like

**Always silent, and never in the layer that would notice.** The engine cannot be wrong about a
composition it never saw, so nothing throws and no test on the core side can fail.

- **A frame-driven surface acting on a superseded cursor.** `Terminal.positionTextarea` ran from the
  frame subscription with no composition gate, so an output frame moved the IME anchor out from under the
  candidate window while the user was mid-composition (#637). The engine was behaving correctly; the
  consumer was answering a question the frame cannot answer. **Measured, not inferred** — with the
  Windows Korean IME the Hanja candidate window followed the anchor down the screen as unsolicited
  output moved the cursor.
- **Gating the *frame* and calling the surface closed.** #637's guard covered only the unforced path,
  and the retained cursor cell went on advancing behind it — so any caller that overrode the cache
  (`element` mousedown → `Terminal.focus()`, and `focus()` is public, so also a consumer restoring
  focus after a dialog) delivered exactly the superseded cell the guard existed to withhold (#649).
  The lesson generalises past this anchor: **a guard placed on one writer is not a rule about the
  state.** Both entrances are now closed at the single seam they share.
- **Reading a two-question predicate as one question.** The guard is keyed on
  `CompositionController.composing` (`isComposing`), *not* `active` (`isComposing ||
  isSendingComposition`). `active` outlives the candidate window by one deferred commit read, and a
  continuous-CJK `compositionstart` lands inside precisely that window — so the broader predicate
  would swallow the re-sync that exists to place the candidate window, in ordinary Korean/Japanese
  typing. Measured cost of the confusion: swapping the two leaves **all 398 unit tests green**,
  because the wiring needs a DOM; only the e2e control discriminates them.
- **A rule inferred from engine state where no engine state exists.** A composition has no
  representation to consult, so *"what should happen during one"* can only be decided, never derived
  from a frame. Writing such a rule as if it followed from the frame is how it ends up decided once per
  site.
- **A reference read as authoritative before its contradiction is resolved.** xterm.js is the only
  reference with the same architecture here, and on the question *may the anchor move mid-composition*
  it looked self-contradictory: `_syncTextArea` bails while composing, yet
  `CompositionHelper.updateCompositionElements` rewrites the textarea's position on every render *while
  composing*. **#637's measurement resolved it, and the resolution is the reusable part**: because the OS
  re-reads the anchor, whoever writes the position owns the candidate window — so xterm suppresses the
  *involuntary* writer and keeps the *voluntary* one. Two writers with different intents, not two rules.
  The general lesson stands: in this area a reference disagreeing with itself means a measurement is
  owed, not that one site should be picked.
- **One reference generalised into a rule.** The shared rule across all three references is *the anchor
  tracks where the user's composition is, never where the output cursor went* — and **two of the three
  satisfy it by re-aiming mid-composition with no gate at all** (ghostty folds preedit width into the
  rect it pushes per key; alacritty picks the point from the preedit when one exists). justerm-web
  froze instead, and only because **this invariant is why it had to**: the preedit did not reach this
  side, so *"where the composition is"* had no representation beyond *"where it started"*. Reading
  xterm's suppression as the rule rather than as one codebase's expression of it is the mistake this
  bullet exists to name — it was made and corrected inside #637 itself.
  **Updated 2026-08-03 (#249, ADR-0028): justerm-web re-aims too now, and the freeze did not have to
  give.** This bullet predicted it would, since #249 supplies the missing representation. What
  actually resolved it is that *writer* and *rule* are different things: the preedit writer knows
  where the composition is, so it re-aims and never consults the guard, while the guard stays a rule
  about the **involuntary** writers — the frame stream and the focus path — which is all #637 and
  #649 ever measured. xterm has exactly this shape and it was visible the whole time
  (`updateCompositionElements` never goes through `_syncTextArea`); reading it as "suppression vs
  re-aiming" hid it.
- **A measurement destroyed by the act of observing it.** Screen-capturing a live composition moves
  focus, which commits it and clears the textarea — so the artifact under observation disappears exactly
  when it is recorded. #637 drew a wrong conclusion from such a capture once (*"Hanja conversion happens
  after the composition ends"*) before the maintainer corrected it from live observation. Anything in
  this territory that needs a real IME has to be watched, not screenshotted.

## What is **not** part of this fact

Kept because the boundary is where the mistake is available. That the browser owns the *state* does not
settle what the terminal should *show*: #592 considered ghostty's rule — a preedit outranks DECTCEM, so
an explicitly hidden cursor is shown as a solid block while composing — and **rejected** it, because
revealing a DECTCEM-hidden caret would invert `cursorCommand`'s contract for a rare case. That is a
product decision by the maintainer, not a derivation, and it is not this invariant's to overturn.

## The roster lives in the spine, not here

**Which issues are instances is tracked on spine #640**, deliberately. This note holds the fact; the
roster and the open questions are a different kind of thing and want a different home.

That split is #552's measured result, recorded in [theflow.md](../../agents/theflow.md): a hand-copied
roster inside ADR-0025 went stale in five places within three days while the rule itself needed no
edit. A roster wants a mutable home and a rule wants an immutable one, so they separate **even after
the rule exists** — the same pairing as
[the cell size is derived state](cell-size-is-derived-state.md) with spine #630.

What the spine carries and this note deliberately does not: the current instance list, and the open
question — *who owns what a composition puts on screen, and does that ownership extend to visibility?*

## Discovery history

Kept because it is about the *rule*, not about who is currently on the list: **every behaviour derived
this fact locally, and none of them recorded it.**

- **#116** established the mechanism — the hidden textarea, the composition gate on keys, and the
  committed-text-as-raw-intent decision. The fact was true from that moment and was written nowhere.
- **#592** was the first behaviour to need it: the caret must stop blinking while composing, and the
  only place that can know is the browser-event handler, because *"composition is a browser fact the
  engine never sees"* — a comment at the call site, in one branch of one file.
- **#631** needed it again from the other side: the anchor must be correct when the OS reads it, and
  the OS reads it at composition start. It was found by a completeness pass asking a question about the
  *cell*, not about compositions.
- **#637** is where the absence started costing: whether a frame may move the anchor mid-composition
  could not be answered from any of the three sites above, because each recorded only its own case. It
  was settled by measuring a real IME, and the cost of *not* having this note was paid twice over —
  the demo could not even produce the precondition (its cursor is a constant, and `positionTextarea`'s
  cache is keyed on the cursor cell, so appended output moved the anchor zero times), which is the same
  property that makes the page able to reproduce **#631**. One page, two defects, and the property that
  enables one blinds the other.

## Where it will recur

**The next surface that has to decide what happens during a composition, and reaches for a frame field
to decide it.** There is none, and there never will be — so the decision gets made locally again,
correctly, and recorded nowhere. The test: if a behaviour's rule contains the words *"while composing"*
and its only possible input is a browser event, it is subject to this invariant.

Three named places it is already queued to recur:

- ~~**#249 (draw the preedit inline).**~~ **Landed 2026-08-03 under ADR-0028.** It supplied the
  representation this fact says is absent — and the fact itself is unchanged, because the preedit
  still reaches no frame and no wire: it goes consumer → renderer directly, which is why the renderer
  now holds one piece of state with no engine counterpart. The predicted inversion did **not**
  happen; see the bullet above for what happened instead. The half that was right: the anchor gained
  a second, *voluntary* writer, and it bypasses the involuntary one exactly as xterm's does.
- **Any new predicate on composition state.** `active` and `composing` answer different questions and
  read identically at a call site. The wiring is invisible to `pnpm test` — the widget needs a DOM and
  the unit suite runs in `environment: "node"` — so a wrong predicate ships green, as measured on #649.
  A new consumer of composition state owes an e2e control, not a unit test.
- **A second reader of the anchor's position.** The OS IME was assumed to be the only one until #649
  measured the browser's own focus steps as a second, and an AT tool or magnifier remains an unmeasured
  candidate for a third. Each additional reader narrows what may be done with the anchor while frozen,
  and none of them announces itself.
