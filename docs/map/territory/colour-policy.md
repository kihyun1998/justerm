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
- **Widget chrome takes its colour through the CSS cascade, not through the palette** (#926). The
  web `Scrollbar` thumb's inline `background-color` is a `var()` chain — `--justerm-scrollbar-thumb-active`
  → `-hover` → `--justerm-scrollbar-thumb` → the pre-#926 `rgba(255,255,255,0.25)` — so a consumer
  that sets nothing sees no change and one that sets only the rest colour gets it in every state.
  The longhand, not the `background` shorthand: the shorthand resets `background-clip` and the rest
  inline, which a stylesheet reaching the thumb by `SCROLLBAR_THUMB_ATTRIBUTE` could then not undo.
  - **No `<style>` is injected, and that is the point.** xterm.js injects one per `Terminal` with an
    unscoped selector (`browser/Viewport.ts`, the `_styleElement` block @ 699f553), so with N
    terminals on a page the last one created paints every pane's slider — PenTerm measured exactly
    that. A custom property inherits from the pane it is set on, so each pane follows its own scheme
    and a scheme change needs no call into the widget.
  - **The widget holds the state, because an inline style cannot say `:hover`.** `thumbState` is
    `active` for as long as a drag the thumb started is held, wherever the pointer is — the
    `window`-bound drag listeners outlive the hover. xterm.js has the same three states and holds
    `xterm-active` from drag start to drag end (`browser/scrollable/abstractScrollbar.ts`).
  - **A drag ends on a buttonless move as well as on `mouseup`**, and starts on the primary button
    only (`startsThumbDrag`, `dragStillHeld`). Before, a release the page never received — a context
    menu opened on press, a window switch mid-drag — left the thumb following a pointer with no
    button down, and with the active colour that became a visibly stuck thumb. xterm.js ends its drag
    the same way, on a move whose `buttons` no longer match the press (`globalPointerMoveMonitor.ts`).
    It does not undo #814's inert drag through a hidden pane: that pointer still has its button down.
- **The bit positions mirror `justerm_core::CellFlags`** — the renderer decodes the same word the
  engine packed, so a flag added on one side is a silent no-op on the other until both move.

### Where the policies meet the highlight (`frame.rs` `pack_instances`)

- **The fg channel is keyed on selection coverage, not on the winning highlight** (#430, xterm's
  model). The selection-only fg rules (#224, #227, #239) survive on a cell whose bg the active match
  outranks: xterm's `CellColorResolver` keys its selection stage on `$isSelected`, while xterm's active match
  is a bg-only top decoration (justerm's is an overlay kind, see [decoration](decoration.md)). `selectionForeground` (#227) forces a selected cell's fg to the injected
  colour — never a match's — over the cell's own or a bottom decoration's fg; a tile glyph discards it
  (below), and being selection-only it never triggers the #230 re-dim.
- **A tile glyph under a selection fuses into the band** (#239, #241). xterm re-tints it toward the
  *raw* selection colour (not the effective post-blend bg), from the cell's own undimmed fg, discarding
  `selectionForeground`. An inverse cell with a Default bg is "treated as transparent" (#241): it
  contributes no colour of its own, so its fg becomes the band over whatever *is* beneath — the raw
  selection colour with no blend, or (#453) the selection over a bottom decoration's bg when one painted
  there. That band is recomputed with the cell taken out of the stack, **not** read from `eff_bg`, which
  is the band over *this* cell and for an inverse cell carries the cell's own colour (probe: `0x97AFDF`
  against the raw `0x3060C0`) — exactly what "transparent" says to drop. With no decoration it is the
  raw selection colour, byte-identical to xterm (`CellColorResolver.ts:139` sets it flat). The
  decoration folds in only when the *selection* is the layer painted over it: a match paints solid
  (#400) and erases the decoration from the bg channel, so blending over it would compose a stack no
  pixel shows. The code tests `deco_bg && kind == Selection` as the collapsed form of "did the bg
  channel blend": this arm requires `is_inverse`, which makes `should_blend` unconditionally true, so
  the two are equal here — valid as long as no future `HighlightKind` both outranks `Selection` and
  blends.
- **The re-tint starts from `cell_fg`, after bold→bright** (#223), and that is the model's answer:
  ADR-0019 rule 1 puts `L0` at "the cell after inverse and bold→bright", and rule 4 sends a
  background-class glyph's ink through the bg fold from there. xterm differs for a bold + ANSI 0–7 +
  tile + selection cell, re-tinting from the *base* ANSI colour (its `CellColorResolver` bypasses the
  `+8`) — a corner-of-corner, sub-perceptible under the 0x80 blend. That is documentation for a
  consumer porting from xterm, not a defect: ADR-0019 makes xterm a design input for cell composition
  rather than a validator. #398 asked for the xterm value and was closed won't-fix on exactly this
  rule; do not "restore parity" without amending the ADR. (Its older framing — a family change to keep
  justerm-web byte-neutral — is doubly dead: the widget's compositing half went with #504.)
- **The fg policy applies once, against the effective bg, on the undimmed fg** — xterm's model
  (`TextureAtlas._getMinimumContrastColor`), which the renderer can follow because the highlight is
  already folded into `eff_bg` (beamterm could not, so justerm-web double-passed; a compromise the
  renderer sheds, which the #272 two-lens pinned). Minimum contrast (#225) is checked **first**: if it
  fires, the corrected fg wins and DIM is skipped — mutually exclusive, xterm `TextureAtlas.ts:329` —
  and a dim cell that already clears the *halved* ratio is dimmed instead (#232). A selected cell's DIM
  is cleared (#224, xterm `& ~BgFlags.DIM`), so its text stays legible over the highlight and the ratio
  is not halved. A decoration fg override on a dim, unselected cell **keeps** the DIM (#230: xterm
  leaves `BgFlags.DIM` set, so the override is dimmed too) and is re-dimmed before contrast; the base
  fg's own dim is the `!fg_overridden` arm, so exactly one path dims the fg. DIM is a property of the
  *cell*, so a dim cell's underline is dim too (#513 rule 6); the line re-dim shares `fg_overridden`
  because a decoration that set the fg set both.
- **A tile glyph is excluded from the contrast demand, and the exclusion is scoped** (#226). It
  exists because `ensure_contrast_ratio` is a function of `eff_bg`: two cells of one tiling run over
  different backgrounds get nudged differently and the run *seams*. Once a decoration has taken the
  glyph (#508) there is no tile left, and the remaining ink is `I_line`, TEXT class by rule 4 — so the
  exclusion has no referent there and must not reach it; an undecorated tile still keeps it (the control
  in `minimum_contrast_reaches_the_line_on_a_taken_tile`). The line inks run the same two policies
  again rather than sharing the glyph's result, because the two inks can start from different colours
  and need different corrections — and the line's contrast gate is the glyph's verbatim, a correction
  of #513's first shape: an underline is as continuous across cells as a tile is, and dropping the term
  let a `────` run under `minimumContrastRatio` change colour at a background boundary — the symptom
  #513 exists to remove, re-entered through contrast. On a taken tile the gate is open (#508): the
  glyph is gone, the line is the only ink, and a decoration paints one colour across its whole span
  anyway, so nothing can seam. Where the line's ink forks and which cells compute it:
  [cell compositing](cell-compositing.md) § The packer.

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
- `justerm-renderer/src/webgl/grid_state.rs` — `set_palette`, `set_bg_alpha`, `set_bold_to_bright`,
  `set_minimum_contrast_ratio`, `set_selection_foreground`
- `justerm-web/src/scrollbar.ts` — `thumbBackground`, `thumbState`, `SCROLLBAR_THUMB_ATTRIBUTE`: the
  thumb's colour, the one piece of widget chrome a consumer themes; `startsThumbDrag`,
  `dragStillHeld`: when the drag that holds it `active` starts and ends

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
