# justerm — architecture & contract

The detailed spec an implementer references. justerm is a **pure terminal engine**: bytes in →
terminal state → viewport/damage/scroll/selection out. No I/O, no IPC, no rendering, theme-agnostic.
See `CLAUDE.md` for the boundary invariants and `CONTEXT.md` for vocabulary. Key decisions with
rejected alternatives are in `docs/adr/`. This contract was grilled and cross-validated against prior
art (Mosh / Alacritty / Warp / VS Code / beamterm); the origin/rationale record is PenTerm's
`.scratch/rust-terminal-engine/PRD.md` (history only — this file is justerm's authoritative spec).

## Engine API (shape)

- `feed(&mut self, bytes)` — push a VT byte slice (caller does the PTY/SSH I/O).
- `viewport_snapshot(rows) -> Grid` — current visible cells + cursor.
- `damage() -> Iterator<{line, left, right}>` — changed line ranges, each with a changed column span.
- `scroll_delta() -> Option<{count, region}>` — a first-class scroll op since last frame.
- `resize(cols, rows)` — re-layout / reflow.
- selection: `begin/extend/clear(point, side, mode)`, `selection_range()`, `selection_text() -> String`.
- `search(query) -> Matches` (grid + scrollback).
- `encode_key(event) / encode_mouse(event) -> bytes` — mode-dependent input encoding (the engine owns
  the modes that decide encoding).
- events: OSC title, bell, OSC8 hyperlink, OSC7 cwd — a subscribe surface for consumers.

## Cell

Fixed-width record for fast typed-array decode:

- `content`: a **grapheme cluster** (combining marks/emoji-correct; primary code point inline, rare
  multi-code-point clusters reference a side-table — keeps cells fixed-width).
- `fg` / `bg`: **color references** — `Default | Indexed(u8 0..255) | Rgb(u8,u8,u8)`. **Never resolved
  hex** — the consumer/renderer maps indices→hex via its (frozen) scheme. Engine is theme-agnostic.
- `attrs`: standard 8 (bold/dim/italic/underline/blink/inverse/hidden/strikethrough). The record
  **reserves room** for underline style+color and an OSC 8 hyperlink id so adding them later is not a
  format change.
- `width`: 1 (normal) or 2 (wide / CJK fullwidth).

## Damage = line + column span (+ scroll op)

Emit changed line ranges, each carrying the changed column span (`{line, left, right}` — Alacritty's
`LineDamageBounds` grain). **Not** full-frame (wastes IPC/idle power on small updates), **not**
cell-level (finer than terminals mutate; gratuitous). Scroll is a **first-class op** (shift rows + new
rows) so moderate scrolling moves content instead of redrawing; degrades to all-rows-dirty on
floods/resize. Damage is an *efficiency* axis, not quality — same pixels either way.

## Viewport / scrollback / scroll

- The **engine** owns the full screen + scrollback ring + alt-screen + **scroll offset + follow-bottom**.
  The consumer sends scroll *intents* (wheel/page/jump-to-bottom); the engine resolves them to a window.
  ("new output while scrolled up — follow or stay?" is bound to an output event → must live here.)
- The consumer/renderer may cache a transient **overscan band** (viewport ± a screen) for instant
  small scrolls — a cache, not ownership; the engine stays authoritative.
- **Alt-screen** is an internal DEC mode; transparent — the engine emits whichever screen is current.

## Cadence — ack-paced state-diff (the consumer protocol)

The engine remembers the consumer's **last-acked screen state**; the diff it produces is
`last-acked → current` (line+column spans). The consumer applies a frame, then **acks**; the engine
sends the next diff only after the ack (≤1 in flight). Everything falls out of the last-acked baseline:
intermediate-state skip (a slow consumer's missed frames collapse into one diff), flow control (a slow
consumer gets larger diffs less often — never a pile-up, never discards), and pacing (the ack
round-trip is the collection interval, phase-aligned to the consumer's vsync). No separate timer.

## Selection

Engine-owned. Type = char / word / line / **block**; anchor = point + **side (left/right)**.
`selection_range()` → highlight; `selection_text()` → copy text (respects type, wide chars,
wrapped-line joining, trailing-whitespace trim, **across scrollback** — the engine holds all cells).
Cursor blink is *not* an engine concern (consumer-local animation); the engine only reports cursor
position/style/visibility.

## Serialization (the wire format the engine offers)

Binary, **reference-based** (matches the Cell above — references, not RGB), fixed-width cell records +
a grapheme side-table for rare multi-code-point clusters. Designed for a consumer to ship over its own
transport (e.g. a Tauri Channel) and decode straight into typed arrays. The engine provides the
*format*; transport is the consumer's job.

## Hidden VT state — model these (and grow this list)

A correct-*looking* model (cell + cursor + advance + wrap) silently omits subtle state real terminals
track. These are invisible from first principles / this contract — only a reference impl (`vte` /
alacritty / xterm) or vttest reveals them. **Before implementing any VT-semantics slice (#2, #3, #4,
#6, #7, #10): read how a reference terminal handles that area and enumerate the hidden state (flags,
deferred behavior) it tracks — then add what you find here.** Seeds (caught in #2 review, 2026-06-16):

- **Pending-wrap (deferred last-column wrap).** Printing into the last column does *not* advance to
  the next line — the cursor stays put with a `wrapnext` flag, and wrap happens on the *next* print.
  Eager wrap is a classic off-by-one bug (lines shift). [#2]
- **Wide-char spacer is a distinct marker, not a blank.** The trailing column of a width-2 char must
  carry a "wide-char spacer" marker (flag/variant), not a plain blank — else overwrite, erase,
  selection, and cursor positioning go wrong. [#2]
- **Background Color Erase (BCE).** Erase (ED/EL) fills cleared cells with the *current SGR
  background*, not default. [#7; note in #2 if deferred]

The *systematic* catch for this whole class is #7's vttest harness + dogfood — this list is only the
famous few caught by review. Pull vttest early so VT-semantics slices verify against it from the start.

## How a consumer integrates (context, not justerm's work)

PenTerm (first consumer) wraps justerm: feeds PTY/SSH bytes, ships the binary diff over a Tauri
Channel, and in the webview a **thin adapter** resolves color references → RGB (via the session's
frozen scheme) and maps attrs (inverse/dim/hidden → colour manipulation) before handing cells to the
**`beamterm`** WebGL2 renderer; the adapter draws the cursor. Selection highlight is rendered by
beamterm but the selection *model + text* stay in justerm (so copy reaches scrollback). This
integration is tracked in PenTerm, not here — but it defines what the engine's output must serve.

## Prior-art basis (one line each)

- **Mosh (SSP):** server keeps screen state, syncs diffs, skips intermediates — our cadence's ancestor;
  its scrollback failure (synced only the screen) is why scrollback is engine-owned here.
- **Alacritty:** `LineDamageBounds` (damage grain) + the `Selection`/`to_range`/`to_string` model we
  mirror on `vte`. (We do *not* depend on `alacritty_terminal` — see ADR-0001.)
- **Warp:** forked Alacritty's model + native GPU render — confirms the model base; its render path is
  the full-native option we do not take.
- **VS Code:** raw-bytes-over-IPC + watermark flow control — the counter-example; our parse-in-engine +
  diff gives flow control for free.
- **beamterm:** the adopted renderer (see ADR-0002).
