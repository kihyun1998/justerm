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

## Code

- `justerm-renderer/src/context_loss.rs` — the state machine (pure, host-tested)
- `justerm-renderer/src/webgl.rs` — the event closures, resource recreation, and the four exports
  (browser-only)

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. Whether either reference handles context loss, and how,
has never been checked — and one of them (alacritty) is not a browser renderer at all, so the
comparison set for this territory is smaller than for the rest of the crate.

## Cross-cutting invariants

- [a wasm `Err` payload is thrown verbatim](../invariant/wasm-err-payload-is-thrown-verbatim.md) —
  every failure this territory reports (no document, no canvas, no WebGL2 context) crosses into JS
  as a **string primitive**, so a consumer's `catch` sees no `.message`, no `.stack` and
  `instanceof Error === false`. Unchanged by #662, which fixed the decoder's single site because
  ADR-0008 obliged that shape there; nothing obliges it here yet

## Blast radius

- [frame adapter](frame-adapter.md) — its persistent grid is what a restore replays from; if that
  were ever discarded on loss, recovery would need the engine's cooperation
- [glyph atlas](glyph-atlas.md) — every slot is a GPU resource and does not survive; the atlas has to
  be rebuilt, not merely re-bound
- [GPU upload](gpu-upload.md) — the "last uploaded" state it diffs against is invalidated by a loss,
  so a restore has to force a full upload rather than a diff
- [widget lifecycle](widget-lifecycle.md) — the consumer sets the timeout and reacts to the callback

## Known holes / open

- **Zero governing records** for a recovery path whose failure mode is a permanently blank terminal.
- **No reference comparison at all**, and the usual comparison set does not apply cleanly.
- **The interaction with the upload planner is stated here and nowhere else.** That a restore must
  invalidate the diff baseline is exactly the kind of cross-territory rule this map exists to hold,
  and it currently has no test naming it.
