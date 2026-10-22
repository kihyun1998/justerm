# Territory — wire format

## What it is

The bytes a [frame](frame.md) travels in, and the two functions that round-trip it: `encode` and
`decode`, both in `justerm-core`. The engine provides the *format* and **both directions**; the
*transport* is the consumer's — justerm has no IPC by identity. The JS-side reader of these bytes is
its own territory, [WASM decoder](wasm-decoder.md).

## Governing decisions

- [**ADR-0005 — binary reference-based serialization**](../../adr/0005-binary-reference-based-serialization.md)
  — binary, little-endian, **fixed-width cell records** (18 bytes when 0005 was written; 14 since
  v14/#621), colour *references* not resolved RGB. Rejects Mosh's protobuf baseline-diff and
  xterm.js's escape-sequence re-emit (which a non-parsing GPU renderer cannot consume).
- [**ADR-0008 — wasm-decode as a separate crate**](../../adr/0008-wasm-decode-binding-separate-crate.md)
  — who decodes on the other side, that `decode` is the boundary that rejects malformed input, and
  the version lockstep.
- ADR-0013 / 0014 / 0015 / 0016 each record a `WIRE_VERSION` bump; v9–v18 have none, and the
  format's full changelog is the comment on `VERSION` in `serialize.rs`, not a file.
- [ADR-0020](../../adr/0020-what-qualifies-for-the-frame-snapshot.md) decides *what* may ride; it
  is the [frame](frame.md)'s decision, adjacent here only through group counts (R3).
- `docs/architecture.md` § Serialization holds the field-by-field byte arithmetic.

## Design model

Source: ADR-0005/0008 for the shape; the remaining rules from `serialize.rs`'s doc-comments and the
tests named under `## Code` (read at 7bb31f7).

- **Fixed stride is the whole point.** `CELL_RECORD_LEN = 14` (`c` u32 · `fg` u32 · `bg` u32 ·
  `flags` u16), every cell the same width, so a consumer takes **one contiguous typed-array view**
  with no per-field parse. Every other decision here defends that property. It was 18 until v14
  (#621) — the record shrank because the two variable-length references left it, not because a field
  was dropped (measured −20.9 % on an ordinary frame, `encode_cell_record` doc).
- **Anything rare or variable-length becomes a sparse per-span group, keyed by span-relative
  column** — combining clusters, OSC 8 link references and underline colours. The record carries only
  what *every* cell has. Whether a group inlines its value or points into a frame-local table is
  decided per group by whether cells share it: a URI is shared, so `link_table` stays and is interned
  (inlining measured +171…403 %); a cluster is not, so it is inlined with no table (−0.5 %) (#621,
  `Span` doc).
- **Presence bits never travel as bits.** `C_COMBINED`, `LINK_PRESENT` and `UCOLOR_PRESENT` live in
  the content and colour words that `Cell::c`/`encode_color` strip, so `decode` re-arms each from its
  group's entries (#531). A new group added without a re-arm ships a value its gate hides.
- **A count's width follows what bounds it.** The span count is `u16` (one span per damaged line ≤
  `MAX_ROWS`). The link-table count, URI length, combining count/length and link count are `u32`
  (#621: the engine could hold more than `u16` described). The three viewport-projected overlay
  groups (`selection`, `matches`, `active_match`) have `u32` counts (#621 — a one-character search at
  1000×133 produced 66 000 spans, wrapped to 464, and `decode` returned `Ok` with fabricated groups
  after it, `encode_overlay_spans` doc). The **marker group keeps a `u16` count** although it is not
  viewport-bounded (several marks share a row): what makes it safe is a bound on the *producer* —
  `MAX_MARKERS` caps a buffer's live population at `u16::MAX`, and only the active buffer is
  projected (#721). The absolute-line marker group left in v16 (#490); the remaining group measured
  0–36 records where the departed one carried 2 000–8 000 (#490, attributed). The **`ucolors` count
  is also `u16`** (see Known holes).
- **A header scalar may exist to shrink the frame.** v15's `evicted_total`/`marker_epoch` and v16's
  `marker_count` are the price of the absolute-line group leaving — a group removed by adding the
  smallest thing that lets the consumer reconstruct it ([frame](frame.md)).
- **Colours are references, never hex.** `Default | Indexed(u8) | Rgb(..)` packed by `encode_color`
  as a tag byte plus 24-bit payload, so `Default`, `Indexed(0)` and `Rgb(0,0,0)` stay distinct.
  Palette resolution happens after decode, in the consumer.
- **The record reserved room, and the prediction was paid out.** The underline **style** spent
  `flags` bits 11–13 in #829 with **no record change and no version bump** — the `VERSION`
  doc-comment records why: the bytes did not change, only their meaning; an older decoder degrades a
  styled underline to a plain one rather than mis-reading it. The underline *colour* went a different
  way: a colour reference, not a fixed-size attribute, so #520 gave it a sparse group (v13). `flags`
  bits 14–15 are still marked reserved for a hyperlink id (`cell.rs` comment).
- **`encode` and `decode` both live in core**, so the round-trip is testable without a consumer — a
  contract only one side can execute is untested. The JS decoder reuses `encode_cell_record` and
  `encode_color` rather than re-implementing the layout ([WASM decoder](wasm-decoder.md)).
- **`WIRE_VERSION` is a single `u8`** and `decode` rejects any other value outright
  (`DecodeError::BadVersion`). No negotiation, no backward compatibility: version and payload move
  together. Rule: a feature bumps it when it changes the bytes, not when it changes the meaning
  (`VERSION` doc, #829/#830/#831).
- **Payload placement is validated against the frame's own header; annotations are not (#582).** A
  span, a sparse group key and the scroll region name cells a consumer *writes to*
  (`justerm-web/src/cell-mirror.ts` assigns `cells[y * cols + x]`), so a value outside the declared
  `cols`/`rows` is `BadSpan` — malformed input, not repairable. An empty region (`top > bottom`) rides
  through. Overlay spans, marker rows and the cursor name positions a consumer *scans*, so they ride
  unchecked and clamping them is consumer policy (ADR-0017). The rule is one-directional: `encode`
  returns `Vec<u8>` and cannot refuse, so it **drops** an out-of-span group key rather than narrowing
  it onto a live column (measured: key 65539 armed column 3), with a `debug_assert` as detector.
- **The header answers to the engine's floor, one comparison earlier (#663).** `cols < MIN_COLUMNS`
  or `rows == 0` is `BadGeometry`, its own variant because the comparison points the other way
  (header against engine, not part against header), and it is checked before any span. Impossibility
  does not select these two fields: `display_offset > scrollback_len` and an out-of-grid cursor are
  impossible too and correctly ride through, because they are annotations; `cols`/`rows` are the
  reference frame the payload rule measures against. **Rejected, not clamped, and not because of
  ownership** — the references split 2–1 with all three at a site they own (alacritty and xterm.js
  clamp, ghostty refuses), so ownership separates nothing. It is rejected because this boundary
  cannot repair: widening `cols` re-indexes a frame instead of fixing it. The **ceiling is not a
  check**: `cols`/`rows` are `u16` and `MAX_COLUMNS`/`MAX_ROWS` are `u16::MAX`, so the field bounds
  it.
- **`ScrollOp.count` is capped at the producer, not at decode (#661).** `isize` in memory, `i16` on
  the wire; `Term::scroll_delta` caps it at the region's height and `MAX_SCROLL_COUNT`, because a
  wrapped count arrives with the opposite sign (a 32 KB `feed` of newlines encoded 32 768 as
  −32 768). `decode` still accepts an over-height count from a foreign frame — measured harmless,
  because every consumer bounds the *source* row against `[top, bottom]` before indexing
  (`decode` comment).
- **Outside input.** `decode` reads bytes a consumer hands back over its own transport, treated as
  attacker-influenced (`tests/robustness.rs`). Its allocation is bounded by the input's own length:
  the `u32` counts deliberately have no `Vec::with_capacity`, so a 12-byte buffer cannot declare
  4 billion entries into an OOM. Only the span vector reserves up front, from a `u16` count.

## Code

- `justerm-core/src/serialize.rs` — `WIRE_VERSION`, `CELL_RECORD_LEN`, `MAX_SCROLL_COUNT`, `encode`, `decode`, `encode_cell_record`, `encode_color`, `encode_overlay_spans`, `decode_overlay_spans`, `DecodeError`
- `justerm-core/src/term.rs` — `MIN_COLUMNS`, `MAX_COLUMNS`, `MAX_ROWS`, `MAX_MARKERS`
- `justerm-core/tests/serialize.rs` · `wire_capacity.rs` (#621 capacity) · `span_bounds.rs` (#582 placement) · `header_geometry_floor.rs` (#663) · `robustness.rs` (decode from arbitrary bytes)
- `justerm-wasm-decode/tests/wire_enum_stays_exhaustive.rs` — the executable roster of enums whose members are mapped onto wire values outside core

## Reference behaviour

- [Per-cell payload length](../../agents/reference-facts.md#per-cell-payload-length--three-of-four-leave-a-cluster-unbounded-the-binding-one-caps-it-and-a-uri-is-a-different-answer-621-verified-2026-07-29-xterm-row-added-867-2026-09-08)
  — three of the four pinned trees leave a grapheme cluster unbounded; xterm, the binding one, caps a
  cell's stored marks at 2 by default but discards `ZWJ` before that store; xterm.js caps an OSC
  payload at 10 000 000 chars. No bound is near `u16::MAX`, so #621's `u32` widening stands. This
  note said "no reference caps a cluster" until #867 added the xterm row. Since #1038 the engine
  caps both (`MAX_CLUSTER_TAIL`, `MAX_LINK_URI`) for memory, not for the wire; the `u32` prefixes
  remain the wire's contract with any producer, which is why `wire_capacity.rs` lengthens its cluster
  on an engine frame rather than through `feed`.
- [Validating a decoded payload against its own declared geometry](../../agents/reference-facts.md#validating-a-decoded-payload-against-its-own-declared-geometry-582-verified-2026-07-31)
  — no reference decodes a serialized grid; the rows settle the floor value (xterm.js
  `MINIMUM_COLS = 2`) and that no reference bounds a grid's upper end.
- [Scrolling a region by more than its own height](../../agents/reference-facts.md#scrolling-a-region-by-more-than-its-own-height-661-verified-2026-07-31)
  — the #661 producer cap.

**The choice of format itself is uncompared.** ADR-0005 argues against Mosh and xterm.js by
description, not pinned rows; the rows above answer narrower questions.

## Cross-cutting invariants

- [row-keyed side maps](../invariant/row-keyed-side-maps.md) — the combining, link and
  underline-colour groups are this format's instance of "the fixed record is full, put it beside and
  gate it with a bit". Its rule 4 is this format's own: the gating bit is not encodable, so `decode`
  reconstructs every presence bit from group membership.
- [a wasm `Err` payload is thrown verbatim](../invariant/wasm-err-payload-is-thrown-verbatim.md) —
  `DecodeError`'s variant name is the entire diagnostic a JS consumer gets (ADR-0008 decided a JS
  `Error`, variant name in the message; the binding shipped a string primitive for eleven releases,
  #662).
- [a wire field narrower than the value it carries](../invariant/wire-field-narrower-than-its-value.md)
  — the producer-side half of the count-width rule: the rule picks a field's width; keeping the value
  inside it is never this file's job, since `encode` cannot refuse.
- [the renderer's copies of core encodings are held by convention](../invariant/renderer-encodings-are-held-by-convention.md)
  — `encode_color` and the cell flag word are the source of truth the renderer copies without depending on core

## Blast radius

**A `WIRE_VERSION` bump is the highest-consequence change in the repo.** Published registries are
immutable, and a consumer decoding a wrong layout gets garbage cells, not an error — which is why the
bump is gated by a byte rather than by semver.

- [release](release.md) · [published surface](published-surface.md) — a bump ships on `v*`, which
  publishes the crate and the npm decoder at once; the two must agree on the layout.
- [WASM decoder](wasm-decoder.md) — re-reads every field and asserts `wireVersion()` against core's;
  a field added here and not there is invisible to every JS consumer.
- [frame](frame.md) — the shape being encoded; a new group is a new version.
- [frame adapter](frame-adapter.md) — decodes into GPU buffers and never parses escape sequences,
  the constraint that ruled out re-emit; it validates the same geometry independently.
- [widget lifecycle](widget-lifecycle.md) · [accessibility](accessibility.md) — `cell-mirror.ts`,
  `accessibility-dom.ts` (`CellMirror` from `frame.cols`/`frame.rows`) and `accessibility.ts`
  (`tree.resize(frame.rows)`) size themselves from the header, trusting `decode` to have bounded it.

## Known holes / open

- **No compatibility story is recorded.** "Version and payload move together, mismatch is rejected"
  is the implemented behaviour; nothing states whether that is a decision or an interim position.
  #829 records the cost of *not* bumping: a mixed-version npm install silently flattens styled
  underlines.
- **The header's `cols`/`rows` are unchecked upward, and that is not fixable here.** Consumer sites
  size themselves straight from the header, so the widget has no stated stance on a hostile frame at
  the top end. No reference bounds a grid's upper end, and the renderer asks the GL implementation
  rather than guessing; a "sane" cap would be the only arbitrary constant in the stack.
- **The `ucolors` group count is `u16`** while its sibling per-span groups went `u32` in #621.
  Measured after #582: not reachable — rejecting `right >= cols` caps a decodable span at 65 535
  cells, one short of a wrap. A `Span` whose `cells` is longer than `right - left + 1` (nothing in the
  type ties them) can make `encode` write a wrapped count; the measured instance decoded as
  `Truncated`. Not searched: whether a tuned inconsistent span lands on a silent `Ok`.
- **No reference comparison of the format choice** (§ Reference behaviour).
