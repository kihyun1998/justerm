# ADR-0005: Binary, reference-based, fixed-width serialization (frame = damage + side-table)

Status: accepted (2026-06-18)

## Context

`#6` must turn one damage cycle (`damage()` + `scroll_delta()`, ADR-0003) into a
byte buffer a consumer can ship over its own transport (e.g. a Tauri Channel)
and decode cheaply on the far side. `docs/architecture.md` §Serialization fixes
the intent (binary, reference-based, fixed-width records + a grapheme
side-table, decode straight into typed arrays); this ADR records *why* that shape
over the alternatives, cross-checked against prior art.

Two axes were decided.

### Axis 1 — encoding shape

- **A — binary fixed-width records + side-table.** Each cell a constant-stride
  record; rare multi-code-point clusters referenced by an index into a per-frame
  side-table. Decodes as a contiguous typed-array view, no per-field parse.
  (GPU terminals — Alacritty/wezterm/beamterm — pack cells into fixed-stride
  instance/SSBO buffers exactly this way.)
- **B — protobuf baseline-diff framebuffer (Mosh SSP).** A schema-tagged,
  variable-length message; the sender diffs a kept copy of the last screen.
  Mosh also *infers* scrolling heuristically (new row 0 == old row N ⇒ shift).
- **C — escape-sequence re-emit (xterm.js serialize addon).** Serialize the
  screen back into a VT byte string; the receiver replays it through a parser.

B is already half-rejected by ADR-0003: we record the scroll op and accumulate
ack-gated bounds, so we have neither a baseline copy nor a need for heuristic
scroll inference — protobuf's variable-length, schema-tagged framing also fights
the "decode straight into typed arrays" goal. C is disqualified by the consumer:
`beamterm` is a GPU renderer with **no VT parser**, so an escape-sequence stream
would force a parser back into the consumer — the opposite of handing it ready
cells. A is the only shape that decodes into a renderer instance buffer without
re-parsing.

### Axis 2 — references vs resolved pixels

The consumer renderer `beamterm` takes an **8-byte resolved** cell
(`u16` atlas-glyph-id + 24-bit fg RGB + 24-bit bg RGB). The tempting shortcut is
to serialize *that* directly — zero consumer-side work. It is rejected: it would
make the engine resolve colour references to hex and map code points to a
specific font atlas, breaking the **theme-agnostic** and **font/atlas-agnostic**
invariants (CLAUDE.md) and coupling justerm to one renderer. The engine ships
*references*; resolving reference → RGB (frozen scheme) and codepoint → atlas id
is the consumer's thin adapter (architecture.md §"How a consumer integrates").
So #6's record sits exactly one layer **above** beamterm's `CellDynamic`.

## Decision

Adopt **A, reference-based**: a binary, little-endian frame of fixed-width
**16-byte** cell records plus a per-frame grapheme side-table. The engine
provides **both** `encode` (a damage frame) and `decode` (so the round-trip is
the acceptance test); transport stays the consumer's job (CLAUDE.md: no IPC).

Frame = header (magic/version/flags, `cols`/`rows`, `Full | Partial`) → optional
scroll op `{top, bottom, count}` (applied before spans) → spans (`{line, left,
right}` + cells) → side-table (only clusters referenced this frame, frame-local
indices). Cell record (LE): `c` u32 (Unicode scalar, not atlas id), `fg`/`bg`
u32 each (tag `Default|Indexed|Rgb` + 24-bit payload), `flags` u16, `extra` u16
(frame-local, 0 = none) = **16 bytes**, 4-aligned. Future underline style+colour
ride `flags`' spare bits (11–15) + the colour tags' spare bits; an OSC 8
hyperlink id is a versioned addition (own index + side-table), so no padding
field is reserved. Width derives from `flags & WIDE_CHAR`.
The format-specific hidden state (wide-char halves, side-table re-indexing,
scroll/span ordering, colour tagging, flag/codepoint splitting, empty-vs-Full
frames) is enumerated in architecture.md §"Hidden VT state" `[#6]`.

## Consequences

- **Decodes into a renderer buffer without a parser** — the fixed 16-byte stride
  is one contiguous view; the consumer's adapter does two cheap maps (ref → RGB,
  codepoint → atlas id) to reach beamterm's 8-byte cell. C (escape re-emit) could
  never do this.
- **Invariants hold to the wire.** Reference-based keeps theme-agnosticism;
  codepoint (not atlas id) keeps font-agnosticism — the format is reusable beyond
  beamterm.
- **Versioned, extensible without a break.** `reserved` + a header `version` mean
  underline style/colour and OSC 8 hyperlinks land later without a format change
  (matches the Cell's reserved bits).
- **Round-trippable and testable in isolation.** `decode` exists so #6 verifies
  `encode → decode == input` (incl. wide chars + side-table) with no PTY, no
  transport, no renderer — the boundary that makes justerm independently testable.
- **Cost: an encode pass per frame** walking the damaged spans and collecting the
  referenced side-table entries. Bounded by damage size (not the whole screen),
  matching ADR-0003's incremental grain.
- **Reversible-ish.** If a future consumer wanted resolved pixels, a *second*
  encoder variant could emit beamterm's 8-byte cell directly — but that lives
  with the consumer/renderer, never displacing the reference-based engine format.
