# Aggregate — damage & viewport

**This note owns no detail.** Two concepts that meet at one rule.

| Concept | Note |
|---|---|
| What changed since the last ack | [damage](damage.md) |
| Which window of the buffer is on screen | [viewport](viewport.md) |

## Why they sit together

**The viewport decides what damage means.** Damage is recorded against the *screen*, but the consumer
renders a *window* — and while that window is frozen, screen changes are not visible, so they are not
damage. `Term::damage` returns an empty `Partial` whenever `display_offset > 0`, and the scroll that
unfreezes the window sets `full_damage` instead.

That is the entire coupling: one conditional, in one direction. Damage reads the viewport; the
viewport knows nothing about damage.

## The design this replaced, and why it is worth remembering

`docs/architecture.md` carried this as an **open question** for weeks, owed a *translation layer* —
"map screen damage → viewport damage, suppress or translate scroll ops while scrolled". No such layer
was built and none is needed, because the answer turned out to be a definition rather than a mapping:
damage is defined against **what the consumer can see**, and the screen is merely where it happens to
be recorded.

That paragraph outlived its issue (#13, closed) and a reader planning work from it would have rebuilt
a solved problem. It is the sharpest example of the failure this map's `## Governing decisions`
sections exist to expose: a spec can go stale in a way that costs the **work**, not just a lookup.
