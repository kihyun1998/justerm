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
- **Four mechanised checks, deliberately at different moments.**
  `justerm-wasm-decode/tests/readme_pins.rs` ties a constant the README *quotes* to the constant that
  owns it and fails on **every PR**. `.github/scripts/check-published-readme.mjs` rejects expiring
  claims — *"under construction"*, *"lands in #N"*, *"coming soon"* — at **publish time only**,
  because an in-progress crate may honestly call itself a scaffold in the repo. Snapshotting that
  sentence onto a registry is what makes it a lie.
  `.github/scripts/check-published-pointers.mjs` rejects a repo-only pointer — an ADR or issue
  number — in a manifest `description` **or** a published README, on **every PR**. The moment
  differs from the README gate for a reason: an expiring claim is honest when written and turns
  later, so only the tag can judge it, while an ADR number in a registry blurb is wrong when typed.
  Catching it at PR time costs a commit; catching it at publish costs a re-tag, and npm never lets a
  version be re-published at all.
  `.github/scripts/check-published-rustdoc.mjs` applies the same rule to the **rendered rustdoc**
  of every crate that reaches crates.io, also on every PR.
- **A doc-comment on a published crate is the front page, not an internal note (#953).** The
  hole two bullets down — *"doc-comments are a published surface with a narrower gate than
  READMEs"* — was measured rather than estimated: **384 bare pointers across 56 of `justerm-core`'s
  61 own pages** on the published `0.21.0`, plus `justerm` 0.5.1's `//!` ending *"See ADR-0010 in
  the repository."* And the `description` gate was **pointing at it**: its failure message told
  authors to put the pointer in the crate's `//!` header, the one published surface nothing read.
  - **It reads `cargo doc` output, not sources, and that is load-bearing.** A link nested inside a
    code span — `` `[CLAUDE.md](url)` `` — is a correctly-linked reference to any source scan and
    renders as literal text to a reader. 14 sites came out that way from the mechanical linking
    pass and **doubled** their own finding count, because the label and the URL then both read as
    bare paths. Only the rendered page tells the two apart.
  - **`#844` is an executable string, not prose.** `justerm-core/tests/public_struct_reasons.rs`
    reads `l.contains("#844")` over a published struct's doc block, so rewriting a paragraph's
    reasoning while dropping only the number reddens it (measured, on `Span`). `[#844](url)`
    contains `#844` — linking is the only form that satisfies the test and the reader at once,
    which is why the attribute rulings are linked where the provenance tickets are deleted.
  - **What it cannot see, stated because the gate otherwise reads as more coverage than it is.**
    The rustdoc **source view** (`src/<crate>/*.rs.html`, behind every item's `Source` link) is
    the whole file — 1118 hits for `justerm-core`, 760 of them ordinary `//` comments — and is
    deliberately out of scope, because `CLAUDE.md` sends the why and the measured value to
    `docs/map/`, so a comment naming a territory note is that policy working. A page rustdoc
    **inlined from a dependency** is skipped for a harder reason: `justerm` re-exports
    `justerm-core = "0.6"`, and those 37 pages carry prose already on crates.io that no edit here
    can reach. And the balanced-docblock extractor is *correct* rather than currently
    load-bearing — swapping in the naive non-greedy one drops 9.3% of the scanned text and finds
    the same 372 pointers — so a shrink there cannot redden the gate, and the scanned page,
    docblock and character counts are printed on every run instead.
  - **It is the first CI step that touches `justerm-facade` at all**, which the
    [workspace-exclusion](../invariant/workspace-exclusion-is-gate-invisibility.md) note predicted:
    the tombstone is outside the root workspace, so `cargo doc --workspace` never built it and its
    own `--manifest-path` run had to be added beside the gate.
- **The same rule lands differently on the two surfaces, and the difference is what it can link.**
  A `description` is a bare string with nowhere to put a URL, so *any* pointer in it is unresolvable
  by construction. A README can link out, so only a **bare** one is rejected there —
  `[ADR-0010](…/0010-….md)` passes and a naked `ADR-0010` does not. That is why the gate blanks
  markdown links before it reads, and why it blanks them with **same-length** whitespace: shortening
  the text first drifts every reported line number upward, which sends the reader to the wrong line
  (measured: `justerm-core/README.md` reported `:37` for a hit on `:57`).
- **`publish = false` does not mean unpublished, and that reading shipped text to npm 21 times
  (#941+).** It means *not to crates.io*; `justerm-wasm-decode` and `justerm-renderer` both reach npm
  through `wasm-pack`, which lifts `description`, `keywords` and `readme` out of Cargo.toml into the
  `pkg/package.json` it generates — a path the manifest itself never names. Both crates carried an
  ADR pointer in `description` (*"See ADR-0008."*, *"(ADR-0018, supersedes ADR-0002)"*) from their
  scaffold commit to v0.21.0, printed on the npm page to readers who cannot resolve either number.
  The `keywords` lines above them already carry a comment saying wasm-pack propagates them; the field
  one line up was read as an internal note anyway. **The registry, not the manifest's own `publish`
  key, is what decides whether a string is published.**
- **A `description` is the one published surface that cannot link.** That is the whole argument for
  the rule, and it is why READMEs are exempt rather than swept along: a README reaches `docs/adr/`
  by absolute URL and several do. A bare one-line blurb has nowhere to put the link, so a pointer in
  it is unresolvable by construction — no judgement about prose style is needed, or made.
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
- **A frame member crosses as a primitive; a value space's names live at module scope (#860).**
  This was recorded here as an unresolved 3:1 split — three core enums crossing as bare numbers
  against `UnderlineStyle`'s named one — and that framing was the mistake. Grouped by *what Rust
  type the value came from* the surface splits; grouped by **where the value hangs** it does not.
  Measured on the published `0.17.0` tarball: `DecodedFrame` has **33 members and every one is a
  primitive** (`number` / `boolean` / a typed array / `string[]`), and module scope holds the named
  things — `Flags`, `UnderlineStyle`, and the accessors that read a value out of a column. 33 for
  33 — **but that count settles the first clause only**, and saying it settles both was this
  note's own error before the completeness pass caught it. Every frame member *is* a primitive;
  whether every value **space** has a module-scope home is a second question with a second answer,
  below.
- **The grouping is derived, not observed.** A frame member cannot take a decoder-version type
  because `justerm-web/src/types.ts` declares `DecodedFrame` *source-agnostic* on purpose — "a
  frame may arrive decoded from a backend wire (frame mode) or be produced by an in-wasm engine
  (future)" — so pinning a member to one decoder's enum contradicts the reason that mirror exists.
  ADR-0008's adopted Axis-3 shape says the same thing from the other side, listing `cols` / `rows` /
  `kind` / `scroll` as **scalar getters**. `underlineStyle` reached the widget only because it is a
  module-scope *function*, never a frame member — the four values were never one axis.
- **Clause 2, enumerated — two value spaces had no module-scope home, and only one is closed.**
  The shape to look for is not "a bare number" but the three-part one: *no decoder export, roster
  hand-copied into a consumer, and published from there.*
  - **Marker kind — closed by #860.** Copied into `justerm-web/src/markers.ts`, published from
    `src/index.ts`, cast in unchecked, ungated against the wire. `markerKind()` gives it the home;
    the type-level roster gate on the web side is release-gated behind the pin bump, the way
    `underlineStyle`'s own consumer half was (#831 → #862).
  - **Mouse wanted-events bits — decoder half closed by #884.** `DOWN`/`UP`/`WHEEL`/`DRAG`/`MOVE`
    ride inside the `mouseWantedEvents` member and the decoder exported the `u8` and no constants
    for it, so the names were hand-declared in `justerm-web/src/input.ts` and published from
    `src/index.ts`, with the only test over them feeding the object back into itself — the
    list-checked-against-a-copy-of-itself shape this note records `flags()`'s guard falling to.
    `mouseEventBits()` gives them a home. **It takes `Flags`'s shape, not `markerKind`'s**, and
    that is the rule's second half doing work: a mask's members are bits *inside* the value, so no
    enum can answer "which of five" about a set. Where the value hangs decides *where the names
    live*; whether it is a set or a choice decides *what shape they take*.
    The web half — gating `input.ts` against the published constants — waits on the same pin bump
    as the marker kind's, and both land in one slice.
  - **`cursorShape`'s three names and `kind`'s two** live only in prose. `kind`'s roster was never
    copied into a consumer as *values*. **`cursorShape`'s now is** (#927): `justerm-web`'s
    `resolveCursorShape` maps `block`/`underline`/`bar` to `0`/`1`/`2` to resolve the consumer's
    default under an unset application shape, and nothing checks that mapping against the decoder.
    It is a `switch`, not an object lookup, because the style arrives from JS unchecked and an
    object lookup answers `"constructor"` with a function.
    The same class as `flags()`, at three members.
- **What the named form buys is a roster that is enumerated rather than restated** — this section's
  own thesis, one surface up. The prose mapping ships *verbatim* into the published `.d.ts`, where
  nothing checks it and nothing can rewrite it.
  **It does not buy the exhaustive `match`.** This note used to say it did (*"a scalar mapping
  written as `as u8` would carry no such guarantee"*), and #860's body inherited the sentence. No
  scalar mapping here is written `as u8`: `lib.rs` converts all three with exhaustive `match`es,
  `justerm-core/src/serialize.rs` converts the same three the same way one crate earlier, and
  `tests/wire_enum_stays_exhaustive.rs` keeps core's enums exhaustive so those matches cannot stop
  being total. A variant added upstream was already a compile error under either shape. The
  retracted argument is left visible rather than deleted: it is the ground three tickets were
  weighed on.
  **Two neighbouring copies are *true* and were deliberately not edited**, which is the harder half
  of a retraction: [pen](pen.md)'s link to this note, and `underline_style`'s own doc-comment, each
  say the binding mirrors the core enum through an exhaustive `match`. That is a fact about the
  conversion and neither claims the *naming* bought it — so the sweep's answer here is "these
  stand", recorded so the next sweep does not spend the question again.
- **#860's second false premise, measured and recorded for the same reason.** Its body also says a
  numeric enum is *"a soft break for TypeScript consumers (assignable outward, not inward)"*.
  Measured on tsc 5.9.3 against `justerm-web`'s real call patterns — `f.kind === 0`, a write into a
  `Uint32Array`, `f.cursorShape ?? 0`, and a plain-object `{ kind: 0 }` fixture — **all pass**, with
  `{ kind: 7 }` reddening as the positive control. Inward works for a member literal, and a widened
  `number` flows in **silently**, so the enum does not gate that direction either. The cost is not
  the reason to prefer or avoid a name here; the roster is.
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

### The `#[non_exhaustive]` question, for structs (#844)

**A `Default` is the consumer-chosen form of what the attribute imposes, so where a `Default` exists
or is meaningful, the attribute is declined.** Measured, not argued: with the attribute,
`Frame { cols, rows, kind, ..Default::default() }` from outside the crate is
`error[E0639]: cannot create non-exhaustive struct using struct expression` — functional-update
syntax is banned too. So the attribute does not *add* forward compatibility on top of `Default`; it
removes the caller's choice of how to take it. That is #843's rule — *an exhaustive type does not
force anyone; it preserves their option to be forced* — reaching the same answer on a struct, and
it is the reason #844's premise (*"the trade is genuinely different"*) is right about the mechanism
and lands on the same verdict anyway.

Three questions, in order, and every published struct falls out of them:

1. **Does anything outside this crate build one?** No public function accepts it and there are zero
   out-of-crate literal sites → the attribute binds nothing. Most of the surface is here.
2. **Is there a `Default`, or would one be meaningful?** Yes → `Default`, no attribute. `Frame` and
   `KeyEvent` already have one; `Span` (15 literal sites), `MarkerPosition` (25) and `MouseEvent`
   (8) do not, and *that* is the shape of their follow-up rather than the attribute.
3. **Neither?** Then the attribute is the candidate — and it needs a constructor shipped with it, as
   `image::Limits` does (`#[non_exhaustive]` + all-pub fields + `Default` + `no_limits()`). **No
   struct in this crate is in that position today.**

**What the field does, counted rather than recalled** — 942 distinct crates in this project's
dependency closure: 199 (21%) use the attribute at all; excluding one generated-FFI outlier the
sites are 74% enum / 20% struct / 6% variant, and only 34% of users ever put it on a struct. The
ones that do are types the caller never builds by hand — `clap_builder`, `schemars`, `rustix`,
`libc`, `raw-window-handle`, `hyper`, `tokio`. `syn` states the intent on seven `*Modifiers`
structs: *"This data structure may grow to accommodate future Rust language changes"*, with the
in-progress RFCs listed, and three of them have **zero fields** — a growth slot and nothing else.
The rule in practice is a claim that the growth cause lies outside the author's control, not a
defensive default.

**Where the per-type answers live, and why not here.** On the types, as doc-comments, the way #843
recorded the enums — that is the copy that ships to docs.rs, and a roster in this file would be the
`#552` failure again. `justerm-core/tests/public_struct_reasons.rs` is what keeps them honest: it
derives the published set from `lib.rs`'s own re-exports and fails on a struct carrying neither the
attribute nor a recorded reason, so *"nobody looked"* and *"looked and declined"* stop leaving the
same trace.

## Code

- `justerm-core/README.md` · `justerm-wasm-decode/README.md` · `justerm-renderer/README.md` ·
  `justerm-web/README.md` · `justerm-facade/README.md`
- `justerm-wasm-decode/tests/readme_pins.rs` — the constant pin (per-PR)
- `justerm-core/tests/public_struct_reasons.rs` — every published struct carries the attribute or
  the reason it does not (#844), over a published set derived from `lib.rs` rather than listed
- `justerm-wasm-decode/src/lib.rs` — `Flags`/`flags()` (the eleven named bits),
  `UnderlineStyle`/`underlineStyle()` (the 3-bit field, #831), `MarkerKind`/`markerKind()` (the
  `markerPositions` kind lane, #860), `MouseEventBits`/`mouseEventBits()` (the
  `mouseWantedEvents` mask, #884) and `ModifiedKeyBits`/`modifiedKeyBits()` (the `modifiedKeys`
  mask, #941): the module-scope names a consumer reads a value space by.
  Guarded respectively by `flags_map_covers_every_declared_cell_flag`; by `underline_style` taking
  the core enum, so its `match` is exhaustive over it; — because a `u32` argument can never be
  exhaustive over an enum — by `published_kind`, which `flatten` routes the lane through, plus
  `every_published_kind_is_reachable_through_the_accessor` for the reverse direction; and by
  `mouse_event_bits_covers_every_declared_member`, which asserts against `MouseEvents::all()`
  rather than against a copy of its own list — and `modified_key_bits_covers_every_declared_member`
  the same way against `ModifiedKeys::all()`, plus a check that no two fields name one bit
- `justerm-wasm-decode/tests/wire_enum_stays_exhaustive.rs` — the scan that keeps every core enum
  this crate maps onto a published value exhaustive (#843); its own source list is the roster that
  #831 had to widen
- `.github/scripts/check-published-readme.mjs` — the expiring-claim gate (publish-time)
- `.github/scripts/check-published-pointers.mjs` — the repo-only-pointer gate for `description` +
  published READMEs (every PR). It derives the package list by walking for manifests that carry a
  description rather than holding one, so a newly published package is covered the day it is added;
  it names what it cannot see (prose accuracy, expiring claims, contributor-only content such as
  build commands, a multi-line TOML description)
- Public doc-comments anywhere in `justerm-core/src/` — they ship verbatim as the docs.rs page.
  **Not just `lib.rs`**, which this entry used to say: a page is generated per *public item*, and
  the crate's 61 pages are produced by 18 source files, `term.rs` and `serialize.rs` among the
  largest contributors. `mod` privacy is what decides, not the file — `term/walk.rs`'s `//!` reaches
  no page at all. Derive it rather than trusting a list: the `src/justerm_core/*.rs.html` links in
  `target/doc` name every file that actually produced one
- `.github/scripts/check-published-rustdoc.mjs` — the repo-only-pointer gate for the **rendered
  rustdoc** of every crates.io crate (every PR, after `cargo doc`). Derives its crate list from the
  manifests that lack `publish = false`, reads `<div class="docblock">` on pages rustdoc generated
  from the crate's own source, and hard-fails when a published crate's pages are absent. It names
  what it cannot see: the source view, and any page inlined from a dependency
- `justerm-facade/src/lib.rs` — the tombstone's `//!`, which is its whole docs.rs page and, before
  the gate above, was reached by no CI step in this repository
- `justerm-web/src/types.ts` — `DecodedFrame`, web's mirror of the published decoder's getters;
  width-agnostic by contract, so it gates a column's presence and never its width
- `justerm-web/src/justerm-renderer.ts` — `RendererBackend`, web's mirror of the published
  renderer, and the typed binding in `JustermRenderer.create` that gates it
- `justerm-web/test/published-seam.types.ts` — the decoder-side gate (#646): the published
  decoder's columns must feed the published renderer's parameters, and every decoder getter must be
  mirrored. Checked by `pnpm typecheck`, not by vitest, and it names what it cannot see. §1b (#831)
  adds the level `keyof DecodedFrame` cannot reach — the decoder's **module-scope** exports, where
  a new one lands unreviewed at the moment a version range moves. **It has fired once**: #862's pin
  bump reddened it on `underlineStyle` / `UnderlineStyle` before a line of that ticket was written,
  which is the whole design — the window between a value existing upstream and a consumer noticing
  is closed by the compiler rather than by anyone remembering
- `justerm-web/src/types.ts` — `FlagBits`, a mapped type over the published `Flags` rather than a
  written-out list (#831), which is why it has no roster to go stale; `DecodedFrame` beside it stays
  hand-written, and the two differ because only one of them has a second producer. `UnderlineStyle`
  / `UnderlineStyles` (#862) are type-level references to the decoder's module — the enum's *values*
  are **carried** by `JustermRenderer` rather than re-exported, because a value re-export would make
  the decoder a static runtime import and the widget loads it dynamically on purpose
- `justerm-web/src/justerm-renderer.ts` — `cellStyleContext`, extracted for one reason worth naming:
  wiring `decoder.wireVersion` where the style accessor belongs **typechecks clean** (a function of
  fewer parameters satisfies one of more; a numeric enum accepts a `number`). Narrowing the
  parameter makes that class unrepresentable; the identity test beside it covers what is left
- `justerm-web/package.json` — the two version ranges that decide when a consumed drift is reachable

## Reference behaviour

**None.** No entry in `docs/agents/reference-facts.md`.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the tombstone is outside every `--workspace` command, and the note now records the *other* half:
  the two gates that reached it anyway both derive their work set from the tree, so a crate no
  command names is still covered
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
- **Doc-comments are a published surface with a narrower gate than READMEs — and the hole that is
  left is narrower than the old example suggests.** `check-published-rustdoc.mjs` (#953) rejects a
  bare ADR or issue number in the rendered docs; the **expiring-claim** check still reads READMEs
  only, which is how `Engine::resize` carried *"(Soft-wrap reflow lands in #7.)"* on docs.rs for six
  weeks after #7 closed. **That sentence is now caught** — measured by planting it back into
  `Engine::resize` and re-running the gate — but for the pointer's sake, not the promise's: `#7` is
  a bare issue reference. What is still open is an expiring claim carrying **no number**
  ("coming soon", "not yet implemented") in a doc-comment, which the README gate would reject and
  nothing reads here. Do not read the closed half as the whole.
- **The rendered-rustdoc gate covers crates.io only, so the `.d.ts` surface is still open (#951).**
  `justerm-renderer` and `justerm-wasm-decode` carry `publish = false`, so no docs.rs page exists
  for them and this gate skips them by construction — but wasm-pack lifts their `///` comments
  verbatim into the generated `.d.ts`, where an editor shows them on hover. Measured on the
  published 0.21.0 tarballs: 141 issue numbers, and 14 intra-doc links naming Rust method names the
  JS API does not have. That surface does not exist until `wasm-pack build` runs, so it needs its
  own gate rather than a branch in this one.
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
