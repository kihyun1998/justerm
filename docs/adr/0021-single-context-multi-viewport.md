# ADR-0021: One WebGL2 context draws N terminal grids as viewports (`TerminalSurface`)

Status: **accepted** (2026-08-18, #768; direction accepted 2026-07-22, decided 2026-07-07 in #287,
recorded here 2026-07-21 with its citations verified). Scoped to **renderer resource ownership** and the
widget/canvas relationship; it does not change the core boundary (ADR-0017) or cell composition
(ADR-0019). Implementation is Epic #287, sliced as #768–#776, and **how far it has got is the
tracker's answer, not this line's** — the sub-issue list on #287 moves on its own and a status
restated here does not (this sentence read *"not started"* until #769 falsified it one merge later).
No longer gated: #287's blocker was the single-grid renderer #258, which shipped.

**Amended 2026-08-18 (#768), in two places, and the second is the one a later reader must not skim
past.**

1. **The evidence grade was wrong.** This ADR graded the context ceiling *"anticipated, not observed"*.
   It had been observed, in the consumer, before this ADR was written. Corrected in *Context* and in
   *Alternatives* (D); the argument is unchanged, its footing is stronger.
2. **The tier rule was adjudicated**, as the 2026-07-22 amendment said the first migration would do.
   The contradiction turned out to be a **category error rather than a wrong clause** — both halves
   were true of different questions — so the rule was rewritten rather than having a sentence deleted,
   and the assignment *table* it replaced is gone: the table named a field that does not exist and
   omitted sixteen that do. Three assignments change as a result (the four font/metric selector
   fields, and `instance_vbo`).

## Context

`justerm-web` today is one widget = one canvas = one WebGL2 context, inherited from the xterm.js /
beamterm shape it replaced (ADR-0002 → ADR-0018). One context per terminal costs something on every
frame and every switch, and fails outright at a ceiling.

**Grade of evidence, stated up front**, because this ADR mixes two kinds and a later reader must know
which to re-measure: the per-terminal costs below are **structural** (they follow from the code as
written, and are visible today at any terminal count); the ceiling is **observed** (below); the browser
numbers are **verified against engine source**.

> **Corrected 2026-08-18 (#768). This paragraph read *"the ceiling is anticipated, not observed —
> nothing in justerm or penterm has hit it, because penterm's webview still runs xterm.js and carries
> no justerm npm dependency at all"*.** The premise was true and did not bear on the conclusion. **The
> ceiling belongs to the browser and the number of live terminals, not to which engine draws them** —
> xterm.js's `WebglAddon` takes one context per terminal exactly as `justerm-renderer` does. So
> *"does penterm depend on justerm?"* was the wrong question to grade *"has the ceiling been hit?"*
> with, and it was asked because this ADR was reasoning about its own consumer rather than about the
> resource.
>
> What was actually there, at the time this was written: penterm had **already** hit it and shipped a
> defence. `../penterm/src/blocks/terminal/lib/terminalWebglPort.ts:25-28` caps live contexts at
> `MAX_WEBGL_TERMINALS = 10` and states its reason — *"이 머신은 ~14 에서 이미 GPU 강제 evict(흰
> 화면)가 관측돼, 그 아래로 여유를 둔다"* (white-screen eviction **observed at ~14** on a real
> machine, so the cap sits below it). Twice, in fact: `terminalWebglPool.ts`'s module doc records that
> an earlier *unbounded*-warm version crashed by accumulating contexts across tab switches.
>
> Two things follow, and both are recorded where they belong rather than here — the ~14 threshold
> below Chromium's documented 16 is consistent with this ADR's own finding that eviction is
> least-recently-**flushed** rather than count-exact, and alternative (D) is not a road not taken but
> a shipped bridge (see *Alternatives considered*).

### The recurring cost: N contexts means N of everything

Independent of the ceiling, and paid at any count:

- **An atlas per terminal.** The glyph cache, rasteriser and atlas texture are per-context
  (`webgl.rs`: `atlas`, `rasterizer`, `cache`), so six terminals in the same font hold six byte-identical
  atlases. Sharing them is what the middle tier below exists for.
- **A frame loop per terminal.** Each widget drives its own `requestAnimationFrame`.
- **A re-attach on every show.** A context that was backgrounded and lost is restored by re-baking the
  atlas and re-uploading every resident glyph — the flash on switching back. This happens *below* the
  ceiling too; it is the same mechanism, triggered by the browser reclaiming rather than by count.

### The ceiling: verified numbers, and they are not what the epic assumed

Verified against implementation source rather than folklore (#287's body said "~16" flatly):

- **Chromium** — `content/renderer/webgraphicscontext3d_provider_impl.cc:121-139`:
  `max_active_webgl_contexts = 16` on desktop, **8 on Android**, `max_active_webgl_contexts_on_worker = 4`.
- **Eviction is least-recently-*flushed*, not oldest-created** —
  `third_party/blink/renderer/modules/webgl/webgl_rendering_context_base.cc:498-509` loops
  `ForciblyLoseOldestContext("WARNING: Too many active WebGL contexts. Oldest context will be lost.")`,
  and `OldestContext()` picks the lowest `GetLastFlushIdCHROMIUM()`. The loss is **synthetic**, so a page
  cannot veto it in `webglcontextlost`; evicted contexts are auto-restored when slots free.
- **WebKit** matches the count — `WebGLRenderingContextBase.cpp:190-191`, `maxActiveContexts = 16`,
  `maxActiveWorkerContexts = 4`, evicting the lowest `activeOrdinal()` with the same message.
- **Firefox is the outlier** — `StaticPrefList.yaml`: `webgl.max-contexts = 1000`,
  `webgl.max-contexts-per-principal = 300`, enforced LRU in `WebGLContext.cpp`.

So the correct statement is *"Chromium and WebKit evict at 16 (8 on Android, 4 on workers); Firefox does
not, in practice"* — the ceiling binds on two of three engines, which is enough to design against but not
a universal law. MDN's `webglcontextlost` page does not document count-based eviction at all, which is
why this ADR cites engine source.

### What the ceiling would cost, if reached

A user tiling terminals or opening many sessions crosses 16 without any signal, and the
**least-recently-drawn** terminal — i.e. the one they were not looking at — goes white with no recourse,
since the loss is synthetic and the page cannot refuse it. It is recoverable in principle (the browser
restores evicted contexts when slots free) but not gracefully: recovery is the same full atlas re-bake as
above, now for whichever terminal was reclaimed.

**This is diagnosed, not merely designed against** (corrected 2026-08-18, #768 — it read *"Nobody has
seen it here"*). penterm's relevant surface — Tauri with WebView2 on Windows and WKWebView on macOS —
sits on the two engines that *do* evict, and it is where the white screen was seen, at **~14** rather
than at the documented 16; Android's 8 and the worker limit of 4 do not apply to us. So the question
this paragraph offered a later reader — *"did the recurring costs justify it on their own?"* — is no
longer the one the decision rests on. It is still worth asking, because the recurring costs are the
half that is paid at **any** terminal count, and a design justified only by a ceiling would be
answerable by raising the ceiling.

## Decision

**One app-global WebGL2 context and canvas draws every terminal grid as a viewport.** The renderer
becomes multi-grid: `add_grid` / `set_viewport` / `apply_frame(grid)` and a single render loop that, per
visible grid, sets `gl.viewport` + `gl.scissor` and draws.

**Resources split three ways — global, per-config, per-grid — and the rule below assigns them.** There
is deliberately no table of field names here; see *Why the assignment table was removed*, after the
rule.

### The tier rule (adjudicated 2026-08-18, #768)

**A tier assignment answers two questions, and the 2026-07-22 rule collapsed them into one. That is
what made it self-contradicting: both of its clauses were true, of different questions.**

**D1 — The selector: *whose state decides this?*** Anything a consumer can set differently per terminal
is **per-grid**, always. This is the 2026-07-22 rule's *"by definition"* clause, and it was never wrong
— it is a statement about the **setting**, not about the thing the setting selects.

**D2 — The resource: *where does the selected thing live?*** A resource is **per-config** when two grids
with equal selectors can be served by **one instance**, *and* rebuilding it is expensive enough to repay
keying, refcounting and a lifetime. Otherwise it is **per-grid** — cheap to duplicate, so keying costs
more than it saves. It is **global** when only context loss invalidates it.

So `setFontSize` is per-grid **as a selector**, and the atlas it selects is **per-config**. Nothing has
to be overruled: a per-terminal setting moves that terminal into a different per-config bucket rather
than dragging the resource down a tier. The two questions are only *visible* as two when a resource is
expensive; where it is cheap — `palette` — selector and resource land in the same tier and the
distinction costs nothing to ignore. That collapse is why one rule looked like it could answer both.

**D3 — A setter is not a tier.** A setter is a **write to a per-grid selector plus a re-key of a
per-config resource**, and the geometry setters do both in one call: `set_font_size` assigns
`self.font_size` and then rebakes the atlas, rasterizer, cell size and glyph box. Saying *"`setFontSize`
keys the per-config tier"* reproduces the original category error one level up. Tier the **fields**;
describe a setter by which fields it writes.

**D4 — A tier assigns *ownership*, not residency.** The tier names the site a value is authoritative
at. It does **not** forbid a reader from holding a copy, and for a hot value a copy is expected —
`cell_size` is owned per-config (the ink scan that produces it is expensive) while every grid may cache
it (8 bytes). This is the clause without which D2 is genuinely two-readable, because "expensive to
rebuild" is about the *producer* and "cheap to duplicate" is about the *product*. What a copy owes is
already recorded elsewhere and is not restated here:
[the cell size is derived state](../map/invariant/cell-size-is-derived-state.md), and `theflow.md`'s
*"who owns a fact that several sites read"* — the owner is the producer, never a copy.

**D5 — Out of scope: a diagnostic is not a resource.** `pack_count` is neither a consumer setting nor a
rebuildable resource; it is a counter the browser proofs read (#421). D1–D4 cannot place it because the
question they ask — *what invalidates this?* — does not apply to an observation. Whoever adds the second
one decides where such counters live; this rule does not.

**Why the assignment table was removed.** It listed fields as of 2026-07-21 and had already rotted: it
named `cursor_span`, which is not a field (`cursor_cells` is), and omitted sixteen that exist —
`bg_alpha`, `cursor_contrast`, `cursor_thickness_frac`, `bold_to_bright`, `min_contrast`,
`selection_fg`, `highlight_colors`, the four span vectors, `preedit_*`, `uploaded`, `grid`,
`needs_repack`, `last_blink_on`, `ctx_loss`. D1–D5 place all of them, which is the argument for a rule
over a list: **a table has to be maintained to stay true, and this one was not, while the rule assigns
fields nobody had written when it was drafted.** Two placements changed on adjudication:

- **The four font/metric selector fields** — `font_size`, `font_family`, `letter_spacing`,
  `line_height` — are **per-grid** (D1). What they key stays per-config: `atlas`, `rasterizer`, `cache`,
  `cell_size`, `atlas_cell`, `char_size`, `char_offset`.
- **`instance_vbo` is per-grid, not global.** It was listed global beside `program` and `vao`, but it
  holds a grid's own packed instances, and `uploaded` carries the invariant *"this mirrors what the
  live `instance_vbo` holds"* — one shared buffer plus N per-grid baselines means grid B's upload
  invalidates grid A's baseline silently, and A then diffs against bytes that are gone. D2 places it
  per-grid: no selector, not shareable, cheap to create. **Corollary the first implementation must
  plan for:** the VAO captures `instance_vbo` in its attribute state (`vertex_attrib_pointer_f32` and
  `vertex_attrib_divisor` are called with it bound, inside the VAO), so a per-grid VBO forces either a
  per-grid VAO or a re-pointer per grid per frame.
- **The corollary was settled by #771: the VAO went per-grid too, and D2 is what decided it.** The
  question D2 asks is whether one instance can serve two grids **byte-for-byte**, and a VAO whose
  content is *where grid X's instance buffer is* cannot — the re-pointer alternative does not make it
  shareable, it keeps a per-grid fact in the global tier and rewrites it N times a frame, which is the
  arrangement D2 rejects rather than a cheaper form of it. Nothing else moved with it: the per-vertex
  quad buffer the VAO points at **stays global**, because two grids do share those four vertices
  byte-for-byte. So the global tier holds one program, one quad and the uniform locations; the attribute
  *layout* is still global by construction (one program means one instance format, and per-grid layouts
  are not reachable), while which buffer feeds it is not.

**The consumer gains one new concept.** `TerminalSurface` owns the canvas, the context, the atlas
registry keyed by config, the single `requestAnimationFrame` loop, the grid registry and context-loss
recovery. `Terminal` attaches to a surface and owns its DOM overlay. The xterm-shaped widget experience
is preserved; the only new noun is the surface.

**A forced constraint, accepted knowingly.** WebGL binds a context to exactly one canvas, so one context
means **one canvas**, and every terminal is a transparent DOM overlay positioned over its viewport rect.
Two consequences follow and are accepted: every terminal shares one z-plane, so arbitrary DOM cannot be
interleaved *between* two terminals in stacking order; and the widget's DOM layer must track its rect
(scroll, resize) or the GL viewport and the overlay drift apart. This is a deliberate departure from
xterm's internal structure — positioning stays xterm-shaped, ownership does not.

## Named prior art (each cited only for what it actually establishes)

Verified against real source; two claims carried in #287 were corrected in the process.

- **The compositing technique — three.js.** `examples/webgl_multiple_elements.html` is the closer analogue
  than the more famous `webgl_multiple_views.html`: one renderer over one canvas, each *DOM element's*
  rect read with `getBoundingClientRect()` (l.202) and fed to `setViewport`/`setScissor` (l.218-219), with
  one full-canvas clear per frame before the per-element loop, and the canvas transform-tracked to scroll
  (l.183). That is exactly the shape here — N widgets, one canvas — where `multiple_views` is N rects of
  one scene. Note GL's bottom-origin coordinates and that each view carries its own projection.
- **Per-config atlas sharing — ghostty.** `src/font/SharedGridSet.zig:1-9`: *"a set of SharedGrid
  structures keyed by unique font configuration … allows expensive font information such as the font
  atlas, glyph cache, font faces, etc. to be shared."* Refcounted (`ref(config, font_size)` / `deref(key)`),
  and `SharedGrid.zig:1-19` states the immutability rule this ADR adopts for its middle tier: a grid does
  **not** support resizing or font changes, because *"increasing the font size in one would increase it in
  all"* — a config change means a **new** grid that surfaces switch over to.
  **Correction to #287:** ghostty is *not* precedent for the global tier. It is **one renderer, one GPU
  atlas texture and one render thread per surface** (`Surface.zig:86-92`; `renderer/generic.zig:1586-1599`
  syncs the shared CPU atlas into each renderer's own textures). Its device lives in the *bottom* tier —
  the opposite of this decision. Cite it for the middle tier only.
  **Added 2026-08-18 (#768): it is also the cross-check for D1/D2, in the half this ADR had not
  quoted.** A `Surface` holds `font_grid_key`, `font_size` **and** `font_metrics` side by side
  (`Surface.zig:75-77`), and `setFontSize` writes the selector per-surface (`:2441`) before `ref`-ing
  the shared keyed grid (`:2444`) — *selector per-grid, resource per-config*, implemented rather than
  theorised. It cross-checks **D4** the same way, by doing both halves: it copies the shared grid's
  product onto the surface (`self.size.cell = size` at `:2413`, `self.font_metrics = font_grid.metrics`
  at `:2469`), which is the owner/reader split D4 states. And ghostty keeps its **palette** per-surface
  (`renderer/generic.zig` reads `state.colors.palette`) although two surfaces on one theme could share
  it byte-for-byte — the case clause (a) alone could not explain and D2's cost half does.
  **The direction of this citation matters:** D1–D4 are derived from what keying costs, and ghostty is
  a convergence check, not the authority. Removing ghostty from these sentences leaves the derivation
  standing.
  **Where it does *not* transfer, and this one is load-bearing:** ghostty's atlas **grows, it never
  evicts** — `Atlas.grow` preserves all previously written data (`src/font/Atlas.zig:313-314`) and a
  full atlas returns `Error.AtlasFull` (`:78`, raised at `:177`) rather than repointing a slot. So a
  slot handed to surface A can never be reused under surface B. `justerm-renderer`'s glyph cache is
  **LRU-evicting**, so this ADR imports the arrangement without the precondition that makes it safe —
  see *Consequences*.
- **One context, N panes — wezterm.** `wezterm-gui/src/renderstate.rs:573-579` holds context + glyph cache
  + programs in a `RenderState` owned once **per window** (`termwindow/mod.rs:387`), while `PaneState`
  (l.194-207) carries viewport/selection/overlay and **no GPU resources at all**. It confirms the
  ownership split works at scale, but **not** the mechanism: `render/paint.rs:181-260` takes one quad
  allocator and emits every pane's quads into shared layers in one coordinate space — no per-pane
  viewport/scissor. It also invalidates its cache wholesale on a config change, the same rule ghostty
  states.
- **The rejected workaround — virtual-webgl.** `greggman/virtual-webgl` multiplexes many *virtual*
  contexts onto one real one, motivated by the same cap. Its README rejects itself for our case: *"If
  you're in control of your code then there are arguably better solutions … I have no plans to actually
  use it or maintain it"*, and it names the alternative it would recommend — *"put the canvas of the
  shared GL context full window size in the background and … composite by setting the viewport/scissor"*,
  i.e. this decision. Its stated limits (incomplete WebGL1-on-2 emulation, no error checking, errors
  bleeding across virtual contexts, a `drawImage` copy per canvas per frame) are what a renderer we do
  **not** own would force on us. The one constraint it names that survives into our design is the z-order
  one recorded above.

**The three-tier keying is justerm's own synthesis.** No cited reference splits resources
global / per-config / per-grid: ghostty has app-global font machinery, per-config grids and per-surface
devices; wezterm has per-window GPU state and per-pane non-GPU state with no config tier; three.js tiers
nothing. Only the *middle* tier has direct precedent. This is stated so the tier boundary is defended on
its own merits rather than by appeal to a reference that does not hold it.

**Two of the four citations above cannot currently be checked (recorded 2026-08-18, #768).** This
repo's pinned reference trees are alacritty, ghostty and xterm.js (`docs/agents/theflow.md` § Step 1);
**wezterm and three.js are not among them**, so the wezterm line/file references, the three.js example
coordinates and the `virtual-webgl` README quote are unverifiable here. This is asymmetric and worth
knowing before building on it: **wezterm is the only cited precedent for a bottom tier that holds no
GPU resources**, and it is the unverifiable one — while the *verifiable* reference, ghostty, puts the
GPU device in the bottom tier, which this ADR already records as a correction. The synthesis claim in
the paragraph above is not weakened by this (it asserts that no reference holds the split), and neither
is D1–D4, which are derived rather than cited. What is affected is the arrangement's *plausibility at
scale* argument, which rests on wezterm alone.

**And that argument narrowed on the same day, which is why pinning wezterm was not worth doing to
settle it.** The bullet above cites wezterm for a bottom tier carrying *"no GPU resources at all"*.
D2 moved `instance_vbo` **into** the per-grid tier, so justerm's bottom tier holds a GPU buffer and is
no longer that shape — deliberately, for a reason wezterm's arrangement never has to face (it emits
every pane's quads through one allocator into shared layers, so there is nothing per-pane to hold). So
wezterm still supports *"one context, N panes, split ownership, at scale"*, and no longer supports the
narrower claim. Verifying it would confirm a description of wezterm, not a premise of this decision.

## Consequences

- **Every context-global setter added since 2026-07-07 is a migration item.** The slices since have
  added them all at context scope, because with one grid the distinction is invisible. By D1 every
  setter a consumer can call per terminal writes a **per-grid** selector — `setPalette`,
  `setDecorations`, `setOverlay`, `setActiveMatch`, `setCursor`, `setBoldToBright`,
  `setMinimumContrastRatio`, `setSelectionForeground`, `setBgAlpha`, `setCursorContrast`,
  `setCursorThickness`, `setPreedit`, and **also** `setFontSize`, `setFontFamily`, `setLetterSpacing`,
  `setLineHeight`; those last four additionally **re-key** a per-config resource (D3), which is the part
  that makes them expensive rather than the part that makes them per-grid. `setDevicePixelRatio`,
  `setOnContextLoss` and `setContextRestoreTimeoutMs` stay **global**. Recording the rule is the point:
  a setter added after this ADR gets its tier at birth — and unlike the table this replaced, the rule
  covers setters written after it.
- **The constructor straddles tiers too, and a surface cannot keep its shape.** `new(canvas_selector,
  palette_colors, default_fg, default_bg)` takes a **global** canvas and a **per-grid** palette in one
  call. Enumerating setters misses it; a `TerminalSurface` constructor takes the canvas, and the palette
  arrives with the grid.
- **Context loss becomes an app-level event, not a widget-level one.** One context means one loss and one
  recovery path for every terminal at once — simpler to reason about than N independent losses, but the
  blast radius of a failed restore is the whole app. `TerminalSurface` owns that path.
- **The atlas registry is the mechanism the middle tier needs.** Two terminals with the same font config
  are served by one atlas; a terminal that changes font size joins a different entry rather than
  mutating a shared one.
- **Sharing the atlas turns a bounded hazard into a cross-grid one, and this is the middle tier's real
  cost (recorded 2026-08-18, #768).** The glyph cache is LRU-evicting, and
  [glyph atlas](../map/territory/glyph-atlas.md) already records that *a within-frame eviction can
  corrupt cells already packed in the same frame*. Today that is bounded by one grid's own pack. Shared
  per-config, grid B's pack can evict a slot grid A's instance buffer still points at — and `render`
  re-packs only when `needs_repack` is set, so an **idle** grid never re-packs and the upload diff,
  whose stated purpose is to catch a glyph-slot change on an undamaged cell, never runs for it. The
  result is a wrong glyph with no self-heal until that grid happens to mutate. This changes **in kind**,
  not in degree: the diff was a complete defence while one grid owned its atlas, and stops being one the
  moment a second writer exists. ghostty pays nothing here because its atlas grows and never repoints a
  slot (above); the first implementation of this tier owes an equivalent guarantee — a slot pin for the
  frame, a pack epoch, or forcing every registered grid to re-pack when its atlas evicts.
  **Discharged by #772 (2026-08-19), and it took two of the three rather than one — because the
  three are answers to two different questions.** The list above reads as a menu and is not one: an
  eviction happens **only once a region is full**, so the hazard splits by whether the grids' *live*
  glyph sets fit a region together, and each half needs a different mechanism.
  - **They fit** (the reachable case). A sibling can still evict this grid's slots, because a full
    region evicts on any miss and an idle grid's glyphs age out. Answered by the *pack epoch*, in its
    cheapest form: the glyph cache counts what it repoints, a grid records the count it packed
    against, and `render` re-packs any grid whose configuration has moved on. It converges, and that
    is a property rather than a hope — the re-pack marks this grid's glyphs most-recently-used, so
    the next eviction takes one of the dead slots.
  - **They do not fit.** No epoch converges here, and re-packing alone leaves the grids alternating.
    Answered by the *slot pin for the frame*, made **render-scoped**: the render loop hands one pin
    set to every grid it packs, so a pack that cannot fit beside its siblings is **refused** exactly
    as an over-capacity single frame already was (`ResolveError::FrameExceedsCapacity`) rather than
    drawn wrong. A refusal also dirties every grid sharing that configuration, which is what makes
    the outcome a fixed point instead of an alternation; registration order decides which grid keeps
    its glyphs, the same order the draw loop already uses.
  **What this was measured against, because pixels alone cannot see it.** Two grids at 1200 distinct
  live glyphs each, against a region holding 1953: before the pin, one grid drew 911 lit subpixels
  beside its sibling and **891 alone** — stably wrong, every frame, with no error anywhere and a
  green pixel suite. After: 891 every frame, matching the control, with the refusal reported on every
  frame. A control in the same run is the only thing that separates the two.
- **A global input does not belong *in* the key; it belongs to the path that rebuilds every entry
  (sharpened 2026-08-19, #772 — this bullet read *"the key is (font family, size, spacing, DPR)"*).**
  The conclusion it drew was right and is now implemented: re-keying one entry and rebuilding all of
  them are separate paths in the registry's lifetime. The premise was not. One canvas means one
  drawing buffer and one `devicePixelRatio`, so the DPR is **globally constant across the registry at
  any instant** — no two live entries can differ in it, and a component every key shares cannot
  separate two keys. Putting it in would buy nothing and cost something real: a DPR change would have
  to rewrite every key, and until it did, every entry's key would be a lie. So the key is the four
  consumer selectors (family, size, letter-spacing, line-height) and the DPR is a global input every
  entry is baked at; `setDevicePixelRatio` rebuilds all of them in place, atomically — which is the
  one case where editing a shared entry is right rather than wrong, because nobody is being moved
  into a configuration they did not ask for. `max_texture_size` has the same shape and the same
  answer.
  **ghostty does hash the DPI** — `DesiredSize` is `{ points, xdpi, ydpi }` (`src/font/face.zig:50`),
  hashed into the grid key (`src/font/SharedGridSet.zig:564`) — and it does not transfer, for the
  reason already recorded for the cell in [multi-viewport](../map/territory/multi-viewport.md): a
  ghostty `Surface` is an OS window that can be dragged onto a monitor of its own density, so its DPI
  genuinely *is* per-surface. N viewports on one canvas have one density between them.
- **Memory becomes a scale question the current shape never had.** All grids' instance buffers are
  resident; a hidden terminal costs its buffer even when not drawn. **Measured in #770, so this now
  carries a number rather than a worry**: ≈171 B/cell as an upper bound (a wasm-heap slope, which
  includes each frame's transient copy), i.e. ≈800 KiB for a resident 120×40 grid and ≈3 KiB for a
  registered grid that has produced no output yet. The method and the caveats are in
  [multi-viewport](../map/territory/multi-viewport.md) rather than here, because the number will move
  with the instance format (#513, #455) and a record is a poor place for a value that tracks code.
- **Per-grid preedit needs an obligation ADR-0028 does not state.** That record already places the
  preedit per-grid under D1, and the reason it gives — consumer-settable per terminal — survives this
  adjudication verbatim. What it could not anticipate is N of them: a document has one focused element,
  so at most one grid should hold a non-empty composition, and nothing today clears grid A's when focus
  moves to grid B. ADR-0028's D1 ("each composition surface has exactly one writer") is about writers,
  not about how many surfaces may be non-empty at once. Whichever slice makes preedit per-grid owes
  that clause.
- **After #258, which has shipped.** The single-grid renderer had to be complete first; it is (Epic
  #258 closed, `justerm-web` switched in #273). This is a hoist of an existing structure, and the seams
  it hoists along (`build_pipeline`, `build_atlas`) already exist.

## Alternatives considered

- **(A) Keep one context per widget.** Rejected on the recurring costs first — an atlas, a frame loop and
  a re-bake-on-show per terminal, all paid at any count — and on the ceiling second: at the workload the
  product is for (tiling, many sessions) it fails outright, with a white terminal the page cannot refuse.
  Note the ordering: if the ceiling were the only argument, (D) would be a cheaper answer.
- **(B) `virtual-webgl`-style multiplexing.** Rejected on the author's own reasoning — it exists for apps
  that cannot change their renderer, costs a `drawImage` copy per canvas per frame, and carries an
  incomplete emulation layer with no error checking. We own the renderer.
- **(C) One canvas per widget, blitting from a shared offscreen context.** Rejected: it is (B)'s copy cost
  without (B)'s excuse, and it buys back only the z-order interleaving the constraint above gives up.
- **(D) Cap live contexts ourselves and recycle them (an LRU pool of contexts).** Rejected: it
  reimplements the browser's own eviction one layer up, keeps the re-attach flash, and still re-bakes an
  atlas per recycle — the cost this decision removes entirely.
  **Not a road not taken (recorded 2026-08-18, #768).** penterm shipped (D) —
  `terminalWebglPool` / `terminalWebglPort`, capped at 10 — before this ADR was written, and its own
  module doc names its demolition condition: *"Path 1(단일 컨텍스트 렌더러 …)이 착지하면 이 pool 전체가
  은퇴한다 — 이건 그때까지의 flash 완화 다리(bridge)다"*. Both predicted costs were paid there and are
  visible in its history: the first version released a context as soon as a terminal was hidden and
  every workspace switch flashed, which is why it was reworked into bounded warm retention. So (D)'s
  rejection is no longer a prediction — it is a downstream measurement, and this epic is what retires
  the bridge.

## Out of scope

- **Multi-window.** A second browser window has its own context by construction; whether surfaces can be
  shared across windows is an open question in #287, not decided here.
- **Heterogeneous cell sizes.** Two grids with different fonts imply different cell geometry in one
  canvas; the tier table admits it, the coordinate bridge for it is unspecified (#287 open question).
- **The consumer's layout.** Which terminal occupies which rect, and when, is the app's (ADR-0017).
