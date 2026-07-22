# ADR-0021: One WebGL2 context draws N terminal grids as viewports (`TerminalSurface`)

Status: **accepted as direction** (2026-07-22; the direction was decided 2026-07-07 in #287, recorded here
2026-07-21 with its citations verified). The ADR-0012 pattern: the decision stands, the implementation is
future work, and the tier rule below governs setters added before it lands. Scoped to **renderer resource
ownership** and the widget/canvas relationship; it does not change the core boundary (ADR-0017) or cell
composition (ADR-0019). **Not started** — but no longer gated: #287's blocker was the single-grid renderer
(#258), which shipped.

## Context

`justerm-web` today is one widget = one canvas = one WebGL2 context, inherited from the xterm.js /
beamterm shape it replaced (ADR-0002 → ADR-0018). One context per terminal costs something on every
frame and every switch, and fails outright at a ceiling.

**Grade of evidence, stated up front**, because this ADR mixes two kinds and a later reader must know
which to re-measure: the per-terminal costs below are **structural** (they follow from the code as
written, and are visible today at any terminal count); the ceiling is **anticipated, not observed** —
nothing in justerm or penterm has hit it, because penterm's webview still runs xterm.js and carries no
justerm npm dependency at all (`../penterm/package.json`, checked 2026-07-21). The browser numbers are
**verified against engine source**.

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

**This is designed against, not diagnosed.** Nobody has seen it here. penterm's relevant surface — Tauri
with WebView2 on Windows and WKWebView on macOS — sits on the two engines that *do* evict at 16, so the
ceiling is the right one to design against; Android's 8 and the worker limit of 4 do not apply to us. If
this ADR is ever revisited, the honest question is not "was the ceiling real?" (the source above settles
that) but "did the recurring costs justify it on their own?" — which they are claimed to, above.

## Decision

**One app-global WebGL2 context and canvas draws every terminal grid as a viewport.** The renderer
becomes multi-grid: `add_grid` / `set_viewport` / `apply_frame(grid)` and a single render loop that, per
visible grid, sets `gl.viewport` + `gl.scissor` and draws.

**Resources split three ways, and the tier is decided by what invalidates the resource:**

| tier | invalidated by | today's fields (`webgl.rs`) |
|---|---|---|
| **global** — one per context | context loss | `gl`, `raw_gl`, `canvas`, `program`, `vao`, `instance_vbo`, the `u_*` uniform locations, `max_texture_size`, `dpr`, `size` |
| **per-config** — one per (font family, size, DPR, spacing) | a font/metric change | `atlas`, `rasterizer`, `cache`, `cell_size`, `atlas_cell`, `char_size`, `char_offset`, `font_size`, `font_family`, `letter_spacing`, `line_height` |
| **per-grid** — one per terminal | that terminal's own frame | `palette`, `instances`, `instance_count`, `grid_size`, `cursor`, `cursor_span`, `last_flags`, `last_cols` |

**The rule, not just the table:** a resource is *per-config* when two grids sharing that configuration can
share the resource **byte-for-byte**, and *per-grid* when a difference between two terminals must be
visible on screen. Anything a consumer can set differently per terminal is per-grid **by definition** —
if it could not differ per terminal, it would not be a setting.

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

## Consequences

- **Every context-global setter added since 2026-07-07 is a migration item.** The epic's tier table was
  written when the renderer had few setters; the slices since have added them all at context scope,
  because with one grid the distinction is invisible. By the tier rule above, `setPalette`,
  `setDecorations`, `setOverlay`, `setActiveMatch` and `setCursor` are **per-grid**; `setBoldToBright`,
  `setMinimumContrastRatio`, `setSelectionForeground`, `setBgAlpha`, `setCursorContrast`,
  `setCursorThickness` are per-grid too — a consumer can set them per terminal, which is the test — while
  `setFontSize`, `setFontFamily`, `setLetterSpacing`, `setLineHeight` key the **per-config** tier and
  `setDevicePixelRatio`, `setOnContextLoss`, `setContextRestoreTimeoutMs` stay **global**. Recording the
  rule is the point: a setter added after this ADR gets its tier at birth.
- **Context loss becomes an app-level event, not a widget-level one.** One context means one loss and one
  recovery path for every terminal at once — simpler to reason about than N independent losses, but the
  blast radius of a failed restore is the whole app. `TerminalSurface` owns that path.
- **The atlas registry is the mechanism the middle tier needs.** Two terminals with the same font config
  share one atlas byte-for-byte (ghostty's rule); a terminal that changes font size joins a different
  entry rather than mutating a shared one.
- **Memory becomes a scale question the current shape never had.** All grids' instance buffers are
  resident; a hidden terminal costs its buffer even when not drawn. #287's open questions list this.
- **Blocked by, and after, #258.** The single-grid renderer must be complete first — this is a hoist of
  an existing structure, and the seams it hoists along (`build_pipeline`, `build_atlas`) already exist.

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

## Out of scope

- **Multi-window.** A second browser window has its own context by construction; whether surfaces can be
  shared across windows is an open question in #287, not decided here.
- **Heterogeneous cell sizes.** Two grids with different fonts imply different cell geometry in one
  canvas; the tier table admits it, the coordinate bridge for it is unspecified (#287 open question).
- **The consumer's layout.** Which terminal occupies which rect, and when, is the app's (ADR-0017).
