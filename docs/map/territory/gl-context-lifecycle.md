# Territory — GL context lifecycle

## What it is

Surviving the browser taking the GPU away. A WebGL context can be lost at any moment — GPU reset, tab
backgrounded, driver eviction — and **every GL object it owned is destroyed with it**. The browser
fires `webglcontextlost`, and *may* later fire `webglcontextrestored`. This territory is the state
machine that decides what the renderer does in between.

## Governing decisions

- [ADR-0027 — a liveness question is answered by the source that owns the answer](../../adr/0027-liveness-is-answered-by-the-source-that-owns-it.md)
  — **when GPU work may be attempted, and which predicate each site asks.** Its conformance map
  resolves every entry point in this territory, and it *derives* the two that are still wrong rather
  than listing them. Promoted from spine #689 (closed) after the rule produced a site — #695 — that
  nobody had reported
- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — owning a GL context at
  all is this crate's premise; it decides nothing about the loss behaviour

## Design model

- **"May later fire" is the whole difficulty.** Restoration is not promised, so the machine cannot be
  written as "wait for the event" — it needs a timeout and a way to tell a consumer that recovery is
  overdue rather than pending.
- **The state machine is pure and host-tested; the browser wiring is not.** Event closures and GL
  resource recreation live in the wasm layer, while what the renderer *should do with the current
  frame* is a value this module returns. That is the same split the packer, the upload planner and
  the frame adapter use.
- **The consumer is told, not guessed at.** `is_context_lost`, `is_restore_overdue`,
  `set_on_context_loss` and `set_context_restore_timeout_ms` are four of the crate's exports — the
  timeout is a consumer policy, and overdue-ness is a question a consumer can ask rather than infer.
- **Loss destroys GPU state, not the CPU-side model.** The persistent dense grid in the
  [frame adapter](frame-adapter.md) survives, which is what makes a restore a re-upload rather than a
  re-send from the engine.
