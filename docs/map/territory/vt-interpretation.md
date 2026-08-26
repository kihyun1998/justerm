# Territory — VT interpretation

## What it is

The write path: bytes in, screen state out. `vte` splits the stream into actions and this territory
executes them — print a glyph, move the cursor, erase a region, scroll a margin, set a mode. It is
what remains in `term.rs` after #584 moved the read surfaces out, and it is still the largest single
file in the engine.

**Its contract is as much about what is *not* implemented as what is.** `architecture.md`'s
§"Hidden VT state" is a 30-entry catalogue of behaviour that has to be modelled and largely is not —
for a terminal engine, that list is half the specification.

## Governing decisions

- [**ADR-0001 — build on `vte`, not `alacritty_terminal`**](../../adr/0001-build-on-vte-not-alacritty-terminal.md)
  — delegate the genuinely hard parsing to a stable crate; own everything above it
- [**ADR-0004 — follow the DEC spec where Alacritty merely omits a spec'd behaviour**](../../adr/0004-spec-faithful-when-alacritty-omits.md)
  — the tie-breaker when the reference is silent rather than deliberate
- [ADR-0025 — row and wide-pair cell state ownership](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  — D2's per-verb table is a rule *about* these verbs, and #584 deliberately left the write path
  unsplit so that conformance stays readable in one pass

## Design model

- **`Perform` is the whole entry surface.** `print` · `execute` · `csi_dispatch` · `esc_dispatch` ·
  `osc_dispatch` — five methods, and everything the engine does to state hangs off them.
- **Modes are hidden state the engine owns and reports nowhere.** Origin (DECOM), autowrap (DECAWM),
  insert (IRM), newline (LNM), reverse wraparound, bracketed paste, synchronized output,
  colour-scheme updates, grapheme clustering — each changes what a later byte *means*.
- **Several modes are tracked but not acted on**, deliberately: the engine records the flag and the
  consumer owns the behaviour (synchronized output's paint-hold, colour-scheme notification). That
  pattern repeats often enough to be the territory's signature — see ADR-0017.
- **An empty OSC field means something different in every family, so there is no general rule to
  reach for.** Measured while adding the cursor slot (#832), because the obvious generalisation is
  wrong in both directions: for a **dynamic colour** (OSC 10/11/12) an empty field *addresses its
  slot and changes nothing*, and the stack still advances past it, so `OSC 10 ; ; <bg>` is the
  documented way to reach the background alone (xterm `misc.c:3684`, `:3687`); for a **hyperlink**
  (OSC 8) an empty URI *closes* the current link; for a **title** (OSC 0/2) an empty string *is* the
  new title. And the neighbour that looks identical is not: xterm's OSC 4 path has no skip at all —
  an unparseable name **aborts the remaining pairs** (`misc.c:2993-3003`, *"quit on any error"*),
  which is a third answer again. The rule is per family, decided at its own reference site.
- **Tab stops are explicit per-column state**, not a modulo: HTS sets, TBC clears, default every
  eighth column. A modulo would be wrong the moment an application moves one.
- **The scroll region redefines what "scroll" means.** DECSTBM changes which rows `IND` / `RI` /
  `LF` move and which leave the screen, so nearly every vertical-motion verb reads it.
- **RIS and DECSTR are two reset strengths** and the split is itself hidden state — what each does
  *not* clear is the part that matters.
- **The write path was deliberately not extracted** by #584: splitting by VT verb would scatter
  ADR-0025's row and wide-pair invariants across files, which is the failure that record exists to
  name.

## Code

- `justerm-core/src/term.rs` — the `Perform` implementation and every verb: `print`, `execute`,
  `csi_dispatch`, `esc_dispatch`, `osc_dispatch`, `put_tab`, and the mode flags they read
- `justerm-core/src/lib.rs` — `Engine::feed`, which is only `parser.advance(&mut term, bytes)`; the
  `Parser` and `Term` are separate fields because `advance` borrows both mutably
- `docs/architecture.md` §"Hidden VT state" — the catalogue of what is modelled, partly modelled and
  not modelled

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Soft wrap is a row property](../../agents/reference-facts.md#soft-wrap-is-a-row-property) — which
  verbs end a wrap, per verb, in two references
- [What a blanked / freed cell is made of](../../agents/reference-facts.md#what-a-blanked--freed-cell-is-made-of)
  — what an erase fills with

ADR-0004 is the rule that governs how these are read: a reference's **silence** is not permission,
and where it merely omits a spec'd behaviour the spec wins. That is a different stance from "match
the reference", and it is the one this territory operates under.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

Every stateful territory downstream, because this is where state is written.

- [soft wrap](soft-wrap.md) · [wide glyph](wide-glyph.md) — the verbs are the subjects of ADR-0025's
  per-verb table
- [cursor position](cursor-position.md) · [pen](pen.md) — moved and stamped here
- [damage](damage.md) — every mutation records a span; a verb that forgets is invisible
- [marker](marker.md) · [selection](selection.md) — the anchor-maintenance calls sit inside these
  verbs, line for line beside each other
- [reflow](reflow.md) — `resize` lives here too, and was deliberately not extracted
- [input encoding](input-encoding.md) — the modes it reads are set by *these* verbs, which is why the
  encoder cannot live in the consumer

## Known holes / open

- **The hidden-state catalogue is 30 entries and no territory owns most of them.** They are
  distributed across this map by subject, but the catalogue itself has no home in the graph — it is a
  section of a spec file, and the only artifact that knows what is *not* built.
- **Conformance is accumulated, never declared complete.** Coverage grows dogfood-first, so
  "not implemented" is a normal state here rather than a defect — and nothing distinguishes
  *deliberately absent* from *not reached yet* except prose. The perpetual tail is tracked in #47.
- **Modes tracked but not acted on have no single list.** Each is documented at its field; a consumer
  discovering which flags it must honour has to read the struct.
- **ADR-0004's tie-breaker is stated once and applied everywhere.** Whether a given verb followed the
  spec or the reference is not recorded per verb.
