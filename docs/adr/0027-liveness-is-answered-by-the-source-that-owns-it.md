# ADR-0027: A liveness question is answered by the source that owns the answer, never by a proxy

Status: **proposed** (2026-08-03, #689). Promotes the model that accreted across #639, #688 and #695 —
four guard decisions in one crate, two of them made inside a single change, each decided as its own
"which check goes here" and each producing the next.

**Amended 2026-08-20 (#774), conformance map only — D1–D4 untouched.** The open ✗ row named
`apply_frame` *and* `apply_damage`; the second never had a liveness question to answer, so the row is
split and the `apply_damage` half struck. This is the kind of revision the paragraph below invites —
a derivation error, corrected by reading the code the derivation is about, not a change of mind.

**This is a derivation, not a product judgement.** Every clause below follows from what the browser
actually guarantees about a WebGL context plus the shape of this crate's own modules, so a better
derivation retires it. Nothing here is a taste call, and the reference the *browser* question would go to cannot arbitrate
it: xterm.js has **no position** — `rg isContextLost` over `src` + `addons` returns 0 hits at pin
`699f5537`.

**Amended 2026-08-04 (#579): one reference does arbitrate, and it agrees.** This paragraph, and the
prior-art section below, both said the non-browser references had no context-loss concept at all.
alacritty has one, and asks the driver at the point of use rather than a flag — D1 reached
independently. It is recorded below rather than here because it changes the record's *support*, not
its content: a derivation that a second implementation arrived at by another route is a stronger
derivation, and this one was believed to have none.

## Context

`justerm-renderer` owns a GL context that the browser may destroy at any moment. Almost every entry
point therefore has to answer some version of *"may I do GPU work right now?"* before it acts, and
the crate answered it four different ways in four places. Each answer was locally plausible. Two were
wrong, and both were wrong in a window that is invisible from the site that got it wrong.

**The fact everything below turns on, measured rather than assumed:** the browser destroys a context
**synchronously** and merely **queues** `webglcontextlost`. So there is a window in which
`gl.isContextLost()` is already `true`, `drawingBufferWidth` already reads `0`, and this crate's own
state machine still says "live". Measured in Chromium 2026-08-03 (#639, re-measured for #688 and
#695). The mirror holds on the way back: a context can be usable again before we have processed
`webglcontextrestored`, so the flag can say "live" while the GL objects it describes are the
destroyed ones.

That single fact is why *"is the context lost"* has more than one true answer at the same instant,
and why a guard can be written, reviewed, mutation-tested and still be asking the wrong thing.

What made this a cluster rather than three bugs is that each site reached for whichever nearby signal
resembled the answer, and the resemblance held everywhere except in that window:

| Proxy consulted | What it actually answers | Where it failed |
|---|---|---|
| `ContextState::is_lost()` — our flag, set by the listener | *"has a loss been **reported** to us"* | #639's first fix. It went green and left the defect it was written for reachable verbatim |
| `getContext("webgl2")` returning successfully | *"a context object exists"* | #688. Measured: on a lost canvas it hands back **the same lost object**; glow then panics enumerating its extensions |
| the same flag again, one layer over, via `action()` | as row 1 | #695. `action()`'s own comment says its ordering exists *precisely* for the lost→restored→lost window; the ordering is right and the predicate is a proxy, so it fails in the first slice of that window |
| the read-back itself being non-zero | *this one is the answer* — kept as the contrast | #639's second fix. The site that **holds** the value tests the value |

## Decision

**D1 — A liveness question is answered by the source that owns the answer, never by something that
correlates with it.** "Owns" is not a matter of taste: for *"is this context usable"* the owner is the
context (`WebGl2RenderingContext::is_context_lost`) and, for anything whose validity also depends on
resources we have not yet rebuilt, our own rebuild bookkeeping. An event listener's flag owns a
different fact — *whether we have been told* — and is never a substitute for either.

**D2 — A caller that already holds the answer tests the value it holds.** If the call has just read
something back from the driver, that read *is* the answer, and it is the better predicate for a second
reason: it is also right for every other cause of a degenerate value, not only for a lost context.
`resize` rejects a non-positive `drawingBuffer` read-back rather than asking anyone (#639, #339).

**D3 — A caller that must ask consults every source that can disagree, and which sources those are
follows from where the caller stands.** Not "always ask both" — *always ask the ones that can differ
here*:

- mid-life entry points can disagree in both directions (the pre-dispatch window and the mirror
  window), so they consult the context **and** the flag: `gpu_work_must_wait()`;
- the constructor has no flag yet. A freshly built `ContextState` reports "live" unconditionally, so
  it is a **constant**, not a weaker signal — the only source that can disagree there is the context
  (#688).

**D4 — A value published to a consumer is a report of what we were told, and never a predicate for our
own work.** `isContextLost()` keeps flag semantics deliberately: *"has a loss been reported"* is the
honest thing to tell a consumer, and it is the wrong thing for us to branch on. The corollary is the
load-bearing half, because it is structural rather than advisory: **a module that can only see reports
can only answer report questions.** `context_loss.rs` is pure and host-tested *by design* and cannot
reach the context, so it can decide *whether to notify a consumer* — but a decision about whether GPU
work may be attempted cannot be a function of its state alone.

### Conformance map (resolved *against* D1–D4)

| Site | Predicate today | Resolved by |
|---|---|---|
| `resize` | its own `drawingBuffer` read-back | **D2** ✓ |
| `set_device_pixel_ratio`, `set_font_size`, `set_font_family`, `adopt_spacing` | `gpu_work_must_wait()` (context ∨ flag) | **D3** ✓ |
| the constructor (#688) | `webgl2.is_context_lost()` alone | **D3** ✓ — the flag is a constant here |
| public `isContextLost()` | flag | **D4** ✓ — and since #579 the only row with a *measured* consumer: the widget observes the flag and the context disagreeing, so D4's "different question" is a demonstrated fact rather than a derivation |
| `on_restore_deadline`'s `!is_lost`, `restore_overdue()` | flag | **D4** ✓ — these decide a *consumer notification* |
| `render` → `action()` | context ∧ flag, composed inside `action` | **D3 ✓ — resolved by #695.** Was the defect this record derived; D4's corollary said why it was inevitable (an "ask" question placed in a report-only module) and also how to fix it: the module is *given* the answer it cannot fetch, as `ContextLiveness` |
| `apply_frame` | **none** | **D3 ✗ — a defect, and the one row still open.** Harmless only because `restore` does `invalidate_baseline` *and* `bake_all_glyphs`; recorded with that validity condition in `docs/map/territory/gl-context-lifecycle.md`. Untouched by #695 — it packs from its own call, not from `render`. **Both halves of that condition were observed holding in #774** (`demo/context-loss-grids.html` feeds this path on a dead context with a cache-only glyph), which bounds the row's cost without closing it |
| ~~`apply_damage`~~ | n/a | **Struck 2026-08-20 (#774): it never had a liveness question.** It is an inherent method on `GridTier`, which holds the buffer handles but not the `glow::Context`, so the tier split (#769) makes a GL call from it impossible to write. It scatters and sets `needs_repack` (#421); the pack behind it is `render`'s, resolved one row up. Recorded as a struck row rather than deleted because the pairing above it was wrong in this record and in one map note at once, and the correction *shrinks* the open defect — the reachability argument for the ✗ row leaned on `apply_damage` being the path the real consumer uses, and `justerm-web` calls only `apply_damage` |
| `render`'s `Draw` path in the pre-dispatch window | as above | **D3 ✓ — resolved by the same change**, since both arms are behind one predicate now. The recorded measurement inverts: `packs()` went **+1** in that window and is now **0**, asserted by `demo/context-loss-race.html` |

The last three rows are what earned this record: they were **derived**, not restated. #695 was found
by asking D3 of every entry point rather than from a symptom, and the final two rows were classified
before anyone asked about them. Two of the three are now resolved — by #695, whose fix D4's corollary
also shaped — which is the record doing the job it was promoted for rather than evidence going stale.

**Tests it reproduces, and one whose scope it narrowed.** No test in `context_loss.rs` was ever
contradicted by this record. One was *narrowed* by it:
`a_loss_during_a_pending_rebuild_skips_rather_than_rebuilding_on_a_dead_context` tested the
lost→restored→lost ordering *only in its reported form*, because a pure state machine had no way to
express the pre-dispatch window at all — D4's corollary is the reason. **#695 closed that gap** by
giving the module the context's answer: `the_same_loss_before_its_event_is_dispatched_still_skips_the_rebuild`
is the sibling that reaches the window, and it fails against the pre-#695 implementation.

## Named prior art

- **xterm.js** — cannot arbitrate, and the absence is itself the finding: `isContextLost` appears
  nowhere in `src` + `addons`, and `drawingBuffer` nowhere either, so it never performs the read that
  makes a lost context dangerous and has no position on the event-vs-state race. Recorded in
  `docs/agents/reference-facts.md`.
- **beamterm** — the **negative** precedent, and it is named in `action()`'s own comment: its
  `render_frame` checks pending-rebuild before lost (`terminal.rs:334`) and so rebuilds on a context
  it has already been told is dead. Ours is a different mistake in the same family — we rebuild on one
  that is dead but has not told us yet — which is exactly why the ordering being right did not save
  #695.
- **alacritty** — **this entry was wrong and its correction strengthens the record** (2026-08-04, found
  by #579's completeness pass). It read *"n/a by layer, not by omission: neither is a browser renderer
  and neither has a context-loss concept"*. alacritty has both: `make_current` asks
  `renderer.was_context_reset()` — `glGetGraphicsResetStatus` under `GL_KHR_robustness` — or catches
  glutin's `ErrorKind::ContextLost`, then recreates the context and the renderer in place
  (`alacritty/src/display/mod.rs:561`, `:564`, `:576-595`; `renderer/mod.rs:281`, `:304`, pin
  `852e971`). **It is a positive precedent for D1/D2, reached independently:** it asks *the driver, at
  the point of use*, never a flag an earlier event set — in a codebase with no queued-event race to
  have taught it the lesson. So the rule this record derives is not peculiar to browsers.
  What alacritty still cannot arbitrate is D4, and the split is exact: being an application, it
  recovers synchronously with no deadline, no notification and nobody to tell. *"No reference to lose
  to"* holds for what to publish to a consumer and fails for which source a guard asks — and this
  entry had collapsed the two.
- **ghostty** — n/a, re-verified rather than inherited from the sentence above: no context-loss,
  device-loss or graphics-reset concept anywhere in `src` at pin `e6e26e1`.

## Consequences

- `docs/map/territory/gl-context-lifecycle.md` § *Governing decisions* stops saying **None**. That
  territory has been carrying a recorded known hole — *"zero governing records for a recovery path
  whose failure mode is a permanently blank terminal"* — and this closes it.
- New questions in this area arrive as **conformance items under this record** rather than as fresh
  decisions. The first two are already open: **#695** and, if it is ever fixed rather than left
  covered-from-behind, the `apply_frame` row.
- **#287 (multi-viewport) inherits it.** One context serving N grids means one liveness answer shared
  by N surfaces, and every row of the conformance map becomes *"usable for which viewport?"*. D1
  answers it by construction — the owner of the answer is still the context, and per-viewport state is
  rebuild bookkeeping, i.e. D3's second source.
- The scattered copies of this reasoning — currently re-argued at the constructor, at
  `gpu_work_must_wait`, at `is_context_lost`, at `action()`, and in the territory note — can point
  here instead of each re-deriving it. That duplication is what this record exists to end.

## Alternatives considered

- **One shared predicate at every site.** Rejected on measurement, not taste: `resize` holds a value
  that answers the question better than any query would, and is right about causes a query cannot see
  (#639). D2 exists because the uniform answer was tried and was worse.
- **Keep the whole frame decision in `context_loss.rs` and give it liveness.** Not rejected — D4's
  corollary narrows #695 to exactly two shapes (the caller guards before consulting `action()`, or
  `action()` takes liveness as an argument) and deliberately does not choose between them. That choice
  belongs to #695, with its cost to the module's purity recorded there.
- **Make `isContextLost()` report the context rather than the flag.** Rejected by D4: a consumer asking
  *"was I told"* is asking a legitimate and different question, and #579 (the unwired consumer surface)
  meant nobody had yet tested either answer against a real consumer.
  **#579 landed on 2026-08-04 and the rejection holds — now measured rather than derived.** The widget
  wires all four exports and its browser proof asserts the disagreement window *exists* before
  asserting anything inside it: immediately after `WEBGL_lose_context.loseContext()`,
  `gl.isContextLost()` is `true` while the widget's `isContextLost()` is still `false`
  (`justerm-web/e2e/demo.spec.ts`, `raceWindow`). So the two answers are observably different facts
  at the consumer surface, not one rounding the other — which is what D4 asserted and what nothing
  had yet checked.
  What the consumer half **adds** to D4 is a shape the record did not have to state while the surface
  was unwired: **a report a consumer can read is not the same as a report it can be pushed.** The push
  half has its own lifecycle — `set_on_context_loss` takes a `Function` with no unset and clears its
  slot only in `Drop`, so the notification outlives any teardown short of `free()`. The widget closes
  it from its own `dispose` (`justerm-web/src/context-loss.ts`), matching xterm.js, whose disposable
  clears the pending restore timeout (`addons/addon-webgl/src/WebglRenderer.ts:161-163`). D4 governs
  *what a published value means*; who stops it arriving is the consumer's lifecycle question and is
  tracked on spine #605, not here.
- **Leave it as a spine.** This anchor set its own falsifier — *"if this rule derives a fourth site
  nobody had to be told about, or settles a question before it is asked, it has earned ADR-0027"* —
  and both halves fired. Leaving it open past that would keep two homes for one throughline, each
  holding half the roster.
