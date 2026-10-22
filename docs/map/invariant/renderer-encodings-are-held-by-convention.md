# Invariant — the renderer re-declares core's encodings with no dependency, so only convention keeps them equal

## The fact

`justerm-renderer` does not depend on `justerm-core` (`justerm-renderer/Cargo.toml` names no justerm
crate). It still reads words that core packed, through the decoder's columns, so it carries its own
copy of each encoding. Nothing compiles or tests those copies against core's; a change on core's side
is a silent misreading on the renderer's until both move.

Copies at HEAD 7bb31f7, and which are gated:

| Encoding | Core (source of truth) | Copy | Gate |
|---|---|---|---|
| Cell flag bits | `justerm-core/src/cell.rs — CellFlags` (`BOLD` … `WIDE_CHAR_SPACER`) | `justerm-renderer/src/attrs.rs` — `BOLD`, `DIM`, `ITALIC`, `UNDERLINE`, `BLINK`, `INVERSE`, `HIDDEN`, `STRIKETHROUGH`, `WIDE_CHAR`, `WIDE_CHAR_SPACER` | **none** |
| Underline style field (bits 11..=13) | `cell.rs — CF_USTYLE_SHIFT` / `CellFlags::underline_style` | `attrs.rs — USTYLE_SHIFT`, `USTYLE_MASK` | **none** (the recorded duplication — `attrs.rs` and `justerm-wasm-decode`'s `underline_style` doc both say so) |
| Colour reference tag (`0` Default, `1` Indexed, `2` Rgb, high byte) | `justerm-core/src/serialize.rs — encode_color` | `justerm-renderer/src/render_policy.rs — resolve_slot` (tag `0`), `palette.rs — resolve_indexed_or_rgb` (tags `1`/else) | **none** |
| Colour reference tag | same | `justerm-wasm-decode/js/colors.js` (`decodeColorRef`, `resolveRgb`) | **gated**: `justerm-wasm-decode/tests/web.rs — decode_color_ref_matches_rust_encoding`, `resolve_rgb_matches_rust_encoding` feed core's `encode_color` output to the JS |
| Cell record / colour packing in the decoder | `encode_color` | none — `justerm-wasm-decode` depends on core and calls it | **by construction** |
| Character width | `unicode-width = "0.2"` in `justerm-core/Cargo.toml` | `unicode-width = "0.2"` in `justerm-renderer/Cargo.toml`, for preedit width (ADR-0028) | **none** — two lockfiles (`Cargo.lock`, `justerm-renderer/Cargo.lock`), both resolving 0.2.2 today; the manifest comment asks they not drift |

So the gated copies are the ones in a crate that depends on core; every copy in the renderer is not.

## Why it is cross-cutting

The copies sit in unrelated renderer modules — flag decoding for compositing, colour resolution for
policy, width for the preedit — and none reads another. They share one cause, which no edge between two
territories could carry: the renderer is a separately published crate outside the workspace
([ADR-0018](../../adr/0018-justerm-renderer.md); the root `Cargo.toml` excludes it because its web-sys/glow
dependencies are wasm32-only), and making it depend on core to share the constants is a decision
nobody has taken. Each copy's comment records its own
duplication; none records that the others exist.

## Territories it holds in

- [colour policy](../territory/colour-policy.md) — the colour tag copy (`resolve_slot`,
  `resolve_indexed_or_rgb`) and the flag-bit mirror it reads; its Known holes records the missing
  parity test.
- [cell compositing](../territory/cell-compositing.md) — reads the flag bits and the underline style
  field from `attrs.rs` to build each instance.
- [wire format](../territory/wire-format.md) — owns the source of truth (`encode_color`, the flag word
  carried in each cell record) that every copy mirrors.
- [WASM decoder](../territory/wasm-decoder.md) — the contrast case: depends on core, reuses
  `encode_color`, and holds its JS colour helpers to it by test.

## What a violation looks like

No error anywhere. A flag bit moved or added in core draws as the wrong attribute or nothing in the
renderer (bold as dim, a new flag a no-op); a new colour tag reads as `Rgb` (`resolve_indexed_or_rgb`'s
fall-through) and draws black or a wrong colour; diverging width tables put the preedit caret one cell
off. Core's own tests, the wasm decoder's parity tests and the renderer's unit tests all stay green,
because each tests its own copy.

## Discovery history

- **#36** (2026-06-22, `aa870fa`) — the decoder's JS colour helpers got their parity tests (`// --- #36: colour-helper parity`
  in `justerm-wasm-decode/tests/web.rs`), the one copy that is gated.
- **#829** (2026-09-04, `fcdcb7f`) — the underline style field reached the renderer and its re-declared
  position became the recorded duplication.
- Reading for this map (2026-10-22): the per-copy comments and the colour-policy hole were each true
  locally; the table above is the first place they are counted together.

## Where it will recur

Any change to `CellFlags`, the colour tag in `encode_color`, the cell record, or the `unicode-width`
version in core: grep `justerm-renderer/src` for the renderer's copy and move it in the same change.
Any new renderer feature that reads a column core packed: it needs its own copy too, and is subject to
this until the renderer depends on core or a parity test spans the two.
