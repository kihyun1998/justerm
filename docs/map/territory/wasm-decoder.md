# Territory — WASM decoder

## What it is

`justerm-wasm-decode`: the engine's own `decode` compiled to WASM and published to npm, so a web
consumer reads [wire format](wire-format.md) bytes with the same decoder the native side tests and
fuzzes, instead of a hand-written TypeScript mirror. It exposes one fallible entry, `decodeFrame`,
returning a `DecodedFrame` whose cell data are structure-of-arrays typed-array views over WASM memory
plus a span directory, and a handful of **format-level helpers** — the fixed formats and standards a
consumer would otherwise re-implement (`wireVersion`, `flags`, `underlineStyle`, `markerKind`,
`mouseEventBits`, `modifiedKeyBits`, `buildPalette`, `isValidRegex`, and the hand-written
`resolveRgb` / `decodeColorRef` in `colors.js`). It stops at *references*: colour values, the font
atlas and caret drawing stay the consumer's. GLOSSARY: "WASM decoder".

## Governing decisions

- [**ADR-0008 — WASM decode binding: a separate crate**](../../adr/0008-wasm-decode-binding-separate-crate.md)
  — the whole territory. Axis 1 separate crate (not a core feature), Axis 2 reuse core's `decode`
  verbatim, Axis 3 flat views + span directory (no per-cell boundary crossing), Axis 4 keep the wire
  stride; errors throw a JS `Error` carrying the variant name; version lockstep with core; a
  `wire_version()` export. Its v0.2.0 amendment moves cells to structure-of-arrays and draws the
  boundary as **"justerm owns every fixed format or standard; the consumer owns every theme value and
  render policy"**, with one permitted JS mirror (`resolveRgb`/`decodeColorRef`) because the per-cell
  loop forces it. Its v0.6.0 amendment renames `justerm-wasm` → `justerm-wasm-decode`, reserving
  `justerm-wasm-engine` for an in-wasm engine. Performance is an explicit non-goal.
- [ADR-0010 — all-prefixed crate naming](../../adr/0010-all-prefixed-crate-naming.md) — the rename
  and the npm `justerm-wasm` deprecation.
- [ADR-0005 — binary reference-based serialization](../../adr/0005-binary-reference-based-serialization.md)
  — the format it decodes; adjacent, owned by [wire format](wire-format.md).
- [ADR-0007 — robustness testing](../../adr/0007-robustness-testing-property-and-fuzz.md) — the
  coverage this crate inherits by reusing `decode`; it decides the testing, not the binding.

## Design model

Status: ADR-0008 decides the shape; the rest is read from `justerm-wasm-decode/src/lib.rs`, its tests
and the workflows at 7bb31f7.

- **Two layers in one file.** `flatten` turns a decoded `Frame` into a pure `Flat` (host-testable with
  plain `cargo test`, which `cargo test --workspace` runs); `DecodedFrame` is the thin
  `#[wasm_bindgen]` layer exposing `Flat`'s buffers. Only the second needs a wasm runtime.
- **One decoder, reused.** `decode_frame` calls `justerm_core::decode` and nothing else parses bytes,
  so format drift is structurally impossible and the no-panic property and fuzz coverage come with it.
  The cell record and colour packing are reused through core's `encode_color` — no second definition.
