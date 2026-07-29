# Territory — wire format

## What it is

The bytes a [frame](frame.md) travels in, and the two functions that round-trip it. The engine
provides the *format* and **both directions**; the *transport* is the consumer's — justerm has no
IPC by identity.

## Governing decisions

- [**ADR-0005 — binary reference-based serialization**](../../adr/0005-binary-reference-based-serialization.md)
  — binary, little-endian, **fixed-width 18-byte cell records**, colour *references* not resolved
  RGB, plus a grapheme side-table. Rejects Mosh's protobuf baseline-diff and xterm.js's
  escape-sequence re-emit (which a non-parsing GPU renderer cannot consume)
- [**ADR-0008 — wasm-decode as a separate crate**](../../adr/0008-wasm-decode-binding-separate-crate.md)
  — who decodes on the other side, and why it is its own crate on the same version lockstep
- Each of ADR-0013 / 0014 / 0015 / 0016 records a `WIRE_VERSION` bump — the format's changelog is the
  ADR sequence, not a file
- `docs/architecture.md` §Serialization holds the field-by-field byte arithmetic

## Design model

- **Fixed stride is the whole point.** `CELL_RECORD_LEN = 18`, every cell the same width, so a
  consumer takes **one contiguous typed-array view** with no per-field parse. Every other decision
  here defends that property.
- **Anything variable-length becomes a side-table with a frame-local index.** Grapheme clusters
  (`side_table`) and OSC 8 URIs (`link_table`) are referenced by a `u16` in the cell record —
  the wire's version of the same escape hatch the in-memory rows use.
- **Colours are references, never hex.** `Default | Indexed(u8) | Rgb(..)` encoded as a `u32`. The
  engine is theme-agnostic by identity, so palette resolution happens after decode, in the consumer.
- **The record reserves room** for underline style/colour and a hyperlink id, so adding them later is
  not a format change.
- **`encode` and `decode` both live in core**, which is what makes the round-trip testable without a
  consumer — the format is a *contract*, and a contract only one side can execute is untested.
- **`WIRE_VERSION` is a single `u8`** and a decoder rejects a mismatch outright (`DecodeError`).
  There is no negotiation and no backward compatibility: version and payload move together.

## Code

- `justerm-core/src/serialize.rs` — `WIRE_VERSION`, `CELL_RECORD_LEN`, `encode`, `decode`,
  `encode_cell_record`, `encode_color`, `DecodeError`
- `justerm-wasm-decode/` — the JS-side decoder (ADR-0008), version-locked to core
- `justerm-web/src/types.ts` — `DecodedFrame`, a **hand-written mirror** of the wasm getters

## Reference behaviour

One axis only —
[per-cell payload length](../../agents/reference-facts.md#per-cell-payload-length--nobody-caps-it-but-one-fails-loudly-621-verified-2026-07-29):
no reference caps a grapheme cluster or a URI, and the one whose storage *can* run out returns a
named error rather than truncating. That bears on #621.

**The choice of format itself is still uncompared.** ADR-0005 argues against Mosh and xterm.js by
description rather than by pinned rows, so the comparison that picked this shape has never been
re-checked against those sources — and the row above does not touch it. Read the section as covering
one measured question, not the territory.

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — the grapheme table, the link table and
  the underline-colour group are this format's instance of the same "the fixed record is full, put it
  beside and gate it with a bit" pattern the in-memory rows use. Its **rule 4 is this format's own**:
  the gating bit is not encodable, so `decode` reconstructs every presence bit from group membership,
  and a group added without a re-arm ships a value its gate hides

## Blast radius

**A `WIRE_VERSION` bump is the highest-consequence change in the repo.** Registries are immutable, and
a consumer decoding a wrong layout gets **garbage cells, not an error** — which is why this is an
unconditional Step 5 trigger.

- [release & published surface](release-and-published-surface.md) — a bump ships on `v*`, and that
  tag fires **two** publishes at once (crates.io + npm)
- [frame](frame.md) — the shape being encoded; a new group is a new version
- [frame adapter](frame-adapter.md) — decodes into GPU buffers and never parses escape sequences, which is
  the constraint that ruled out re-emit
- `justerm-web/src/types.ts` — hand-written, so a new field is `undefined` in TypeScript until
  someone mirrors it **and** the wasm package is republished

## Known holes / open

- **`types.ts` is an ungated copy of the wasm surface.** A field can exist in core, ship in the
  binding, and be invisible to the web widget with no error anywhere — the failure is a silent
  `undefined`.
- **No reference comparison of the format choice** (§Reference behaviour) — the alternatives were
  rejected from description, and the one axis now measured is unrelated to that decision.
- **No compatibility story is recorded.** "Version and payload move together, mismatch is rejected"
  is the implemented behaviour; nothing states whether that is a decision or an interim position.
- **The format cannot carry every value the engine legitimately holds, and says so only at decode.**
  The grapheme-cluster and OSC 8 URI runs ride `u16` length prefixes with nothing bounding the
  producing side, so a cluster past `u16::MAX` encodes and then fails its own `decode` — measured,
  and filed as #621. Note this is *not* the "is this input malformed" question the span-bounds work
  asks: the engine's value is correct and the field is too small for it. The two look alike and a
  fix for one does not reach the other.
