# Territory — published surface

## What it is

Everything a stranger reads before writing a line of code, and everything that is frozen the moment
it ships. Registries snapshot a README at publish time, so it is the front page every new consumer
sees — and **nothing about it is compiled.** No test imports it, no compiler sees it, no constant in
it is checked against the constant it names unless someone builds that check.

How a version gets there is [release](release.md).

## Governing decisions

**None.**

- [ADR-0010 — all-prefixed crate naming](../../adr/0010-all-prefixed-crate-naming.md) decided the
  *names* on the registries, and produced the tombstone below — but nothing governs what the
  published prose must say or how it is kept true

## Design model

- **Immutability is the whole constraint.** crates.io and npm never rewrite a published artifact;
  only a yank comes back. Every other area is fixable by a commit — here a mistake is a permanent row
  in someone else's dependency graph.
- **Two mechanised checks, deliberately at different moments.**
  `justerm-wasm-decode/tests/readme_pins.rs` ties a constant the README *quotes* to the constant that
  owns it and fails on **every PR**. `.github/scripts/check-published-readme.mjs` rejects expiring
  claims — *"under construction"*, *"lands in #N"*, *"coming soon"* — at **publish time only**,
  because an in-progress crate may honestly call itself a scaffold in the repo. Snapshotting that
  sentence onto a registry is what makes it a lie.
- **The publish-time check fires after the tag is already pushed**, so a false positive costs a
  re-tag. That is why its phrase list is kept tight rather than thorough.