- **The JS shape is not the wire's shape.** Cells are de-interleaved into columns — `codepoints`,
  `fg`, `bg`, `underlineColor` (`Uint32Array`), `flags` (`Uint16Array`), `extra`, `link`
  (`Uint32Array`, widened from `u16` with the wire in #621/v14) — and `spans` is a stride-5 directory
  (`line, left, right, cell_offset, cell_count`). Where the wire is sparse the binding **densifies**:
  `underlineColor` reads `0` (Default) where absent, and since v14 inlined clusters on the wire, the
  binding re-interns them into a frame-local `sideTable` indexed 1-based by `extra`, keeping the JS
  contract unchanged. Overlay groups are stride-3 `(row, left, right)` columns (`OVERLAY_STRIDE`);
  markers are stride-5 `(id, row, kind, exitPresent, exitBits)` (`MARKER_STRIDE`).
- **Every column is a getter, and a view.** wasm-bindgen compiles each into an accessor that builds a
  new view (or, for `sideTable`/`linkTable`, a new `string[]`) per read, and a view is invalidated
  when WASM memory grows — so read each once into a local, copy anything kept past the frame, and
  `free()` the frame (README; PenTerm frees after its listeners run). The invariant note holds the
  measurements.
- **Scalars cross as primitives.** `evictedTotal` is `f64`, not `BigInt`: exact to 2^53, four orders
  of magnitude past a reachable eviction count, and a `BigInt` on one side of a subtraction is a
  `TypeError` (#490). `cursorShape` is `undefined` when unset (v17, #927).
- **A frame member is a primitive; a value space's names live at module scope (#860).** Named things
  — `Flags`, `UnderlineStyle`, `MarkerKind`, `MouseEventBits`, `ModifiedKeyBits` — are module-scope
  exports read once, never frame members, so `DecodedFrame` stays source-agnostic for the web mirror.
  A field that is a value, not a bit, ships as an accessor (`underlineStyle`), since no mask answers
  "which of six". The reasoning is [published surface](published-surface.md)'s.
- **Errors throw a real `Error` (#662).** `decode_frame` maps a `DecodeError` to
  `js_sys::Error::new(format!("{e:?}"))`, so `.message` is the variant (with its payload, e.g.
  `BadVersion(11)`) and a stack is present. ADR-0008 recorded this in 2026-06; the binding threw a
  string primitive for eleven releases until #662 measured it.
- **Version lockstep is a workspace inheritance, checked at publish.** `version.workspace = true`
  ties the crate to root `[workspace.package]` (with `justerm-core` and `justerm-pty`), so one bump
  moves all three; `publish = false` keeps it off crates.io, and `publish-wasm.yml` builds with
  `wasm-pack --target bundler`, folds in `colors.js`/licences (`scripts/finish-pkg.mjs`), and fails if
  the tag differs from the built package version. A `v*` tag publishes it — see [release](release.md).
- **`wireVersion()` is the load-time check, and the consumer runs it.** `justerm-web` neither decodes
  nor checks the version — it calls `buildPalette`, `flags` and `underlineStyle` and takes frames from
  a `FrameSource`. The host decodes: PenTerm asserts `wireVersion() === 18` before subscribing and
  frees each frame after its listeners (penterm 7dadc3c95
  `src/blocks/terminal/lib/nativeFrameSource.ts` — `createNativeFrameSource`).
- **Two consumers, two pins.** `justerm-web/package.json` depends on `justerm-wasm-decode` by caret
  range (`^0.22.0` at 7bb31f7 — on 0.x that resolves within 0.22.x only), and types the frame through
  its own hand-written `DecodedFrame` mirror in `src/types.ts`, deliberately `ArrayLike<number>` so
  plain-object fixtures satisfy it. PenTerm pins `justerm-wasm-decode` `0.30.0` exactly and overrides
  `justerm-web>justerm-wasm-decode` to the same (penterm 7dadc3c95 `package.json` —
  `pnpm.overrides`), with `justerm-core` `0.30` on its backend. The mirror's drift gate is
  `justerm-web/test/published-seam.types.ts` (#646), which runs under `pnpm typecheck` against the
  *published* package the lockfile resolves.
- **Outside input.** `decodeFrame` takes bytes off a consumer's transport, treated as
  attacker-influenced; everything it allocates is linear in the input (core's `decode` reserves only
  from a `u16` span count, and `flatten` sizes its columns by the decoded cells). `buildPalette` reads
  at most 16 entries; `isValidRegex` delegates to core's dialect check.

## Code

- `justerm-wasm-decode/src/lib.rs` — `Flat`, `flatten`, `flatten_overlay_spans`, `SPAN_STRIDE`, `OVERLAY_STRIDE`, `MARKER_STRIDE`, `DecodedFrame` (getters `codepoints`, `fg`, `bg`, `underline_color`, `flags`, `extra`, `link`, `spans`, `side_table`, `link_table`, `selection_spans`, `match_spans`, `active_match_spans`, `marker_positions`, `evicted_total`, …), `decode_frame`, `wire_version`, `is_valid_regex`, `Flags`, `flags`, `UnderlineStyle`, `underline_style`, `MarkerKind`, `marker_kind`, `published_kind`, `MouseEventBits`, `mouse_event_bits`, `ModifiedKeyBits`, `modified_key_bits`, `build_palette`
- `justerm-wasm-decode/js/colors.js` · `colors.d.ts` — `resolveRgb`, `decodeColorRef`, the one hand-written mirror
- `justerm-wasm-decode/tests/web.rs` — JS-boundary tests, `#![cfg(target_arch = "wasm32")]`, run by `wasm-pack test --node` in CI's `wasm` job (invisible to `cargo test --workspace`)
- `justerm-wasm-decode/tests/wire_enum_stays_exhaustive.rs` (#843) · `readme_pins.rs` — host tests, run by `--workspace`
- `justerm-wasm-decode/scripts/finish-pkg.mjs` · `smoke.mjs` · `smoke-decode.mjs` · `smoke-engine-frame.mjs`; `examples/gen_engine_frame.rs` · `gen_smoke_frame.rs`
- `Cargo.toml` (root) — the workspace.package version; `justerm-wasm-decode/Cargo.toml` — version.workspace, publish = false, crate-type
- `.github/workflows/test.yml` (`wasm` job) · `publish-wasm.yml`
- `justerm-web/src/types.ts` — `DecodedFrame` (the mirror); `justerm-web/test/published-seam.types.ts` — the seam gate

## Reference behaviour

**None** in `docs/agents/reference-facts.md`. ADR-0008 cross-checks the shape against named prior art
(beamterm's fixed-stride cell buffer, xterm.js addon-webgl's packed `uint32` cells, the
wasm-bindgen/wasm-pack ecosystem) by description, not pinned rows. The
[published surface](published-surface.md) note compares how Ruffle and Automerge consume their own
wasm output (no version-range mirror at all).

## Cross-cutting invariants

- [a decoded frame's columns are getters](../invariant/decoded-columns-are-getters.md) — this crate
  is the producer: every `#[wasm_bindgen(getter)]` it adds inherits both rules (read once into a
  local; copy what outlives the frame).
- [a wasm `Err` payload is thrown verbatim](../invariant/wasm-err-payload-is-thrown-verbatim.md) —
  `decode_frame` is the decoder's only fallible export, and its `js_sys::Error` construction is what
  decides every consumer's `catch`; a Rust-side `is_err()` cannot tell the shapes apart, only
  `tests/web.rs` can.
- [workspace exclusion is gate invisibility](../invariant/workspace-exclusion-is-gate-invisibility.md)
  — the target-gate sibling: `tests/web.rs` is `#![cfg(target_arch = "wasm32")]`, so `cargo test
  --workspace` compiles it to nothing and reports success; its assertions run only in CI's `wasm` job.
- [the renderer's copies of core encodings are held by convention](../invariant/renderer-encodings-are-held-by-convention.md)
  — the contrast case: this crate reuses `encode_color` and holds `colors.js` to it by test, which the renderer's copies lack

## Blast radius

- [wire format](wire-format.md) — a `WIRE_VERSION` bump or a new group must be flattened here, or it
  is invisible to every JS consumer; `wireVersion()` and `readme_pins.rs` assume the README quotes the
  shipped version.
- [frame](frame.md) — a new `Frame` field needs a getter; a getter that is not a primitive breaks the
  source-agnostic mirror.
- [published surface](published-surface.md) — getters, module-scope exports and their `///` prose
  ship verbatim into the generated `.d.ts`; `types.ts` mirrors them by hand and the seam gate checks
  only the published version the lockfile holds.
- [release](release.md) — a `v*` tag publishes this package with core; the web widget sees a change
  only after its caret range moves, and PenTerm only after its exact pin moves.
- [frame adapter](frame-adapter.md) · [cell compositing](cell-compositing.md) — consume the columns'
  widths (`extra` as `u32` since #621/#627); a width change here must still feed the renderer's
  parameters.
- [widget lifecycle](widget-lifecycle.md) — `JustermRenderer` imports this package dynamically for
  `buildPalette`/`flags`/`underlineStyle`; a module-scope export renamed here breaks construction.
- [colour policy](colour-policy.md) — `buildPalette`'s 16..255 formula and `resolveRgb`'s tag decoding
  are the fixed standard every palette resolution assumes.

## Known holes / open

- **`types.ts` is a hand-written copy of the getter list**, kept honest only by
  `published-seam.types.ts` against whatever version the web lockfile resolves (0.22.0 at 7bb31f7,
  eight minors behind core). A getter added to a newer decoder is `undefined` in the widget's types
  until both the range moves and someone mirrors it.
- **The web package's own range lags the version every real consumer runs.** `^0.22.0` cannot
  resolve 0.30.0, so PenTerm needs a `pnpm.overrides` entry to give the widget the decoder it pins;
  the web's own tests and e2e run on 0.22.0. Not a live defect: decoder code has not changed since
  the range was taken (4ed55d3; 11f1fbb touched only comments in `serialize.rs`), so both versions
  are one decoder; the gap opens with the next change
  to `justerm-wasm-decode/src` or `serialize.rs`.
- **Nothing in the family checks `wireVersion()` for the consumer.** ADR-0008 offers it as optional;
  the widget does not call it, so a consumer that skips the check gets `BadVersion` per frame instead
  of one load-time failure. PenTerm checks it, and then drops any frame `decodeFrame` throws on
  without reading the error (`createNativeFrameSource`'s empty `catch`), relying on the next frame to
  repaint.
- **The renderer's fallible exports still throw bare strings** (the invariant's other territories);
  only this crate's one export was fixed by #662.
- **`tests/web.rs` runs only in CI's `wasm` job** — a local `cargo test --workspace` compiles it to
  nothing, so a boundary regression is first seen there or under a local `wasm-pack test --node`.
