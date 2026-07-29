# Territory — colour policy

## What it is

Turning a colour *reference* into an RGB value, and then applying the rules that change it before
anything is composited: inverse, bold→bright, dim, conceal, and the minimum contrast ratio. This is
where the family's theme-agnostic promise is finally cashed — the engine never resolves a colour, and
this is what resolves it.

## Governing decisions

- [ADR-0019 — the cell composition model](../../adr/0019-cell-composition-model.md) — governs the
  *model* these policies feed, not the policies themselves
- **`palette.rs` cites ADR-0002 for the injection principle, and ADR-0002 is superseded** by
  ADR-0018. The principle it names — the consumer owns the scheme and injects it — is live and is
  stated as a boundary invariant in `CLAUDE.md`; the citation points at a tombstone

## Design model

- **A colour reference is a tagged `u32`** — high byte the tag (`0` Default, `1` Indexed, `2` Rgb),
  low 24 bits the payload. Kept in **lockstep with `justerm_core::encode_color`** and the wasm
  decoder's `js/colors.js`: three implementations of one encoding.
- **The consumer injects the scheme; the renderer only resolves.** No default palette is authoritative
  here — this is the theme-agnostic boundary in its final form.
- **Policies apply in RGB space, before compositing.** Inverse swaps fg/bg; bold→bright promotes an
  indexed colour; dim fades the foreground *toward the background* rather than to a fixed value.
- **Conceal is one mechanism for two SGR features** (#282). Hidden and blink both render background
  only — glyph coverage and decorations suppressed — by pointing the cell at the blank slot rather
  than by a per-feature branch.
- **Minimum contrast is a faithful port of xterm's `ensureContrastRatio`**: nudge the foreground's
  luminance away from the background in 10% steps until the WCAG ratio is met. *Colours* carry no alpha here,
  so it works on packed `0xRRGGBB`. The clause used to read "justerm has no alpha", which stopped
  being true of the family when #577 made `set_bg_alpha` reachable from the widget — the correction
  is still computed on the nominal opaque background, and **both references that have the feature do
  the same** (xterm.js shifts the alpha byte off before taking luminance, `common/Color.ts:297`;
  ghostty composites first and still reads only `bg.rgb`, `common.glsl:97-110`). None of the three
  can know what is behind the window, so this is a limit of the idea, not a gap in the port.
- **The bit positions mirror `justerm_core::CellFlags`** — the renderer decodes the same word the
  engine packed, so a flag added on one side is a silent no-op on the other until both move.

## Code

- `justerm-renderer/src/palette.rs` — `Palette`, `resolve_indexed_or_rgb`
- `justerm-renderer/src/render_policy.rs` — `ColorPolicy`, `resolve_cell`, `dim_foreground`
- `justerm-renderer/src/contrast.rs` — `ensure_contrast_ratio`
- `justerm-renderer/src/attrs.rs` — the SGR flag decode, `is_inverse` / `is_dim` / `is_concealed`,
  `glyph_field`, `BLANK_SLOT`
- `justerm-renderer/src/glyph_class.rs` — `treat_glyph_as_background_color`: the **exception**.
  Powerline separators and box-drawing elements butt against the neighbouring cell, so a contrast
  nudge on one opens a visible seam. xterm excludes them (`excludeFromContrastRatioDemands`) and
  re-tints them toward the selection colour instead; since #507 the set is **unioned with what this
  crate draws itself** ([built-in block glyphs](builtin-block-glyphs.md))
- `justerm-renderer/src/webgl.rs` — `set_palette`, `set_bg_alpha`, `set_bold_to_bright`,
  `set_minimum_contrast_ratio`, `set_selection_foreground`

## Reference behaviour

**Partial** in `docs/agents/reference-facts.md` § "Background transparency — the shape of the knob"
(#577), which pins the alpha side: where each reference puts the knob, which cells it reaches, and —
the row that lands in this territory — that xterm.js and ghostty both compute minimum contrast
*ignoring* the background's alpha, by two different routes.

**Still unpinned: the port claim itself.** `contrast.rs` describes itself as a *faithful port* of
xterm's `ensureContrastRatio`, and nothing checks the step-by-step behaviour against the source. A
port is the strongest possible claim about a reference; #577 pinned what the function does about
*alpha*, not that the nudge matches.

**Provenance worth knowing (#504):** these modules cite justerm-web siblings they were ported from —
`render-policy.ts`, `render-core.ts`, `glyph-class.ts` — and **those modules no longer exist.** The
widget's compositing half was removed when the renderer took it over (#273), so this crate is now the
family's only implementation. The citations are history, and the module docs say so rather than
leaving a reader to discover it.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

- [cell compositing](cell-compositing.md) — every policy here runs before the layers are composited;
  the two are one pass in the code and two concepts in the model
- [wire format](wire-format.md) — the tagged-`u32` encoding is shared with `encode_color`, so a
  change is a three-implementation change
- [pen](pen.md) — the engine writes the references this resolves; a new attribute is a `CellFlags`
  bit here as well as there
- [selection](selection.md) · [active match](active-match.md) — `set_selection_foreground` decides
  whether selected text keeps its own colour

## Known holes / open

- **A stale ADR citation in `palette.rs`.** The injection principle is real and current; the record
  it names (ADR-0002) is superseded, and the live statement lives in `CLAUDE.md`'s boundary
  invariants instead.
- **A "faithful port" with no pinned row for the port itself.** `ensure_contrast_ratio` claims
  fidelity to xterm's implementation and nothing checks the nudge against the source. #577 pinned
  the neighbouring question — what both references do about a *translucent* background — which is
  what makes the remaining hole a narrower and more answerable one than it was.
- **Three implementations of one colour encoding** — core, the wasm decoder, and this crate — held in
  lockstep by convention. Only the wire version gates any of it.
