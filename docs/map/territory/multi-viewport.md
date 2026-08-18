# Territory — multi-viewport rendering

## What it is

One WebGL2 context drawing **N terminal grids** as viewports, instead of one context per terminal.
A tabbed or split-pane host needs many grids on screen; browsers cap the number of live WebGL
contexts, and each one carries its own glyph atlas and shader program.

**This territory started as a record with no code**, the inverse of every other note here: those
describe behaviour that exists and hunt for the decision behind it, while this one had the decision
and nothing implementing it. #769 gave it its first code, so it is now the ordinary shape — but the
decision still reaches much further than the implementation, and `## Code` is where that gap is
stated rather than here, so this paragraph does not need rewriting as each slice lands.

## Governing decisions

- [**ADR-0021 — one WebGL2 context draws N terminal grids as viewports (`TerminalSurface`)**](../../adr/0021-single-context-multi-viewport.md)
  — the direction, the `TerminalSurface` shape, a three-layer resource split and the rules for
  assigning a resource to a layer. Read its `Status:` line rather than any restatement
- The build plan lives in epic **#287** (a GitHub issue, so not a graph node)

## Design model

**The tier split is built (#769); everything else below is designed and not built** — read ADR-0021
for the authoritative form, and `## Code` for what exists today.

- **One context, N viewports**, with per-grid `scissor` + `viewport` rectangles rather than per-grid
  contexts. The browser's context cap is the forcing constraint.
- **Resources sort into three tiers**: **global** (per context — the program, the VAO, the uniform
  locations), **per-config** (per font configuration — the atlas, rasteriser, glyph cache and the cell
  geometry derived from them), and **per-grid** (per terminal). The record's contribution is the *rule
  for assigning* something to a tier, not the tier list — that rule is what an implementation is
  measured against, and it was adjudicated in #768: a tier assignment answers **two** questions, the
  *selector* (whose state decides this — per-grid whenever a consumer can set it per terminal) and the
  *resource* (where the selected thing lives — per-config only when one instance can serve two grids
  *and* rebuilding it is expensive enough to repay keying). Read ADR-0021's D1–D5, not this summary.

  > **This bullet used to name the tiers "per-context / per-surface / per-frame", which is not what the
  > record says.** Two of the three differed, and the middle one differed in kind: the sharing axis is a
  > font *configuration*, not a surface. Corrected in #768 along with the two blast-radius entries
  > below — all three came from one overloaded noun (next bullet).
- **"Surface" means one thing here and the opposite in the references — the single most reliable way
  to get this territory wrong.** ADR-0021's `TerminalSurface` is **one per app**: the canvas, the
  context, the atlas registry and the single frame loop. Ghostty's `Surface` is **one per terminal**,
  and it owns a renderer, a GPU atlas texture and a render thread of its own. So a sentence borrowed
  from the reference with the word intact inverts its meaning. When writing about a per-terminal thing
  in this territory, say **grid**.
- **Named prior art, cross-checked**: three.js's multiple-views (scissor + viewport), Ghostty's
  shared grid, WezTerm's glyph cache. `virtual-webgl` is recorded as a **counter-example** —
  multiplexing contexts underneath rather than sharing one honestly. **Only Ghostty is checkable**:
  the pinned reference trees are alacritty, ghostty and xterm.js, so the three.js and WezTerm
  citations in the record cannot be verified from here.

## Code

The tier split exists; nothing multi-grid does yet.

- `justerm-renderer/src/webgl.rs` — `GlobalTier`, `ConfigTier` and `GridTier`, with `JustermRenderer`
  as the facade holding one of each (#769). The per-grid operations are inherent methods on
  `GridTier`, so they are already written in the form a registry would multiply

Still absent: no terminal-surface type, no scissor path, no grid registry, no atlas keyed by
configuration — the renderer still draws exactly one grid, and the facade still holds exactly one of
each tier. (Names of things that do not exist yet are left un-backticked on purpose: every symbol
under this heading is resolved against the source, so a code-span for an unbuilt type fails the note
gate — which is the gate doing its job.) What changed is that the three are now separable *types* rather than one struct, so the
next slice adds a registry instead of first having to find the seams.

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Multi-viewport resource tiering](../../agents/reference-facts.md#multi-viewport-resource-tiering--how-the-one-reference-that-shares-font-machinery-splits-it-768-verified-2026-08-18)

The rest of ADR-0021's prior art still lives inside the record as prose rather than as pinned rows,
and two of its four sources (three.js, WezTerm) have no pinned tree at all — so for those the argument
for the shape remains unverifiable at the moment someone builds against it.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

**Still effectively nothing**, though no longer for the original reason. This note used to say a
territory with no code has no blast radius; #769 gave it code, and the radius is *still* empty —
the tier split changed no behaviour and no public surface, so nothing downstream can observe it. A
territory can hold code and touch nothing, which is what a pure structural slice looks like.

The list below is what lands **when the multi-grid work does**, and the entries above the fold in
`## Code` say how much of that has happened:

- [glyph atlas](glyph-atlas.md) — the atlas becomes **per-config**, shared by every grid on the same
  font configuration, which is the whole economy of the design. Not per-context: two grids in
  different fonts hold different atlases on one context, and that difference is the tier
- [GL context lifecycle](gl-context-lifecycle.md) — one loss would take down **every** grid at once
  rather than one, and the restore path is currently written for a single surface
- [cell geometry](cell-geometry.md) — the cell size stops being a property of the renderer and becomes
  a property of a **font configuration**, which a grid selects into. The **device-pixel ratio does
  not** follow it: one canvas means one drawing buffer and one `devicePixelRatio`, so DPR stays global
  and is a *component of the config key* rather than a per-grid value. (This entry read "each surface
  has its own cell size **and device-pixel ratio**" until #768 — the DPR half was borrowed from
  Ghostty, where a `Surface` is an OS window that can sit on its own monitor, so its grid key hashes
  `xdpi`/`ydpi`. That does not transfer to N viewports on one canvas.)
- [GPU upload](gpu-upload.md) — the diff baseline becomes **per-grid**, and so does the instance buffer
  it mirrors: one shared buffer with N baselines would let one grid's upload silently invalidate
  another's. ADR-0021 listed `instance_vbo` as global until #768 corrected it
- [widget lifecycle](widget-lifecycle.md) — a shared context outlives any one widget, which is a
  lifecycle question nothing currently owns

## Known holes / open

- **This note was written as "a record with no implementation", which was its own state** — the
  code-first passes could not see this territory at all, because there was nothing to read. #769 gave
  it its first code, so that is no longer true; the entry is kept because the *shape* is what made
  this note necessary, and a territory can re-enter that state.
- **Half the design is still unverified against its own prior art.** Ghostty's half is now pinned
  (above); three.js and WezTerm have no local tree, so if either has moved nothing would notice before
  someone builds against it.
- ~~**The single-context assumption is not marked anywhere in the renderer.**~~ Closed by #769: the
  three tier structs *are* the marking, and they mark it in the one way a comment cannot — a per-grid
  method that reaches another tier does not compile. What #769 measured while doing it is worth
  keeping, because it is evidence about the record rather than about the code: moving 309 field
  accesses into three tiers produced **one** cross-tier compile error, so the split ADR-0021 asserts
  matches the structure the code already had.
- **Four operations genuinely span tiers, and they are the ones a registry has to thread.**
  `apply_frame` and `repack_from_grid` reach `resolve_and_pack`; `set_letter_spacing` and
  `set_line_height` reach `adopt_spacing`. All four need the atlas or the cell it derives, i.e. the
  per-config tier — so packing and cell derivation are cross-tier by nature, not by accident, and a
  grid registry must pass a configuration in rather than expecting these to become per-grid.
- **The middle tier has a hazard the record now names but nothing yet answers.** Sharing one atlas
  across grids makes [glyph atlas](glyph-atlas.md)'s within-frame eviction corruption a *cross-grid*
  event, and the upload diff — which is the defence today — cannot see it, because a grid that is not
  re-packing never re-diffs. ADR-0021 records the hazard and names three candidate guarantees; picking
  one is the atlas-registry slice's job.
