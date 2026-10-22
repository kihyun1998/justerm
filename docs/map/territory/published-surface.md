# Territory — published surface

## What it is

Everything a stranger reads before writing a line of code, frozen the moment it ships: each
package's manifest `description`, its README, the docs.rs pages rendered from `///` and `//!`, the
`.d.ts` files wasm-bindgen and tsup generate, and the names and shapes a consumer imports. Registries
snapshot all of it at publish time, and **none of it is compiled** — no test reads a README and no
compiler checks a doc-comment's claim, unless someone builds that check. This territory is those
surfaces and the checks built so far, including the seams where the family consumes its own
published packages (`justerm-web` over the published decoder and renderer).

How a version gets there is [release](release.md).

## Governing decisions

**None.**

- [ADR-0010 — all-prefixed crate naming](../../adr/0010-all-prefixed-crate-naming.md) decided the
  names on the registries and produced the tombstone; nothing governs what the published prose must
  say or how it is kept true
- [ADR-0008 — wasm-decode as a separate crate](../../adr/0008-wasm-decode-binding-separate-crate.md)
  shapes the decoder's API (Axis-3 lists `cols` / `rows` / `kind` / `scroll` as scalar getters) but
  not its prose
- `CLAUDE.md`'s "What a comment holds" rule names this note as where the published-surface list
  lives; it is a repo rule, not a record

## Design model

From reading the gates and their headers, with measurements attributed to the issue that took them.

- **Immutability is the whole constraint.** crates.io and npm never rewrite a published artifact;
  only a yank comes back. A mistake here is a permanent row in someone else's dependency graph.
- **The surfaces and their gates, each at a deliberate moment.**

  | surface | gate | when |
  |---|---|---|
  | a constant a README quotes | `justerm-wasm-decode/tests/readme_pins.rs` | every PR |
  | an expiring claim in a README (*"under construction"*, *"lands in #N"*, *"coming soon"*, *"not yet implemented"*, *"work in progress"*, *"this is the scaffold"*) | `check-published-readme.mjs` | publish time only |
  | a repo-only pointer (ADR or issue number, repo path) in a `description` or README | `check-published-pointers.mjs` | every PR |
  | the same rule on the rendered rustdoc of every crates.io crate | `check-published-rustdoc.mjs` | every PR, after `cargo doc` |
  | the same rule on every text file an npm package publishes, plus link labels | `check-published-package.mjs` | `wasm` and `renderer-proofs` jobs, after `wasm-pack` |
  | a published struct's `#[non_exhaustive]` decision | `justerm-core/tests/public_struct_reasons.rs` | every PR |
  | the widget's two consumed seams | `justerm-web/test/published-seam.types.ts` | every PR, as `pnpm typecheck` |

  The moments differ for a reason. An expiring claim is honest when written and only becomes a lie
  when snapshotted, so only the tag can judge it — an in-progress crate may call itself a scaffold in
  the repo. A repo-only pointer is wrong the moment it is typed, so it is caught where a fix costs a
  commit rather than a re-tag (npm never lets a version be re-published). The publish-time check
  fires after the tag is pushed, so a false positive costs a re-tag; that is why its phrase list is
  kept tight.
- **The same rule lands differently on each surface, and the difference is what it can link.** A
  `description` is a bare string with nowhere to put a URL, so any pointer in it is unresolvable by
  construction and rejected. A README, a docs.rs page and a `.d.ts` hover all render markdown, so
  only a **bare** pointer is rejected there — `[ADR-0010](…)` passes. That is why the README gate
  blanks markdown links before reading, and blanks them with **same-length** whitespace (`readmeProse`):
  shortening the text drifts every reported line number (measured: `justerm-core/README.md`
  reported `:37` for a hit on `:57`).
