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

**The tier split (#769), the grid registry (#770) and the viewport draw loop (#771) are built;
everything else below is designed and not built** — read ADR-0021 for the authoritative form, and
`## Code` for what exists today.

- **One context, N viewports**, with per-grid `scissor` + `viewport` rectangles rather than per-grid
  contexts. The browser's context cap is the forcing constraint. Built in #771, in the shape three.js
  uses: one clear, then per rect a viewport + scissor + that rect's own clear + its own draw, with the
  projection re-sized to the rect. Two things are ours rather than the reference's and both follow
  from who produces the rect — the **y-flip** (our input is a top-origin DOM box; three.js's caller
  supplies bottom-origin fractions of a canvas the renderer scales) and the **full-canvas clear**
  (our grids need not tile, so a pixel can be outside every rect).
- **Resources sort into three tiers**: **global** (per context — the program, the shared quad buffer,
  the uniform locations), **per-config** (per font configuration — the atlas, rasteriser, glyph cache and the cell
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

The renderer **holds** N grids and **draws** every one that has been placed.

- `justerm-renderer/src/webgl.rs` — `GlobalTier`, `ConfigTier` and `GridTier`, with `JustermRenderer`
  as the facade holding one global tier, one config tier and a *registry* of grids (#769, #770). The
  per-grid operations are inherent methods on `GridTier`, which is what let the registry multiply the
  struct rather than rewrite the methods
- `justerm-renderer/src/registry.rs` — `GridRegistry`, `GridId`, `Viewport`, `RegistryError` (#770),
  and `Viewport::gl_rect`, the top-origin → bottom-origin flip the draw loop applies (#771).
  Pure and host-tested: the payload is a type parameter, so the registry never learns that a grid
  carries GPU state and the whole of it is testable off the wasm32 target. `GridId::DEFAULT` is the implicit
  grid every export predating the per-grid setters acts on, and it is the one the registry refuses to
  remove
- `justerm-renderer/demo/grid-registry.html` — the browser proof for the registry's observable
  behaviour, and where the resident-memory number below was measured
- `justerm-renderer/demo/multi-viewport.html` — the browser proof for the draw loop: three grids in
  three rects on one canvas, the y-flip asserted by placing two of them at opposite ends of the
  buffer, and the single-grid output compared against a control captured in the same run (#771)

Still absent: no terminal-surface type, no atlas keyed by configuration — every grid still selects
into the single per-config tier, and every export except addGrid/removeGrid/setViewport/
clearViewport/gridCount/isGridDrawn/applyDamageTo still acts on the implicit default grid. (Names of
things that do not exist yet are left un-backticked on purpose: every symbol under this heading is
resolved against the source, so a code-span for an unbuilt type fails the note gate — which is the
gate doing its job.)

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Multi-viewport resource tiering](../../agents/reference-facts.md#multi-viewport-resource-tiering--how-the-one-reference-that-shares-font-machinery-splits-it-768-verified-2026-08-18)
- [A terminal registry, and what "registered but not drawn" is made of](../../agents/reference-facts.md#a-terminal-registry-and-what-registered-but-not-drawn-is-made-of-770-verified-2026-08-19)
- [Drawing N views on one canvas — the mechanism reference, and the two places it does not reach](../../agents/reference-facts.md#drawing-n-views-on-one-canvas--the-mechanism-reference-and-the-two-places-it-does-not-reach-771-verified-2026-08-19)

**three.js is now pinned** (#771 needed a fact from it, which is the condition #768 set for pinning
one). **WezTerm still is not**, so a quarter of ADR-0021's prior art remains unverifiable at the
moment someone builds against it — pin it for the question that needs it rather than in the
abstract.

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

**A published surface, and — since #771 — observable behaviour behind it.** This note used to say a
territory with no code has no blast radius; #769 gave it code and the radius stayed empty, because a
tier split changes nothing anyone can see. #770 added *reach*: six new wasm exports
(`addGrid` / `removeGrid` / `setViewport` / `clearViewport` / `gridCount` / `isGridDrawn`) ship on the
`renderer-v*` track, so [published surface](published-surface.md) has something to carry; #771 adds a
seventh (`applyDamageTo`) and makes the set *do* something — a placed grid paints.

The addition is still strictly additive and a single-grid consumer is unaffected: every export
predating the per-grid setters acts on the implicit default grid, whose rect is the whole drawing
buffer, so its frame is the same frame it was. What #771 does change for anyone holding more than one
grid is that the renderer now has a **draw order** (registration order) and a **z-order** — a later
grid paints over an earlier one where their rects overlap — and it *replaces* rather than blends,
because each grid's pass opens with a `clear` and a clear writes. So a translucent grid shows the
page behind the canvas and never the grid it overlaps. Tiling panes never produce an overlap, and
the reference behaves the same way for the same reason.

The list below is what lands **when the multi-grid work does**, and the entries above the fold in
`## Code` say how much of that has happened:

- [glyph atlas](glyph-atlas.md) — the atlas becomes **per-config**, shared by every grid on the same
  font configuration, which is the whole economy of the design. Not per-context: two grids in
  different fonts hold different atlases on one context, and that difference is the tier
- [GL context lifecycle](gl-context-lifecycle.md) — one loss takes down **every** grid at once rather
  than one. #771 made `restore` rebuild *every* registered grid's VAO and instance buffer and drop
  every upload baseline, because it had to: with a draw loop, a grid still holding an object from the
  dead context binds a VAO that raises `INVALID_OPERATION` and leaves the *previous* grid's bound, so
  grid B would silently draw grid A's cells. What is still the context-loss slice's is the half above
  the objects — refilling a not-drawn grid's buffer, and proving the recovery **per grid** through the
  real listener path rather than once for the surface
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
- **A non-default grid is now reachable — drawn and fed — and that changed which of these holes are
  hypothetical.** #770 could record "nothing draws or feeds a non-default grid" as what made it safe;
  #771 draws them and `applyDamageTo` feeds them, so each remaining gap has to be read as live.
  - **Closed by #771 because the draw loop made it unsafe to leave open:** `restore` rebuilt one
    grid's instance buffer. It now rebuilds **every** registered grid's VAO and buffer, drops every
    upload baseline, and refills every buffer — with a draw loop the old behaviour is not a blank
    grid but a *wrong* one, since binding a VAO from the dead context raises `INVALID_OPERATION` and
    leaves the previous grid's bound. **What is owed is the evidence, not the code**: no proof
    anywhere loses a context with more than one grid registered
    (`demo/context-loss*.html` are single-grid; `demo/multi-viewport.html` never loses one), so the
    N-grid restore path ships on reasoning. Asserting it **per grid**, through the real listener
    path, is the context-loss slice's real remaining work — and a pass that only checks the drawn
    grid cannot tell "all recovered" from "the visible one recovered".
  - **Still open:** a DPR change, a font change and a spacing change re-key *one* grid's
    configuration — the default's — and a registered grid keeps the four font/metric selectors it
    was born with, which go stale the moment the default's move. Nothing reads them until the atlas
    slice keys an atlas by them, and the window shuts when the implicit exports die.
  - **Still true, and worth not re-deriving:** registering during a loss does not announce itself.
    Chromium's `createBuffer()` hands back a **non-null** object on a lost context (measured #770, in
    the synchronous window and after `webglcontextlost` alike), so the registration succeeds and the
    dead handle is indistinguishable from a live one until something tries to upload through it.
- **What one resident grid costs, measured rather than worried about** (#770, dev build, Chromium
  headless, dpr 1 — the epic's open question routed here). The wasm heap slope between an 800-cell
  and a 10 000-cell populated grid is **≈171 B/cell**, which is an *upper* bound: the heap never
  shrinks, so it carries each frame's transient JS→wasm copy of the cell columns (~18 B/cell) as well
  as the state that stays. At that rate an 80×24 grid is ≈320 KiB and a 120×40 grid ≈800 KiB. An
  **empty** registered grid — a terminal that has produced no output yet — is ≈3 KiB, dominated by
  its own 256-colour palette, and 64 of them cost less than one populated grid. Read that 3 KiB as
  ±1 KiB: 64 registrations moved the heap by exactly three 64 KiB pages, so page granularity is most
  of the signal at that scale and the figure is quantised to 1024 B. The number that
  matters for *"all grids stay resident"* is the populated one: ten hidden 120×40 terminals are on
  the order of 8 MB of wasm heap, held so that showing one is a placement rather than a rebuild.
- ~~**Not drawn gates the draw, and nothing yet gates the work behind it.**~~ **Closed by #771,
  which gated the work too** — the draw loop skips a grid with no viewport before it packs it, so a
  hidden grid that is still being fed pays the scatter and nothing after it, and the dirty flag it
  leaves set makes the first render after it is placed pack it exactly once. The reasoning and the
  number are kept because they are what the choice was made on. `Option<Viewport>` decides
  whether a grid paints. It said nothing about whether a hidden grid still *packs and uploads* every
  frame it is fed — and the consumer's adoption design keeps a hidden workspace's Blocks mounted and
  feeding (penterm's `terminal-single-context-adoption` PRD: `ContentArea`'s `display:none` mount
  policy is kept, and each mounted Block feeds decoded frames). So after the per-grid setters land,
  a hidden terminal's per-frame CPU cost is real. **Measured, so the decision has a number** (#770,
  120×40, release wasm, two environments agreeing within ~10 % — headless SwiftShader and a real
  NVIDIA/D3D11 browser): scattering a frame costs ≈**0.04 ms**, and the pack + upload behind it
  ≈**0.33 ms**, against ≈0.003 ms to draw. So an ungated hidden grid costs ≈**0.4 ms per frame**, of
  which the scatter is what a fed grid pays regardless and the rest is what a gate could take back —
  ten hidden terminals at 60 fps would be ≈4 ms of a 16.7 ms budget, a quarter of the frame spent on
  pixels nobody sees. The upload is not the expensive half: an identical frame (whose diff uploads
  nothing) and a frame where every cell changes cost the same to within noise, so this is wasm CPU
  and it transfers. Both references gate more than the draw and **disagree on how much** (the rows
  under `## Reference behaviour`): ghostty skips the CPU cell rebuild as well as the paint, alacritty
  only the paint. #771 took ghostty's amount — not by deferring to it, but because the gate falls out
  of the draw loop for free and self-heals, and 0.4 ms per hidden terminal per frame is not a price
  worth paying for a `continue` that was already being written.
- **The middle tier's hazard went LIVE at #771, one slice ahead of the guarantee meant to cover it.**
  Sharing one atlas across grids makes [glyph atlas](glyph-atlas.md)'s within-frame eviction
  corruption a *cross-grid* event, and the upload diff — the defence today — cannot see it, because a
  grid that is not re-packing never re-diffs. ADR-0021 records the hazard, names three candidate
  guarantees, and assigns the choice to the atlas-registry slice. **What changed is the reachability,
  not the assignment**: there has only ever been one `ConfigTier`, so the glyph cache was always
  shared — but until #771 only the default grid could pack into it. `applyDamageTo` plus the per-slot
  re-pack loop mean a second grid now evicts from the same LRU. Concretely: grid A goes idle holding
  instances that address atlas slots; grid B keeps packing different glyphs; once the combined live
  set crosses a region's capacity (2048 normal / 2048 wide — CJK- or emoji-heavy content, not ASCII),
  B's pack repoints slots A still addresses, and A never re-packs so nothing self-heals. **Not
  measured** — bounding it needs a probe that packs two grids past the capacity — and no consumer
  reaches it yet, since nothing outside a proof page registers a second grid. Recorded here so the
  atlas slice inherits a live hazard rather than a future one.
