# Territory — consumer events & query replies

## What it is

Two outbound channels that are **not** the frame. Events are point-in-time notifications the engine
accumulates while parsing (bell, title change, clipboard request); replies are bytes the application
asked for and expects back on the PTY (DA, DSR, DECRQM, colour queries). Both are **drained**, never
pushed.

The second territory this map missed entirely on its first pass — nothing about it is cell state, so
nothing about it appears in the frame.

## Governing decisions

**One, and it covers a property of the payload rather than the channel itself.**

- [**ADR-0029 — a coordinate carries its instant, or is re-asked**](../../adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md)
  — its **D4** is this channel's clause: an event can only *carry*, because its payload is detached
  from its instant by the queue before any consumer sees it, and what it carries is checked against
  its pull-side sibling rather than against a list of axes. That settles what a coordinate-bearing
  variant owes. It does **not** settle what earns a place on this channel at all — see the hole below
- [ADR-0020 — what qualifies for the frame snapshot](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  is the record that explains why these are *not* in the frame: its first rule is state versus event,
  and an event fails it by construction. It decides the frame's membership, not this channel's shape
- `docs/architecture.md` §"Hidden VT state" carries both contracts — *"consumer events are
  pull-drained, and OSC 8 is not one of them"* and *"query replies are an outbound channel, drained
  pull-style and kept apart from events"*

## Design model

- **A marker's birth and death are both events, and the pair is load-bearing since #490.**
  `MarkerDisposed` was enough while every live marker rode every frame — absence was observable.
  Once the index is *pulled*, a population that only ever shrinks is silently wrong, so
  `MarkerCreated { id, line, kind, evicted_total, epoch }` is the mirror. ADR-0020 R1 is why neither
  is a frame field: an appearance and a disappearance are occurrences, not state.

- **An event that carries a coordinate carries the instant it is true at, or it carries nothing
  (#737).** This entry read *"`line` is absolute on the same basis the frame header reports"* until it
  was measured false. A single `feed` can create a marker and then evict, so the frame closing that
  batch reports an origin the event's line predates; two batches with the same mark and the same three
  evictions in opposite orders were identical on **every** channel a consumer can see — the event
  line, both frame bases, the epoch, `marker_count` — and three lines apart in truth. The repair is
  the pairing `MarkerIndex` already uses: the line travels with its `evicted_total`. The general
  shape is what makes this a design-model entry rather than a bug note — a frame is a *snapshot* and
  carries its own basis by construction, while an event is a *point in time* whose payload outlives
  the instant that gave it meaning.
  **And "the instant" is not one scalar (#741).** The repair above was written as *carry the basis*,
  which is the axis the reproducing batch happened to move; markers also move **non-uniformly**, and
  a birth queued when `marker_epoch` bumps describes a buffer that no longer exists. The rule that
  needs no forcing case is stated against the sibling rather than the axis: **an occurrence carries
  every scalar the pull answering the same question carries**, here `MarkerIndex`'s full
  `(line, evicted_total, epoch)`. This channel has now had the same fact re-derived at two
  granularities in one day, which is why the invariant note below exists rather than a third bullet.

- **Pull, not push — and the alternative is named.** The engine queues during `feed` and the consumer
  takes with `drain_events`, mirroring `damage` / `frame` / `reset_damage`. No callback crosses the
  boundary, so the engine stays decoupled from the consumer's event loop. alacritty's `EventListener`
  is the push model this deliberately does not copy, because it would couple them.
- **Two channels, kept apart on purpose.** Events describe *something happened*; replies are **bytes
  the application is waiting for** and must reach the PTY, in order. Merging them would make a
  transport obligation look like a notification a consumer may ignore.
- **OSC 8 hyperlinks are deliberately absent.** A hyperlink is per-cell state — *which cells are
  links* — not a point-in-time event, so it is modelled like graphemes in its own slice. This is the
  clearest worked example of ADR-0020's state-versus-event rule, and it lives in a module comment.
- **Draining is destructive and the consumer owns the cadence.** Nothing bounds the queue if a
  consumer never drains; the engine has no timer and no back-pressure of its own.
- **Replies are raw bytes, not typed.** The engine encodes; the consumer writes them to the PTY
  without interpretation — the same "mechanism here, transport yours" split the wire format uses.

## Code

- `justerm-core/src/event.rs` — `TermEvent`, the event surface
- `justerm-core/src/term.rs` — `Term::drain_events`, `Term::drain_replies`, and the
  `Term::report_*` methods that queue replies (`report_background`, `report_foreground`,
  `report_palette_color`, `report_color_scheme`)
- `justerm-core/src/lib.rs` — `Engine::drain_events`, `Engine::drain_replies`

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. alacritty's `EventListener` is named as the rejected
alternative in a module comment — a rejected design is exactly the claim worth pinning, since it is
the argument for the current one.

## Cross-cutting invariants

- [RIS keeps configuration and drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — both queues survive `ESC c`, and this is the only territory where the reset *adds* to one: every
  marker's disposal is announced into `events` before the rebuild, so the consumer drops decorations
  that now name nothing
- [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  — this is the channel where it bites hardest, because an occurrence's payload outlives the instant
  that gave it meaning and the frame's basis does not reach here. `MarkerCreated` is the worked case
  (#737), and the note lays the three channels side by side, which is the only view that shows two of
  them silent

## Blast radius

- [frame](frame.md) — the boundary partner. Anything that fails ADR-0020's state test lands here
  instead, so the two are decided together
- [hyperlinks](hyperlinks.md) — the worked example of that split: per-cell state, not an event
- [input encoding](input-encoding.md) — replies travel the same direction as encoded input and share
  the consumer's PTY write path, but are generated by parsing rather than by a user action
- [widget lifecycle](widget-lifecycle.md) — the consumer must drain both on a cadence it chooses, and
  a reply that is dropped is an application hang rather than a missing notification

## Known holes / open

- **Still no record for either channel's *membership*.** ADR-0029 (above) reached this territory in
  2026-08-06, but only for what a coordinate-bearing payload owes — the channels themselves are
  unrecorded, and one of them carries a *response obligation*: a dropped reply hangs the application
  waiting for it.
- **No bound on either queue.** A consumer that never drains grows memory with no signal, and no
  document states whose problem that is.
- **The rejected push model is unpinned**, and it is the whole argument for the pull design.
- **`TermEvent`'s membership has no record.** What earns a place as an event — as opposed to frame
  state or a reply — is answered by ADR-0020 for the frame and by nothing for this channel.
