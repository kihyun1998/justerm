# ADR-0001: Build the engine on `vte`, not `alacritty_terminal`

Status: accepted (2026-06-16)

## Context

justerm needs a VT parser plus a grid/scrollback/selection state model. Three options:

- **`vte`** — the Paul-Williams ANSI parser only (a `Perform` trait); stable, widely used. The grid,
  scrollback, cursor, selection, and damage are *not* provided — we write them.
- **`alacritty_terminal`** — parser + grid + scrollback + selection + regex search + damage in one
  ~8.6K-SLoC crate. But it is an *internal* crate of Alacritty with no public-API stability guarantee;
  it churns between releases.
- **`wezterm-term`** — similar, heavier, also not a stable public API.

The genuinely hard part (the escape-sequence state machine) is `vte`, and `vte` is stable.

## Decision

Depend on **`vte`** and write our own grid + scrollback + selection + search + damage on top. Do
**not** depend on `alacritty_terminal` or `wezterm-term`. Reference their source (and Warp's
architecture) as *reading material* for how to handle the VT-compliance long tail correctly.

## Consequences

- **No dependency-churn risk** — `vte` is stable; the state model is ours and tailored to the
  grid-diff contract (`docs/architecture.md`).
- **We own VT compliance forever** — the long tail (DEC modes, scroll regions, reflow, mouse
  reporting, bracketed paste) is most of that 8.6K SLoC and surfaces as dogfood breakage. Grown
  incrementally from a common-90% base; the *contract/bones* are correct from day one.
- Reversible-ish: the engine is behind its own API, so swapping the parser dependency later is
  contained — but the model is ours regardless.