- **What this surface publishes is mostly a map of *names*, and a name map is only as complete as
  what its guard is derived from (#831).** The decoder's `flags()` hands a consumer eleven named
  bits so nobody hard-codes a bit value — and its guard asserted those eleven against a list copied
  from the same eleven, so a member `CellFlags` gained and this one did not was invisible to it
  forever. **Three levels of the same shape were live at once**, and each fell to the same repair:
  the union is now asserted against `CellFlags::all()`, which comes from the declaration; the
  `#843` scanner had it *one level up*, its list of **core source files** omitting `cell.rs`, so
  every type in that file was outside the scan with no roster entry left behind to go missing (a
  lost file is strictly worse than a lost type); and `justerm-web`'s `FlagBits` — this package's
  copy of that same map — had drifted to **nine of the eleven**, missing `wide_char` and
  `wrapline`, with the seam gate structurally unable to see it. The rule: prefer a check that
  enumerates the thing being published over one that restates it, and when neither is possible, say
  in the guard what it cannot see.
- **A hand-written mirror needs a reason, and the reason does not transfer between mirrors.**
  `DecodedFrame` is hand-written *because a frame reaches the widget from any producer on any
  decoder version* — width-agnosticism is the contract, and importing the decoder's types would
  break it. That justification was silently inherited by `FlagBits`, which has no second producer
  at all: nothing but the decoder makes those constants. Once asked separately, it derives —
  `{ [K in Exclude<keyof Flags, "free">]: number }` — and the roster stops existing rather than
  being guarded. Testability is untouched; a test still passes `{ bold: 1, … }`, it just can no
  longer pass a *subset*.
- **The prior art does not carry a mirror at all**, which is what made the question worth asking.
  Ruffle's web wrapper imports its types straight from the generated `.d.ts`
  (`import type { RuffleInstanceBuilder } from "../dist/ruffle_web"`) against a **relative path in
  its own build**, so no version range exists; Automerge's JS package declares **no runtime
  dependency** and vendors the wasm output into its own `dist/`, eliminating the skew a different
  way. Neither consumes its own family's wasm package by npm version range — that is justerm's own
  choice, bought with independent release tracks, and it is the root the two mirrors grow from.
- **A field is not a flag, and the published shape has to say which it is.** The same `flags[i]`
  word carries eleven yes-or-no bits and one 3-bit *value*. Exporting a twelfth mask would have
  passed every guard here and still left every consumer shifting by hand, so the style ships as an
  **accessor** rather than a mask (`underlineStyle`). That much is derived: no mask can answer
  "which of six".
- **Whether that accessor returns a *named* value is a separate choice, and this surface is now
  split on it (#831).** `UnderlineStyle` is the first core enum to cross as a `#[wasm_bindgen]`
  enum. Three crossed before it and all three ship as bare numbers with the mapping in prose —
  `cursorShape` (`-> u8`, "0 = Block, 1 = Underline, 2 = Bar"), `kind` (`FrameKind` → 0/1) and the
  marker kind (0..4) — each mirrored in `justerm-web/src/types.ts` as a plain `number`. A
  documented scalar would have answered "which of six" identically, so the enum is *chosen*, not
  derived, and the three precedents are counter-examples rather than agreement. Recorded here
  unresolved on purpose: the open question is whether the named form is the direction and the three
  become debt, or whether `underlineStyle` is the outlier. Nothing on this surface decides it.
- **What the named form does buy, and this part is mechanical:** the core→binding conversion is an
  **exhaustive `match`**, so a variant added upstream fails to compile in the binding rather than
  arriving on npm unnamed. A scalar mapping written as `as u8` would carry no such guarantee.
- **crates.io rewrites relative links**, resolving them against the crate's README subdirectory —
  so `[x](../CLAUDE.md)` in a crate README does reach the repo root. npm does **not**, and
  `justerm-web@0.7.0` shipped two broken links because of it (#473).
- **The family consumes its own published surfaces, so a mirror of one is the same uncompiled prose
  as a README — except a type can be gated and a paragraph cannot.** `justerm-web` depends on the
  *published* `justerm-wasm-decode` and `justerm-renderer` by version range, and declares each one's
  shape itself. It has a gate on exactly one of those two seams. `JustermRenderer.create` binds the
  real renderer class to its own `RendererBackend` by a **typed declaration, not a cast**, so a
  signature drift in the published renderer is a compile error here; it fired on its first real test
  (#645), naming an `apply_damage` call site that a hand-written list of sites did not contain. The
  decoder seam went ungated through #627, the decoder having widened its cluster-index column to u32
  while the adapter went on narrowing it back.
- **The gate on that seam does not run through `types.ts`, and that is why it took a while to find
  (#646).** `DecodedFrame` types every column `ArrayLike<number>` *deliberately*, so that plain-object
  demo and test fixtures satisfy it, and that accepts a `Uint16Array` and a `Uint32Array` alike — so
  the mirror this package owns is the one surface that structurally cannot pin a width. The fact worth
  asserting turned out to be one layer out and to be about the **family** rather than about this
  package: *the renderer's parameters must be able to take what the decoder produces.* `justerm-web`
  is merely the only place where both published types are in scope, so the check lives here while
  routing through neither of this package's own declarations. **It used to be true that `src/` took
  no decoder type at all**; that was never the rule it read as, and #831 spent it deliberately —
  `types.ts` now takes one `import type { Flags }`, erased at emit, the way `justerm-renderer.ts`
  and `accessibility-dom.ts` have taken `Palette` all along. What the width assertions must not
  route through is *this package's own declarations*, and they still do not. Two classes fall out of it, both derived rather than listed: a **width** that
  stops feeding (`Feeds<decoder column, renderer parameter>`) and a **getter this package never
  mirrored** (`Exclude<keyof wasm, keyof web>` must be `never` — the #129/#135 class, which a
  hand-kept roster would have to predict and `keyof` does not).
- **A version range decides when a consumed drift becomes reachable, which is not when it is
  introduced.** An npm 0.x caret is `>=0.N.0 <0.N+1.0`, so a widened column published by one family
  member is inert in another until that pin moves. The window a mismatch is dangerous in opens at
  the *bump*, not at the *tag* — which is where a gate on the paragraph above would fire, and why
  #633 sequences its steps rather than treating the tag as the deadline.
- **The tombstone is a published surface with no code behind it.** `justerm-facade` exists so that
  `justerm = "0.5"` dependants keep compiling *while learning the name changed* — its entire purpose
  is the message, and its fourteen lines of `pub use` are the delivery mechanism.

## Code

- `justerm-core/README.md` · `justerm-wasm-decode/README.md` · `justerm-renderer/README.md` ·
  `justerm-web/README.md` · `justerm-facade/README.md`
- `justerm-wasm-decode/tests/readme_pins.rs` — the constant pin (per-PR)
- `justerm-wasm-decode/src/lib.rs` — `Flags`/`flags()` (the eleven named bits) and
  `UnderlineStyle`/`underlineStyle()` (the 3-bit field, #831): the names a consumer reads a cell's
  attributes by, guarded by `flags_map_covers_every_declared_cell_flag` and by the exhaustive
  `match` in `underline_style`
- `justerm-wasm-decode/tests/wire_enum_stays_exhaustive.rs` — the scan that keeps every core enum
  this crate maps onto a published value exhaustive (#843); its own source list is the roster that
  #831 had to widen
- `.github/scripts/check-published-readme.mjs` — the expiring-claim gate (publish-time)
- Public doc-comments in `justerm-core/src/lib.rs` — they ship verbatim as the docs.rs page
- `justerm-web/src/types.ts` — `DecodedFrame`, web's mirror of the published decoder's getters;
  width-agnostic by contract, so it gates a column's presence and never its width
- `justerm-web/src/justerm-renderer.ts` — `RendererBackend`, web's mirror of the published
  renderer, and the typed binding in `JustermRenderer.create` that gates it
- `justerm-web/test/published-seam.types.ts` — the decoder-side gate (#646): the published
  decoder's columns must feed the published renderer's parameters, and every decoder getter must be
  mirrored. Checked by `pnpm typecheck`, not by vitest, and it names what it cannot see. §1b (#831)
  adds the level `keyof DecodedFrame` cannot reach — the decoder's **module-scope** exports, where
  a new one lands unreviewed at the moment a version range moves
- `justerm-web/src/types.ts` — `FlagBits`, a mapped type over the published `Flags` rather than a
  written-out list (#831), which is why it has no roster to go stale; `DecodedFrame` beside it stays
  hand-written, and the two differ because only one of them has a second producer
- `justerm-web/package.json` — the two version ranges that decide when a consumed drift is reachable

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md`.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the tombstone's README is published and reached by no gate at all, because the crate it belongs
  to is outside every `--workspace` command
- [a decoded frame's columns are getters](../invariant/decoded-columns-are-getters.md) — what the
  published decoder hands a consumer is an accessor, not a property, and the width-agnostic mirror
  that makes the seam flexible is also what hides it: every fixture in the repo is a plain object,
  where the same code costs nothing
- [a wasm `Err` payload is thrown verbatim](../invariant/wasm-err-payload-is-thrown-verbatim.md) —
  the other half of what a stranger consumes: not the values a call returns but the value it
  *throws*, decided in Rust, stated in no README and typed in no `.d.ts`, and frozen at publish
  like the prose above it

## Blast radius

- [release](release.md) — publishing is what freezes this, so the two are one event seen from two
  sides
- **Every territory whose behaviour a README or doc-comment describes.** This surface is a *mirror*
  of the others, and a mirror drifts silently: `justerm-renderer/README.md` announced "the GPU
  pipeline lands in #260+" across six published versions, and `justerm-wasm-decode/README.md` told
  readers to assert `wireVersion() === 2` against a shipped 12
- [wire format](wire-format.md) — the constant most often quoted in published prose, and the one the
  per-PR pin exists for
- [frame](frame.md) · [frame adapter](frame-adapter.md) — the shapes web mirrors. A column added or
  retyped there reaches the widget only through the two declarations under `## Code`, and only once
  a version range moves

## Known holes / open

- **Zero governing records** for a surface whose defining property is that it cannot be corrected.
- **Doc-comments are a published surface with a narrower gate than READMEs.** The expiring-claim
  check reads READMEs only, which is how `Engine::resize` carried *"(Soft-wrap reflow lands in #7.)"*
  on docs.rs for six weeks after #7 closed.
- **Only one constant is pinned.** `readme_pins.rs` covers `wireVersion()`; any other number a README
  quotes is unchecked, and a README that starts quoting a new one gets no pin unless someone adds it.
- **Both seams are gated now (#646), but the decoder-side gate fires at the pin bump, not at the
  drift.** `justerm-web` consumes *published* packages, so a column that widens on master is inert
  here until a version range moves — the gate makes the window's *end* automatic and a half-bumped
  state unmergeable (the two ranges are independent pins), which is strictly more than "remembered",
  and still not an early warning. An earlier signal would have to live where the decoder is *built*,
  and that means either importing the Rust toolchain into the one CI job deliberately built without
  it, or pinning the getter list in `justerm-wasm-decode/src/lib.rs` against a checked-in roster —
  a roster again, one language over. Neither was taken.
- **What no type on this seam can see**, recorded because the gate's existence otherwise reads as
  more coverage than it is: a column with no consumer that declares a width (`link`, `linkTable`,
  `markerPositions` — every path here takes it as `ArrayLike<number>`), and any
  change to what the values *mean* at an unchanged width. `link` is the live instance of the first:
  it widened to u32 in the same decoder release as `extra` and arrived at the #633 pin bump with
  nothing observing it. Harmless — nothing narrows it — but it is the class, and the gate is blind
  to it by construction.