- **Construction is the one entry point that refuses instead of deferring, and it is the only one
  where the *binding* decides the failure shape.** The five below can defer because there is a
  renderer to defer *into*; a constructor has no state machine yet, nothing to replay at `restore`,
  and no object to hand back — so it returns `Err` (#688). What forces the guard's exact position is
  not this crate's code but glow's: `Context::from_webgl2_context` enumerates the extensions
  (`get_supported_extensions().unwrap()`) and **panics** on the `null` a lost context answers with,
  so the check sits above *that* call, not above the first parameter this crate reads. The read
  itself is harmless — `get_parameter_i32` answers `0` for a `null`. A panic is also the one failure
  here that leaves the family's error shape: it arrives as a `RuntimeError`, not as the bare string
  every other fallible path throws.
- **Every entry point that changes the geometry takes the request and defers the GPU work.** Five of
  them can arrive mid-loss — the DPR, the font size, the font family, the spacing policy and the
  resize — and none may reject the call, because a consumer cannot see the loss to hold it back
  (that surface is unwired, #579). So each stores what it was given and lets `restore` re-derive
  from it; nothing is queued, because the stored value *is* the queue. The four setters skip an
  atlas re-bake that a dead context would return invalidated; `resize` skips reading the drawing
  buffer back, which on a dead context answers 0 and would floor the grid to one cell (#639).
- **"Is the context lost" has two answers and they disagree for a whole window — so the predicate
  is chosen per site, never shared out of habit** (#639). The browser destroys a context
  *synchronously* and merely **queues** `webglcontextlost`; the mirror holds on the way back. So the
  state machine's flag — the honest thing to report to a *consumer*, since it tracks what we have
  been told — lags the context itself, and an internal caller guarding on it is guarding on the
  wrong thing. Measured: in the pre-dispatch window `gl.isContextLost()` is already `true`,
  `drawingBufferWidth` already `0`, and the flag still `false`. The rule that falls out:
  a caller that **has** the answer in hand tests that (`resize` rejects a non-positive read-back,
  which is also right for any other cause of one), and a caller that must **ask** consults both
  sources, since each covers the window the other misses. The constructor is the third case and it
  falls out of the same rule rather than adding one: it asks the **context alone**, because the flag
  it would also consult does not exist yet — a freshly built state machine reports "live"
  unconditionally, so consulting it there is not a weaker predicate but a constant (#688, measured
  red as a mutation on `context-loss-construct.html`).
  This is the territory's most expensive shape so far: #639's first fix guarded on the flag, went
  green, and left the defect it was written for reachable verbatim.
- **`render` asks both sources, and the pure module is *given* the one it cannot fetch** (#695,
  ADR-0027 D3/D4). `ContextState::action` takes a `ContextLiveness` the wasm layer reads off the
  context and composes it with the flag it owns. Both arms of that decision used to run on a dead
  context in the pre-dispatch window: the `Rebuild` arm rebuilt and **threw** (an empty
  shader-compile log, `"justerm-renderer: "`), and the `Draw` arm packed — `packs()` +1, measured —
  resolving glyphs into a dead atlas. Both now skip; `demo/context-loss-race.html` asserts the
  no-throw and the `packs()` delta of **0**, in a window whose existence it checks first.
  **Why the argument rather than a guard at the caller**: `webgl.rs` is wasm32-only, so a guard
  there is invisible to `cargo test` — the composition lives in the pure module precisely so both
  windows have a host test. The cost taken with it is that the argument is a place to lie, which no
  host test can catch; that is what the browser section covers, and a mutation confirms the split
  (call site pinned to `Usable` → 326 host tests green, proof red).
- **What still runs the pack on a dead context: `apply_frame` and `apply_damage`.** They reach the
  pack → rasterise → `upload_glyph` → `upload_instances` chain from their own call with **no
  liveness predicate at all**, for the *whole* loss rather than one window — a consumer streaming
  output through a multi-second GPU recovery pumps every frame through it. **A cleared concern, not
  a safe design**, and the clearance is conditional: it holds only because `restore` does two
  separate things — `invalidate_baseline`, so the #263 diff cannot skip the re-upload of instances
  the GPU never received, and `bake_all_glyphs` over `cache.entries()`, so a slot marked resident
  but never uploaded is re-rasterised. Remove or narrow either and this becomes a silent defect: a
  frame the consumer submitted, saw acknowledged, and never sees. It is the one row of ADR-0027's
  conformance map still resolving as ✗.
- **What "defer" costs, stated once because each site pays it.** A value the consumer normally reads
  back synchronously — a clamped grid, an atlas-shrunk cell — is settled at restore instead, and the
  consumer is not told. That is the same missing signal as #579, reached from the other side.

## Code

- `justerm-renderer/src/context_loss.rs` — the state machine (pure, host-tested)
- `justerm-renderer/src/webgl.rs` — the event closures, resource recreation, and the four exports
  (browser-only)

## Reference behaviour

Two questions have been checked; the rest of the territory has not. alacritty and ghostty are not
browser renderers at all, so the comparison set here is smaller than for the rest of the crate — that
part is unchanged.

- [Resizing while the GL context is lost](../../agents/reference-facts.md#resizing-while-the-gl-context-is-lost--the-reference-never-asks-the-question-639-verified-2026-08-03)
  — a **negative** result, and the useful kind: xterm's resize handler runs unguarded through a loss,
  which reads as permission until you see *why* it can. It never asks the driver what it granted, so
  the read that a dead context answers with 0 does not exist there. Absence of a guard is not
  evidence about the guard
- [Reading a GL parameter that a lost context answers with `null`](../../agents/reference-facts.md#reading-a-gl-parameter-that-a-lost-context-answers-with-null-688-verified-2026-08-03)
  — the reference reads the *same* parameter in its *own* constructor with no guard, so the shape is
  shared and this layer is not the one that drifted. What differs is entirely the binding: JS carries
  a `null` on, glow unwraps it. Not indifferent, though — xterm's other two parameter reads *are*
  falsy-guarded, so a `null` there becomes a throw

Still unchecked: whether either reference notifies on a never-restored context beyond xterm's timeout
(the #327 comparison), and what any of them does with GPU resources it cannot rebuild.

## Cross-cutting invariants

- [a wasm `Err` payload is thrown verbatim](../invariant/wasm-err-payload-is-thrown-verbatim.md) —
  every failure this territory reports (no document, no canvas, no WebGL2 context) crosses into JS
  as a **string primitive**, so a consumer's `catch` sees no `.message`, no `.stack` and
  `instanceof Error === false`. Unchanged by #662, which fixed the decoder's single site because
  ADR-0008 obliged that shape there; nothing obliges it here yet. **A panic is a third shape and
  that note's recurrence list does not reach it** — it is neither a new fallible export nor a
  `map_err`: it crosses as a `RuntimeError` *object*, i.e. the one thing here that a consumer's
  `catch` could tell apart from the rest. #688 removed this territory's only site, by guarding above
  the glow call that produced it rather than by changing what anything throws

## Blast radius

- [frame adapter](frame-adapter.md) — its persistent grid is what a restore replays from; if that
  were ever discarded on loss, recovery would need the engine's cooperation
- [glyph atlas](glyph-atlas.md) — every slot is a GPU resource and does not survive; the atlas has to
  be rebuilt, not merely re-bound
- [GPU upload](gpu-upload.md) — the "last uploaded" state it diffs against is invalidated by a loss,
  so a restore has to force a full upload rather than a diff
- [cell geometry](cell-geometry.md) — every deferring entry point above is one of its setters or the
  resize, so a change to what derives the cell changes what a loss window has to hold
- [widget lifecycle](widget-lifecycle.md) — the consumer sets the timeout and reacts to the callback

## Known holes / open

- ~~**Zero governing records**~~ — **closed 2026-08-03 by ADR-0027.** The anchor was spine `#689`,
  opened on an explicit falsifier: *derive a fourth site nobody had to be told about, or settle a
  question before it is asked*. Both halves fired — #695 was found by asking the rule of every entry
  point, and the same pass classified two further sites without being asked — so the spine promoted
  and closed. Kept here rather than deleted because the *shape* is the reusable part: this territory
  went from zero records to one by opening a cheap hypothesis at the second rhyming issue instead of
  waiting for the archaeology that produced the repo's other two records at cluster sizes of 20 and 9.
- **One site still resolves against ADR-0027 as a defect**, not as an open question: the unguarded
  `apply_frame` / `apply_damage` chain, safe *only* by the validity condition stated in the design
  model above. (`render`/`action()` was the other; #695 closed it.) Nobody has been asked whether it
  is worth fixing — the answer turns on whether the clearance is a design or an accident, and that
  is a judgement, not a measurement.
- **No reference comparison at all**, and the usual comparison set does not apply cleanly — see
  ADR-0027's *Named prior art* for why the absence is itself the finding.
- **The interaction with the upload planner is stated here and nowhere else.** That a restore must
  invalidate the diff baseline is exactly the kind of cross-territory rule this map exists to hold,
  and it currently has no test naming it.
