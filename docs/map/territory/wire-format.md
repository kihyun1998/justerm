# Territory — wire format

## What it is

The bytes a [frame](frame.md) travels in, and the two functions that round-trip it. The engine
provides the *format* and **both directions**; the *transport* is the consumer's — justerm has no
IPC by identity.

## Governing decisions

- [**ADR-0005 — binary reference-based serialization**](../../adr/0005-binary-reference-based-serialization.md)
  — binary, little-endian, **fixed-width cell records** (18 bytes when 0005 was written; 14 since
  v14/#621), colour *references* not resolved
  RGB. Rejects Mosh's protobuf baseline-diff and xterm.js's
  escape-sequence re-emit (which a non-parsing GPU renderer cannot consume)
- [**ADR-0008 — wasm-decode as a separate crate**](../../adr/0008-wasm-decode-binding-separate-crate.md)
  — who decodes on the other side, and why it is its own crate on the same version lockstep
- Each of ADR-0013 / 0014 / 0015 / 0016 records a `WIRE_VERSION` bump — the format's changelog is the
  ADR sequence, not a file
- `docs/architecture.md` §Serialization holds the field-by-field byte arithmetic

## Design model

- **Fixed stride is the whole point.** `CELL_RECORD_LEN = 14`, every cell the same width, so a
  consumer takes **one contiguous typed-array view** with no per-field parse. Every other decision
  here defends that property. It was 18 until v14 (#621) — the record shrank because the two
  variable-length references left it, not because a field was dropped.
- **Anything rare or variable-length becomes a sparse per-span group, keyed by column** — combining
  clusters, OSC 8 link references and underline colours all ride one. The record carries only what
  *every* cell has. Whether a group inlines its value or points into a frame-local table is decided
  per group by whether cells share it: a URI is shared, so `link_table` stays and is interned; a
  cluster is not, so it is inlined and there is no table (#621, measured both ways).
- **A group's count is `u32` iff the group is viewport-bounded.** The three overlay span groups and
  the two new per-span groups scale with the viewport, and the header admits a viewport far larger
  than `u16::MAX` cells, so their counts are u32. The two *marker* groups keep `u16` counts on
  purpose: they report every live marker rather than a viewport projection, which is the one
  ADR-0020 R3 violation the ADR records against itself, and widening them would make it cheaper to
  keep (that is #490's, not this file's).
- **Colours are references, never hex.** `Default | Indexed(u8) | Rgb(..)` encoded as a `u32`. The
  engine is theme-agnostic by identity, so palette resolution happens after decode, in the consumer.
- **The record reserves room** for underline style/colour and a hyperlink id, so adding them later is
  not a format change.
- **`encode` and `decode` both live in core**, which is what makes the round-trip testable without a
  consumer — the format is a *contract*, and a contract only one side can execute is untested.
- **`WIRE_VERSION` is a single `u8`** and a decoder rejects a mismatch outright (`DecodeError`).
  There is no negotiation and no backward compatibility: version and payload move together.
- **Payload placement is validated against the frame's own header; annotations are not (#582).** A
  span, a sparse group key and the scroll region name cells a consumer *writes to*, so a value
  outside the declared `cols`/`rows` is `BadSpan` — it is malformed input, not a repairable one.
  Overlay spans, marker rows and the cursor name positions a consumer *scans*, so they ride
  unchecked and clamping them is consumer policy (ADR-0017). The rule is one-directional by
  construction: `encode` returns `Vec<u8>` and has no channel to refuse, so it drops an
  unrepresentable group key rather than narrowing it onto a live column, and asserts in debug.

## Code

- `justerm-core/src/serialize.rs` — `WIRE_VERSION`, `CELL_RECORD_LEN`, `encode`, `decode`,
  `encode_cell_record`, `encode_color`, `DecodeError`
- `justerm-wasm-decode/` — the JS-side decoder (ADR-0008), version-locked to core
- `justerm-web/src/types.ts` — `DecodedFrame`, a **hand-written mirror** of the wasm getters

## Reference behaviour

One axis only —
[per-cell payload length](../../agents/reference-facts.md#per-cell-payload-length--nobody-caps-a-cluster-the-one-that-can-run-out-grows-and-a-uri-is-a-different-answer-621-verified-2026-07-29):
no reference caps a grapheme **cluster**, and the one whose storage *can* run out **grows until
the payload fits** rather than truncating or failing. The **URI** axis is different and was
originally recorded wrong here: xterm.js *does* cap an OSC payload, at 10 000 000 chars, discarding
the whole sequence silently. Neither bound is near `u16::MAX`, so #621's direction is unaffected —
but read the correction notes in that section before citing it. Its first version concluded the
opposite of its own citation, and its second extended a cluster-only finding to URIs without a
URI-side row.

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
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — the producer-side half of the *"`u32` iff viewport-bounded"* rule stated above. That rule picks a
  field's **width**; the note says who is obliged to keep the value inside it, and the answer is
  never this file — `encode` returns `Vec<u8>` and has no way to refuse

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
- **~~The format cannot carry every value the engine legitimately holds~~ — closed by #621 (v14).**
  Length prefixes and viewport-bounded group counts are `u32`, and the two per-cell indices that
  could not be widened without inflating every record were removed from it instead. Kept as a hole
  only in this sense: it was *not* the "is this input malformed" question the span-bounds work asks
  (#582), the two look alike, and a fix for one still does not reach the other.
- **~~`decode` accepts a payload that does not fit the frame it rides on~~ — closed by #582.** Spans,
  sparse group keys and the scroll region are now read against the header's own `cols`/`rows`
  (`BadSpan`), and `encode` drops rather than narrows an out-of-span group key. **The residue was
  the part that is not validation, and it is closed by #661**: `ScrollOp.count` is `isize` in memory
  and `i16` on the wire, and the accumulator had no cap, so a single 32 KB `feed()` of newlines
  encoded `32768` as `−32768` — an up-scroll arriving as a down-scroll on a `Partial` frame. Fixed
  at the producer (`Term::scroll_delta` caps at the region height and at `MAX_SCROLL_COUNT`), because
  a bound in `decode` would have rejected a frame the engine emits. **What is still open is the
  mirror of that**: `decode` accepts an over-height `count` from a *foreign* frame, which is now
  possible to reject without rejecting our own output — left out of #661 as new strictness on a
  published decoder, the same standalone argument as #663.
- **The header's own `cols`/`rows` are unchecked, and the two ends of that are different problems.**
  Four consumer sites size themselves straight from the header — `accessibility-dom.ts` allocates a
  `cols × rows` mirror and a per-row array, `accessibility.ts` builds one DOM element per row and
  loops rows twice — so the widget has no stated stance on a hostile or corrupt frame while `decode`
  explicitly has one.
  **The ceiling is not fixable and the map should stop anyone trying.** No reference bounds a grid's
  upper end (xterm.js has `MINIMUM_COLS`/`MINIMUM_ROWS` and no maximum), and the one layer that
  *could* know the limit — `justerm-renderer` — refuses to guess it, asking the GL implementation and
  adopting what it grants. A "sane" cap in the widget would be the only arbitrary constant in the
  stack. Rows in
  [reference-facts § validating a decoded payload](../../agents/reference-facts.md#validating-a-decoded-payload-against-its-own-declared-geometry-582-verified-2026-07-31).
  **The floor is fixable and non-arbitrary**: 2 columns is agreed by xterm.js (*"Less than 2 can mess
  with wide chars"*) and by justerm's own `MIN_COLUMNS` (#547), enforced at every engine entry point,
  while `decode` accepts `cols: 0` and `cols: 1` — **#663**, kept out of #582 because the two rest on
  different arguments (a write index a consumer walks off, vs a geometry the engine calls impossible).
- **The `ucolors` group count is still `u16`** while its two sibling per-span groups went `u32` in
  #621 — a standing exception to the *"`u32` iff viewport-bounded"* rule stated above. **Measured
  after #582: not reachable.** Rejecting `right >= cols` caps a decodable span at 65 535 cells, one
  short of the 65 536 in-range keys the count needs to wrap, and the frame that would carry them
  (`right = 65535`) is now `BadSpan`. A `Span` that *lies* about its own width (`cells` longer than
  `right - left + 1`; nothing in the type ties them) can still make `encode` write a wrapped count,
  but the instance measured decoded as `Truncated`, not as a silent `Ok`. **Not searched**: whether a
  tuned inconsistent span lands on a silent `Ok`. Left as an inconsistency with no reachable
  consequence rather than a defect.
