# Territory — GL context lifecycle

## What it is

Surviving the browser taking the GPU away. A WebGL context can be lost at any moment — GPU reset, tab
backgrounded, driver eviction — and **every GL object it owned is destroyed with it**. The browser
fires `webglcontextlost`, and *may* later fire `webglcontextrestored`. This territory is the state
machine that decides what the renderer does in between.

## Governing decisions

**None.**

- [ADR-0018 — build justerm-renderer](../../adr/0018-justerm-renderer.md) — owning a GL context at
  all is this crate's premise; nothing decides the loss behaviour

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
- **`render` asks the flag too — and gets away with it, for a reason that lives in another
  function.** It branches on `ContextState::action()`, so in the pre-dispatch window it takes the
  `Draw` path on a context that is already dead. Measured (2026-08-03, a throwaway probe during
  #688): `packs()` goes up by one, so it really does pack — and since the pack *is* the resolve
  (`repack_from_grid` → `resolve_frame` → upload, one synchronous chain), the frame's two
  never-before-seen glyphs took cache slots and were uploaded into a dead atlas. That last step is
  read off the call chain, not separately observed; the counter and the pixels are the measurements.
  Nothing throws, and after the restore `render()` **alone** repaints that exact frame,
  pixel-identical to the same frame submitted to a live context (`[24,33,22,8,17,32]` both ways).
  **The validity condition — this is a cleared concern, not a safe design.** It holds only because
  `restore` does two separate things: `invalidate_baseline`, so the #263 diff cannot skip the
  re-upload of instances the GPU never received, and `bake_all_glyphs` over `cache.entries()`, so a
  slot marked resident but never uploaded is re-rasterised. Remove or narrow either and this site
  becomes a silent defect — a frame the consumer submitted, acknowledged, and never sees. It is
  deliberately **not** on #689's roster for that reason: the site does consult a proxy, but it is
  covered from behind rather than asking the right question, and counting it as a failure would
  pad the evidence for a rule nothing there tested.
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

- **Zero governing records** for a recovery path whose failure mode is a permanently blank terminal.
  The open cluster is anchored at spine `#689` (*this crate keeps asking a proxy whether the GPU is
  usable*) rather than at a record — the rule it would promote derives three sites so far, two of
  them found inside one change, and the anchor exists to see whether it derives a fourth. The roster
  lives there and deliberately not here.
- **No reference comparison at all**, and the usual comparison set does not apply cleanly.
- **The interaction with the upload planner is stated here and nowhere else.** That a restore must
  invalidate the diff baseline is exactly the kind of cross-territory rule this map exists to hold,
  and it currently has no test naming it.
