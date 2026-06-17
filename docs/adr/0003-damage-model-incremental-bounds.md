# ADR-0003: Damage as incremental line+column bounds with ack-gated reset and a first-class scroll op

Status: accepted (2026-06-17)

## Context

`#4` must expose what changed since the consumer's last frame so the renderer
need not re-send the whole screen. `docs/architecture.md` fixes the *grain*
(line range + changed column span) and the *cadence* (§Cadence: the engine
remembers the consumer's last-acked state and produces a `last-acked → current`
diff, ack-paced, ≤1 in flight, with intermediate-state skip and flow control).
Three prior-art models were considered:

- **A — incremental damage bounds (Alacritty).** Each line carries a
  `LineDamageBounds { left, right }`; mutations `expand()` the span; a `full`
  flag forces a whole-screen redraw. Alacritty *resets every frame* and, on
  scroll, calls `mark_fully_damaged()` — it has **no** first-class scroll op.
- **B — baseline diff (Mosh).** Keep a copy of the last-acked screen and diff
  cell-by-cell to produce spans. Mosh *detects* scrolling heuristically (a new
  row 0 matching an old row N ⇒ infer a shift, emit DECSTBM + newlines).
- **C — per-line sequence numbers (wezterm).** Each line has a `seqno`;
  `changed_since(seqno)` yields the dirty lines. The consumer tracks its own
  acked seqno. wezterm tracks **line** granularity only — no column spans.

The tempting choice was C: the acked seqno *is* §Cadence's "last-acked state" as
a single number, with intermediate-skip and flow control for free and no
baseline copy. But two of our requirements break it:

1. **We need column spans, not just line granularity.** A span must be relative
   to *this consumer's* ack point (union of changes since the ack). A single
   per-line `(left, right)` cannot serve that without resetting the span at the
   ack — which reintroduces the very reset/baseline coupling C was meant to
   avoid. wezterm uses seqno precisely *because* it only needs line granularity.
2. **We have one consumer.** §Cadence is a single ack round-trip. Seqno's payoff
   is *multiple* consumers each tracking their own seqno (wezterm is a
   multiplexer); with one consumer, "reset on ack" is equally simple.

Once column spans + single consumer are required, the column span inherently
needs either a per-ack baseline (B) or a reset-on-ack (A). B costs a full grid
copy plus an `O(rows × cols)` diff every frame and infers scroll heuristically;
A marks incrementally (no copy, no full diff) and gives column spans natively.

## Decision

Adopt **A — incremental line+column damage bounds — with two adaptations**:

- **Reset is ack-gated, not per-frame.** Damage accumulates from the consumer's
  last ack; `reset_damage()` runs on ack. This *is* §Cadence's last-acked
  baseline: the accumulated span since the ack = the union of intermediate
  changes (intermediate-skip), a slow consumer simply gets a larger accumulated
  diff (flow control), and ack-gating gives ≤1 in flight.
- **Scroll is a first-class op, recorded — not detected.** Because the engine
  *executes* the scroll (#3), it records `scroll_delta() → { top, bottom, count }`
  directly, rather than inferring it from a diff as Mosh must. On scroll, the
  shifted rows are **not** individually damaged (the consumer shifts them); only
  newly exposed blank lines and subsequent writes are. Floods/resize/alt-clear
  degrade to a `full` damage flag (Alacritty's `mark_fully_damaged`).

Public surface (matches the architecture's API shape):
`damage() → Full | Partial(iter of { line, left, right })`,
`scroll_delta() → Option<{ top, bottom, count }>`, `reset_damage()`.

## Consequences

- **Aligned with §Cadence from the start**, so the cadence work (#13;
  architecture.md §Cadence) builds the ack-pacing on top of this damage state
  without a breaking API change — the reason this is settled before
  implementation (CLAUDE.md: the bones correct from day one).
- **Native column spans, no baseline copy, no per-frame full diff.** Cost: every
  mutation site (print, erase, scroll, later cursor moves) must record damage —
  invasive but local and matching Alacritty.
- **First-class scroll op** delivers the Mosh-style "move rows, don't redraw"
  win that Alacritty forgoes; degrades to `full` on floods/resize.
- **Reversible-ish.** If a *multi-consumer* contract is ever adopted (one engine
  → several independent views), revisit C (per-line seqno) — it is the right
  model there. Single-consumer today makes A strictly simpler.
- Damage interacts with scrollback (#3): the consumer renders the *viewport*, so
  while scrolled up under follow-bottom "stay" the viewport is unchanged even as
  the screen scrolls — viewport-vs-screen damage mapping is owned by the cadence
  work (#13; architecture.md §Cadence).
