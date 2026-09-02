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
- **An empty OSC field means something different in every family — and, inside one family, in every
  reference.** Two independent traps, measured while adding the cursor slot (#832), and the second
  is the one that bites.

  *Across families*, the obvious generalisation is wrong in both directions: for a **dynamic colour**
  (OSC 10/11/12) an empty field *addresses its slot and changes nothing* while the stack still
  advances past it, so `OSC 10 ; ; <bg>` is how xterm reaches the background alone (`misc.c:3684`,
  `:3687` — its *implementation*; `ctlseqs.txt:2082` documents only the stack); for a **hyperlink**
  (OSC 8) an empty URI *closes* the current link; for a **title** (OSC 0/2) an empty string *is* the
  new title. The neighbour that looks identical is not: xterm's OSC 4 path has no skip at all — an
  unparseable name **aborts the remaining pairs** (`misc.c:2993-3003`, *"quit on any error"*).

  *And `OSC 52` is a fifth answer, added by #828*: an empty **target** field is neither "skip" nor
  "unrecognised" — it *names the clipboard*, and it is the only form real applications emit (tmux
  3.2a, captured). That makes the family table complete on the axis rather than merely longer: an
  empty field means skip for a colour slot, close for a hyperlink, the new value for a title,
  reset-everything for `OSC 104`, and **a default target** for a clipboard request. Nothing
  generalises across the five, which is the entry's point; what generalises is that each one has a
  deliberate rule somewhere and none of them is the obvious one. The `OSC 4` arm is still undecided
  (#834). Note the shape #828 added on the *payload* side too, since it looks like the same question
  and is not: a payload **field that is absent** (`OSC 52 ; c`) and an **empty payload**
  (`OSC 52 ; c ;`) are different sequences — the second is a store of the empty string, which is how
  the sequence clears a selection.

  *Within* the dynamic-colour family the references then split **3–1 on the advance**, which no
  amount of reading one of them reveals. xterm, xterm.js and vte all consume the empty slot and move
  to the next; ghostty tokenizes the payload with `tokenizeScalar`, which **drops empty fields
  entirely**, so `OSC 10 ; ; <spec>` sets its *foreground* where the other three set the background
  — under a comment claiming *"This matches the xterm behavior"*. The divergence is a consequence of
  choosing `tokenize` over `split`, and ghostty has no test that would catch it. Rows in
  [`reference-facts.md`](../../agents/reference-facts.md#cursor-colour). justerm follows the three,
  which ADR-0004 settles independently: `ctlseqs.txt:2082` indexes by *parameter*, not by value.
- **A sequence can make the engine *retain* something it previously only relayed (#823).** XTWINOPS
  `CSI 22 t` / `CSI 23 t` push and pop the window title, and answering a pop requires holding the
  title — so parsing OSC 0/2 and forwarding the string, which had been enough since #12, stopped
  being enough. The general shape is worth naming because it will recur: *a later sequence can turn
  a pass-through into state*, and nothing about the original relay says so. Two axes are involved
  (window title and icon name), each with its own bounded stack, because the sequence's second
  parameter selects one and `vim` uses all three values — a single stack restores the wrong string,
  which is what alacritty does, its dispatch never reading past the first parameter. That is a
  choice among **three** models rather than two, and the spec makes none of them: xterm keeps one
  stack of `{icon, window}` *pairs* and walks back through older slots when the popped member is
  empty, which handles the axis correctly by a different mechanism and does not share a depth
  budget with two stacks. Rows in [`reference-facts.md`](../../agents/reference-facts.md). The optional
  third parameter (direct stack-slot access) is a **deliberate divergence from the spec**, decided
  on a five-way reach measurement recorded in #823 and pinned by a test whose name says so.
- **A private prefix is an *intermediate* here, so an unrouted `(prefix, final)` pair is
  unreachable rather than unhandled — and the difference is invisible (#824).** `vte` collects
  `0x3C..=0x3F` (`< = > ?`) into the same `intermediates` slice as the true 0x20..0x2F bytes, and
  `csi_dispatch` returns early on any intermediate it does not name. So a sequence in that family is
  not "missing from the `match`" — the `match` is never reached, and adding an arm for its final
  does nothing. DA2 (`CSI > c`) sat there from the beginning; the kitty `u` path had already opened
  one such pair, and DA2 is the second. Two consequences worth carrying: the fix is always a guard
  *above* the catch-all, keyed on the pair rather than on the prefix alone (a `.first()` match makes
  `CSI > $ c` DA2, which is what alacritty does and the other three references do not); and the
  remaining members are silent by construction, so their count is a measurement rather than a
  reading — across this repo's captures `CSI > m` (XTMODKEYS) occurs 7 times and `CSI > q`
  (XTVERSION) **zero**, which inverts the order the reference trees suggest. Rows in
  [`reference-facts.md`](../../agents/reference-facts.md). The unrouted rest is #47 tail.
- **An OSC payload arrives unbounded, and a handler that builds anything from one bounds it
  itself (#828).** Measured with a throwaway probe rather than read off the crate: `vte` is built
  with its default features, so its OSC accumulator is a `Vec<u8>` and **not** the
  `ArrayVec<_, MAX_OSC_RAW = 1024>` of its `no_std` path — a 4 MB `OSC 52` reaches `osc_dispatch`
  complete, as three params totalling 4 000 003 bytes. The consequence is easy to state backwards: a
  bound on a handler cannot stop the engine allocating, because the parser already did. What it
  stops is the *second* allocation — the decoded value and whatever the handler then hands a
  consumer. `MAX_CLIPBOARD_BASE64` is the only one today; the payloads `OSC 0/2`, `OSC 7` and
  `OSC 8` retain are bounded by nothing, which is a fact about this territory and not a claim that
  it is wrong.
- **Tab stops are explicit per-column state**, not a modulo: HTS sets, TBC clears, default every
  eighth column. A modulo would be wrong the moment an application moves one — and since #826 that
  is two verbs' problem rather than one, because `CBT` walks the same table backwards. The two walks
  are written as mirrors for exactly that reason: a count repeats the *walk*, so the directions
  cannot disagree about where a stop is, and `HT` followed by the same number of `CBT` returns to
  where it started. One thing they deliberately do **not** mirror is the deferred wrap — see
  [cursor position](cursor-position.md), where justerm turns out to be the outlier against all four
  references.
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
