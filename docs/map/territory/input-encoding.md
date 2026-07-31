# Territory — input encoding

## What it is

The inverse of `feed`: a key, mouse, paste or focus event becomes the byte sequence a TUI expects on
its stdin. What those bytes are depends on **DEC modes the engine learned from the *output* stream** —
so the encoder cannot live where the events are.

It spans two crates by necessity, and the split is exact: the **web widget normalises DOM events into
intents**, and **core turns intents into bytes**. Neither half can do the other's job — the browser
has the event, the engine has the modes.

## Governing decisions

- [ADR-0016 — mouse mode as a wanted-events mask in the frame](../../adr/0016-mouse-mode-wanted-events-mask-in-frame.md)
  — how the consumer learns *which* events the application wants, so it can route rather than guess
  (wire v7→8). It decides the routing signal, not the encoding
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  the modes are engine state, so the encoding is engine work; the DOM event is the consumer's

Nothing governs the encoding itself.

## Design model

- **The engine owns the modes; the encoders are pure.** `event + modes → bytes`, so the consumer's
  I/O stays its own concern and the functions are testable without a PTY.
- **Modes are hidden state learned from output.** DECCKM, mouse tracking and encoding, focus
  reporting, bracketed paste — an application turns them on by *printing*, and the same keystroke
  therefore encodes differently depending on what was printed earlier. This is the sharpest instance
  of `architecture.md`'s "input encoding is mode-gated" entry.
- **This is the legacy xterm baseline** — the common 90% every TUI speaks. The kitty keyboard
  protocol (`CSI u` plus a negotiated progressive-flag stack) is a **stateful superset**, deliberately
  deferred, and it rewrites only what legacy cannot express.
- **The web half normalises, it does not encode.** Its intent types mirror `input.rs` as a contract;
  the protocol bytes are the backend's job. A consumer that encoded in the browser would have to
  replicate the mode tracking, which it cannot see.
- **The input target is a hidden textarea, not the canvas** — a canvas cannot receive IME events at
  all. Focus restoration must go through the widget's own `focus()`; focusing the canvas kills typing
  and IME together. So `element` does **not** have to be focusable and the widget never makes it so
  (#649) — but a consumer that makes it focusable must `preventDefault` the pointer-down, because the
  browser's focusing steps run *after* the widget's handler and would blur the textarea it just
  focused. xterm.js has the same pairing (`preventDefault()` then focus).
- **A composition freezes the anchor for every writer, forced or not** (#637 for the frame stream,
  #649 for the point-of-use re-sync). The predicate is "a candidate window is open" — `isComposing`,
  not the broader `active`, which outlives it by one deferred read and so would swallow the
  `compositionstart` re-sync in continuous CJK.
- **An IME confirmation is a raw text intent**, not a paste — bracketed-paste markers would tell the
  application something untrue about where the text came from.

## Code

- `justerm-core/src/input.rs` — the mode flags and the encoders' shared types
- `justerm-core/src/term.rs` — `Term::encode_key`, `encode_mouse`, `encode_paste`, `encode_focus`,
  and the mode flags they read (`bracketed_paste`, and the DEC modes tracked from output)
- `justerm-web/src/input.ts` — DOM events → intent objects; the intent types mirror the backend
  contract
- `justerm-web/src/composition.ts` — IME composition, including the backspace-during-composition case
  reported as one delete

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. The encoders are described as the legacy xterm
baseline, and the IME delete case cites xterm's `C0.DEL` in a comment — an implementation claim about
a named reference with no pinned row, in the area where a wrong byte is invisible until an
application misbehaves.

## Cross-cutting invariants

- [the cell size is derived state](../invariant/cell-size-is-derived-state.md)
  — `CellGeometry` is a cell divisor with a lifetime and a unit, and nothing type-checks either
  (#578)
- [an IME composition is browser-owned state the engine never sees](../invariant/composition-is-browser-owned-state.md)
  — **this territory owns the mechanism the invariant is about**: `composition.ts`, the hidden textarea
  as the real input target, and the decision that a confirmation is a raw `text` intent. The fact has
  been true since #116 and was recorded nowhere, so the behaviours downstream of it (#592 the caret,
  #631 the anchor) each derived it locally. The consequence that reaches other territories is the one
  worth carrying out of here: a composition has **no frame to key on**, and the frame stream keeps
  describing a cursor that knows nothing about it

## Blast radius

- [frame](frame.md) — `mouse_events` is a header scalar (ADR-0016), and it exists so the consumer can
  route an event to the application or keep it local
- [events & replies](events-and-replies.md) — replies travel the same direction and share the
  consumer's PTY write path, but are generated by parsing rather than by a user action
- [accessibility](accessibility.md) — the hidden textarea is both the input target and
  part of the accessibility surface, so a change to focus handling reaches both
- [selection](selection.md) — mouse intents drive selection when the application has *not* asked for
  mouse events; the wanted-events mask is what decides which

## Known holes / open

- **Zero governing records for the encoding**, in a territory where being wrong produces a
  misbehaving application rather than an error.
- **The kitty keyboard protocol is deferred, not decided.** `architecture.md` describes it as a
  negotiated flag stack that rewrites only what legacy cannot express — a design sketch with no
  record and no issue-level commitment.
- **Two mode sets have to agree across a crate boundary.** The web mirrors `input.rs`'s intent types
  by hand, the same ungated mirroring `types.ts` does for the frame.
- **In-progress IME composition is not rendered inline in the grid**, so what the user is typing is
  invisible until confirmation. Tracked: #249 — now one member of spine **#640**, which holds the
  question both it and #637 need answered.
- **The IME anchor's *other* readers are only partly known.** Measured (#649): the browser's focus
  steps are a real second reader — focusing the textarea scrolls the nearest scrollable ancestor, and
  the destination tracks the anchor 1:1, so a stale anchor scrolls the page proportionally wrong.
  Whether an AT tool or magnifier is a *third* reader is still unmeasured, and it is the open question
  on spine #640 that decides whether the focus-time re-sync can be dropped for xterm's
  `focus({ preventScroll: true })`.