- **`publish = false` does not mean unpublished** (#949). It means *not to crates.io*.
  `justerm-wasm-decode` and `justerm-renderer` reach npm through wasm-pack, which lifts
  `description`, `keywords` and `readme` out of `Cargo.toml` into the generated `pkg/package.json`.
  Both carried an ADR pointer in `description` from their scaffold commit through 21 tags each
  (`check-published-pointers.mjs` header). The registry, not the manifest's `publish` key, decides
  whether a string ships.
- **crates.io rewrites relative links and npm does not** (#473, verified against the live crate
  page in #477). crates.io resolves them against the crate's README directory, so
  `[x](../CLAUDE.md)` reaches the repo root; npm leaves them relative, and `justerm-web@0.7.0`
  shipped two broken links.
- **A doc-comment on a published crate is the front page** (#953). On the published
  `justerm-core` 0.21.0 the bare pointers ran to several hundred across nearly every page (this note
  recorded 384 on 56 of 61 pages; the gate's header records 407 on 54 of 58), plus the tombstone's
  `//!` ending *"See ADR-0010 in the repository."* The `description` gate's failure message had
  been telling authors to move the pointer into the crate's `//!` — the one published surface
  nothing read.
  - **It reads `cargo doc` output, not sources** — a link inside a code span is correct to a source
    scan and renders as literal text (14 sites came out that way from the mechanical linking pass,
    #953). Only the rendered page tells them apart.
  - **`#844` is an executable string.** `public_struct_reasons.rs` reads `contains("#844")` over a
    published struct's doc block, so a rewrite that drops only the number reddens it; `[#844](url)`
    satisfies the test and the reader at once.
  - **What it cannot see**, stated so the gate is not read as more coverage than it is: the source
    view (`src/<crate>/*.rs.html`, the whole file, 1118 hits for `justerm-core` of which 760 are
    ordinary `//` comments — out of scope because `CLAUDE.md` sends the why to `docs/map/`), and pages
    rustdoc inlines from a dependency (37 of the tombstone's 38 pages are core 0.6's, immutable). The
    balanced-docblock extractor is correct rather than load-bearing — the naive one drops 9.3% of the
    text and finds the same pointers (#953) — so the scanned counts are printed on every run instead.
  - **It is the first CI step that touched `justerm-facade`**, which needed its own
    `cargo doc --manifest-path` step beside it because `--workspace` cannot see the crate.
- **The `.d.ts` carries a defect the other surfaces cannot: a name the reader can act on and should
  not** (#951). wasm-bindgen copies a `///` verbatim and renames the method under it, so `addGrid`'s
  tooltip named `[set_viewport]` one line above a declaration spelled `setViewport`; 20 such labels
  shipped, plus a Rust `pub const` (`MARKER_STRIDE`) with no `#[wasm_bindgen]` that reaches JS under
  no spelling. The gate checks a label against **the same file's own declarations** (`declaredNames`),
  so it needs no roster and a rename moves both halves or it fires. Bare pointers were the larger
  count (164) and the smaller problem.
  - A hover renders markdown, so the bare-vs-linked rule applies here too, and a *dead* target is
    worse than none: every link wasm-bindgen ships is `](Self::x)`, a clickable thing that goes
    nowhere.
  - **What the gate could not see, twice, the same shape** (#951). It read only ` * ` lines, so
    `colors.d.ts`'s `//` header carrying `(#36)` reported clean (caught before merge); it then scanned
    `*.d.ts`, so the hand-written `colors.js` with three pointers was invisible and merged. The scope
    is now `package.json`'s `files`, npm's own allowlist. A gate named after a file type had already
    decided its own answer.
  - **Locally, `touch` the source before rebuilding** (gate header): a doc-comment changes no code,
    so cargo reuses the crate and wasm-pack re-emits the previous `.d.ts`. That produced two false
    green mutation results in #951; CI has nothing to reuse.
- **A published name map is only as complete as what its guard is derived from** (#831). `flags()`
  hands a consumer eleven named bits so nobody hard-codes one, and its guard once asserted those
  eleven against a copy of the same eleven. Three levels of that shape were live at once, and each
  fell to the same repair: `flags_map_covers_every_declared_cell_flag` now asserts against
  `CellFlags::all()`; the #843 scanner's source list omitted `cell.rs` (a lost file is worse than a
  lost type) and now includes it; and `justerm-web`'s `FlagBits` had drifted to nine of eleven
  (missing `wide_char`, `wrapline`) and is now derived. The rule: prefer a check that enumerates the
  published thing over one that restates it, and where neither is possible, say in the guard what it
  cannot see.
- **A hand-written mirror needs a reason, and the reason does not transfer between mirrors.**
  `DecodedFrame` (`types.ts`) is hand-written because a frame reaches the widget from any producer on
  any decoder version — width-agnosticism is the contract. `FlagBits` has no second producer, so once
  asked separately it derives (`Exclude<Extract<keyof WasmFlags, string>, "free">`) and its roster
  stops existing. A test can still pass `{ bold: 1, … }`, just no longer a subset.
- **The prior art carries no mirror at all** (`docs/agents/release.md`). Ruffle's web wrapper imports
  its types from its own build's generated `.d.ts`; Automerge vendors its wasm output into its own
  `dist/`. Neither consumes its own wasm package by npm version range — that is justerm's choice,
  bought with independent release tracks, and it is the root both mirrors grow from.
- **A field is not a flag, and the published shape says which.** One `flags[i]` word carries eleven
  yes-or-no bits and one 3-bit value. A twelfth mask would have passed every guard and still left
  consumers shifting by hand, so the style ships as an accessor (`underlineStyle`, #831). No mask can
  answer "which of six".
- **A frame member crosses as a primitive; a value space's names live at module scope** (#860).
  Measured on the published `0.17.0`: `DecodedFrame` had 33 members and every one was a primitive
  (`number`, `boolean`, a typed array, `string[]`), while module scope held the named things —
  `Flags`, `UnderlineStyle`, and the accessors. The grouping is derived, not observed: `types.ts`
  declares `DecodedFrame` source-agnostic, so a member pinned to one decoder's enum contradicts the
  reason the mirror exists. That count settles only "every member is a primitive"; whether every
  value space has a module-scope home is a second question:
  - **Marker kind** — `MarkerKind` / `markerKind()` (#860), routed through `published_kind` with
    `every_published_kind_is_reachable_through_the_accessor` for the reverse direction.
  - **Mouse wanted-events bits** — `MouseEventBits` / `mouseEventBits()` (#884), guarded by
    `mouse_event_bits_covers_every_declared_member` against `MouseEvents::all()`. It takes `Flags`'s
    shape, not `markerKind`'s: a mask's members are bits inside the value, so no enum can name a set.
    Where the value hangs decides where the names live; set-or-choice decides their shape.
  - **Modified-key bits** — `ModifiedKeyBits` / `modifiedKeyBits()` (#941), guarded the same way
    against `ModifiedKeys::all()`, plus a check that no two fields name one bit.
  - The widget's hand-kept copies of the first three (`markers.ts`, `input.ts`) stay, because two
    top-level wasm-bindgen imports race their init and the widget loads the decoder dynamically on
    purpose; what they owe is agreement. Names are compared at type level in
    `published-seam.types.ts` §1b (#885); values are compared at runtime for `MarkerKind` and
    `MouseEvents` in `published-decode.test.ts`.
  - **`cursorShape`'s three values** live only in prose on the decoder, and `resolveCursorShape`
    (#927) maps `block` / `underline` / `bar` to `0` / `1` / `2` with nothing checking that against
    the decoder's `match`. It is a `switch` because the style arrives from JS unchecked and an object
    lookup answers `"constructor"` with a function.
- **What the named form buys is a roster that is enumerated rather than restated — not an exhaustive
  `match`.** An earlier version of this note said a scalar mapping written `as u8` would lose the
  match, and #860's body inherited it. No mapping here is written that way: `lib.rs` converts with
  exhaustive `match`es, `serialize.rs` does the same one crate earlier, and
  `wire_enum_stays_exhaustive.rs` keeps core's enums exhaustive so those matches stay total. The
  retraction is kept because three tickets were weighed on it. [pen](pen.md)'s link here and
  `underline_style`'s doc-comment say the binding mirrors the enum through an exhaustive `match`;
  both are true and were deliberately left alone.
- **#860's second false premise, measured** (tsc 5.9.3, #860): a numeric enum is not "a soft break
  inward" for TypeScript consumers. `f.kind === 0`, a write into a `Uint32Array`,
  `f.cursorShape ?? 0` and a `{ kind: 0 }` fixture all pass, `{ kind: 7 }` reddens as the control,
  and a widened `number` flows in silently. The roster, not the cost, is the reason to prefer a name.
- **The family consumes its own published surfaces, and a type can be gated where a paragraph
  cannot.** `justerm-web` depends on the published decoder and renderer by version range and
  declares each one's shape itself:
  - **The widget's renderer seam is gated at `build`** (#775). `RendererBackend` extends
    `SurfaceBackend` rather than restating it, and `JustermRenderer.build` takes a
    `TerminalSurface<RendererBackend>`, so a member that drifts between the published renderer and
    either interface fails to compile where the composed surface is passed in. It is an interface,
    not the imported wasm type, so the wiring is testable behind a fake with no GL context. Its first
    form, a typed declaration in `create`, fired on its first real test (#645).
  - **The decoder seam is gated in `published-seam.types.ts`** (#646), not through `types.ts`,
    because `DecodedFrame` types every column `ArrayLike<number>` on purpose and so cannot pin a
    width. The fact asserted is about the family: *the renderer's parameters must take what the
    decoder produces* (`Feeds<…>`), and *every decoder getter is mirrored*
    (`Exclude<keyof wasm, keyof web>` is `never`, the #129/#135 class). §1b (#831) adds the
    decoder's **module-scope** exports, a named list that goes red at the pin bump that makes a new
    export reachable; it fired at #862's bump on `underlineStyle` / `UnderlineStyle` before that
    ticket's first line was written. #627 is what ran ungated before it: the decoder widened its
    cluster-index column to u32 while the adapter narrowed it back.
  - **`types.ts` takes decoder types only as `import type`** (#831: `Flags`), erased at emit, the way
    `justerm-renderer.ts` and `accessibility-dom.ts` take `Palette`. What the width assertions must
    not route through is the widget's own declarations, and they do not.
- **The decoder members the widget carries** (#862, `cellStyleContext`). The widget hands out the
  decoder's `underlineStyle` / `UnderlineStyle` rather than letting a consumer re-import them, and
  that one wiring is invisible to the type system: replacing `decoder.underlineStyle` with
  `decoder.wireVersion` typechecks and answers `16` for every cell, because a function of fewer
  parameters is assignable where one of more is expected and a numeric enum accepts a `number`.
  `decoder.Flags` in place of `decoder.UnderlineStyle` is caught. So `cell-style-context.test.ts`
  asserts reference identity, and the extraction exists for that test.
- **A binding added in the repo is absent until published.** The widget consumes the renderer from
  npm, so a new binding is missing at runtime and from the `.d.ts` until a `renderer-v*` tag ships
  it. Declared required, it would make the widget fail to typecheck against every older renderer;
  called unguarded, it would throw. So such members are optional and guarded — `setPreedit`,
  `setSuggestion`, `setLinkHover` on both `RendererBackend` and `Renderer`.
- **A version range decides when a consumed drift becomes reachable, not when it is introduced.** An
  npm 0.x caret is `>=0.N.0 <0.N+1.0`, so a widened column published by one family member is inert in
  another until that pin moves. The dangerous window opens at the bump, which is where the seam gate
  fires and why #633 sequenced its steps. At HEAD penterm steps outside the widget's range entirely
  (penterm 7dadc3c95 `package.json` — `pnpm.overrides`, `justerm-web>justerm-wasm-decode` `0.30.0`
  against the widget's `^0.22.0`).
- **The tombstone is a published surface with no code behind it.** `justerm` 0.5.1 exists so
  `justerm = "0.5"` dependants keep compiling while learning the name changed; its whole content is
  the message — a `//!` page, a `description`, a README — over one `pub use justerm_core::*`.
- **What bounds the input.** Published prose is authored here, and every gate reads a bounded file
  set it derives itself (manifests in the tree, a crate's own rustdoc pages, a package's `files`).

### The `#[non_exhaustive]` question, for structs (#844)

**A `Default` is the consumer-chosen form of what the attribute imposes, so where a `Default` exists
or is meaningful, the attribute is declined.** With the attribute, `Frame { cols, rows, kind,
..Default::default() }` from outside the crate is `error[E0639]` — functional-update syntax is banned
too — so the attribute does not add forward compatibility on top of `Default`; it removes the
caller's choice of how to take it. That is #843's rule (an exhaustive type preserves the caller's
option to be forced) reaching the same answer on a struct.

Three questions, in order:

1. **Does anything outside this crate build one?** No → the attribute binds nothing. Most of the
   surface is here.
2. **Is there a `Default`, or would one be meaningful?** Yes → `Default`, no attribute. `Frame` and
   `KeyEvent` have one at HEAD; `Span` (15 out-of-crate literal sites), `MarkerPosition` (25) and
   `MouseEvent` (8) do not, and that is the shape of their follow-up (their doc-comments).
3. **Neither?** Then the attribute is the candidate, with a constructor shipped beside it as
   `image::Limits` does. No struct in this crate is in that position.

**What the field does** (#844, counted over 942 crates in this project's dependency closure): 21%
use the attribute at all; excluding one generated-FFI outlier the sites are 74% enum / 20% struct /
6% variant, only 34% of users ever put it on a struct, and the structs that carry it are ones a caller never builds by hand (`clap_builder`,
`schemars`, `rustix`, `libc`, `hyper`, `tokio`; `syn` on seven `*Modifiers` growth slots). The
attribute is a claim that growth lies outside the author's control, not a defensive default.

**The per-type answers live on the types**, as doc-comments, because that copy ships to docs.rs and a
roster here would go stale (`#552`). `public_struct_reasons.rs` derives the published set from
`lib.rs`'s re-exports and fails on a struct carrying neither the attribute nor a recorded reason, so
"nobody looked" and "looked and declined" stop leaving the same trace.

## Code

- `justerm-core/README.md` · `justerm-pty/README.md` · `justerm-wasm-decode/README.md` ·
  `justerm-renderer/README.md` · `justerm-web/README.md` · `justerm-facade/README.md`
- `justerm-wasm-decode/tests/readme_pins.rs` — `readme_quotes_the_shipped_wire_version`,
  `readme_does_not_name_the_tombstone_crate`
- `.github/scripts/check-published-readme.mjs` — `EXPIRING`
- `.github/scripts/check-published-pointers.mjs` — `readmeProse`, the derived manifest walk
- `.github/scripts/check-published-rustdoc.mjs` — reads `docblock` divs on a crate's own pages
- `.github/scripts/check-published-package.mjs` — `declaredNames`, `docProse`
- `justerm-core/tests/public_struct_reasons.rs` — `CORE_SOURCES`, the published set from `lib.rs`
- `justerm-wasm-decode/tests/wire_enum_stays_exhaustive.rs` — keeps each mapped core enum
  exhaustive (#843)
- `justerm-wasm-decode/src/lib.rs` — `Flags` / `flags`, `UnderlineStyle` / `underline_style`,
  `MarkerKind` / `marker_kind` / `published_kind`, `MouseEventBits` / `mouse_event_bits`,
  `ModifiedKeyBits` / `modified_key_bits`, and the guards
  `flags_map_covers_every_declared_cell_flag`, `mouse_event_bits_covers_every_declared_member`,
  `modified_key_bits_covers_every_declared_member`,
  `every_published_kind_is_reachable_through_the_accessor`
- `justerm-wasm-decode/js/colors.d.ts` · `colors.js` — hand-written, shipped beside the generated
  declarations
- `justerm-renderer/src/webgl.rs` and its `webgl/` children — the `///` that becomes the renderer's
  declaration file
- Public doc-comments in `justerm-core/src/` — each public item's page; module privacy decides, not
  the file (`term/walk.rs`'s `//!` reaches no page). Derive the set from the
  `src/justerm_core/*.rs.html` links in `target/doc`
- `justerm-facade/src/lib.rs` — the tombstone's whole docs.rs page
- `justerm-web/src/types.ts` — `DecodedFrame`, `FlagBits`, `UnderlineStyle`, `UnderlineStyles`
- `justerm-web/src/justerm-renderer.ts` — `RendererBackend`, `JustermRenderer.build`,
  `cellStyleContext`
- `justerm-web/src/terminal-surface.ts` — `SurfaceBackend`
- `justerm-web/src/cursor.ts` — `resolveCursorShape`
- `justerm-web/test/published-seam.types.ts` — `Feeds`, `NotMirrored`, `ReviewedDecoderExports`,
  `UnreviewedDecoderExports`, `UnpinnedByThisFile`
- `justerm-web/test/published-decode.test.ts` · `cell-style-context.test.ts`
- `justerm-web/package.json` — the two version ranges

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. The mirror comparison (Ruffle, Automerge) is recorded
in `docs/agents/release.md` without a pinned tree, and the `#[non_exhaustive]` census is #844's
count over this project's own dependency closure.

## Cross-cutting invariants

- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the tombstone is outside every `--workspace` command; the two gates that reached it anyway both
  derive their work set from the tree
- [a decoded frame's columns are getters](../invariant/decoded-columns-are-getters.md) — what the
  published decoder hands a consumer is an accessor, and the width-agnostic mirror that makes the
  seam flexible also hides that: every fixture in the repo is a plain object
- [a wasm `Err` payload is thrown verbatim](../invariant/wasm-err-payload-is-thrown-verbatim.md) —
  the value a call throws is decided in Rust, stated in no README, typed in no `.d.ts`, and frozen at
  publish like the prose

## Blast radius

- [release](release.md) — publishing is what freezes every surface here; the two are one event seen
  from two sides
- **Every territory whose behaviour a README or doc-comment describes** — this surface mirrors them,
  and a mirror drifts silently: `justerm-renderer/README.md` announced "the GPU pipeline lands in
  #260+" across six published versions, and the decoder README told readers to assert
  `wireVersion() === 2` against a shipped 12
- [wire format](wire-format.md) — `WIRE_VERSION` is the constant published prose quotes most, and
  the one the per-PR pin exists for
- [frame](frame.md) · [frame adapter](frame-adapter.md) — the shapes the widget mirrors; a column
  added or retyped there reaches the widget only through the two seams above, and only once a range
  moves
- [input encoding](input-encoding.md) · [marker](marker.md) — `MouseEvents`, `ModifiedKeys` and
  `MarkerKind` are published value spaces with hand-kept copies in the widget; a member added in
  core must reach the decoder's names and the widget's copy, or a consumer cannot name it

## Known holes / open

- **Zero governing records** for a surface whose defining property is that it cannot be corrected.
- **An expiring claim with no number is caught only in READMEs.** The rustdoc gate catches
  *"(Soft-wrap reflow lands in #7.)"* on `Engine::resize` only because `#7` is a bare pointer
  (re-measured by planting it back, #953); "coming soon" or "not yet implemented" in a doc-comment or
  a `.d.ts` reaches nothing.
- **`justerm-web`'s `dist/index.d.ts` is not gated in CI.** tsup bakes `src/**` JSDoc into it as
  wasm-bindgen bakes `///`. Measured on `justerm-web@0.17.0`: 627 pointers, 345 since removed, 210
  woven into sentences no pattern can rewrite (#951 era). `check-published-package.mjs` can read it,
  and is deliberately not wired into the `web` job until those land, since a gate allowed to be red
  stops being one.
- **Only one constant is pinned.** `readme_pins.rs` covers `wireVersion()`; any other number a README
  quotes is unchecked.
- **The decoder-side gate fires at the pin bump, not at the drift.** An earlier signal would need
  the Rust toolchain in the `web` job, which is built without it on purpose, or a checked-in getter
  roster — a roster again. Neither was taken.
- **What no type on the decoder seam can see**: a column no renderer parameter declares a width for
  (`link`, `linkTable`, `markerPositions` — `link` widened to u32 at decoder 0.12.0 with nothing
  observing it), and any change of meaning at an unchanged width.
- **`ModifiedKeys`' values are not checked against the decoder at runtime** — only its names, in
  `published-seam.types.ts`. `MarkerKind` and `MouseEvents` have the runtime check (#885); this one
  was added after and did not get it (found 2026-10-22).
- **`cursorShape`'s mapping in `resolveCursorShape` is unchecked** against the decoder.
- **`Span`, `MarkerPosition` and `MouseEvent` have no `Default`**, the follow-up their #844
  doc-comments record.
- **A consumer can run the widget outside its declared range** (penterm's override, above); no gate
  here can see that combination.
