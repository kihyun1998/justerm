# Aggregate — release & published surface

**This note owns no detail.** Two concepts, one event seen from two sides: the tag that publishes a
version also freezes the prose that ships with it.

| Concept | Note |
|---|---|
| How a version leaves the repo — tracks, tags, the lockstep, the tombstone | [release](release.md) |
| What a stranger reads once it has — descriptions, READMEs, rustdoc, `.d.ts`, the consumed seams | [published surface](published-surface.md) |

## Why they sit together

The release machinery decides **when** a surface becomes permanent, and the published-surface gates
decide **what** may be on it at that moment. Neither can be taken back once a tag is pushed — only a
yank comes back — so every change to one is checked against the other. The publish-time README gate
is the seam itself: it is a step inside the release workflows, guarding a surface the release does
not otherwise inspect.

## Where the two halves are wired by hand

Most published-surface gates derive their work set from the tree — the pointer gate walks for
manifests, the rustdoc gate for crates without `publish = false`, the package gate for a package's
own `files` — so a new package is covered the day it is added, whichever track ships it. **The
expiring-claim gate is the exception.** Each publish workflow names its READMEs by path
(`publish-crate.yml` names `justerm-core/README.md` and `justerm-pty/README.md`), so a crate added to
a track is covered by every prose gate except this one until someone adds its README to that
workflow. A release change that adds a package therefore owes a published-surface edit, and nothing
derives it.

The tombstone sits on both sides with no track: frozen on the registry, outside the workspace, and
reached by the derived gates only because they walk the tree
([workspace exclusion](../invariant/workspace-exclusion-is-gate-invisibility.md) holds that detail).
