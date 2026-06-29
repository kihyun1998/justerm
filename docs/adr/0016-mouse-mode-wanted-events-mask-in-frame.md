# ADR-0016: Carry the mouse tracking mode as a wanted-events mask in the frame (wire v8)

Status: accepted (2026-06-29, #129) — bumps WIRE_VERSION 7 → 8.

## Context

S6 (#111) input and the S4 (#112) scrollbar both need the web to decide **locally** whether a mouse/wheel
event goes to the app (report it) or stays local (selection / scrollback). That decision needs the current
mouse tracking mode, but `DecodedFrame` exposed only cursor + scroll. S6 worked around it with a
consumer-supplied `mouseReporting()` predicate (default false); routing was not autonomous.

The engine already tracks the mode (`Term.mouse_protocol`: Off/X10/Normal/ButtonEvent/AnyEvent) and the
coordinate encoding (`mouse_encoding`). xterm.js drives its browser routing from `onProtocolChange` — read
at source (`src/common/services/MouseStateService.ts`), it emits **not** the protocol name but the
*bitmask of event types the active protocol wants* (`CoreMouseEventType` flags), and dispatch is two
separate steps: `restrictMouseEvent` (does this event report?) then `encodeMouseEvent` (format the bytes).

## Decision

Expose the **wanted-events mask** in the frame header — *not* the protocol enum, and *not* the encoding —
and bump `WIRE_VERSION` 7 → 8:

- `MouseEvents` is a `u8` bitflag set: `DOWN`, `UP`, `WHEEL`, `DRAG` (motion with a button), `MOVE` (bare
  motion). `MouseProtocol::wanted_events()` is the protocol→mask table (Off=∅, X10=DOWN, Normal=DOWN|UP|
  WHEEL, ButtonEvent=+DRAG, AnyEvent=+MOVE).
- `frame()` carries `mouse_events: MouseEvents` in the header, like the cursor scalars (and like the
  derived `cursor_visible`). The decoder exposes `mouseWantedEvents() -> u8`; the web routes an event to
  the app when its bit is set, else keeps it local — replacing S6's predicate.
- The encoding is **not** on the wire: the web sends an intent and the backend encodes via `encode_mouse`,
  so the coordinate encoding is irrelevant to the routing decision (it only formats the report bytes).

### Why the mask, not the protocol enum — single source

The protocol→which-events-report mapping is fixed VT semantics that **already lives in justerm-core**, in
`encode_mouse`'s restriction (`input.rs` — Off→None, X10→press-only-no-wheel, motion gated by mode). If the
wire carried the raw protocol enum, the consumer (or the WASM decoder) would have to re-implement that VT
table to route — duplicating VT semantics **across the core/wasm boundary**, the exact drift class the
binding-parity gate exists to catch (0.4.0). Carrying the *derived mask* keeps the table in one place:
`wanted_events()` is now the single source that **both** the frame mask **and** `encode_mouse`'s gate
consume, so the routing mask and the encode-time restriction cannot diverge.

This is not the cursor-shape/colour-reference pattern (expose the raw value, let the consumer interpret) —
that pattern applies when interpretation is *consumer policy* (how to draw a cursor, which palette to
resolve a colour against). Protocol→events is not consumer policy; it is spec the engine owns, so the
engine resolves it. (Verified the analogy did not transfer before relying on it.)

## Consequences

- **WIRE_VERSION 7 → 8.** `encode` writes one mask byte in the header; `decode` reads it; `DecodedFrame`
  gains `mouseWantedEvents`; `wireVersion()` lockstep. A v7 buffer is rejected at the version gate.
- **Autonomous routing.** justerm-web reads the mask off the frame; the `mouseReporting()` predicate seam
  from S6 goes away. Wheel routing (#112) and mouse routing (#111) consult the same bits.
- **One source of the VT table.** `encode_mouse` was refactored to gate on `wanted_events()` (its inline
  X10/motion restriction now falls out of the shared mask); the existing mouse-encoding tests guard that
  the behaviour is unchanged.
- **Small.** One header byte; no overlay-coordinate machinery (this is a flag, not a span/anchor).

## Alternatives considered

- **Expose the protocol enum (`MouseProtocol`); the consumer maps it.** Rejected — duplicates the VT
  protocol→events table outside core (in the decoder or the web), across the binding boundary, with drift
  risk. The mask keeps it single-source. (`MouseProtocol` therefore stays crate-internal — the consumer
  never sees it.)
- **Also carry the coordinate encoding.** Rejected — the web does not encode mouse reports (it sends
  intents; the backend's `encode_mouse` formats them), so the encoding is irrelevant to routing and has no
  reason to cross the wire.
- **Leave the consumer predicate (S6 status quo).** Rejected — routing is not autonomous; the consumer
  must be told the mode out-of-band, and the scrollbar/input slices each re-derive it.
