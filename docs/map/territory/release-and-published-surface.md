# Aggregate — release & published surface

**This note owns no detail.** Two concepts, one event seen from two sides.

| Concept | Note |
|---|---|
| How a version leaves the repo | [release](release.md) |
| What a stranger reads once it has | [published surface](published-surface.md) |

## Why they sit together

**Publishing freezes prose as well as code.** The same tag push that ships a crate snapshots its
README onto a registry, and neither can be taken back — so a release is not only a version event, it
is the moment every claim in the published text becomes permanent.

That is why the two checks guarding them fire at different moments: a constant a README *quotes* is
pinned on **every PR**, while a claim that *expires* is rejected at **publish time**, because an
in-progress crate may honestly call itself a scaffold in the repo and may not on a registry.

## The property both inherit

**One-way.** Every other territory in this map is fixable by a commit. Here, the only thing that
comes back is a yank, and a mistake is a permanent row in somebody else's dependency graph.

Two consequences worth carrying into any change that touches either:

- **The tombstone.** `justerm-facade` publishes as the old crate name, frozen at `0.5.1`, outside the
  workspace so the lockstep cannot drag it, gated by nothing. It has zero commits since creation and
  is invisible to every activity metric — and it is the clearest case of why this area is a territory
  rather than a chore.
- **The blast radius is the whole map.** The published surface is a *mirror* of every other
  territory's behaviour, and mirrors drift silently: a README announced an unimplemented GPU pipeline
  across six published versions, and a doc-comment promised reflow "lands in #7" for six weeks after
  #7 closed.
