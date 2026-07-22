# ADR-0015: Decoration marker primitive + overlay marker group (wire v7)

Status: accepted (2026-06-29, #118) — bumps WIRE_VERSION 6 → 7. **Amended 2026-07-22**: the group's
record has been widened since, so the layout quoted below is the v7 shape and no longer what ships.
#159 (wire v10) added the OSC 133 mark **kind** + exit code to each marker, taking `MARKER_STRIDE`
from 2 to **5**. The decision — a marker primitive with its own append-only overlay group, positions
projected per viewport, death by event rather than absence — is unchanged; only the record grew.
Marked inline at the Wire bullet.

## Context

S15 (#120) draws decorations — gutter glyphs, line highlights, rulers — anchored to a buffer *line* that
must survive scrollback eviction and column reflow. The engine had the mechanism but not the handle: the
selection's anchors already shift in lockstep with cap eviction, in-screen region/RI scroll, and reflow
(`selection_evict_oldest`, `selection_rotate_region`, the reflow `extras` path), but that machinery is
private to the live selection — no durable, externally-held handle existed. A consumer that stored an
absolute line index itself would see it go stale on the next eviction/reflow.

xterm.js has exactly this primitive (read at source, `src/common/buffer/Marker.ts` + `Buffer.ts`): a
`Marker { line, id, isDisposed }` whose `line` the `Buffer` auto-tracks —
`onTrim(amount) { marker.line -= amount; if (marker.line < 0) marker.dispose() }`, plus insert/delete
shifts — and `dispose()` fires `onDispose`. A `DecorationService` anchors decorations to these markers. So
the gap is precisely the xterm `Marker`: a line handle the buffer keeps valid and disposes when the line
leaves. xterm is **in-process** (the marker is a live object the renderer reads); justerm's frame mode must
*serialize* the marker's viewport position and *signal* its disposal across the boundary.

## Decision

Add a core **marker primitive** and carry visible markers in the overlay's third group (bump
`WIRE_VERSION` 6 → 7):

- **`Marker { id: MarkerId(u32 monotonic), line: usize absolute }`**, held in `Vec<Marker>` on `Term`.
  `Engine::add_marker(viewport_row) -> MarkerId` resolves the row to an absolute line (like a selection
  anchor) so it tracks that content; `remove_marker(id)` disposes it.
- **Re-anchor at the same points the selection moves**: cap eviction (`line -= 1`), region/RI scroll
  (rotate ±1 in the region), reflow (the marker's line rides the `reflow_pane` points list and reads its
  new position back from `extras`). A marker whose line *leaves* the buffer — evicted past the cap, or
  scrolled off a region edge — is **disposed**.
- **Wire**: the overlay section's third group — `u16 count` then `(marker_id u32, row u16)` per marker
  visible in the current viewport (off-screen markers omitted). The decoder exposes a `markerPositions`
  getter (flat `Uint32Array`, `MARKER_STRIDE = 2`); `wireVersion()` tracks the bump in lockstep
  (ADR-0008). This realizes the third-group slot ADR-0014 reserved.
  **[v7 shape — superseded by #159 at wire v10.** Each marker now also carries its `MarkerKind`
  (`Plain` / OSC 133 `PromptStart` / `CommandStart` / `OutputStart` / `CommandFinished(Option<i32>)`)
  plus the exit code's presence and value, so `MARKER_STRIDE` is **5**. Read
  `justerm-core/src/serialize.rs` (`MarkerPosition`, `MarkerKind`) for the shipped record — the
  append-only growth this ADR designed for is exactly what happened.**]
- **Disposal is an event, not frame absence**: `TermEvent::MarkerDisposed(id)` rides the existing
  `drain_events` queue (xterm's `onDispose` equivalent). A marker missing from a frame may merely be
  scrolled off-screen; only the event means *gone*. Explicit `remove_marker` fires it too, so the
  consumer's decoration cleanup is one path regardless of cause.

### Why markers survive an alt-screen swap (and selection/highlights do not)

This completes the lifecycle taxonomy the overlay section models — each kind of overlay data is handled by
its nature, not uniformly:

| data | on a coordinate-shifting mutation | on an alt-screen swap |
| --- | --- | --- |
| selection (user-authored) | re-anchor; clear if unanchorable | **clear** |
| search highlights (query-derived) | **invalidate** (re-search) | invalidate |
| markers (persistent anchors) | **re-anchor; dispose if the line leaves** | **survive** |

Markers anchor *primary* content, which is frozen while the alt screen is up, so their absolute
coordinates stay valid across the excursion; they are merely dormant (a frame on the alt screen carries no
marker positions) and reappear on return. Clearing them like the selection would defeat the point — a
decoration on a primary line must still be there after the user visits a pager. The grain is set by what
the data *is*, not by a blanket rule.

## Consequences

- **WIRE_VERSION 6 → 7.** `encode` writes the marker group; `decode` reads it; `DecodedFrame` gains
  `markerPositions`; `wireVersion()` lockstep. A v6 buffer is rejected at the version gate.
- **New engine surface**: `add_marker` / `remove_marker` + `TermEvent::MarkerDisposed`. The marker shift
  is co-located with the selection shift at each mutation point, so the two stay coherent and the existing
  selection regression tests guard the shared call sites.
- **Cheap and append-only.** No markers cost 2 bytes (a zero count). The overlay section is now three
  groups (selection / matches / markers); a future group still appends at the next bump.
- **Frame = position, event = death.** The split keeps the persistent "where is it" on the cadence-paced
  frame and the one-shot "it's gone" on the event queue — the consumer never confuses off-screen with
  disposed.

## Alternatives considered

- **Signal disposal by absence from the frame.** Rejected — indistinguishable from scrolled-off-screen;
  the consumer would drop a decoration that is merely out of view. Disposal needs its own channel.
- **Invalidate markers on mutation, like search highlights.** Rejected — markers are persistent anchors
  the engine owns, not a re-derivable query result; the consumer cannot "re-search" a decoration. They
  must re-anchor, exactly as the selection does.
- **Re-anchor markers through the alt swap / clear them like the selection.** Rejected — the primary
  buffer is frozen under the alt screen, so there is nothing to re-anchor *through*; clearing would lose
  decorations across a benign pager visit. Dormant-and-restore is correct.
- **A separate marker message/channel instead of the overlay group.** Rejected for the same reason as
  ADR-0014's side-channel: marker positions are viewport state re-projected every scroll, which `frame()`
  already does — a second transport would duplicate that projection and risk desync.
