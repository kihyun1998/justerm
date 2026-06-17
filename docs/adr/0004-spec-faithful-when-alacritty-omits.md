# ADR-0004: Follow the DEC spec where Alacritty merely omits a spec'd behaviour

Status: accepted (2026-06-17)

## Context

ADR-0001 makes `alacritty_terminal` the **gold state-model reference** justerm
mirrors (without depending on it). Most of the time deferring to Alacritty is
right. But VT semantics have edges where Alacritty and xterm diverge, and "follow
the gold reference" stops being a clear instruction. Two such edges have already
surfaced in #8:

1. **DECOM unset (`docs/architecture.md` Hidden VT state).** Resetting origin
   mode: Alacritty leaves the cursor where it is; xterm homes it. The specs and
   implementations genuinely differ — there is no single "correct" answer. The
   repo chose to **follow Alacritty** and noted xterm differs.
2. **DECRC origin-mode restore (#8).** `ESC 8` / `CSI u` restores the saved
   cursor. The DEC spec (DECSC/DECRC save set) clearly includes origin mode
   (DECOM) alongside position, rendition, and charsets. xterm restores it.
   Alacritty's saved `Cursor` clone simply does **not** carry origin mode (it
   lives in a separate `term.mode` flag) — not a deliberate divergence, an
   omission. An app that does DECSC → toggle DECOM → DECRC gets the wrong origin
   state under Alacritty's behaviour.

Treating both the same way ("always follow Alacritty") would propagate an
omission into a latent bug. Treating them ad-hoc ("xterm here, Alacritty there")
looks inconsistent. We need a rule that distinguishes them.

## Decision

Refine ADR-0001's "Alacritty is the gold reference" with a tie-break:

> **Follow Alacritty on genuine ambiguities** — edges where the specs or
> reference implementations legitimately differ and there is no single correct
> behaviour. **Follow the DEC/xterm spec where Alacritty merely omits or
> under-implements a behaviour the spec clearly mandates.**

Applied:

- **DECOM unset → Alacritty** (genuine ambiguity; cursor stays put). Unchanged.
- **DECRC restores origin mode → xterm** (spec mandates it; Alacritty omits).
  The DECSC save set is therefore: cursor position, pen/SGR, origin mode, and
  pending-wrap. (Charsets join the set when a charset slice lands; not yet
  modelled.)

The deciding axis is **correctness/compatibility, never performance** — saving
one extra bool in a rarely-hit control path is free, and the print/parse hot loop
is untouched.

## Consequences

- **Consistency restored.** DECOM-unset (Alacritty) and DECRC-origin (xterm) are
  no longer contradictory — they are two applications of one rule.
- **A reusable test for future edges.** When the next Alacritty/xterm divergence
  appears, classify it first: ambiguity → Alacritty; spec-mandated-but-omitted →
  spec. New cases are recorded in `docs/architecture.md` Hidden VT state with the
  classification, not re-litigated from scratch.
- **Slightly more than a pure mirror.** justerm now intentionally exceeds
  Alacritty on spec-omission edges. This is deliberate: ADR-0001 chose Alacritty
  for its *model design*, not for byte-exact behaviour, and CLAUDE.md's "bones
  correct from day one" favours the spec where it is unambiguous.
- **Reversible per-edge.** Each application is a localised behavioural choice; if
  a specific call proves wrong for a real consumer, revisit that edge without
  disturbing the rule.
