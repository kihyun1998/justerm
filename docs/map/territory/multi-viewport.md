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

**Every slice of this design is now built** — the tier split (#769), the grid registry (#770), the
viewport draw loop (#771), the per-config atlas registry (#772), the per-grid contract (#773), the
surface-wide context loss (#774) and the consumer's `TerminalSurface` (#775). Read ADR-0021 for the
authoritative form and `## Code` for what exists today. (This sentence named four slices and called
the rest "designed and not built" until #775; it is the kind of status claim `docs/map/README.md`
warns about, so prefer `## Code` over it if the two ever disagree again.)

- **A renderer holds no terminal until the consumer registers one** (#773). Until S5 it arrived
  holding an implicit grid drawn over the whole buffer, so that the exports predating the per-grid
  setters had something to act on; a surface holding two terminals would then hold three grids, and
  the third would paint under both. Every per-grid export names its grid now and the implicit one is
  gone, which also removed the last place where a *rect* had a producer other than the consumer's
  measured box.
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
- **The middle tier is keyed by the four consumer selectors and *not* by the DPR** (#772), although
  ADR-0021's prose said otherwise and the one reference that shares font machinery does hash its
  density. One canvas means one drawing buffer and one `devicePixelRatio`, so the DPR is globally
  constant across the registry — a component every key shares cannot separate two keys, and putting
  it in would only make every key wrong the moment it changed. A density change is therefore the
  *rebuild-all* path rather than a mass re-key, and it is the one case where editing a shared entry
  in place is right: nobody is being moved into a configuration they did not ask for. ADR-0021's own
  bullet is sharpened rather than contradicted; the ghostty divergence is recorded there.

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
- `justerm-renderer/src/config_registry.rs` — `ConfigRegistry`, `ConfigKey`, `ConfigId` (#772): the
  refcounted map from a font configuration to the resources serving it. Pure and host-tested for the
  same reason the grid registry is — the payload is a type parameter, so the registry never learns
  that a configuration owns a GPU texture. `ConfigRegistry::ids_wanted_by` is where a restore asks
  which entries survive a re-key (#788): the question has to be answered *before* the reconcile
  runs, and asking it here is what makes the wrong version — the one that also required a current
  holder — impossible to write
- `justerm-renderer/demo/multi-viewport.html` — the browser proof for the draw loop: three grids in
  three rects on one canvas, the y-flip asserted by placing two of them at opposite ends of the
  buffer, and the single-grid output compared against a control captured in the same run (#771)
- `justerm-renderer/demo/per-config-atlas.html` — the browser proof for the atlas registry (#772):
  sharing and release read as counts (`atlasCount`, and `bakes` as a delta, the `packs` precedent),
  the immutability of a shared entry read as *pixels* — a sibling's rect is compared byte-for-byte
  across another grid's font change — and two configurations drawn side by side, each `█` run
  measuring its own grid's cell width. It also loses and restores a context with two grids drawn

- `justerm-renderer/demo/per-grid-state.html` — the browser proof for the per-grid setters (#773):
  two grids on one configuration side by side, each holding its own palette, selection, active
  match, decorations and cursor. Deliberately **differential** — every check sets one grid and
  compares the other's whole rect byte-for-byte against a capture taken before the call, so it
  asserts *reach* rather than the blend formula it would otherwise have to encode
- `justerm-renderer/demo/context-loss-grids.html` — the browser proof that one loss is one recovery
  for **every** registered grid (#774): four grids in four states across one real
  lose-and-restore cycle driven through the browser's own listeners, each read back on its own rect.
  Two of them have no viewport when the
  context dies, which is why this page exists at all — a pixel pass cannot see a grid that does not
  paint, so the assertion is that *placing* it after the restore shows what it was fed before the
  loss, with no re-bake, no re-pack and no re-feed

- `justerm-web/src/terminal-surface.ts` — `TerminalSurface` (#775), the consumer half: one canvas, one
  context, N attached terminals. **Registration hands back a `GridLease`, not an id** (#805): the
  renderer's ids are numbers because they cross the wasm boundary, and `reference-facts.md` records
  the condition that makes them safe — *a stale handle in JS has to fail loudly*. The web layer had
  been softening that instead, with three registry methods silently ignoring an id they no longer
  held; a lease removes the question rather than answering it quietly, and it is the idiom every
  other registration in the package already uses. It owns the grid registry, the single presenting loop, the density
  watcher and context-loss recovery, and it is constructible under vitest through `SurfaceDeps`, so
  the composition itself is host-tested. `SurfaceBackend` is the **surface-scoped half of the renderer
  backend**, and the split is not a judgement this package makes: it is the renderer's own 0.15.0
  signatures, where a call naming a grid acts on one terminal and a call naming none acts on what they
  share. Measured while splitting — 25 members stayed with the per-grid interface and **every one of
  them takes a grid**; 11 moved out and **none of them does**
- `justerm-web/src/justerm-renderer.ts` — `JustermRenderer` is now the per-grid object: it holds a
  surface, its own grid, and the blink loop and reduced-motion listener that really are per terminal.
  `create` composes a surface and claims sole tenancy (which is what keeps #331's exactness — sizing
  the buffer to `cols * cell` is only available while one grid fills the canvas); `attach` joins a
  surface the host built and takes its rect from `setViewportRect`

- `justerm-web/demo/shared-surface.ts` · `justerm-web/demo/shared-surface.html` ·
  `justerm-web/e2e/shared-surface.spec.ts` — **the consumer-side browser proof** (#776, S8): two
  terminals at two font sizes on one canvas, each a transparent DOM overlay over its own viewport.
  It is the epic's real round-trip and not an illustration of it — measured when the slice started,
  nothing outside `justerm-web/src` called `TerminalSurface.open`, `JustermRenderer.attach`,
  `observeViewportRect` or `onDensityChange`, so the adapter's shared-tenant branch had never
  executed in a browser. What it asserts that a renderer-side proof cannot: that the GL viewport
  lands where the *DOM overlay* is, read as a pixel at a point derived from the two elements' boxes.
  Its page background is part of the evidence — the gutter between the panes is buffer no grid was
  placed over, so it stays transparent, which is what separates two rects on one canvas from one grid
  spanning both

**The design, the code and the proof now meet.** This section carried *"still absent: no
terminal-surface type"* through six slices; #775 built it and #776 proved it in a browser, so the
epic's remaining work is a tracker question rather than a missing piece here.

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Multi-viewport resource tiering](../../agents/reference-facts.md#multi-viewport-resource-tiering--how-the-one-reference-that-shares-font-machinery-splits-it-768-verified-2026-08-18)
- [A terminal registry, and what "registered but not drawn" is made of](../../agents/reference-facts.md#a-terminal-registry-and-what-registered-but-not-drawn-is-made-of-770-verified-2026-08-19)
- [Drawing N views on one canvas — the mechanism reference, and the two places it does not reach](../../agents/reference-facts.md#drawing-n-views-on-one-canvas--the-mechanism-reference-and-the-two-places-it-does-not-reach-771-verified-2026-08-19)
- [Sharing font machinery between terminals — how the one reference that does it refcounts, keys and invalidates](../../agents/reference-facts.md#sharing-font-machinery-between-terminals--how-the-one-reference-that-does-it-refcounts-keys-and-invalidates-772-verified-2026-08-19)

**three.js is now pinned** (#771 needed a fact from it, which is the condition #768 set for pinning
one). **WezTerm still is not**, so a quarter of ADR-0021's prior art remains unverifiable at the
moment someone builds against it — pin it for the question that needs it rather than in the
abstract.

## Cross-cutting invariants

- [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md)
  — this territory is where the rule's second clause lives, and where it was found. A surface is
  **shared by construction**, so a terminal handed one must not end it; a terminal that *composed*
  one must. Same rule one level down in the registry: a terminal releases its own grid, the surface
  releases every grid still registered when it ends. Promoted out of
  [widget lifecycle](widget-lifecycle.md) by #775, which falsified the phrasing that territory had
  predicted

## Blast radius

**A published surface, and — since #771 — observable behaviour behind it.** This note used to say a
territory with no code has no blast radius; #769 gave it code and the radius stayed empty, because a
tier split changes nothing anyone can see. #770 added *reach*: six new wasm exports
(`addGrid` / `removeGrid` / `setViewport` / `clearViewport` / `gridCount` / `isGridDrawn`) ship on the
`renderer-v*` track, so [published surface](published-surface.md) has something to carry; #771 adds a
seventh (`applyDamageTo`) and makes the set *do* something — a placed grid paints; #772 adds an
eighth and ninth, `atlasCount` and `bakes`, which carry no behaviour and exist so that *sharing* is
something a consumer or a proof can measure rather than assume.

**#773 ends the additive phase** — it is the contract step of the expand–contract sequence, and the
break is deliberately concentrated in one release (0.15.0). Every per-grid export gained a grid
parameter and throws on an id it does not know; `resize(cols, rows)` split into `resize_grid` and
`resize_surface`, the latter taking device pixels so the surface and the rects placed on it are in
one space; `add_grid` gained the four font selectors, optional and trailing, so a grid joins a
sibling's atlas instead of baking one it abandons a line later. Two consequences reach a consumer
beyond the signatures: the single-grid arrangement is now three calls it makes rather than one the
renderer made, and a **density change invalidates every device-pixel quantity the consumer gave** —
the surface's size as well as every rect — because only the consumer can re-measure them. The
renderer holds none of them across it: converting would mean converting through its own copy of the
density, which lags by construction, so what it does instead is re-bake the atlases and leave the
measurements alone.

What #771 changed for anyone holding more than one
grid is that the renderer now has a **draw order** (registration order) and a **z-order** — a later
grid paints over an earlier one where their rects overlap — and it *replaces* rather than blends,
because each grid's pass opens with a `clear` and a clear writes. So a translucent grid shows the
page behind the canvas and never the grid it overlaps. Tiling panes never produce an overlap, and
the reference behaves the same way for the same reason.

The list below is what lands **when the multi-grid work does**, and the entries above the fold in
`## Code` say how much of that has happened:

- [glyph atlas](glyph-atlas.md) — **landed in #772**: the atlas, rasteriser, glyph cache and the cell
  geometry derived from them are keyed per font configuration and refcounted, so every grid on the
  same configuration shares one set and the last to leave releases it. That is the whole economy of
  the design. Not per-context: two grids in different fonts hold different atlases on one context,
  and that difference is the tier. What came with it: a shared cache means one grid's pack can
  **repoint a slot another grid's instances still address**, which the upload diff cannot see — so a
  grid re-packs when its configuration's eviction count moves
- [GL context lifecycle](gl-context-lifecycle.md) — one loss takes down **every** grid and **every
  configuration** at once rather than one. #772 added the second half: `restore` re-bakes every live
  configuration's atlas at the live density, keeping each one's glyph slots, and then reconciles any
  grid whose selectors moved while the context was dead — a setter in that window writes the selector
  and defers, so the grid names a configuration it no longer matches. #771 made `restore` rebuild
  *every* registered grid's VAO and instance buffer and drop
  every upload baseline, because it had to: with a draw loop, a grid still holding an object from the
  dead context binds a VAO that raises `INVALID_OPERATION` and leaves the *previous* grid's bound, so
  grid B would silently draw grid A's cells. #774 closed the half above the objects — not with code,
  which was already there, but with the proof that reaches a grid holding **no viewport** at the
  moment the context dies
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
- [widget lifecycle](widget-lifecycle.md) — a shared context outlives any one widget. **#775 gave that
  an owner**: `TerminalSurface` holds the canvas, the context, the density watcher, the restore
  listener and the loss relay, all of which sat on the per-terminal object before and would therefore
  have been taken down by the *first* terminal to be disposed. Measured in a real browser: with two
  terminals attached, disposing one leaves the other's rect byte-identical (ink 2232 → 2232) and the
  survivor still recovers from a real lose/restore cycle with no re-feed

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
    leaves the previous grid's bound. **What was owed was the evidence, not the code**, and it
    arrived in two pieces. #772 supplied the drawn half incidentally —
    `demo/per-config-atlas.html` loses and restores a context through the real listener path with
    **two grids registered and both drawn**, asserting the whole drawing buffer repaints
    byte-for-byte plus one atlas bake per live configuration. **#774 supplied the rest**, which is
    the half a pixel pass structurally cannot see: `demo/context-loss-grids.html` holds two grids
    with **no viewport** when the context dies, and asserts the recovery by *placing* them
    afterwards. Read the shape rather than the page — "a grid that does not paint cannot be
    photographed" is why this took its own slice, and it is the same shape any future
    registered-but-idle state will have.
  - ~~**Still open:** a registered grid keeps the four font/metric selectors it was born with, which
    go stale the moment the default's move.~~ **Closed by #772, by making them true rather than by
    keeping them in step.** A font, spacing or family change moves the *default* to a different
    configuration and leaves every other grid on the one it selected — so a registered grid's
    selectors and the entry it draws through agree by construction, and `GridTier::new` now unpacks
    the selectors *from* the key so the two cannot even be born disagreeing. There is one window in
    which they do drift, deliberately: a setter arriving while the context is lost writes the
    selector and defers the move, because an atlas cannot be baked on a dead context. `restore`
    reconciles it. Nothing reads a grid's selectors in that window — `render` skips a lost frame
    entirely — and a grid *registered* inside it is born at the configuration actually in force
    rather than the one the default has asked for and not yet got, which is the only answer available
    and is recorded at `add_grid`.
  - **A DPR change is a different path from all of that, and stayed one:** it rebuilds every entry in
    place rather than re-keying anything, because one canvas has one density (see `## Design model`).
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
  policy is kept, and each mounted Block feeds decoded frames). So a hidden terminal's per-frame
  CPU cost is real, and has been since the per-grid setters landed (#773). **Measured, so the decision has a number** (#770,
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
  B's pack repoints slots A still addresses, and A never re-packs so nothing self-heals.
  **Discharged in part by #772, and the residue is a different question from the one ADR-0021 asked.**
  A grid now re-packs when its configuration's eviction count moves — the cheapest form of the
  record's second candidate guarantee — and `demo/per-config-atlas.html` drives it: 4 glyphs from one
  grid, then 1953 from a sibling (exactly the normal region's dynamic capacity), then the sibling's
  live set shrinks and the first grid repairs itself with no new frame from the consumer. Turning the
  comparison off makes those four cells draw the sibling's glyphs, which is the corruption made
  visible: ink over the four cells goes 207 → 421.
  **Choosing between ADR-0021's three candidates revealed that they answer two different questions**,
  so #772 took two of them. An eviction happens *only once a region is full*, which splits the hazard
  by whether the grids' **live** glyph sets fit a region together:
  - *they fit* — the **pack epoch** above, which converges because the re-pack makes that grid's
    glyphs most-recently-used and the next eviction takes a dead slot;
  - *they do not fit* — the **slot pin**, made render-scoped: one pin set for the whole pack loop, so
    a grid that cannot fit beside its siblings is **refused** rather than drawn wrong, exactly as an
    over-capacity single frame already was. A refusal dirties every grid on that configuration, which
    is what makes it a fixed point rather than an alternation — without that the frames alternate
    between correct-and-reported and wrong-and-silent (measured).
  **The measurement is the part worth keeping**, because it is what a pixel suite structurally cannot
  see. Two grids at 1200 distinct live glyphs each, region 1953: before the pin, one grid drew 911 lit
  subpixels beside its sibling and **891 alone** — stably wrong, every frame, no error anywhere, and
  every proof green. After: 891 every frame, equal to the control, with the refusal reported on every
  frame. `demo/per-config-atlas.html` runs both halves, and the alone-control in the same run is the
  only thing that tells them apart.
