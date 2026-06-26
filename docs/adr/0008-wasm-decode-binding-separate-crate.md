# ADR-0008: WASM decode binding — a separate crate exposing the canonical decoder as a zero-copy web artifact

Status: accepted (2026-06-19); amended (2026-06-19, v0.2.0 follow-up to #34 — the JS cell exposure
moves from a packed byte view to structure-of-arrays, and the package gains format-owned colour
helpers; the boundary is refined, not breached. See the amendment at the end.)

Note: like ADR-0002, the binding runs in the *consumer's* webview, not in justerm's core
runtime. This ADR is recorded here because it decides how the engine's wire format (ADR-0005)
crosses the process boundary into a *web* consumer, and that shapes both the packaging of this
crate and the output contract the decoder must serve.

## Context

`#34` asks justerm to provide the **canonical web decoder**. The wire format (`encode`/`decode`,
JT magic, ADR-0005) crosses a process boundary when a web consumer (PenTerm — first consumer)
embeds the engine: the native engine `encode`s in the backend, the bytes ship over the consumer's
IPC, and the webview must `decode` them. Today the only `decode` is the native Rust one
(`src/serialize.rs`). A JS/web consumer would otherwise re-implement the decoder in TypeScript,
duplicating the format and drifting every time the wire `VERSION` bumps — and that hand-written
mirror would *not* inherit the proptest + cargo-fuzz coverage the Rust decoder carries (ADR-0007,
#33).

Scope is **decode only**. Colour resolution (ref → RGB via the consumer's frozen scheme),
attr → renderer mapping, codepoint → atlas glyph-id, and cursor drawing stay the *consumer's*
adapter (theme- and renderer-specific — CLAUDE.md boundary; ADR-0002). `encode` stays native
(backend-only); the web side never encodes.

Four axes were decided, each cross-checked against named prior art (beamterm — the renderer this
output ultimately feeds; xterm.js addon-webgl — a WebGL terminal renderer's cell-buffer shape; the
wasm-bindgen / wasm-pack ecosystem — binding mechanics).

### Non-goal — performance is *not* the rationale

This binding is adopted for **maintenance and consistency**, not speed. The justification is entirely
single-source-of-truth (one `decode`, so the format cannot drift as `VERSION` bumps) plus inheriting
the decoder's robustness coverage (ADR-0007 — proptest + the #33 fuzz-found overflow). Performance is
a deliberate non-goal, recorded here so a later reader does not mistake it for one.

`decode` is binary byte-parsing — memory-bound integer reads + char/colour validation — the workload
category where WASM's edge over a JIT'd JS decoder is *smallest* (WASM wins on compute-heavy work,
not byte-munging). Concretely, the per-frame difference is single-digit microseconds in either
direction: well under 0.5% of a 60 fps frame budget, and dwarfed by the consumer's render/GPU upload.
For the common case — a small, damage-bounded frame (ADR-0003) decoded in a Tauri webview where the
bytes already arrive in JS — the WASM path's mandatory copy-in + call overhead can make it *marginally
slower* than a pure-JS decoder; WASM only reaches parity (and a slim win) on large full-redraw frames,
which the bounded-damage + ack-paced cadence (#13) rarely produce. WASM's real costs, conversely, are
one-time/elsewhere: module load + instantiation, ~tens of KB of artifact, and the toolchain in CI —
none of which a JS decoder pays. So the trade is explicit: a few microseconds per frame (invisible)
and some build complexity, paid to buy structural drift-immunity and inherited fuzzing. Were speed the
only axis, a hand-written TS decoder would be the right call.

### Axis 1 — packaging: separate crate, not an in-crate feature

The tempting shortcut is a `wasm` feature on the `justerm` crate itself (`crate-type =
["cdylib", "rlib"]`, `#[cfg(feature = "wasm")]` on the binding module, `wasm-bindgen` an optional
dep). It is **rejected**. The drift-elimination win it seemed to offer — "reuse `decode` without
duplication" — does not actually depend on co-location: `decode` is already `pub`
(`lib.rs`), so a separate crate calling `justerm::decode(bytes)` shares the exact same code with
zero duplication. The version-lockstep concern dissolves too — a Cargo workspace with
`version.workspace = true` makes the binding crate inherit the core's version, so a single bump
still moves both.

What the in-crate shortcut *costs* is real and one-directional:

- **Dependency hygiene.** `wasm-bindgen` / `js-sys` are heavy, wasm-only deps. Even feature-gated,
  they enter the core crate's `Cargo.lock` and risk accidental enablement via workspace feature
  unification. justerm's identity is a *pure engine with three deps* (`vte`, `unicode-width`,
  `bitflags`); the binding must not erode that.
- **Boundary fit.** A wasm-bindgen binding is glue for one specific consumption environment (the
  web) — transport/adapter territory, the same side of the line CLAUDE.md keeps out of the core
  ("no IPC, no rendering; the consumer owns transport"). It belongs *next to* the engine, not
  *inside* it.
- **Build isolation.** A dedicated crate is `crate-type = ["cdylib"]`, wasm32-targeted; the core
  stays a plain `rlib` and the native test / fuzz / bench gates never even parse the binding code.
  An in-crate `cdylib` would change the core crate's build artifacts.

Prior art: beamterm itself is a Cargo *workspace* (`beamterm-core`, `beamterm-renderer`,
`beamterm-data`, `js/`) — the renderer-as-separate-wasm-crate pattern this mirrors.

### Axis 2 — reuse the native `decode`, do not re-author

The binding calls the existing `justerm::decode` verbatim. This is the whole point of the issue:
**one implementation of the format, shared by both sides → drift is structurally impossible**, and
the binding inherits the decoder's robustness guarantees (ADR-0007 — the no-panic property and the
#33 overflow regression) for free. A TS re-implementation would re-open both: format drift on
`VERSION` bumps *and* an un-fuzzed parser of attacker-influenced bytes.

The wire `VERSION` constant is additionally exposed to JS (a `wire_version()` export) so the
consumer can assert at load time that the WASM decoder and the backend encoder agree; `decode`
already returns `BadVersion` on mismatch, so a stale artifact fails loudly rather than mis-parsing.

### Axis 3 — output shape: flat cell-buffer *view*, not rich JS objects, not resolved cells

The decoder must hand cells to JS **without a per-cell JS↔WASM boundary crossing** (#34 AC3). Three
shapes were weighed:

- **A — a JS array of cell objects** (`[{c, fg, bg, flags}, …]`). Rejected: marshalling hundreds–
  thousands of objects across the boundary per frame is exactly the cost that would negate the WASM
  win. This is the classic "inefficient WASM" failure mode, and the criterion exists to forbid it.
- **B — resolved beamterm cells** (the 8-byte `CellDynamic`: u16 glyph-id with style bits 10–14 +
  24-bit fg + 24-bit bg). Rejected: it would force the engine to resolve colour refs → RGB and map
  codepoints → a font atlas, breaking the theme- and atlas-agnostic invariants (ADR-0002/0005). The
  decoder sits one layer *above* `CellDynamic`; that resolution is the consumer adapter's job.
- **C — a flat contiguous cell buffer exposed as a typed-array *view*, plus a small span
  directory.** Adopted. The decoder concatenates every span's cells into one buffer and exposes:
  - `cells`: a `Uint8Array` **view** into WASM linear memory over the concatenated records
    (zero-copy, the bulk data);
  - `spans`: a small flat `Uint32Array` of `{line, left, right, cell_offset, cell_count}` per span
    (so JS walks the *directory*, never per cell);
  - `side_table` (grapheme clusters) and `link_table` (OSC 8 URIs): small, exposed via getters;
  - `cols` / `rows` / `kind` / `scroll`: scalar getters.

  Total boundary crossings are **constant** (a pointer + length for `cells`, one small `spans`
  array, the rare tables) — independent of cell count, which is what AC3 requires.

Prior art convergence: beamterm packs cells into a fixed-stride 8-byte buffer; xterm.js's
addon-webgl stores cells as packed `uint32`s (`Content`: codepoint + `IS_COMBINED` + width;
`Attributes`: RGB + 2-bit colour mode + flag bits) with combining content held in side storage.
A fixed-stride flat array of `{codepoint, fg, bg, flags, extra, link}` with rare clusters in a
side-table — i.e. exactly justerm's wire record — is the shape both renderers already consume.

### Axis 4 — stride: keep the wire's 18-byte record as-is, do not re-align

The wire cell record is `c` u32 · `fg` u32 · `bg` u32 · `flags` u16 · `extra` u16 · `link` u16 =
**18 bytes, 2-aligned** (ADR-0005). The decoder exposes that record *unchanged*; it does not
re-lay cells to a 4/8-byte-aligned stride. Re-alignment's only benefit is enabling JS
`Uint32Array`/`Uint16Array` views (whose `byteOffset` must be 4-aligned) instead of `DataView`
(which reads any offset). But the record is **heterogeneous** (mixed u32/u16), so even at an aligned
stride a single typed-array gives no clean field access — the consumer reads via `DataView`
regardless. And re-alignment fights the wire's purpose: 18→24 B would inflate every frame on the
transport by a third for a read-cost the consumer cannot measure (its per-cell adapter loop is
dominated by the atlas lookup + colour resolve, not an unaligned `DataView` read). So the 18-byte
record is correct for transport and adequate for read; re-alignment is a premature in-decoder
transform we decline.

## Decision

Ship a **separate `justerm-wasm` crate** in a Cargo workspace (`version.workspace = true` →
version-lockstep with the core), `crate-type = ["cdylib", "rlib"]` (`cdylib` is the wasm artifact;
`rlib` lets the crate's own test/bench targets link it — the *core* crate, not this one, is what
stays rlib-only), depending on `justerm` + `wasm-bindgen` + `js-sys`. It exposes a single `#[wasm_bindgen]` entry, `decode_frame(&[u8]) ->
Result<DecodedFrame, JsValue>`, that calls `justerm::decode` and presents the result as Axis-3's
flat-buffer-view shape. The core `justerm` crate is untouched — its dependency set and boundary
invariants hold.

- **Zero-copy lifetime.** `DecodedFrame` owns the decoded `Vec<u8>` cell buffer; `cells` is a view
  into it (`js_sys::Uint8Array::view` / a `ptr`+`len` pair). The view is valid only while WASM
  linear memory is not grown/reallocated, so the consumer must read it before the next decode call;
  this caveat is documented in the README.
- **Errors throw.** `DecodeError` maps to a thrown JS `Error` (variant name in the message) — the
  validation the decoder exists to provide surfaces loudly, never silently mis-parses.
- **Build + publish.** `wasm-pack` builds both `--target bundler` (PenTerm's Vite/webpack path) and
  `--target web` (no-bundler `<script type=module>`). Published to the **public npm registry**, its
  package version derived from the crate version (one coordinated bump moves crate + artifact).
- **Tested in CI.** `wasm-pack test --node` decodes the `tests/serialize.rs` golden fixtures (built
  with the native `justerm::encode`) through the WASM path and asserts the Frames match the native
  `decode` — the build-parity guard for AC2.

## Consequences

- **Drift eliminated structurally.** One `decode`, both sides; `wire_version()` lets the consumer
  assert encode/decode agreement at load. The TS-mirror maintenance burden and its un-fuzzed
  re-implementation both disappear.
- **Core purity preserved.** `justerm`'s three-dependency, no-IPC/no-render boundary is untouched;
  the wasm-only deps live entirely in the binding crate. Native test/fuzz/bench gates are unaffected.
- **Boundary held to the web.** The decoder stops at references (codepoint + colour ref); colour
  resolution, atlas mapping, and cursor draw remain the consumer adapter's per-cell loop — the same
  loop it would run regardless of WASM-vs-TS, so WASM does not add it. The win is ownership of the
  format/validation code, not the elimination of that loop.
- **Constant boundary cost.** The flat-view + span-directory shape keeps JS↔WASM crossings O(1) in
  cell count; the "rich objects across the boundary" failure mode is designed out.
- **Cost: a wasm toolchain in CI** (`wasm-pack`, a wasm32 target) and an npm publish step gated on
  the crate version. Bounded — decode-only export tree-shakes the parser out of the artifact.

## Alternatives considered

- **Hand-written TS decoder (status quo spike).** Rejected as the durable answer — it duplicates the
  format and drifts on `VERSION`, and re-implements an attacker-facing parser without ADR-0007's
  coverage. Retained only as PenTerm's *temporary* bridge until this artifact publishes (so PenTerm
  integration is not blocked on #34).
- **In-crate `wasm` feature.** Rejected — Axis 1: erodes core dependency hygiene and the boundary
  for no benefit a separate crate + workspace versioning doesn't already give.
- **Emit resolved beamterm cells.** Rejected — Axis 3 B: breaks theme/atlas-agnosticism and couples
  the engine to one renderer; the adapter resolves, the decoder does not.
- **Cell objects across the boundary.** Rejected — Axis 3 A: per-cell marshalling is the inefficiency
  AC3 forbids.

## Amendment (2026-06-19, v0.2.0) — cell exposure as structure-of-arrays + format-owned colour helpers

### What prompted it

The 0.1.0 shape above exposes cells as one packed `Uint8Array` of 18-byte records and documents the
byte offsets + the tagged-u32 colour encoding in the README for the consumer to hand-parse. That
re-opens, *at the cell level*, the exact drift the WASM decoder closed at the frame level: a
consumer's `DataView` offset reader and colour-tag unpacker are a hand-written mirror of
`encode_cell_record` / `encode_color` that breaks silently when the record changes. The same holds
for the xterm Indexed-16..255 cube/grayscale formula — a fixed standard (not a theme), so
reimplemented per consumer it drifts (a wrong level-table entry = wrong colours).

### Refined boundary (the durable principle)

The original "decode only; colour resolution stays the consumer's adapter" was too coarse. Sharper:

- **justerm owns every fixed *format* or *standard*** — the wire records, the tagged-u32 colour-ref
  encoding, and the xterm Indexed-16..255 formula. Hand-mirroring any of these in a consumer is
  drift; the package owns the single implementation.
- **The consumer owns every *theme value* and *render policy*** — the 16 base ANSI colours + default
  fg/bg (and their hex→u32 and name→index), the attribute interpretations (inverse, dim, hidden,
  bold→bright), the font atlas, and the cursor.
- `resolveRgb` sits exactly on the line: a *mechanical* function (justerm) that takes the consumer's
  *palette* as an argument (theme). justerm still never knows a hex value — it is handed them.

So "colour resolution = the consumer's" sharpens to "the *policy and values* are the consumer's; the
*mechanical mapping over fixed formats/standards* is justerm's."

### Decided API (v0.2.0) — supersedes the packed `Uint8Array` exposure of Axes 3–4 for the *JS surface*

(The `justerm::decode` reuse, validation, and span directory are unchanged.)

- **Cells as structure-of-arrays views** — `codepoints` / `fg` / `bg` as `Uint32Array`, `flags` /
  `extra` / `link` as `Uint16Array`; each a zero-copy view into WASM memory, alongside the `spans`
  directory. The consumer reads `frame.fg[i]` with no offset/stride knowledge — the typed-array type
  *is* the layout contract, so adding a future field does not break index reads. Chosen over
  packed+reader: no per-cell object allocation, and no hand-written offset reader to parity-test.
  (PenTerm confirmed it consumes cells in place — decode → resolve → build beamterm cell → discard —
  with no Worker transfer / re-send / cache that a single packed buffer would favour, and our views
  are not transferable out of WASM memory without a copy anyway.)
The colour/flag helpers follow one rule — *a JS mirror of a Rust format is allowed only where AC3's
per-cell no-crossing forces it; everything else is sourced from Rust (structural, no mirror)*:

- **`flags` — raw `Uint16Array` + Rust-exported bit constants** (`flags()` via `wasm-bindgen`, read
  once and cached: `flags[i] & F.BOLD`). The bit positions come straight from Rust `CellFlags`, so
  there is no JS mirror to drift. Which bits to act on (and how — bold→bright, skip the wide spacer,
  dim) stays render policy; the constants give only the bit positions.
- **`buildPalette(ansi: Uint32Array(16)) → Uint32Array(256)` — WASM (Rust).** Per-scheme, not
  per-cell, so AC3 does not force JS: it lives in Rust and returns an **owned** copy of the 256
  resolved indices (`0..15` = the supplied ANSI colours, `16..255` = the fixed xterm cube/grayscale
  formula). An owned copy, not a view — the palette outlives many decodes. The formula lives in Rust,
  covered by a Rust unit test against published xterm values — no JS mirror, no separate formula
  parity check. It takes **only** the 16 ANSI colours: the default fg/bg are not part of the 256
  (they are the tag-0 `Default` case), so the consumer keeps them and assembles the resolveRgb
  palette object itself — `{ colors: buildPalette(ansi16), defaultFg, defaultBg }`.
- **`resolveRgb(ref, palette, role) → 0xRRGGBB` (+ `decodeColorRef`) — JS, the *single* mirror.**
  Per-cell hot loop ⇒ AC3 forces JS. Pure, alloc-free: `Default` → role's default, `Indexed` →
  `palette[i]`, `Rgb` → passthrough. `role` (fg/bg) is required so `Default` picks the right default.
  The only Rust format it mirrors is the tagged-u32 colour-ref *encoding* (`>>24` / payload).
  *Excludes* inverse/dim/hidden/bold→bright — render policy the consumer applies afterward (e.g.
  "dim = −50% luminance vs alpha" cannot be an argument the way a palette can).

The public `.d.ts` exports `resolveRgb` / `decodeColorRef` / `buildPalette` / `flags()` with types —
that is the consumer's consumption contract.

### Consequences

- **Wire format unchanged by this ADR.** This change only affects how decoded data is *presented*
  to JS; it did not bump `WIRE_VERSION` (2 at the time of this ADR; later raised to 3 by #38's cursor
  fields). It is a breaking *JS API* change → npm **0.2.0** (the crates.io crate's Rust API
  is unaffected). Safe to make now: PenTerm has not yet integrated the WASM decoder (it bridges with a
  temporary TS decoder).
- **Parity obligation — one mirror only.** The sole JS mirror is `resolveRgb` / `decodeColorRef`'s
  tagged-u32 encoding, covered by a Rust↔JS parity test: a `wasm-bindgen` cross-import (a Rust test
  imports the JS helpers, encodes known `Color`s via `encode_color`, asserts the JS results agree),
  run in the existing `wasm-pack test --node` lane — no new JS test runner. Flags constants and
  `buildPalette` are Rust-sourced, so they need no parity test (a Rust unit test covers the xterm
  formula; the flag bits come from `CellFlags` directly).
- **Boundary refined, not breached.** justerm-wasm still never knows a hex value or a font atlas; it
  gained ownership of *fixed formats/standards* only — a sharpening of, not a departure from, the
  core's theme/renderer-agnostic invariants.

## Amendment (2026-06-26, v0.6.0) — crate renamed `justerm-wasm` → `justerm-wasm-decode`

The all-prefixed naming convention (ADR-0010, #100) renames this crate from **`justerm-wasm`** to
**`justerm-wasm-decode`** (and the core `justerm` → `justerm-core`). Nothing about the decision above
changes — the separate-crate packaging (Axis 1), the `justerm-core::decode` reuse (Axis 2), the
flat-buffer / structure-of-arrays output (Axis 3 + the v0.2.0 amendment), and the 18-byte stride
(Axis 4) all stand. The rename only makes the **decode-only scope explicit in the name**:

- `-decode` states that this crate is the *decoder* binding, distinguishing it from a future
  **`justerm-wasm-engine`** — a reserved sibling for an in-wasm `feed`/engine binding (the "in-wasm
  mode" this ADR's `encode`-stays-native scope deferred). That split was implicit before; the suffixes
  now name it.
- The npm package name follows the crate name automatically (wasm-pack derives it), so the published
  artifact becomes `justerm-wasm-decode`. The old npm `justerm-wasm` is `deprecate`d with a redirect
  message; see ADR-0010 for the tombstone strategy and the npm↔crates.io signal asymmetry.

The wire format and the JS API are **unchanged** by this rename — it is a packaging/name change only,
carried by the v0.6.0 version bump (the binding's first publish under the new name).
