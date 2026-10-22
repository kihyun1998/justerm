# Invariant — a change to what is on screen that no frame carries owes its own present

## The fact

The canvas is redrawn only when something presents it, and the drawing buffer keeps the last frame
until then. A frame from the engine presents on its own path; anything else that changes what the
screen should show — where a grid sits, whether it is drawn at all, a tint, a cleared buffer — has no
frame behind it, so **the site that makes the change must present (or request a present) itself**.
Otherwise the old pixels stay until something unrelated happens to present, and on an idle page that
is never.

Four sites hold it at HEAD 7bb31f7, each by its own call:

1. `justerm-web/src/justerm-renderer.ts — JustermRenderer.applyGrid` ends in `present()` on both
   branches, the hidden one (`clearViewport`) and the placed one (`setViewport`), because a placement
   change is a change on screen (#801).
2. `justerm-web/src/terminal-surface.ts — TerminalSurface.addGrid`'s lease release closure
   (`GridLease.release`) calls `requestRender()` after `backend.removeGrid` (#939): a grid leaving the
   draw set of a living surface stayed on screen until a sibling rendered — forever if it was the last.
3. `terminal-surface.ts — TerminalSurface.announceDensity` ends in `requestRender()` (#808): the host's
   handler re-sizes the surface, a resized buffer is a cleared one, and both callers presented *before*
   handing over, so without it the last thing to touch the buffer is the clear.
4. `justerm-renderer.ts — JustermRenderer.setFocused` calls `backend.render()` on a focus change even
   when there is no caret (#912): the selection tint moved, and `issueOverlay` only retains and marks a
   re-pack. Its sibling policy setters skip the present when there is no cursor; this one must not.

The check is one question: after this call, does the screen differ from the buffer, and is a frame
guaranteed to arrive? If no frame is guaranteed, present here.

## Why it is cross-cutting

The sites sit in two classes and three concerns — placement, registration, density, focus — and none
calls another; they share only the primitive they end in (`render` / `requestRender` / `present`). The
shared assumption is that "the next frame will draw it", which holds on the frame path and nowhere
else. Each site was found separately, and in each case the change was already *applied* (the grid
resized, the lease removed, the tint adopted) — only the present was missing, which is why the defect
reads as correct state.

The browser proofs were blind to all four for the same reason: every probe presents before it reads,
so a `readPixels` assertion sees the new state whether or not the site presented. #801 was found by
reading the compositor; #939's test stops the timers and reads a composited screenshot with no present
of its own; #912's tint gap was caught by an adversarial pass, not a test.

## Territories it holds in

- [multi-viewport](../territory/multi-viewport.md) — `applyGrid` presents on every placement (#801), and
  the lease's release requests a coalesced present (#939): a grid leaving the draw set owes one by
  whichever door it leaves.
- [GL context lifecycle](../territory/gl-context-lifecycle.md) — `announceDensity` re-presents after the
  host's handler, on both density paths (`setDevicePixelRatio` and a restore that adopts a new ratio)
  (#808).
- [caret drawing](../territory/caret-drawing.md) — `redrawCursor` is the only present on the
  no-new-frame paths, so `setFocused` presents without a caret when the tint moved (#912).
- [widget lifecycle](../territory/widget-lifecycle.md) — owns `render()`'s two shapes (a sole tenant
  presents at once, a shared tenant coalesces) that every site above reaches, and the blink loop's
  hidden-terminal gate, the converse: a tick that changed nothing visible must *not* present the shared
  canvas (#801).

## What a violation looks like

Stale pixels with correct state. A hidden pane keeps its picture; a shown pane stays unpainted; a
released grid lingers; after a density change both panes read `0,0,0,0` at a correctly sized buffer;
the selection keeps its active tint after blur with the caret hidden. It reproduces only on an **idle**
page — any frame, blink or sibling present hides it — and only where the change has no frame of its
own: a shared surface with a still sibling, a full-screen TUI with `DECTCEM` off.

## Discovery history

- **#801** (2026-08-25, `e6b43e1`) — hiding a pane left its pixels, showing one left it blank, on an
  idle two-terminal page; `applyGrid` gained the present.
- **#808** (2026-08-24, `a1ea03c`) — with the line removed and a host that pushes no frame, both panes
  read `0,0,0,0` at a correct `1800x680` buffer.
- **#912** (2026-09-16, `2f3c2f1`) — guarding `setFocused`'s present like its neighbours adopted the tint
  and never painted it.
- **#939** (2026-09-18, `5305d6b`) — `hide()` already presented; the lease release, the other door out of
  the draw set, did not.

## Where it will recur

Any new method on `JustermRenderer` or `TerminalSurface` that changes what is drawn without a frame:
a new policy setter (colour, opacity, cursor style), a new way to leave or re-enter the draw set, a
buffer resize. If it changes the picture and is not on the frame path, it presents — and a test of it
must read a composited result with no present of its own, or it cannot fail.
