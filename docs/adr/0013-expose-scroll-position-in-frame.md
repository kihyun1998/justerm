# ADR-0013: Carry scroll position (display_offset + scrollback_len) in the frame header (wire v5)

Status: accepted (2026-06-29, #112) — bumps WIRE_VERSION 4 → 5.

## Context

S4 (#112) is the scrollback **scrollbar**. xterm v6 draws it with VS Code's vendored
`SmoothScrollableElement` (a custom DOM slider — there is no native overflow scrollbar over a canvas, so
justerm-web needs a custom slider too). Its thumb is sized and positioned from two numbers
(`Viewport.ts:159-169`, read at source):

- `scrollHeight = cell.height × buffer.lines.length` — total content height = **all** lines
  (scrollback + screen).
- `scrollTop = ydisp × cell.height` — the viewport's display offset into that content.

So the web needs **the viewport's scroll offset + the total line count** to draw the thumb. The engine
already tracks both:

- `Term.display_offset` (`term.rs:131`) — "how many lines the viewport is scrolled up from the bottom;
  0 = following the live screen" (xterm's `ydisp` / alacritty's `display_offset`).
- `Term::scrollback_len()` (`lib.rs:168`).

But **neither crosses the wire.** `DecodedFrame` carries `cols`/`rows`, the cell SoA, cursor, and the
scroll *op* — not the scroll *position*. The format excluded it on purpose: *"(`display_offset > 0`) is
the consumer/cadence concern in #13, out of this format's scope"* (`architecture.md:292`). At the time
no consumer needed it; the scrollbar is the first that does.

## Decision

Add the viewport scroll position to the **frame header**, next to the cursor scalars, and bump
`WIRE_VERSION` 4 → 5:

- `display_offset: u32` — lines scrolled up from the bottom (`0` = following the live screen).
- `scrollback_len: u32` — lines in scrollback; the total content height is `scrollback_len + rows`.

The decoder exposes them as `displayOffset` / `scrollbackLen` getters. justerm-web computes the thumb:
`thumbRatio = rows / (scrollback_len + rows)`, `thumbTop = (scrollback_len − display_offset) / total`.

### Why the header, and why reverse the "out of scope" stance

**First principles.** In frame mode the model is across an IPC boundary, so any state the consumer needs
must be *sent*. xterm reads `buffer.ydisp` + `buffer.lines.length` in-process (no wire); the frame-mode
equivalent of that read **is** putting the two numbers on the wire. They are per-frame *state*, not cell
content — exactly like the cursor, which "rides in the header because the cursor moves with [the frame]"
(`serialize.rs:62`). Scroll position rides for the same reason; the cursor scalars are the precedent and
the header is the consistent home.

**Named prior art.** xterm `Viewport`/`Buffer` (`ydisp` + `lines.length`) and alacritty `Grid`
(`display_offset`, read for ADR-0009/0011) both keep these as first-class viewport scalars the renderer
reads every frame. The convergence (a scalar offset + a total) is the non-arbitrary shape; we transmit
what they read locally.

**Not a contradiction of #13.** #13/ADR-0005 excluded the offset because *no consumer needed it yet* —
dogfood-driven scope ("bones correct from day one, the tail grows by dogfood", CLAUDE.md). #112 is the
dogfood case that materialised the need. Growing the header by two scalars is the tail growing as
designed, not a reversal of principle.

## Consequences

- **WIRE_VERSION 4 → 5.** `encode` writes the two `u32`s in the header; `decode` reads them; the
  `justerm-wasm-decode` `DecodedFrame` gains `displayOffset` / `scrollbackLen`. The build-parity and
  golden tests extend by the two fields. A v4 buffer fails the version gate (existing behaviour).
- **Follow-bottom is now visible to the web.** `display_offset == 0` means the view follows the live
  screen — the scrollbar (and #112's "follow-bottom" acceptance) reads it directly instead of inferring.
- **Cheap.** 8 bytes per frame, header-only; cell records and the SoA columns are untouched.
- **Generalises the cursor precedent.** The header is now the home for "per-frame viewport state the
  consumer needs but isn't cell content" (cursor, scroll op, scroll position) — a coherent group.

## Alternatives considered

- **A separate scroll-state message/channel.** Rejected — it splits per-frame viewport state across two
  transports the consumer must then resynchronise; the cursor already establishes the frame header as
  that state's carrier, and a scrollbar that lags the frame by a message would mis-track.
- **Reconstruct the offset from scroll-op deltas on the web.** Rejected — the scroll op is a *delta*, not
  an absolute position; accumulating deltas drifts across any dropped/coalesced frame, and full frames
  carry no op at all. The absolute offset must be sent.
- **Leave it out; consumer queries the backend out-of-band.** Rejected — the consumer (penterm) would
  add a side request per render; the frame already crosses the boundary every cadence tick, so the two
  scalars belong on it.
