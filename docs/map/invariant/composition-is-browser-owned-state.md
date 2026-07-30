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

Three territories already hold behaviour derived from this, each having derived it locally, and none of
them can see the others:

- [input encoding](../territory/input-encoding.md) — owns the mechanism: `composition.ts`, the hidden
  textarea as the real input target (a canvas cannot receive composition events), and the decision that
  a confirmation is a raw `text` intent rather than a paste
- [caret drawing](../territory/caret-drawing.md) — the caret stops blinking while composing (#592),
  driven by a `setComposing` notification off a browser event. **That note does not mention composition
  anywhere**, which is the evidence this layer is needed rather than an argument for it
- [widget lifecycle](../territory/widget-lifecycle.md) — the hidden textarea's anchor is re-read at
  composition start, because that is the moment the OS reads it (#631)

The criterion the map uses is *a fact that holds in N territories is invisible from the other N-1 if it
is not written here*. This one is already at three, and the caret territory's silence is the failure in
progress: the same shape as an absolute-index walk being rediscovered three times before
[its floor](alt-screen-buffer-floor.md) got a home.

## What a violation looks like

**Always silent, and never in the layer that would notice.** The engine cannot be wrong about a
composition it never saw, so nothing throws and no test on the core side can fail.

- **A frame-driven surface acting on a superseded cursor.** `Terminal.positionTextarea` runs from the
  frame subscription with no composition gate, so an output frame moves the IME anchor out from under
  the candidate window while the user is mid-composition (#637). The engine is behaving correctly; the
  consumer is answering a question the frame cannot answer.
- **A rule inferred from engine state where no engine state exists.** A composition has no
  representation to consult, so *"what should happen during one"* can only be decided, never derived
  from a frame. Writing such a rule as if it followed from the frame is how it ends up decided once per
  site.
- **A reference read as authoritative where it contradicts itself.** xterm.js is the only reference with
  the same architecture here, and on the open question — may the anchor move mid-composition — its
  `_syncTextArea` bails while composing while `CompositionHelper.updateCompositionElements` rewrites the
  textarea's position on every render *while composing*. Both cannot be load-bearing, so an appeal to
  "the reference does X" is not an answer in this area.

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
  cannot be answered from any of the three sites above, because each recorded only its own case.
