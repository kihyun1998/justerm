# Aggregate — damage & viewport

**This note owns no detail.** Two concepts that meet at one rule, plus the signal that rule forced.

| Concept | Note |
|---|---|
| What changed since the last ack, and whether a frame is owed | [damage](damage.md) |
| Which window of the buffer is on screen, and who moves it | [viewport](viewport.md) |

## Why they sit together

**The viewport decides what damage means.** Damage is recorded against the *screen*, but the consumer
renders a *window* — and while that window is frozen, screen changes are not visible, so they are not
damage. `Term::damage` and `Term::frame_damage` return an empty `Partial` whenever
`display_offset > 0`, `Term::scroll_delta` returns `None`, and the scroll that moves the window sets
`full_damage` instead.

One conditional, in one direction: damage reads the viewport; the viewport knows nothing about damage.

## What the rule cost, twice

**First, a layer that was never needed.** `docs/architecture.md` carried this as an open question for
weeks, owed a *translation layer* — "map screen damage → viewport damage, suppress or translate scroll
ops while scrolled". None was built, because the answer was a definition rather than a mapping: damage
is defined against **what the consumer can see**, and the screen is merely where it is recorded. That
paragraph outlived its issue (#13); a reader planning from it would have rebuilt a solved problem.
§Cadence now records it as settled — and §Serialization still calls the remap "the consumer/cadence
concern in #13", the same staleness one section over.

**Second, a frame with no damage** (#1043). Defining damage by visibility means output while scrolled
up leaves *none* — yet it moves `scrollback_len` and `display_offset` in the header, which the
consumer's scrollbar reads. "Is there damage?" and "is a frame owed?" therefore diverged, and
`Engine::frame_pending` exists as the wider answer `Session::pump` reads
(`session.rs` — `output_while_scrolled_up_sends_a_frame_though_it_leaves_no_damage`). Any consumer that
gates sending on `damage()` alone freezes its scrollbar while the user reads history.
