# ADR-0017: The core/consumer boundary — buffer-derived mechanism in core, policy injected

Status: accepted (2026-06-30, #113) — **amended 2026-07-22**: the routing decision stands, but one of
the grounds it was argued on has been falsified. Core *does* carry a `regex` dependency now
(`justerm-core/Cargo.toml`, since #314 gave `search()` regex/whole-word modes), so every sentence below
that prices an option by "core gains no regex dependency" is quoting a cost that no longer exists. The
mechanism/policy argument — the *pattern* is policy and stays with the consumer — survives untouched;
the dependency-weight argument does not. Marked inline at both places it appears.

## Context

`justerm-core` is a reusable, I/O-less, theme-agnostic VT engine. A **frame-mode** consumer (justerm-web,
and any future one) does not run the engine — it sees only what the frame carries: a *viewport* snapshot
(ADR-0011 cell mirror), plus per-frame viewport state (cursor, scroll, interaction overlays — ADR-0013/0014).
It does **not** hold the whole buffer: scrollback and off-screen rows are not in the mirror.

Slice after slice the same question recurs — does capability X live in core or the consumer? It has been
answered case by case: selection (#109) and search (#110) went to core; colour resolution went to the
consumer (theme-agnostic core); OSC 8 links are core (VT-parsed). S10 (#113, links) forced the question
into the open and exposed that the default was drifting.

### The forcing case (S10 plain-URL links)

A hyperlink reaches the screen from **two sources**:

- **(a) OSC 8 explicit links** — parsed from the VT stream. The cell carries a link-present bit and the URI
  lives in a pool (`cell.rs:229`, `is_linked`); the frame already ships the per-cell `link` index + the
  `linkTable` (wire v2, #26). Unambiguously core.
- **(b) plain-text URL detection** — a heuristic regex over the rendered text (xterm's main web-links
  feature; `addon-web-links/src/WebLinkProvider.ts`, `LinkComputer.computeLink`, read at source). #113
  scoped this "web new."

But (b) cannot be done correctly in a frame-mode consumer. `LinkComputer` assembles the **full logical
line** — joining soft-wrapped rows up and down (`_getWindowedLineStrings`) and correcting wide-char string
indices (`_mapStrIdx`) — before matching. A URL that wraps into the viewport from a row scrolled *above*
the top has its start in scrollback the consumer does not hold, so a web-side regex can never match it.
xterm has no such gap because its web-links addon reads the live buffer **in process**; the gap is
frame-mode-specific, structural, and not a bug. Without a stated rule, #113 mis-placed a buffer-wide
mechanism in the consumer.

## Decision

A capability's **mechanism** lives in `justerm-core` iff **(1)** it is parsed from the VT stream, **or**
**(2)** computing it correctly needs the **whole buffer** (all cells, scrollback, coordinates, soft-wrap,
wide-char) — because a frame-mode consumer holds only the viewport and *physically cannot*. Everything else
— presentation, transport, and interaction that needs only the viewport plus local state — stays in the
consumer. When core computes over the buffer, the consumer **injects the policy** (the search query, the
URL regex, the palette) so core stays policy- and theme-agnostic: **mechanism in core, policy in the
consumer.**

Three tests, applied in order, route any capability:

1. **VT-parsed?** → core (cursor, grid, colour-as-reference, OSC 8).
2. **Needs the whole buffer?** → core mechanism, policy injected (selection text, search, URL spans).
3. **Else** → consumer (colour resolution, hover visuals, pixel→cell, debounce, scrollbar, clipboard,
   transport).

**Guardrail** (so "core by default" does not bloat the engine): core never does I/O, IPC, or rendering;
never interprets references into theme values; never holds presentation or interaction state. It takes the
policy as a *parameter* and returns *buffer-coordinate data*. The bias toward core is a **correctness**
bias, not a convenience one — buffer-wide logic duplicated per consumer is the divergence risk the
quality bar warns about (the demo's hand-rolled `fake-select.ts` is the smell), and a frame-mode consumer
cannot do it right regardless.

### Applied to links: core assembles, consumer matches

(b) plain-URL detection is core mechanism. Concretely, the **(ii)** split: core exposes the viewport's
**logical lines as assembled text plus a string-index → `(row, col)` coordinate map** (joining
soft-wrapped rows including off-screen wrapped context, skipping wide-char spacers); the consumer runs the
URL regex **and** `new URL()` validation over that text and maps matches back to cells. The *URL* regex
and the URL validation never enter core. [~~and core gains **no regex dependency**~~ — false since #314;
see the status note. The link decision is unaffected: the URL pattern is still the consumer's.]
(a) OSC 8 is unchanged — it already ships on the frame.

**Why (ii), not "core runs a consumer-supplied regex → `Vec<Match>`" (i).** (ii) keeps the mechanism/policy
split clean — the regex never leaves the consumer — and keeps core dependency-free, preserving the
lean-engine stance (ADR-0001: vte only, not `alacritty_terminal`). `new URL()` validation is a
consumer/JS primitive anyway. Both options solve the off-screen-start case (core assembles the full text);
(ii) does it without putting a regex crate in the engine.

**The exact shape of the coordinate map is left to implementation (#113)**, per this repo's convention that
ADRs firm up at implementation (real encode/decode detail settles the decision). This ADR fixes the
*direction* (core assembles text + map; consumer owns regex), not the wire layout.

### Named prior art

xterm keeps web-links in an **addon** — but the addon has in-process buffer access, so its code is correct
there and would be *incorrect* in a frame-mode consumer that lacks off-screen cells. The frame-mode
equivalent of "the addon reads the buffer" is core assembling the buffer text and shipping it. Alacritty's
search is "the engine finds, the frontend navigates" (`search.rs`); the same split generalises here —
core assembles, the consumer matches. The convergence (buffer-wide work in the engine, policy and
presentation in the frontend) is the non-arbitrary boundary.

## Consequences

- **#113(b) is reclassified consumer → core mechanism.** (a) OSC 8 is unchanged. The issue is corrected to
  the (ii) split.
- **Existing placements are validated, not reversed.** Selection, search, OSC 8, and theme already follow
  the rule; this ADR codifies it so the placements stop being re-litigated each slice.
- **Future slices route by the three tests** without grilling: an a11y screen-reader mirror reads buffer
  text → core provides the text, the consumer presents; resize px→cols/rows is viewport math → consumer,
  while the grid resize is core.
- **A buffer-wide capability mis-placed in a consumer is a latent frame-mode correctness bug** (the link
  case), not a style choice — the rule turns that class of bug into a routing question caught up front.
- **New core surface: a viewport logical-line text + coordinate-map query.** Its shape is an implementation
  decision (#113); it is expected to ride the existing frame machinery or sit beside it as a `&self` query,
  like `selection_range` / `match_spans`.
- CLAUDE.md's identity section points at this ADR — the inverse of the existing "what core does *not* do"
  list is now stated: *what core does, and why* (buffer-derived mechanism; policy injected).

## Alternatives considered

- **(i) Core runs a consumer-supplied regex → `Vec<Match>` (generalise `search`).** Rejected in favour of
  (ii): it adds a `regex` crate to core (wasm weight, against the lean-engine stance) and pushes the policy
  (the regex) into core's execution. (ii) keeps core dependency-free and the policy entirely in the
  consumer. Revisitable if dogfood shows the text+map export is too costly or web-side matching too slow.
  **Amended 2026-07-22 — half of this rejection has expired.** #314 gave `search()` exactly this shape
  for the *search* feature: the consumer supplies the pattern, core compiles and runs it
  (`SearchOptions.regex`, `is_valid_regex`), and the `regex` crate is in core's manifest. So the
  dependency cost quoted here is already paid and is no longer an argument against (i) for links. What
  did *not* expire is the other half: the pattern is still policy, and core executing one it was handed
  is different from core *owning* which pattern is a URL. If links are revisited, argue (i) vs (ii) on
  that axis and on where the coordinate mapping is cheapest — not on the dependency.
- **(B) Per-consumer implementation of buffer-wide features (xterm's addon model).** Rejected — xterm's
  addon has full in-process buffer access; a frame-mode consumer does not, so the *same* code is correct in
  xterm and incorrect here. It also duplicates buffer logic across consumers (divergence risk).
- **(C) Core owns the policy too (core decides what a URL is, what counts as a match).** Rejected — violates
  the theme/policy-agnostic identity; core takes the regex/query/palette as a parameter instead.
- **(D) Keep deciding case by case.** Rejected — the recurring re-litigation and the silent drift of the
  default (which mis-scoped #113) is exactly the cost this ADR removes.
