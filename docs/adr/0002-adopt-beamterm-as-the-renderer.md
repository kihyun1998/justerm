# ADR-0002: Adopt `beamterm` as the renderer (not a hand-written one)

Status: superseded by ADR-0018 (2026-07-15) — accepted 2026-06-16

Note: the renderer runs in the *consumer's* webview, not in justerm. This ADR records the decision
because it shaped justerm's output contract (what the engine must produce to feed the renderer) — that
contract survives the supersession; only the renderer's *provenance* changed.

Note (superseded — final): **ADR-0018** replaced beamterm with the first-party `justerm-renderer`. The
supersession is now **final**: the `justerm-web` switch landed ([#273](https://github.com/kihyun1998/justerm/issues/273))
and this docs flip ([#274](https://github.com/kihyun1998/justerm/issues/274)) marks it superseded.
beamterm is no longer the active renderer — the record below is retained for the rationale that shaped
the output contract, not as a live decision.

## Context

The consumer needs to paint the engine's grid in a webview (WebGL2 — universal across Tauri webviews;
WebGPU still lags on WebKitGTK/Linux). Options: reuse `@xterm/addon-webgl` (rejected — it couples to a
Terminal instance's internal buffer, no external-grid API), write a fresh WebGL2 renderer (glyph atlas
+ instanced quads — a lot of GPU code), or adopt a purpose-built one.

`beamterm` is a parser-agnostic WebGL2 terminal grid renderer (Rust + WASM/JS bindings): single
draw-call, glyph-atlas, sub-millisecond on 45K cells. It passes the instability test ADR-0001 applied
to the engine: **MIT, v1.0.0 (Mar 2026), explicit SEMVER policy + changelog** (single-author
bus-factor mitigated by MIT — vendor/fork worst case).

## Decision

**Adopt `beamterm`** as the renderer. A thin consumer-side adapter bridges the contract:

- beamterm stores **baked 24-bit RGB** and a narrow attr set → the adapter resolves the engine's color
  **references** → RGB (via the consumer's frozen scheme) and maps our richer attrs (inverse/dim/hidden
  → colour manipulation; bold/italic/underline/strikethrough native). The cursor is drawn by the
  adapter (beamterm leaves it to the caller).
- **Keep our own wire format** (reference-based) — beamterm-data's RGB format is *not* our wire; it is
  fed *after* the adapter, webview-internal.
- **Keep selection engine-owned** — beamterm's built-in selection only sees on-screen cells (it would
  reintroduce Mosh's "can't copy scrollback"). Use beamterm to *render* the highlight; the selection
  model + text extraction stay in justerm.

## Consequences

- Atlas + instancing + sub-ms render come for free; no hand-written WebGL renderer to maintain.
- justerm's output contract must remain feedable to beamterm's `update_cells` after the thin adapter —
  but stays reference-based and theme-agnostic (the adapter, not justerm, knows hex).
- Renderer sits behind the grid contract → a later WebGL2→WebGPU or renderer swap is contained.
