# theflow bindings (justerm)

Project-specific data for the `theflow` skill (the working discipline for a
substantive change to core/wasm/web/renderer). The skill holds the portable
*method* (seven steps + reasoning habits); this file holds justerm's *bindings* —
which reference to read, where the boundary falls, how to prove behavior, which
surfaces describe it, which gates to run, and how the downstream loop closes. The
method defers every concrete value here. (Authored/updated via `/grill-the-flow`.)

This is **not** web-only. Any substantive change to `justerm-core`,
`justerm-wasm-decode`, `justerm-web`, or `justerm-renderer` runs the seven steps.
Skipping a step is allowed only with an explicit "N/A because…" — a silent skip
is an untracked gap. The web form of the flow was established in slice S8 (#109).

Prior art cross-checked throughout: **Mosh · Alacritty · Warp · VS Code ·
beamterm** (convergence = non-arbitrariness).

**Tie-breaker — what wins when prior art and justerm's own evidence disagree.**
Not one value: the authority differs by layer, and flattening it would break one
of them. Prior art is always a *cross-check* that shaves detail a first-principles
model under-reaches; what it is checked *against* is:

| Layer | Authority | Grounds |
|---|---|---|
| **VT parsing / semantics** | the **spec** — above any implementation, including ours | ADR-0004: spec-faithful *where alacritty omits*. A reference's omission is not a licence to omit; this is what backs justerm's conformance claim |
| **Renderer cell composition** | **justerm's own model** (ADR-0019) | xterm is a design input, not a validator. In the four decisions before 0019 it was silent (#494), self-contradictory across its own call sites (#495), the outlier (#459), or demoted (#458) |
| **Glyph bake geometry** (what the rasteriser may do to a glyph before it becomes an atlas slot — place it, scale it, refuse it) | **justerm's own model.** Prior art here is a *mechanism catalogue*, never a validator | Both ADRs next door push this axis away in as many words — ADR-0019's *Out of scope* excludes "glyph rasterisation" and ADR-0022's excludes "rasterisation itself" — so a change here has no record to defer to and needs this row instead. The substance: all three references may leave a glyph to overflow because their quad **is** the glyph's bounding box (alacritty `alacritty/src/renderer/text/glsl3.rs:357`, ghostty `src/renderer/generic.zig:3202`, xterm `addons/addon-webgl/src/GlyphRenderer.ts:53` — whose `a_unitquad * a_size` is the single most decisive line: the quad is the glyph's own size and the cell contributes only an origin), and ADR-0019 gave that capability up deliberately to keep the composite one evaluation per pixel. So their **defaults** rest on something this renderer does not have and are unimportable; their **mechanisms** — xterm's opt-in quad squeeze (`RendererUtils.ts:47`, `OptionsService.ts:51`), ghostty's `Constraint` with `.fit` for symbols (`Glyph.zig:135`, `generic.zig:3175`) — are readable as design inputs. **Added 2026-08-21 (#792)**, which had to state this by hand in its lens brief because the row did not exist — the same cost #768 records one row up |
| **Renderer resource ownership / tiering** (which tier a renderer resource lives in — global, per-config, per-grid) | **justerm's own model** (ADR-0021 D1–D5) | The split has no reference to outrank it: ADR-0021 states in its own prior-art section that *"the three-tier keying is justerm's own synthesis — no cited reference splits resources global / per-config / per-grid"*, and **only the middle tier has direct precedent**. D1–D5 are derived from what keying costs (a lookup, an indirection and a lifetime, bought only when the shared thing is expensive to rebuild), so ghostty is a **convergence check**: removing it from the argument leaves the derivation standing. Two of the four cited sources (wezterm, three.js) have no pinned tree at all, so they cannot arbitrate even in principle — and #768 narrowed what the wezterm one supports, since D2 put `instance_vbo` in the per-grid tier and wezterm is cited for a bottom tier holding *no* GPU resources. **Added 2026-08-18 (#768)**, which had to state this by hand in its lens brief because the row did not exist |
| **Wire / frame / API shape** | **this repo's own precedent** | No external authority exists — no reference serializes a terminal state this way (see the architecture prior-art note below: composing a render-free engine with a state wire is justerm's own bet) |
| **Consumer-facing API shape / units** | **our own API's internal coherence** | ADR-0023: `letter_spacing` is CSS px because `font_size` is, though *both* references use device px. A setting expressed in the same space as an existing one must use that space — an API the consumer has to remember two spaces for is incoherent, not merely different. Same posture as the composition row, one layer down |
| **Performance claims** | **our measurement, on a release build** | A claim about our own throughput was wrong because it was measured on a debug build; a number from a consumer's journey is a hypothesis until re-measured here |
| **Who owns a fact that several sites read** | **our own producer** — the site the fact is *first true* at, never a copy, a derivation or a report of it, however locally correct that proxy is. A reference's placement is **unimportable** here | The references answer this inside architectures that never have to ask it: xterm's `Marker` is a live object the buffer mutates in place (`src/common/buffer/Buffer.ts:646`, SHA `699f553`) so there is no basis to reconcile, and every render-free engine hands off *in-process* (the architecture paragraph below — alacritty by borrow, libvterm by C callbacks, ghostty by lock-shared state). Frame-mode's stateless consumer is what creates the question, so this row is structural, not a preference. Derived independently **four** times before being written down once: ADR-0025 D1 (owner = the property's scope; the encode-time cell bit is *never* the authoritative copy), ADR-0026 D2 (the bound site follows from whether the engine owns a **producer** for the coordinate), ADR-0027 D1 (liveness is answered by the source that owns it — the listener's flag owns *"have we been told"*, a different fact), ADR-0028 D1 (each composition surface has exactly one writer). **The failure form is why these arrive as clusters rather than bugs:** each site reaches for whichever nearby signal resembles the answer, and the resemblance holds everywhere except the window that matters, so every site is locally right and review cannot see it. Ask *which site owns this* before *which local rule is better*. **Scope** — this row places a fact and says nothing about how long an owned value stays true once it leaves the owner; that axis is live and housed one rung down (spine #630 for derived state's lifetime, `docs/map/invariant/a-coordinate-carries-the-instant-it-is-true-at.md` for a published coordinate's basis/epoch, landing with #740), and a second home would split its roster. **Promotion falsifier** — if this row derives a site nobody had to be told about, or settles a question before it is asked, it has earned an ADR; until then it routes, and an ADR over the four records above would be archaeology |

A layer not in this table has no recorded tie-breaker — say so and ask, rather
than borrowing a neighbouring row.

**Deliberate divergences — where justerm does *not* follow its own named prior art,
on purpose.** The table above says who wins an argument; this says which arguments
are already over. It is what Step 5's reference-free restatement test is checked
against: a finding that lands here is `DELIBERATE` with the citation, never a
defect, however confidently a lens reports it.

| We do | The references do | Decided by |
|---|---|---|
| The consumer holds **only the current frame** — no retained terminal state | every reference consumer retains state; even Mosh's receiver keeps a full `Complete` | ADR-0020 R3's grounds (research §6.1). Its (C) *"event-source everything; let the consumer maintain the state"* is rejected **as the default**, so "make the consumer stateful" is not an open question |
| Colours are stored as **references** (`Default`/`Indexed(u8)`/`Rgb`), never resolved | all three resolve a palette in the engine | `CLAUDE.md` identity (theme-agnostic). justerm never learns a hex colour |
| **Per-char `UnicodeWidthChar`** width; cluster width is opt-in behind DECSET 2027 | xterm.js clusters by default | the contract in Step 2 below (#297/#300 → #301, subsumed by #295/#305). A consumer unhappy with it is standing on nothing valid |
| A cell's bg/fg/ink is decided by **our layer model** | xterm's flat `$fg` over a blended `$bg` | ADR-0019 — xterm is a *design input*, not a validator |
| A single-cell glyph the font draws **wider than its box is condensed at bake**, by default, with no setting | xterm.js rescales only behind `rescaleOverlappingGlyphs`, **default `false`**, and then only past 1.5 cells (`src/common/services/OptionsService.ts:51`, `src/browser/renderer/shared/RendererUtils.ts:47`); ghostty gives ordinary letters `.none` and constrains only symbols (`src/renderer/generic.zig:3175`); alacritty never rescales | #792, on the **glyph bake geometry** row above. Their default is *overflow*, which this renderer cannot do on the horizontal axis: the bleed band is sized from the face's declared line box and the Canvas API exposes no horizontal counterpart, so the real choice here is condense-or-destroy rather than condense-or-overflow. Measured before the change: 252 clipped codepoints on DejaVu Sans Mono and 1153 on the demo face, 35 to 629 of them losing over 30 % of their ink, with `Ǆ` reaching the screen as `D`. A lens reporting "the reference makes this optional" is `DELIBERATE` with this row |
| A spacing setting is **CSS px** | both references take device px | ADR-0023 |
| A box with **no area is refused**, not floored — `proposeDimensions` / `gridForBox` answer `undefined` for a `0x0` container, the way they already do for a `NaN` one | **Mixed, and the mixture is the point.** *Converges* with alacritty, which refuses on the raw box with this exact predicate before any arithmetic runs — `if size.width == 0 \|\| size.height == 0 { return; }` (`alacritty/src/event.rs:1958-1964` @ `852e971`), whose comment says it receives `0x0` **routinely**, on window minimize on Windows, and names the downstream harm (ConPTY). *Diverges* from **ghostty**, whose `sizeCallback` has no zero branch (`src/Surface.zig:2482-2496` @ `e6e26e1`) so a zero box reaches `@max(1, calc_cols)` (`src/renderer/size.zig:260`) and is gridded. *Diverges* from **xterm.js**, which refuses a **detached** terminal (`addons/addon-fit/src/FitAddon.ts:61` @ `699f553`) but not a `display: none` one — that still has a `parentElement`, and `Math.max(0, parseInt(...) \|\| 0)` at `:77-78` normalises the unreadable measurement to `0` and then floors it | #810, on the **consumer-facing API shape** row — our own coherence. The reference-free ground is one layer down in this family and predates the question: `justerm-renderer/src/webgl.rs` refuses a zero drawing-buffer read-back with the same predicate and the same sentence — *"A buffer of no size is not a grant, it is the absence of an answer"* (#639). So #810 is this family agreeing with itself, and only the guard's **placement** differs from alacritty's: ours sits inside the pure function because the pure function is the published API. Severity settled the last doubt — the floor proposes `2x1`, and on the **alt screen** a resize is a re-fit that drops rows (`docs/map/territory/reflow.md`, #567) *and* clears the selection on any geometry change (`justerm-core/src/term.rs:1516`, whose comment already reasoned about "a consumer that re-asserts its size every frame (a `fit()` loop)"). **An earlier version of this row claimed all three floor and that "we receive an input they do not"; both were false, and one `rg` over `event.rs` refutes them** |
| Both contrast ratios live on the web **`Theme`** — the text one (`minimumContrastRatio`) and the cursor one (`cursorContrast`) | neither reference puts contrast in its colour scheme: xterm.js types both as *options* and its `ITheme` is colours only (`typings/xterm.d.ts:372`), and alacritty's cursor guard is a non-configurable constant (`alacritty/src/display/content.rs:22`) | #225, extended by #580, on the **consumer-facing API shape** tie-breaker row — our own API's coherence. What the cursor guard defends is `cursorColor`, which is on `Theme`, so the threshold has to travel with a theme swap; splitting the two contrast ratios across two homes is what would be incoherent. Rows pinned in `reference-facts.md` § "Cursor policy knobs" |
| Renderer resources tier **three ways** — global / per-config / per-grid — and the per-grid tier **holds GPU state** (`instance_vbo`) | ghostty tiers font machinery per-config (`SharedGridSet`) but puts the GPU device, atlas texture and render thread **per-surface** (`Surface.zig:86-92`), i.e. its bottom tier is the device; wezterm tiers per-window GPU state and per-pane non-GPU state with **no config tier**, its `PaneState` holding no GPU resources at all | ADR-0021, adjudicated in #768. Both references are *shapes we chose against*, and for opposite reasons: ghostty's arrangement is the one this design exists to remove (a device per terminal), while wezterm's per-pane tier can hold nothing because it emits every pane's quads through one allocator into shared layers — justerm packs per grid and diffs per grid, so its bottom tier must hold the buffer. A lens reporting either as a defect is `DELIBERATE` with this row |
| A **viewport rect** is given in **device px, top-left origin**, and the renderer flips it to GL's bottom-origin y itself | three.js takes a viewport in **CSS px with a bottom-origin y** and multiplies by the pixel ratio it owns (`src/renderers/WebGLRenderer.js:804-816`, SHA `83d8667`), leaving the flip to its caller | #771, on the **consumer-facing units** tie-breaker row — our own API's coherence. `cell_width()` already declares device px to be the space for *"anything that addresses the drawing buffer — `readPixels`, GL interop, a picking rect"*, and the flip needs the **granted** buffer height, which this renderer owns and the consumer's `canvas.height` may not equal (#339). three.js can push both outward because its caller supplies fractions of a canvas the renderer itself scales; ours supplies a measured DOM box. Taking CSS px would also import three.js's rounding step, which is the error #337 exists about |
| A marker is an **object with identity** — `MarkerId` + kind + exit code + column | ghostty stores OSC-133 as a 2-bit field on the row (`page.zig:1976`); alacritty has no line-mark concept at all | ADR-0015. Row-attached state cannot carry any of the four, so "put the marks on the row" is not a smaller version of a marker — it is a different primitive |
| A cell **stores** a variation selector on a non-emoji base (`x` + VS16), so text extraction hands it back | ghostty drops it — *"the terminal does not store those selectors in the cell, so callers must also restore their grapheme break state"* (`src/unicode/grapheme.zig:56`) | #317 §1, on the **spec** row of the tie-breaker above (ADR-0004). Not a UAX #29 disagreement: ghostty's own `graphemeWidth('x', 0xFE0F)` returns `len = 2` (`:315`), so both agree the selector is in the cluster — they differ on whether the cell keeps what the cluster contains. Widths are identical, so this is invisible on screen and observable only in a copy |

Add a row when a decision *chooses against* a reference; that is cheaper than
re-defending it, and the cost of the empty slot is measured — see the #490 entry in
the war-story index.

**Architecture prior art (routes "engine vs renderer / state-sync" questions).**
justerm's frame-mode identity composes two independent lineages: ① a *render-free,
reusable terminal-state engine* — **alacritty_terminal** (Rust, CLAUDE.md's named
model: grid + `Term` + damage, no rendering), **libvterm** (C, bytes→screen-state +
damage callbacks, embedded by neovim), **wezterm-term** (Rust), **libghostty** (Zig,
explicit reusable core; also the #287 multi-viewport `SharedGrid` reference); and ②
*serialized terminal STATE synced over a wire* — **Mosh** SSP (diffs screen state, not
bytes — the canonical remote/thin-client prior art) and **tmux** (server holds the
grid, clients repaint). Most projects do only ONE half: alacritty_terminal/libvterm
split the engine but render in-process (never serialize); Mosh syncs state but exposes
no reusable engine crate. Composing both is justerm's bet (ADR-0012→0018). **The
consequence bites #482:** xterm's O(D) decoration walk (`decoration.marker.line`, read
live) needs whole-buffer live objects, which frame-mode gives up — the consumer holds a
flat marker *snapshot* and pays an O(M) index to correlate; that cost and the
remote/portable/renderer-swappable wins are the *same* choice, not a bug. The
marker/decoration-over-wire combination is a **prior-art gap** (split engines lack
OSC-133 marks; Mosh diffs cells, not semantic marks), so route its *mechanism* pieces
separately — marker→line mapping = xterm (whole-buffer), state diffing = Mosh SSP.
Verified sharpenings (real-source dig, `docs/research/terminal-engine-renderer-architectures.md`):
the true novelty is the **stateless consumer** — every render-free engine hands off
*in-process* (alacritty by borrow, libvterm by C callbacks, ghostty by lock-shared state),
and even Mosh's receiver keeps a full `Complete` state, while justerm's holds only the
current frame. **#482 fix ceiling:** within frame-mode the tractable step is O(D)
*allocation/iteration* — the per-frame snapshot scan stays O(M); going below O(M)/frame
needs marker positions on an out-of-band event channel (#160-style) so the consumer keeps
a persistent, incrementally-updated index (the xterm `_lineCache` / ghostty tracked-ref
model), which per-frame snapshotting structurally cannot have.

## Crate / module map

| Member | In `--workspace`? | Gate note |
|---|---|---|
| `justerm-core` | yes | engine — parsing + grid + scrollback + selection; published to crates.io, API docs on **docs.rs** |
| `justerm-wasm-decode` | yes | wasm decoder binding; published to npm; a public-API change can silently break it (happened in 0.4.0) |
| `justerm-web` | no (pnpm) | web widget; consumes the *published* `justerm-wasm-decode` (the version pin lives in its manifest, not here) |
| `justerm-renderer` | no (excluded) | own renderer (glow/web-sys, wasm32-only); has its own CI jobs |
| `fuzz` | no (own `[workspace]`) | out-of-workspace blind spot |
| `justerm-facade` | no (excluded) | one-shot `justerm` 0.5.1 tombstone, off the version lockstep |

`--workspace` is required at the root (virtual manifest, no `[package]`), and it
**does not even build** the excluded members — so renames / public-path changes
need the separate checks in the gate matrix below.

**Consumers (derive, never guess — check the *right* manifest):**
- *In-repo* — `justerm-web` (consumes published wasm), `justerm-renderer`.
- *Cross-repo* — **penterm** at `../penterm/src-tauri/Cargo.toml` (`justerm-core =
  "0.6.0"`, from crates.io). penterm's Rust dep lives under `src-tauri/`, **not**
  the repo-root manifest — a top-level `grep` misses it and falsely reports "no
  consumer". **Its webview is still xterm.js** (verified 2026-07-21:
  `../penterm/package.json` carries `@xterm/*` and *no* justerm dependency —
  `justerm-wasm-decode` and `justerm-web` adoption is planned, not done, per
  `penterm/src/features/block/lib/isTerminalKind.ts`). So the npm packages have
  **no known consumer**; do not treat penterm as one until that manifest says so.

## Step 1 — reference routing table

Read real source from the **local pinned reference trees** in `../.refs/` (sibling
of the **main checkout**, same convention as `../just-shield`), with
`rg -n <symbol> -A 8`. Working in a worktree, `../.refs/` is *not* that path and
returns zero hits instead of an error — see Step 7 "What a worktree breaks".
**WebFetch is banned** — it summarizes and drops method bodies (e.g. xterm.js
`InputHandler.ts`, 3.7K lines: the registry shows, handler bodies like
`setOrReportIndexedColor` get cut).

| Reference | Path | Pinned SHA (2026-07-24) |
|---|---|---|
| alacritty | `../.refs/alacritty` (sparse: `alacritty_terminal`, `alacritty/src`) | `852e971cddfabe222d2d5bcda466e130f53af207` |
| ghostty | `../.refs/ghostty` (sparse: `src`) | `e6e26e165ab143f087761cee9f8a479801a27ba7` |
| xterm.js | `../.refs/xterm.js` (sparse: `src`, `addons`, `test`, `typings`) | `699f5537b0232e444cb98261b8b3991c3cfecb5e` |
| three.js | `../.refs/three.js` (sparse: `src/renderers`, `examples`) | `83d8667898fd32a6a0f1af92f6d91065db272ce2` |

Create them once (they are outside the repo, so nothing to gitignore):

```bash
mkdir -p ../.refs && cd ../.refs
git clone --depth 1 --filter=blob:none --sparse https://github.com/alacritty/alacritty alacritty
cd alacritty && git sparse-checkout set alacritty_terminal alacritty/src && cd ..
git clone --depth 1 --filter=blob:none --sparse https://github.com/ghostty-org/ghostty ghostty
cd ghostty && git sparse-checkout set src && cd ..
git clone --depth 1 --filter=blob:none --sparse https://github.com/xtermjs/xterm.js xterm.js
cd xterm.js && git sparse-checkout set src addons test typings && cd ..
git clone --depth 1 --filter=blob:none --sparse https://github.com/mrdoob/three.js three.js
cd three.js && git sparse-checkout set src/renderers examples && cd ..
```

**three.js was pinned on 2026-08-19 (#771), and only because a slice needed a fact from it.** ADR-0021
cites four sources and two of them — three.js and wezterm — had no tree, so half its prior art was
uncheckable; #768 declined to fix that in the abstract and said to pin one *when a slice needs a fact,
for that question*. #771 is that slice: it draws N grids as viewports, and three.js's multiple-views
example is the named mechanism reference. Pinning it settled two things a summary would not have —
its per-view loop does **no** full-canvas clear (its views tile the canvas, so it has no gutter to
answer for), and `renderer.setViewport` passes straight to `gl.viewport` with the caller supplying a
bottom-origin `y`, so the y-flip is *not* reference-supplied and is ours because our input is a DOM
box. **wezterm still has no tree**; pin it the same way, for the question that needs it.

**`typings` was added to xterm.js's set on 2026-08-07 (#743), and `test` on 2026-08-06 (#733); the pin
did not move either time.** The second one generalises the first: #743 was about a *consumer-facing*
coordinate, and the corpus for a consumer-facing question is the **published API surface**, which in
xterm.js lives in `typings/xterm.d.ts` and nowhere in `src`. Everything that settled the mechanism came
from there — `IBufferNamespace`'s three handles, `IBuffer.type`, the alt-screen consequence stated in
prose on `Terminal.markers`, and `getLine`'s *"use immediately"* re-ask declaration. With the old set
all four read as absent. **The rule the two share: widen by the kind of question, not by the file you
happened to want** — a semantics question wants `src`, a harness question wants `test`, an API-shape or
contract question wants `typings`. Widening a
sparse checkout is not a pin refresh — it exposes paths that were already at the pinned SHA, so no
recorded line number is invalidated. It is worth doing when a change's real source lives outside
`src`: #733 was a **test-harness** change, and with the old set the reference corpus read as absent
(zero hits — the silent failure this file warns about two paragraphs down), when in fact xterm.js
carries a full Playwright suite at `test/playwright/`. "The reference has nothing on this" is a
claim about a checkout at least as often as about a project.

**Recording what you found: do not type the line number.** `rg` is for *finding*; the
number that ends up in [reference-facts.md](reference-facts.md) comes out of the tree, via
`node .github/scripts/cite.mjs <tree> <path> --find '<text>'` to locate it and
`… <tree> <path>:<line>` to print it with context and emit the `Site` cell. This exists
because the transcription step is where the errors were: five wrong rows in two days
(#610), four of them wrong at the moment they were written, all five from copying a lens
report instead of re-opening the source. The tool resolves the trees from the **main
checkout**, so unlike the bare `../.refs/` above it is correct from a worktree too;
`--pins` checks the local trees against the SHAs in the table above. It does not read the
row *for* you — two of those five were a correct citation with a wrong conclusion drawn
from it, which is a class no tool reaches.

**Why local rather than `gh api`, which this step used to prescribe.** `gh api`
cannot grep: an 8-line fact costs a whole-file fetch (`Terminal.zig` is ~10K lines)
over the network, *and* the whole file lands in context, so every later turn in the
pass is slower too. Latency scaled with `files × size`, not `facts × difficulty` —
which is why a Step 5 pass took 20–30 minutes. Measured on the switch: four cited
facts (alacritty `clear_wide`, its `LEADING_WIDE_CHAR_SPACER` removal, ghostty
`blankCell`/`clearCells`, xterm.js `_eraseAttrData`) resolved in **0.35 s** total.

Two things this buys beyond speed, both load-bearing:

- **A citation can be checked in seconds.** #530's body claimed ghostty defaults a
  freed cell, scored the references 2:1, and was wrong — `printCell` calls
  `Screen.clearCells` (`Screen.zig:1667`, filling `blankCell()` at `:1929`), not
  `page.zig:1215`'s zeroing `clearCells`. The real tally was 3:0. One `rg` settles
  that class of error; a network fetch is expensive enough that nobody re-checks.
- **Line numbers stop rotting.** Issues cite `file:line`; upstream moves. #534 cites
  alacritty `term/mod.rs:1006-1008`, which is `:1007` at the pinned SHA above. Cite
  the SHA with the line, or the reader cannot tell drift from error.

**Refreshing a pin is a deliberate act, not a habit.** `git fetch --depth 1 && git
reset --hard origin/<default>`, then update the SHA here in the same change — a pin
that moves silently makes every recorded citation unverifiable at once. A refresh
also invalidates the line numbers in `reference-facts.md`; re-verify the rows it
moves in the same change.

**Start from `docs/agents/reference-facts.md`, not from a blank tree.** It is the
accumulated map of what each reference actually does — every row `file:line` at the
pinned SHA, grepped before it was recorded. Read the section covering your area first,
then go to source for what is missing, and add what you learn back as a row. Three of
its rows exist because the obvious grep hit gives the *wrong* answer (ghostty's two
`clearCells`, xterm.js's `save`/`restore` bracket around the underline ink, and which
function actually takes `clearWrap`) — that is the class of mistake the file is for.

**The issue axis has the same ledger.** `reference-facts.md` keeps the *code* side from
starting blank; the worked issue's **spine** — or, where its area already carries one, the
record — does that for the *decision* side. Read it before Step 2 (`gh issue view <n>
--comments`) and treat its suspected root as a hypothesis to test, not a finding to build
on. Both #535 (PR #546) and #533 (PR #548) were worked this way against ADR-0025's roster
rather than out of their own bodies, and #546 wrote back: its "gate uniformly" amendment
records that D4 answered a combination the draft had not anticipated.

**And the third ledger is the map — `docs/map/` (hub `docs/map/README.md`), read here,
at the start.** `reference-facts.md` holds the *code* side and the spine holds the
*decision* side; the map holds the side neither can: **what else moves when you touch
this** (`## Blast radius`) and **which facts hold beyond this territory**
(`## Cross-cutting invariants`). Open the territories the change touches, and follow
the invariants they list — those are promoted precisely *because* they are invisible
from the territory you are standing in.

This document already sends you to the map twice, at **Step 5** (the lens brief) and
**Step 6** (coverage + promotion), and both are too late to change a design: by then
the boundary is drawn and the tests are written. `CLAUDE.md` has always called it the
*착수 전 배선도* — the wiring diagram you open **before** starting — and this section is
where that lands in the flow. Worked example, #661: the wire-format territory supplied
the problem statement (its `## Known holes` carried the defect's residue, sharper than
the issue body), the *placement vs annotation* rule that later settled a follow-up
candidate as "not a defect", and the *`u32` iff viewport-bounded* rule the change's new
invariant note was built on. All three were in hand before a line was written — and none
of them is reachable from the issue or from a reference tree.

Reading it at the start is also what makes Step 6's **promotion** obligation possible:
you cannot notice that a fact holds outside its territory if you never read the
territories.

**Read the third column before you open a tree.** It is the tie-breaker table at the
top of this file, indexed by the thing you are about to do — because the two halves
were two hundred lines apart and only *this* one is read at Step 1. What that cost is
the #490 entry in the war-story index: a verified, correctly-cited reference fact
carried to the maintainer as a peer option on a layer where the table gives the
reference no vote at all. The routing column says which tree to open; the authority
column says what a divergence found in it is *worth*, and the second question is the
one nobody was asking. **Neither column is a licence to skip a corpus** — Step 5's
never-drop rule is unchanged, and no authority here makes a reference unreadable; it
makes a divergence `DELIBERATE` instead of `CONFIRMED`.

| Change type | Real source to read | What the reference's word is worth here (tie-breaker row) |
|---|---|---|
| **Web feature (concept/UX)** | its real source — usually **xterm.js** (`repos/xtermjs/xterm.js`; e.g. drag-scroll 50px/15, highlightLimit 1000, `_charsToConsume`); for features xterm lacks, the consumer that built it (e.g. **VSCode** `microsoft/vscode` terminal a11y) | **No recorded row** — the tie-breaker table has none for a concept/UX call, so its closing rule applies: say so and ask, do not borrow a neighbouring row. In practice the reference leads on *what the feature is* and stops at the seam where the answer becomes an API shape or a unit, which the row below governs |
| **Text / coords / VT-semantics (mechanism)** | **xterm.js buffer layer + alacritty real source** + *this repo's siblings* (`docs/architecture.md` §"Hidden VT state" + `search` / `selection` / `logical-lines` cell-walk). Enumerate the hidden state the reference tracks *first* | **VT parsing / semantics** → the **spec** outranks every implementation including ours; a reference's omission is not a licence to omit. This is the one layer where reading the trees is load-bearing rather than corroborating — the hidden state (pending-wrap, spacer, BCE, soft-wrap join) is what only an implementation carries, and #113 → #144 → #207 is the measured cost of not enumerating it |
| **Wire / format / coord / API shape** | *this repo's sibling fields & precedent* — #129 `mouse_events`, #112 scroll, #108 overlay: how they touch struct→encode→decode→Flat→getter→`types.ts` — plus **ADR-0013/0014** (viewport state in the header) and **ADR-0008** (decode boundary). Mirror the most recent sibling verbatim | **Wire / frame / API shape** and **Consumer-facing API shape / units** → **no vote**. No reference serialises a terminal state across a boundary, so none has ever had to answer the question; ADR-0029 and the #743 row of `reference-facts.md` record that as a *mechanism* finding (they make it unaskable) rather than an arbitration. A reference read here is a search index, never a comparand |
| **Renderer cell composition** (what colour a cell's bg / fg / ink ends up) | **ADR-0019 first** — its layer stack, per-channel declaration, paint modes and ink sources answer the combination *by construction*, so start by asking what the model says. Then this repo's siblings (`overlay.rs`, `frame.rs`, `decoration.rs`, `glyph_class.rs`) for the rules in force. **xterm.js is a design input here, not a validator**: read it for *what problem exists* and how it solved it, never as the tie-breaker — a difference from it is not by itself a defect, and in the four decisions before ADR-0019 it was silent, self-contradictory, the outlier, or demoted. A combination the model cannot answer is an ADR-0019 amendment, not a new decision | **Renderer cell composition** → **ADR-0019 governs**; xterm is a design input. With one standing exception worth naming because it was waved off once: *a reference agreeing with another reference against you is signal* (the ADR-0019 first-amendment retraction — xterm's flat `$fg` and alacritty's inverse-text guard had both said so from the start) |

Cutting across all four: **who owns a fact several sites read** is answered by our own
producer, and *"a reference's placement is **unimportable** here"* — so a routing
answer of the form "xterm keeps it on the row / in the marker" is out of scope on
every row of this table, not just the one you are standing on.

**Concept ≠ mechanism (the trap).** A feature has a concept layer *and* a
mechanism layer. It can be novel at the concept layer (absent from xterm.js) yet
its mechanism (text extraction, wrap, wide-char) still lives in xterm/alacritty's
buffer/parser layer — read **both**. (#150 accessible-view: concept = VSCode
`terminalAccessibleBufferProvider`, but extraction semantics = xterm.js
`translateToString` / `isWrapped`; skipping xterm as "no such feature" would miss
the extraction layer.)

**Hidden VT state** lives in `docs/architecture.md` §"Hidden VT state". Add to it
*before* implementing semantics work. Classic examples the naive model omits:
pending-wrap, wide-char spacer, soft-wrap join, BCE. **Removing a field/flag is
the mirror image** — a value read *incidentally* (feeding a boolean, gating a
branch, computed into something else) is unpinned the moment you delete it; grep
every read site first.

**External / registry facts are verification targets too.** Version state, a
published API's shape, a wire VERSION — check the *real* source (the registry, the
raw file), not a sentence about it. justerm's live trap: `justerm-web` consumes
the **published** `justerm-wasm-decode`, so a new binding is `undefined` at
runtime until republished; a local pkg-swap pollutes the pnpm store (`--frozen-
lockfile` won't fix it). **Judge published-package questions in a clean-room
worktree only** (detect drift with `npm pack`; recover with store prune +
`--force`).

**To pin a runtime fact, instrument a throwaway probe.** For a real coordinate /
call-order / emitted-event value, write a disposable probe (renderer: through
`demo/proof.js`, `cell_width()` in device px), read the number, delete the probe,
record it in the issue. Reading code ≠ observing it — the dpr≠1 coordinate bugs
were all *green* on a dpr-1 machine (#328/#331).

**"Unconfirmed ≠ absent."** A summary/search *not showing* a fact does not make it
absent — that is a **gap** (surface as an issue or ask), never a silent
load-bearing assumption. Its inverse: **a cleared concern is recorded with its
validity condition** ("this path is fine *as long as X holds*") in the issue, so
the next person does not re-run the investigation and it does not silently break
the day X changes.

## Step 2 — boundary rule (ADR-0017)

A mechanism is **core** iff it is ① VT-parsing, or ② only correct with the
*whole buffer* (all cells, scrollback, coordinates, wrap, wide-char) — a
frame-mode consumer holds only the viewport and physically cannot. But *policy*
(query · regex · palette · announce policy) is injected by the consumer so the
core stays policy-/theme-agnostic. **Mechanism core, policy consumer.** (web's
write seam = `FrameSource` siblings `SelectionPort` / `SearchPort`…; queries are
`Promise` IPC; web draws frame overlays but never runs the engine.)

The core invariant (justerm's identity — see `CLAUDE.md`): no I/O, no IPC, no
rendering, theme-agnostic (colors stored as `Default` / `Indexed(u8)` / `Rgb`
references only). Owned **by the consumer by definition** (not a workaround):
color interpretation, hover, pixel→cell, debounce, scrollbar, clipboard,
transport.

**Contract ≠ defect (diagnose before fixing "at the root").** When a consumer
reports a "bug", ask *whose invariant broke*. theme-agnostic color and **per-char
`UnicodeWidthChar` width** are contracts justerm *deliberately* holds — a consumer
unhappy with them is standing on nothing valid, and "fixing at the root" means
fixing the consumer, not deleting the contract.

**The boundary is a membrane — it leaks both ways.** A core floor (edition 2024,
a future `rust-version`/MSRV, a new required capability) rides a caret/compatible
range straight *down* to penterm and web. And a contract change makes a
consumer's *rationale* go stale — obliging the Step 6 sweep downstream.

**Two consumers reaching the same workaround = a bug report against the core
default**, not a coincidence. Weigh "add an option but keep the trap as default"
accordingly.

**No consumer workaround for a core defect.** justerm precedent: #297/#300 — a
core VS16 (FE0F) width gap was worked around in the renderer via FE0F detection;
that was blocked, root fix tracked as #301 (later subsumed by mode 2027
#295/#305, tail #303/#304). **When you feel the urge to make a consumer test pass
by compensating: stop, explain to the user, ask whether to root-fix** — don't
work around alone, don't silently file-and-move-on. Then fix at the root or leave
the gap visible + tracked (and assert the *real* behavior honestly).

## Step 3 — the test-trust gate

Beyond `/tdd` RED→GREEN, a passing test earns trust only after two bars: **(1)
discriminating power** — turn the fix off, confirm it goes red (a green from a
test you never saw fail is not evidence), and **(2) right reason** — assert the
side conditions (the callback that must *not* fire, the exact count). justerm
precedent: **#355** — a mutation test needs a *fresh baseline* re-run in the same
pass (both RED = you broke the proof); remove guards one at a time and check a new
guard fires before the old one.

**A third bar, because the two above were all met and the fix was still wrong:
mutate the PREDICATE, not only the placement (#639).** Moving a guard between
neighbouring statements, or deleting it, shakes *where* it runs. It says nothing
about whether it asks the right question — and a predicate swapped for a
**differently-wrong** one is invisible unless some assertion lives in the window
where the two disagree.

What that cost here: `resize`'s lost-context guard asked the state machine's
event-driven flag when the question was *"is the drawing buffer readable"*. A
browser destroys a WebGL context **synchronously** and only **queues**
`webglcontextlost`, so in that window `gl.isContextLost()` is already `true`,
`drawingBufferWidth` already `0`, and the flag still `false`. The proof did
`await once("webglcontextlost")` before the call under test, so it never entered
that window; guard and test were written against the same wrong model of *when* a
context dies and therefore confirmed each other. Placement was mutation-tested and
green. The defect the fix was written for survived it verbatim, measured
end-to-end.

So: ask what else the predicate could plausibly have been, swap it for that, and
require red. Staying green means the assertions never reach the state where the two
differ — add that assertion *before* fixing anything. Two habits fall out, both
cheap:

- **Assert the window exists before asserting behaviour inside it.** #639's proof
  now checks `glSaysLost && !rendererSaysLost` first, so if a browser ever
  dispatches synchronously the section *reports* that instead of passing vacuously.
- **Suspect any condition that is a proxy** — a flag for a state, an event for a
  transition, a successful return for liveness. The same pass found
  `getContext("webgl2")` succeeding being read as "the context is live" when it
  hands back the *same lost object* (#688). That cluster is spine **#689**.

## Step 4 — proof method per layer (real round-trip, not a fake)

| Layer | Real proof |
|---|---|
| **core / wasm** | `encode→decode` round-trip (ADR-0005) · `vttest` · **real PTY capture** (the RHEL 9 VM, `capture-dogfood.sh` — vim/top/htop; TUI needs a foreground timeout, alt-screen apps snapshot just before `?1049l`; access procedure in §"Recording a capture on the VM" below — it is agent-side, no user in the loop). **A capture proves only what its golden asserts, and only what its material contains** (#554): capture tests go through `check_capture`, which pins the char grid *and* the logical lines, because the soft-wrap link is not a character and a char-only golden stays green through a merged logical line. And a TUI capture cannot supply soft-wrap material at all — a program that emits IL/DL positions every row with CUP — so that half comes from `capture-softwrap.sh` (a deterministic printf plus a real `less`). Before trusting a new capture, turn the fix it guards *off* and confirm the golden goes red: the first `softwrap_shifts.raw` exercised all five row-shift verbs and still passed with #540's repair disabled, because later shifts washed the stale flags out. **A corpus can supply an axis and still miss its combination** (#534): after #554/#555 the corpus had soft-wrap material, and all six captures still observed the wide-wrap artefact marker **zero** times — soft-wrap material is not *wide*-soft-wrap material, so "this area is covered now" was true of one axis and false of the pair. Measure the state the fix is about (count it during replay), don't infer coverage from the feature the capture was named for |
| **web** | `pnpm demo` real browser (DPR / coords / render bugs; canvas buffer = CSS×DPR, geometry from `rect.h/ROWS`) + `pnpm test:e2e` (Playwright headless, `webServer` auto-starts `pnpm demo` → real wasm+controller round-trip). a11y proven via **SR-consumed proxies**: announce = aria-live `textContent`, signal = console log; **suppression proof = with SR off, neither appears**. **Two pages since #776, and which one a claim belongs on is not a preference:** `/` (`demo/main.ts`) is the single-terminal widget and the harness ~69 assertions are calibrated against — changing its shape quietly changes what they mean; `/shared-surface.html` is the two-terminal page, and it is the *only* place a shared-surface claim can be proven, since the adapter's `composedSurface === false` branch runs nowhere else. A new spec file needs **its own copy** of the #735 `beforeAll` warm-up: `beforeAll` is per file per worker and `browser` is worker-scoped, so it inherits none |
| **renderer — gate** | `pnpm run build:wasm && pnpm exec playwright test` over `demo/*.html` × dpr **1 / 1.1 / 1.5 / 2**, reading `window.__proof.ok`; coordinates via `demo/proof.js`, `cell_width()` in device px |
| **renderer — eyeball** | **Playwright MCP against a real browser**, never a headless screenshot: `pnpm build:wasm` → serve (`node scripts/serve.mjs`, :8269) → `browser_navigate` a scratch `demo/*.html` → `browser_evaluate` to redraw/scale → `browser_take_screenshot`. The gate and the eyeball are different tools for different questions and neither substitutes: the gate asserts pixels the compositor never touched, the eyeball is the only way to see what a user sees. Delete the scratch page afterwards — both spec runners auto-collect `demo/*.html` |
| **strongest — real consumer** | **penterm.** Link the local build in: `[patch.crates-io] justerm-core = { path = "<worktree>/justerm-core" }` in `../penterm/src-tauri/Cargo.toml` — **point it at the worktree you are editing**, not at the main checkout (`../justerm/…` builds master and the proof passes for the wrong reason). Run penterm's **full** suite. Strongest evidence = a penterm test that *pinned the old bug as expected* now **breaks** while the rest stays green. For a wasm/web change, link via a **clean-room worktree** (a local pkg-swap pollutes the pnpm store) |

Traps this layer must respect:

- **A green headless E2E proves only SR-consumed proxies** (announce · signal) —
  not *visual/DOM* side effects (focus · scroll · reveal). Assert the DOM state
  directly (`document.activeElement` line index, `scrollTop`) **or** drive live
  via Playwright MCP (`browser_evaluate`), then lock the regression into E2E.
  (#166 reveal-focus; #172 live-drive path.)
- **`readPixels` ≠ a screenshot.** Headless SwiftShader composites a
  fractional-CSS canvas to white (#352); a blur metric then reads that as
  "sharpest". Beware tautological proofs (#337) — a check that can only confirm
  its own premise. Don't eyeball at dpr 1 and move on (#328). The corollary is
  the eyeball row above: wanting to *look* at renderer output is not a reason to
  screenshot the headless run — it is the reason to open a real browser.
- **Rebuild the wasm before you look, every time.** `test:proofs` bundles
  `build:wasm`; driving a page yourself does not, so a Rust change you have not
  rebuilt shows you the *previous* binary. The failure is silent and reads
  exactly like "my fix did not work" — costing a hunt for a bug that is already
  fixed. Also set the canvas CSS box from `cssWidth()`/`cssHeight()` on any
  scratch page, or #352 turns it white before you can read anything.
- **A reader that supplies the thing under test cannot fail** (#776) — the sharpest
  form of the tautology above, and it survived being written by someone who had
  just read that warning. A probe that calls `present()` before it reads pixels is
  *itself* what runs the renderer's deferred post-restore rebuild, so a restore
  proof written that way stayed **green with the surface's entire
  `webglcontextrestored` listener deleted**. Read inside the event's own turn with
  nothing presented by the suite instead — listeners fire in registration order and
  the drawing buffer is intact within the task. Its flat sibling, from the same
  slice: an assertion on a coordinate the *page recorded* stays true when the call
  that would have sent it is deleted, so a placement claim has to be a pixel at an
  independently derived point. Both were caught by mutation and neither by reading,
  which is the general lesson — **for each assertion, name the mutation that should
  redden it and run it**, rather than trusting that a green suite observed anything.
- Visual/color changes still need a browser verify even when Step 5 is skipped
  for a closed surface — a synthetic-input unit is not a substitute (#223).
- **An experiment that saturates the workstation is asked about before it is
  spawned — every time, no exceptions.** This box is shared with the
  maintainer's interactive work and with other concurrent agent sessions; a
  measurement that makes it unusable is not a background cost you may absorb on
  their behalf. #735 (2026-08-10) is the case: its harness spawns
  `load.mjs 112` — 112 busy-loop worker threads against **28 logical cores, 4x
  oversubscription** — and the maintainer killed it from Task Manager at 89%
  CPU while other work stalled. **Do not answer this by lowering the load: the
  contention *is* the experiment** (#735 measures 182ms idle vs **4024ms** at
  4x against a 5000ms budget; at 1x cores the failure does not reproduce). So
  the two real repairs are:
  - **Ask first**, and say what you will pin and for how long. If nobody is
    available to ask, the experiment waits.
  - **Box it** — pin the load generator *and* the measured browser to a core
    subset (`(Get-Process -Id N).ProcessorAffinity = 0xFF`) and scale the
    spinner count to keep the same ratio *inside* the box, leaving the rest of
    the machine to its owner. State the cost when you do: absolute
    milliseconds are then incomparable with numbers taken on the whole host —
    only the cold/warm *ratio* survives, so a boxed run needs its own baseline.
  A load generator must also **self-terminate when its parent dies** (poll the
  parent pid, exit). A `finally { taskkill … }` covers a thrown exception and
  nothing else: the harness killed from Task Manager, or hard-killed for any
  other reason, leaves the spinners running forever with no parent to stop
  them — which is precisely how a crashed run becomes a machine nobody can use.

### Recording a capture on the VM

A `capture-*.sh` recording needs a real PTY, and this workstation has none —
Git Bash ships neither `script` nor `expect` (measured 2026-07-27). The RHEL 9.2
box supplies one and is reachable **without the user in the loop**: `justerm-vm`
is a `~/.ssh/config` alias (`192.168.136.135`, `root`, key `~/.ssh/justerm_vm`,
`IdentitiesOnly yes`). Capture → retrieval → golden → gate is all agent-side.

```bash
scp justerm-core/tests/fixtures/capture-softwrap.sh justerm-vm:/tmp/
ssh -tt justerm-vm 'stty rows 24 cols 80; stty size; \
    rm -rf /tmp/capout && mkdir /tmp/capout && bash /tmp/capture-softwrap.sh /tmp/capout'
scp justerm-vm:/tmp/capout/'*.raw' "$SCRATCH/"
```

- **`-tt` is not optional, and its absence is silent.** The agent's shell has no
  tty, so plain `ssh` gives the remote side none either and `script(1)`'s pty is
  left unsized — the TUI then reads a winsize nobody chose and lays out to it.
  Nothing errors. Print `stty size` and confirm `24 80` *inside* the same
  invocation before trusting a byte of the recording.
- **Prove the pipe before trusting a recording.** The deterministic captures are
  byte-reproducible by construction, so re-record them and `sha256sum` against
  the checked-in fixtures — a match proves transfer, locale, line endings and
  `.gitattributes` all round-trip. Measured on setup: `softwrap_shifts.raw` and
  `softwrap_wide.raw` both reproduce identically. A recording of a *live* app
  can never be diffed this way, which is exactly why the deterministic ones are
  the instrument that certifies the path.
- **Pin the locale, and pin it to a UTF-8 one.** The scripts pin `TERM`
  everywhere and `LC_ALL` nowhere; this box is `ko_KR.UTF-8`, and the same htop
  recording differs by locale — measured, `e2 96 bd` (`▽`, its sort indicator)
  appears under the box locale and vanishes under `LC_ALL=C`. So "just pin it to
  C" is the wrong repair: it would strip precisely the Unicode material this
  engine exists to get right. Pin `LC_ALL=C.UTF-8` next to `TERM`.
- ~~**`expect` is absent**~~ — **installed since, verified 2026-08-03** (`/usr/bin/expect`, alongside
  `/usr/bin/script` and `/usr/bin/less`). It came from `expect.x86_64` in
  `rhel-9-for-x86_64-appstream-rpms`, no EPEL needed. The consequence is what the entry was for and
  still applies to a *fresh* box: without it `capture-softwrap.sh`'s real-`less` half silently writes
  a 0-byte file. Check `command -v script expect less` on entry rather than trusting this line — the
  box is mutable and this sentence is not.
- **A capture proves nothing unless its golden can fail, and which golden that is takes deciding.**
  `check_capture` pins two surfaces, and for a given fix usually only *one* of them can move. #685's
  trim lives in text extraction, so the char grid was green in both states by construction — and the
  logical-lines golden was too, until `logical_lines`'s own `trim_end()` normalisation was narrowed
  to match the rule under test. **The harness had the same defect as the engine**, so the capture
  would have been checked in green, unable to fail, while reading as new coverage. Before recording,
  ask which golden can observe the change; after recording, turn the fix off and confirm that golden
  goes red and the other does not.

## Step 5 — adversarial completeness pass (one lens, both corpora)

**Brief the lens from the map first — it is the pre-computed answer to this
step's question.** Open the territories the change touches under `docs/map/`;
their `## Blast radius` is the sibling list and their `## Cross-cutting
invariants` are the facts that hold beyond this territory. That is the corpus-①
brief, and building it by hand is what this step used to spend its budget on —
the expensive half of a lens pass is the main-thread harvest, so a wrong or
missing sibling set is paid for twice.

**One lens, briefed on both corpora.** ① this repo — `docs/map/` (above) +
`architecture.md` §"Hidden VT state"; ② the
reference — xterm.js / alacritty / ghostty real source from the **local pinned
trees** (Step 1's table; `rg`, not `gh api`). These are two *reading assignments
for one agent*, not two agents. Corpus ② is read on every layer, but it does not
*count* the same on every layer — brief item 6 below carries what its word is worth
here, and without it the lens reads three trees with no way to know it is standing
somewhere they have no vote.

The cell-walk sibling set that used to be hand-listed here is now
[alt-screen absolute-index floor](../map/invariant/alt-screen-buffer-floor.md),
which carries the grep that *derives* the sites rather than a count anyone has to
keep true — three artifacts had each hand-written a different set. It was removed
rather than kept alongside the map on purpose: a second copy is what goes stale,
and the copy is always the one the reader happens to open. **Never drop either corpus** because the fix looks
small (#158) — that is what the old never-collapse rule protected, and it is
unchanged.

**Why they merged (2026-07-24, measured on #547).** Splitting by corpus gave the
sibling lens no way to decide *direction*: it saw that two artifacts disagree but
not whether the reference shares the disagreement, so every such finding came back
to the main thread to be settled from cold — against the pinned tree the *other*
lens had open the whole time. Its two rejected findings (`write_glyph` line
destruction; word-select across a separator — both answered "alacritty does the
same") took **~40% of that pass's main-thread calls** and changed no code. Precision
tells the same story: the reference lens, which had to read both sides to compare
at all, returned 3 real defects out of 4 and self-graded the fourth correctly; the
sibling lens returned 2 out of 5. The split also did not buy independence — both
lenses are the same model on the same brief and differ only in which files they
open, so what it actually bought was coverage, at the price of an adjudication
errand.

**Direction — a divergence is not a direction, and the lens now owes you the
direction.** "Differs from the reference" does not by itself say "move to the
reference". It depends on what the two corpora **share**:

- **only the reference diverges** — the sibling is reference-correct and this layer
  alone drifted → move toward the reference;
- **both corpora share the divergence** — sibling == this layer, both drift from the
  reference → a **family** decision: keep the consumer-neutral behaviour now and
  track the reference-parity fix as a coordinated multi-layer change.

Precedent for the rule itself: **#396** (slice-2 `minimumContrastRatio` — the
reference side found justerm-web's double-pass was a *beamterm-forced* compromise
the renderer's own architecture does not need → moved to xterm's single pass, more
correct *and* still web-neutral for the common case) vs **#399** (slice-4 tile
re-tint — the sibling side found `renderer == web` byte-for-byte while the
reference side found *both* diverge from xterm → kept web-neutral for #273, family
fix tracked as #398). Deferrals tracked as issues (#398 tile-retint, #400
search-match-solid), so the closed #272 leaves zero silent gaps.

**A second lens is bought with stance, and only on the unconditional triggers
below.** Same material, opposite job: the first hunts gaps, the second tries to
**refute** them and to break the convergence claim. Both read everything, so both
can still adjudicate a direction and their disagreement is information rather than
an errand.

**Start the pass at GREEN and keep working — it is read-only, so it is not a
barrier.** The lens only reads; nothing it does can conflict with Step 6's doc
sweep or Step 7's gates. Run it as a background agent the moment Step 3 is green
and collect before opening the PR, so the wall clock is `max(lens, rest)` rather
than the sum. The discipline is unchanged and is the *only* part that is
non-negotiable: **no merge before the findings are harvested and dispositioned.**
Sequencing is not discipline; harvesting is.

**Harvest in rank order, and cap what one finding may cost.** "Dispositioned" is
not "each one chased to the ground". The pass runs in the background, but the
harvest is serial main-thread work — which is where a pass actually becomes
expensive, and the part no one budgets: **#547 spent ~9 min of lens wall-clock
(parallel, unfelt) and ~45 main-thread calls processing the result.** Take the
`CONFIRMED` findings first, `INERT` / `DELIBERATE` last and at one line each. If
disposing of a single finding passes **five tool calls** without settling, stop
working it — it goes into the user's batch with what you have and what it would
cost to finish. A finding that costs more to dismiss than the change cost to write
is the pass telling you it found something worth the user's judgement, not a thing
to quietly out-investigate.

**Brief the lens with a frontier and with what is already known.** Without both, a
pass spends most of its budget re-walking ground the last one covered — the wide-glyph
neighbourhood was enumerated from scratch by #528, #529, #533, #534 and #535 in turn.
The brief carries six things:

1. **The frontier** — the functions the diff touches, plus one hop of callers and
   callees, plus the invariants `architecture.md` names for that area. This bounds how
   *far* the lens walks. It does **not** bound whether it runs: that is still
   enumeration risk (above), and the three unconditional surfaces ignore the frontier
   entirely.
2. **The open-issue list** (`gh issue list`). A gap that is already filed comes back as
   one line — *"already filed: #534"* — instead of a fresh three-page write-up. Being
   re-found is signal about reachability, but it is cheap signal; pay one line for it.
3. **The relevant rows of `reference-facts.md`**, so the reference half of the brief
   starts from the map rather than re-deriving it, and so a row that turns out wrong
   gets corrected rather than silently re-learned.
4. **What the last pass on this area found**, when there was one. A lens that knows
   #532 already fixed the leading-spacer half of a predicate looks at the trailing half
   (which is exactly how #535 was found).
5. **The output contract** — the skill's four disposition grades (`CONFIRMED` /
   `UNADJUDICATED` / `INERT` / `DELIBERATE`), stated in the brief itself. The first
   four items bound what the lens *looks at*; this one and the next bound what it costs
   to act on what it returns, which is the half that lands on the main thread. #547's two lenses
   volunteered "inert" and "deliberate" on two findings without being asked — and
   those two were reproduced from scratch anyway, because nothing said a graded
   dismissal is spendable evidence.
6. **What the reference's word is worth on this layer** — the **tie-breaker row** from
   the table at the top of this file (via Step 1's routing table, which now names it per
   change type) *and* the **deliberate-divergence table** next to it. Without them the
   lens is told to read the trees and then graded on a record it was never handed, so a
   divergence on a layer the reference cannot arbitrate comes back `CONFIRMED` and reads
   as urgent. That is #490 exactly, and it is why the repair only half landed: the skill
   got the restatement test and this file got the divergence list, while the brief — the
   one place that decides what the lens *knows* — was not touched, so the check could
   only ever fire on the main thread, after the finding had already been carried. Four
   of the six rows in that table are layers where the answer is *no vote*; a lens that
   does not know which one it is standing on cannot grade its own findings.
   This item does **not** shrink the reading: both corpora stay whole (never-drop, above),
   and 20 rows of `reference-facts.md` are recorded negative results — *"cannot
   arbitrate"*, *"never asks the question"*, *"grants nothing"* — which are worth having
   precisely because someone read the tree on a layer the reference could not win.

Anything outside the frontier that the lens notices anyway is still reported — the
frontier is a search order, not a gag.
Precedent: #113 logical-lines (single-buffer view missed the alt-screen
cross-buffer defect; also surfaced the same bug in `search()` → #144; the
`abs_floor()` centralization covers logical_lines/#113 · search/#144 ·
word-sel/#207). Gate on *enumeration risk*, not diff size; a reactive spike that
keeps catching new gaps is the trigger. Record an explicit skip for a closed
surface.

**Unconditional triggers — three paths that run the pass regardless of the
judgement above, and that are the only places the second (refuting) lens is worth
its cost.** justerm has no money path, no production mutation and nothing
destructive, so the schema's usual examples do not apply; here a path is sacred
when it is **irreversible** (already published) or **silent** (wrong answer, no
crash, user-visible state quietly corrupted). You do not get to skip these because
the diff is small:

1. **`justerm-core/src/serialize.rs` — the wire, and any `WIRE_VERSION` bump.**
   crates.io and npm are immutable; a consumer decoding a wrong layout gets
   garbage cells, not an error. Touching `struct → encode → decode → Flat →
   getter → types.ts` in one crate and not the others is exactly the failure a
   pass that stops reading half the family misses.
2. **The release path — `.github/workflows/*` publish jobs + `docs/agents/release.md`.**
   Publishing is tag-driven and automatic: pushing `vX.Y.Z` ships to both
   registries with no confirmation step, and nothing but a yank comes back.
3. **Absolute-index walks over the concatenated `[scrollback ++ grid]` buffer —
   `abs_floor()` (`term/walk.rs` since #585) and every reader that indexes
   absolutely — grep `abs_floor` *and* the raw `scrollback.len()` walks rather
   than trusting a count written here. The second grep is not redundant with the
   first, and centralising the expression (#585 folded the last open-coded copies
   into calls) does not retire it: every miss so far was a *fresh* unfloored walk
   that never mentioned `abs_floor`, so searching for the function's name cannot
   find the defect this entry exists to catch.** On the alt screen an unfloored index reads the wrong
   region and returns *plausible* text — the caller gets content that is not on
   screen, with no error anywhere.
   **Do not enumerate the affected surfaces here.** This entry, `walk.rs`'s module
   doc and the map's invariant note each hand-wrote a *different* set, and on
   2026-07-28 all three were wrong against the code — the one that had been the
   **first** discovery (`viewport_logical_lines`, #113) was missing from two of
   them while its issue number sat in the same paragraph. The derivation lives
   once, with the invariant:
   [alt-screen absolute-index floor](../map/invariant/alt-screen-buffer-floor.md)
   carries the grep that produces the call sites, plus the ones that satisfy the
   floor *without calling it* — by argument, or by construction — and therefore
   never appear in any grep. Their number is kept there, not here: this sentence
   said "the two" until #601 made it three.
   This is on the list because the completeness pass has found a fresh
   sibling three times — #113 (logical lines) → #144 (`search`) → #207
   (word-selection `prev_pos`) — so "I checked the obvious callers" has a measured
   failure rate here.

## Step 6 — behavior-describing surfaces (sweep by hand)

No change ends at the code; nothing compiles the drift away. Sweep every surface
that *describes* the behavior:

- **`docs/map/` — the dependency graph. Two obligations, and the second is the
  one that makes the map preventive rather than archival.**
  1. **Coverage** — is the territory this change touched present in the map, and
     is its `## Blast radius` list still right? A territory with no governing
     record is a *valid* entry; leave the blank. Coverage may lag.
  2. **Promotion** — *is the fact this fix revealed also true outside this
     territory?* If yes, the change **does not land** until a cross-cutting note
     exists for it under `docs/map/invariant/`. Answer it by grep, not by
     judgement: in this repo the question is usually *"does this hold at any site
     that walks `[scrollback ++ grid]` by absolute index / that writes a `Cell`
     word carrying a row-or-pair fact?"*
  Promotion cannot lag, and that asymmetry is the whole point. The *first* site to
  hit such a fact is where it is discovered, and at that moment no node exists —
  so a map that records invariants only after the third rediscovery is the same
  post-hoc archive as the helper someone eventually extracts. Measured here:
  `abs_floor` holds across several read surfaces and was found **three separate
  times** (#113 → #144 → #207) across months; `Term::abs_floor` was extracted
  *after* the third, and a helper prevents nothing because whoever writes a *new*
  walk never goes looking for it. The `#552` roster is 15 issues of the same shape.
  The map links *out* only — never edit an ADR to add a backlink (Obsidian
  supplies the reverse for free), and use symbols in `## Code`, never line
  numbers.
- **Public doc-comments → docs.rs.** `justerm-core`/`justerm-wasm-decode` ship
  their `///` / `//!` comments verbatim as the crate's **docs.rs** API reference
  (core has ~20 in `lib.rs` alone) — the surface most likely to still describe the
  old behavior. Update them in the same change, and **build them** —
  `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` is a CI gate (Step 7), because
  updating a doc-comment and never rendering it is how 12 public docs ended up
  linking *private* items. That is the failure mode peculiar to this surface: you
  write the comment with the source open, where `store_flags` / `Flat` /
  `bake_config` are all in scope, and it publishes to a page built from public
  items only, where every one of those links is dead. `cargo test` does **not**
  cover this — it runs doctests (the code examples), not link resolution — and
  clippy is a different tool that does not carry rustdoc's lints.
  **A doc-comment can also go stale forward, and nothing catches that.** The
  publish-time phrase check below rejects *"lands in #N"* — but only in a
  **README**, and only at publish. `justerm-core/src/lib.rs`'s `Engine::resize`
  carried *"(Soft-wrap reflow lands in #7.)"* on docs.rs for six weeks after #7
  closed (2026-06-17), because `resize` is a doc-comment, not a README. When you
  close an issue a doc-comment *promises*, grep the sources for its number, not
  just the READMEs. **And not only the sources** — the same rot in
  `architecture.md` §Cadence was worse: it carried an *"Open question … Tracked in
  #13"* after #13 closed, and what shipped was not the design that paragraph
  predicted, so a reader planning work from it would have rebuilt a solved problem.
  A stale pointer costs a wasted lookup; a stale *prediction* costs the work.
  **`docs/map/` belongs in that grep too, for a reason peculiar to it.** A territory
  note may cite an issue two ways and only one survives closing it: as *evidence*
  (*"five designs were built and rejected — read it, the failures are the content"*,
  still true afterwards) or as *status* (*"#606: dispose cannot stop the loop"*,
  false the moment it lands). The second kind arrived in bulk the first time the
  open backlog was read as a map input — 2 such citations became 11 in one session —
  so closing an issue means checking whether a note now asserts something the fix
  disproved. `rg '#<n>' docs/map/` is the whole check.
  Cheap sweep, run when closing anything:
  ```
  rg -n '(lands in|will land|not yet|planned|open question|tracked in|carried over|once #|until #)' \
     --glob '*.rs' --glob '*.md' --glob '*.ts'
  ```
  **Judge this sweep by what it cannot see, never by its hit count.** The
  narrower first version of it (`///` doc-comments only, `lands in|not yet|...`)
  returned exactly one hit, and that one hit read as reassurance — while the
  worse of the two defects sat outside it on both axes, in a `.md` file and
  phrased *"Tracked in"*. A low count means the pattern is clean **or** the
  pattern is narrow, and the two are indistinguishable from the number. When a
  hit turns up, widen the pattern with the phrasing that produced it before
  fixing the hit.
- **Release notes = GitHub Releases** (tag-driven, `docs/agents/release.md`).
  **There is no `CHANGELOG.md`.** Never rewrite a published entry; if the repo and
  the registry would disagree for a version, open a new note, don't edit the
  shipped one.
- **The published README is a *behavior* surface, not just a release artifact.**
  crates.io/npm snapshot each crate's README at publish time, so it is the front
  page every new consumer reads first — and nothing gates it: no test imports it,
  no compiler sees it, no constant in it is checked against the constant it names.
  Both published READMEs drifted the full width of the pivot before anyone looked.
  `justerm-renderer/README.md:15-19` still announced "**Under construction** … the
  scaffold (#259) … a stub that clears the canvas … the GPU pipeline lands in
  #260+" at version **0.6.1** — six published `renderer-v*` tags, 24 modules and
  ~30 wasm methods later. `justerm-wasm-decode/README.md:34` asserted
  `wireVersion() === 2` against `VERSION = 12` (paste the canonical snippet, get a
  failing assert), still told the reader to cell-invert "because beamterm has no
  cursor primitive" (`:105`) after the family renderer grew a native cursor overlay
  (`webgl.rs` `set_cursor`, #270), and version-locked to "the `justerm` crate"
  (`:15, :131`) — a name frozen at the 0.5.1 tombstone since ADR-0010. **Two cheap
  checks cover most of it, and both are now mechanized** — extend them rather than
  re-deriving the rule:
  - *A constant a README quotes must be greppable against its definition.* Pinned
    by a **host unit test next to the crate that owns it**, so it fails on every PR:
    `justerm-wasm-decode/tests/readme_pins.rs` ties the usage snippet's
    `wireVersion() === N` to `justerm_core::WIRE_VERSION` (and rejects the
    tombstone crate name). `include_str!` is deliberate — a moved README fails to
    *compile* instead of skipping the check, and the pin asserts it matched
    something so a reworded snippet cannot make it pass vacuously. A README that
    starts quoting a new constant gets a new pin here.
  - *A README describing the crate's* maturity *expires at the next publish.*
    Enforced at **publish time**, not on PRs — an in-progress crate may honestly
    call itself a scaffold in the repo; snapshotting that sentence onto a registry
    is what makes it a lie. `.github/scripts/check-published-readme.mjs` runs in all
    four publish workflows and rejects "under construction" / "this is the scaffold"
    / "lands in #N" / "not yet implemented" / "coming soon" / "work in progress".
    Keep that list tight: this gate fires after the tag is already pushed, so a
    false positive costs a re-tag.

  Note which surface is *already* gated and why: `justerm-core/README.md` drifted
  least of the four because its usage snippet is a **doctest** (`cargo test` runs
  it). Prose is not checkable, but a code block is — prefer putting a README's
  claims in a form something already executes.
- **Glossary + decision trail** — `CONTEXT.md` (glossary) and `docs/adr/`. If a
  domain term's *meaning* changed, update the glossary in the same change. **The
  ADRs are a *write* surface, not only the one you read at Step 0**: a change that
  falsifies an ADR's premise amends *that ADR* in the same change (0011 and 0012
  carry exactly such amendments). **The decision surviving is not a reason to skip
  the amendment** — what rots first is the *grounds*, and grounds are what the next
  implementer reasons from. ADR-0017:66 still states that "core gains **no regex
  dependency**" and rejects its alternative (i) on exactly that cost, while
  `justerm-core/Cargo.toml` has carried `regex = "1"` since #314 (`search.rs:25`,
  re-exported to JS as `isValidRegex`): the routing decision holds, its price tag is
  fiction, and the rejected alternative effectively shipped for search. An ADR that
  *quotes* a layout or a constant is also a wire mirror and belongs in the sweep
  below — ADR-0015:34-36 still documents `MARKER_STRIDE = 2` from v7 against the
  shipped `5` (v10/#159 widened the record with `MarkerKind` + exit code), a gap
  ADR-0020's own table already records as an ADR-less admission. Since ADR-0019 the
  renderer's cell-composition
  rules live there rather than only in the `frame.rs` / `overlay.rs` doc-comments —
  a change to those rules updates the ADR, and a combination the ADR cannot answer
  is an amendment to it, not a fresh pairwise decision.
- **The wire contract mirror** — a wire/format change touches
  `struct → encode → decode → Flat → getter → types.ts`; `justerm-web/types.ts`
  hand-mirrors the wasm getters, so grep it (#129/#135: `mouseWantedEvents`
  reached `types.ts` only at S16). Also the renderer `demo/*.html` headers and
  spike comments — each promises only what it can demonstrate (don't tell the
  reader to "watch it change" a constant).
- **A producer API with zero consumer call sites is a surface, and the widget is one of its
  readers.** After changing a *published* API — a new `wasm_bindgen` export, a new wire field, a new
  getter — ask what call sites it has downstream, and treat *none* as the finding rather than as
  nothing to report. This is not the wire-mirror sweep two bullets up: that one asks whether a value
  the consumer already uses still travels correctly, this one asks whether a finished mechanism is
  reachable at all.
  **Added by epic #583, whose seven items are all of that one shape** — `cursorBlink`, SGR-5 blink,
  `setBgAlpha`, `setLetterSpacing`/`setLineHeight`, the context-loss surface, cursor
  contrast/thickness, `setDevicePixelRatio`. None was a bug in the producer, none failed a gate, and
  the accumulation had one cause: #417 named itself *"the downstream loop for the renderer's additive
  setters"* and scoped to the three that existed then, so every setter added afterwards needed its
  own consumer-half issue and only one got one. Where a gap was noticed at all it went into a code
  comment — `damageHeader`'s *"the web has no text-blink phase"* — which the backlog cannot see. The
  derivation, so it needs no roster: **the widget is the consumer ADR-0017 names**, so a policy knob
  the renderer declares and the widget cannot reach is a contradiction in the boundary rather than a
  missing feature. Cheap to run: the renderer's exports are its `js_name` list and the decoder's are
  its getters; cross them against `rg` over `justerm-web/src`.
- **Reclaim now-false rationale.** Walk recent PR/issue/release reasoning and
  retract what the new behavior falsified (surviving reasons are usually the
  transitive ones).
- **After an architecture pivot, sweep the whole OPEN backlog — not just recent
  reasoning.** A pivot (ADR-0002→0018 beamterm→`justerm-renderer`, #273) falsifies
  *premises* in issues filed long before it, and nothing fails: code that names a
  deleted dependency stops compiling, but a sentence like "that's a third-party
  renderer concern, out of our hands" is never checked by anything. A stale premise
  is worse than a stale issue — it survives as a **justification for not acting**,
  or sends the next implementer to a file that no longer exists. Sweep found 4/22
  open issues broken by one pivot: #398 (prescribed editing `decoration-render.ts`,
  deleted in #407; its `renderer == web byte-identical` acceptance box lost its
  comparand when web stopped compositing), #249 + #317 §2 (both deferred on "that
  belongs to beamterm / the shared shader" — no such layer exists now, so the
  routing argument, not the severity, was void), #325 ("blocked by S13 #273" long
  after #273 merged, plus a mechanism sentence that was simply wrong). Correct them
  as **comments**, leaving the body as the record of what was believed when.
- **An epic body is a live checklist, not a belief record — edit that one.** The
  rule just above (correct in comments, leave the body) is right for a *defect*
  issue, whose body is the record of what was believed when it was filed. An
  **epic** is the opposite: its checkboxes and Status block are read as the current
  state of the build plan, so leaving them unedited is not preservation, it is a
  false status report. #103 (justerm-web) has **all 16 slices and both core gaps
  closed** and carries **two** `[x]`; its body still routes the reader through
  "#108 … blocks #109, #110" (all three closed), still has S2 rendering
  `DecodedFrame`→**beamterm**, and still declares the slices "grain 검토 후
  AFK-ready". A finished epic that reads as ~10% done is worse than a stale defect
  body — it invites someone to re-open settled work. Tick the box in the slice's
  own PR, the same way the wire mirror is updated in the change that moves it. And
  **sweep the epic's labels with its body**: #287 kept `blocked` for a week after
  its blocker #258 closed — its own newest comment says the block is resolved while
  the label still says otherwise, and `blocked` is the one label that decides
  whether the next agent may pick the work up at all.
- **Cross-check the backlog against itself, not only against the code.** Issues are
  the durable record (DoD ③), so two of them can hold opposite directions for the
  same data with nothing to notice: #440 (search-match ruler lines as a *new
  per-frame wire group*) vs #490, filed a day later (marker lines must leave the
  per-frame snapshot — that payload is frame mode's O(M) ceiling). Same-week issues
  collide too; it is not an age problem, it is two lenses (feature vs performance)
  that never read each other.
  **Filing-time obligation:** before opening a follow-up issue, read the open
  backlog for anything its proposal would *break* — grep by the **artifact it
  touches** (the wire group, the file, the shader stanza, the predicate), not by the
  feature name, since a conflicting issue almost never shares your vocabulary. If
  one is found, cross-link **both** ways in the same act of filing (a one-way link
  is only found by whoever reads the newer issue) and say which decision must come
  first. Three of the eleven corrections in this sweep were pure missing
  cross-links between issues that had already spotted the same seam separately:
  #440↔#490 (wire channel), #494/#495/#496 (one branch's entry condition / fg / bg,
  each filed as its own independent "(a) or (b)" decision), #437↔#441 (one port
  capability, two symptoms).
  **Spine issues — the rule is portable, the values are here.** When one opens, what its
  body holds, how a sibling links it, and where its write-back lands all live in the skill;
  the tracker is GitHub, so its defaults apply unchanged (`part of #<spine>` tracked-by, an
  edited body for the roster, comments for each finding's evidence) and this repo has no
  exception to record. What it *does* supply is the **preemption list — the record table
  below**. An area that already carries a record, *accepted or proposed*, has the
  home a spine would provide, so a sibling there is filed as a conformance item under that
  record and no spine opens beside it. Two live cases, both found while looking for a
  cluster to anchor: **ADR-0025** (proposed at the time, accepted 2026-07-27) was doing exactly a spine's job for the
  row/pair cluster, and **ADR-0020** (accepted) already houses the marker-payload question
  behind #440/#490.

### Where a promoted decision record goes, and what earns one

**Destination + format.** `docs/adr/NNNN-<kebab-slug>.md`, **English**, numbered
sequentially, opening with `Status: accepted (YYYY-MM-DD[, #issue])` and following
the house sections: `Context` (with the forcing case) → `Decision` → `Named prior
art` → `Consequences` → `Alternatives considered`. Amend in place rather than
rewriting history: a status-line note when a later change moves the *reason*
(ADR-0011, #504) or realises a direction (ADR-0012→0018), a `supersedes` /
`superseded by` pair when it is replaced (ADR-0002↔0018). An ADR may carry no
issue number — 0018 and 0019 do not.

**What earns one.** The portable bar governs: **two or more promotion triggers**
(the skill's § "Promotion", which runs on a cluster's clock — *not* inside every
Step 5 pass), not one. Below that it is a decision and belongs in the issue as usual.

**The rung below the record is the spine issue** — the rule is in the skill, this repo's
values in Step 6. The archaeology it exists to prevent, measured here: **ADR-0019 out of 20
issues**, **ADR-0025 out of 9** (#521/#528/#530/#532/#533/#534/#535/#538/#540, plus the
wire-derivation half of #7) — both extracted *after* their clusters had been filed
verb-by-verb.

Areas already known to have hit the bar — check these first, since a new question
in one of them is probably a conformance item under an existing record rather than
a fresh decision. This table is also the **spine preemption check** the filing step
runs: an area listed here already has the home a spine would provide, so a new
sibling attaches to the record instead of opening an anchor beside it.

**Only a decision record preempts a spine — a `docs/map/invariant/` note does not, and the
temptation to read it as one is real enough to have happened (#578).** The two look
interchangeable from a distance: both name a cross-cutting fact, both list where it
holds. But a map note is *descriptive* and belongs in a file, while a roster is *current
state* and belongs somewhere editable — which is #552's measured result, not a
preference: a hand-copied roster inside ADR-0025 went stale in five places in three
days while D1–D4 needed no edit. So a map note and a spine are **complements**, and the
split is the rule they encode — the note keeps the fact, the spine keeps who is on the
list and what is not yet decided. Writing the roster into the note reproduces exactly the
failure #552 exists to record. Live case: the note
[the cell size is derived state](../map/invariant/cell-size-is-derived-state.md) with
spine **#630**.

| Area | Record | State |
|---|---|---|
| Renderer **cell composition** (a cell's bg / fg / ink) | **ADR-0019** | Recorded. Open questions here resolve *against the model*; a combination it cannot answer is an amendment |
| Renderer **resource ownership / tiering** (which tier a resource lives in, and how a consumer setting relates to it) | **ADR-0021** (accepted) | **Listed for the preemption check, not because it was promoted from a cluster** — and that is why it reads differently from every other row here. 0021 is a *direction* record (the ADR-0012 pattern), so it never went through the two-trigger bar; ADR-0022 and 0023 are the same shape and are deliberately **not** listed, because nothing has re-decided them. What earns 0021 the slot is the second job this table does: Epic #287 is sliced into #769–#776, every one of them touching the tier rule, so the area is about to produce siblings and it already has the home a spine would provide — attach a new question to 0021 as a conformance item against **D1–D5**, do not open an anchor beside it. It also did hit the bar in passing: #768 measured its stated evidence grade **false** before it could start, and found two in-repo artifacts requiring opposite things (0021 put DPR global, the map territory note gave every surface its own). The record existed first, which is the outcome the promotion rung is *for* |
| **core ↔ consumer routing** (mechanism vs policy) | **ADR-0017** | Recorded. Its own rejected `(D) keep deciding case by case` is the pattern to watch for |
| **Wire / frame shape** | 0005, 0008, 0013–0016 | Recorded across several, each at a version bump |
| **Row / wide-pair state ownership** (where a row/pair property is stored vs. what it describes, and its set/clear/repair lifecycle) | **ADR-0025** | **Accepted 2026-07-27** (#552) after four slices shipped against D1-D4 with no rule edited (#535/#533/#540/#534). Promoted 2026-07-24 over the soft-wrap / wide-char-spacer cluster (#521/#528/#530/#532/#533/#534/#535/#538/#540 + the #7 wire half) — one rule re-decided per verb, scattered across `end_wrap`'s per-verb table, `drop_artefact_if_erased`, `free_cell`, and four `architecture.md` hidden-state entries. A `Cell` write deciding a fact whose scope is a *row* or a *wide-glyph pair* is the tell. The open cluster resolves as conformance items against its D1–D4. **Its second amendment (#547) came from the other direction** — not a combination D4 could not answer, but a *precondition D4 never stated*: it declared "both halves move together" over a width where both halves cannot fit, while the engine accepted that width. The record now carries `MIN_COLUMNS = 2` as D4's grounds. A rule is unsatisfiable-by-size as easily as it is wrong, and only implementing it against the degenerate end finds that. **Its third amendment (#595, 2026-07-29) is that precondition's mirror** — #547 floored the *screen* so a pair has room, this caps the *glyph* so a pair is enough: `unicode-width` returns 3 for one codepoint and nothing bounded it, so a glyph landed as unmarked blanks that search could not find and word selection split. Same shape, opposite end, and it arrived the same way — by implementing against the degenerate case |
| **Span projection / decoration geometry** (viewport clamp, anchor, precedence) | **ADR-0024** | **Promoted 2026-07-21**, over #120/#198/#202/#457/#458/#459/#461/#463/#480/#498. The triggers that carried it, kept because they are what the bar looks like in practice: "which decoration wins" was decided at three granularities (#452 per-property within a marker, #458 across markers, #498 ruler marks); #452→#457→#461 is a consequence chain, with #461 recorded as *"the vertical mirror of 457"*; and #457 found a repo test comment asserting the opposite of the behaviour. Kept out of ADR-0019 deliberately — consumer policy under ADR-0017, viewport-only; 0024 opens by placing itself on *"the axis ADR-0019 explicitly put out of its own scope"* |
| **An out-of-range coordinate handed in from outside** (which surface bounds it, where, and that a reader may not bound one end of a pair) | **ADR-0026** (proposed) | **Promoted 2026-07-31** over #660 → #671 → #678 — three combinations of {axis, surface, bound-site} decided one at a time, each reinterpreting the last. Two triggers carried it: the references **split 1–1** on the guard (alacritty clamps, xterm hides) *and* xterm contradicts itself across its own call sites; and the third issue re-decided a pair the second had already settled, in a different combination. Its first finding is a conformance fix in the same change (`selection_range` bounded one end of two arms). A new question about an out-of-range coordinate in `justerm-core` is a conformance item against D1–D4, not a fresh decision — but the *consumer* side has its own rule with its own reason, the `docs/map/invariant/` pointer-coordinate note, which this does not subsume |
| **What an IME composition puts on screen** (which writer owns each surface it touches, and whether browser ownership reaches visibility) | **ADR-0028** (accepted) | **Promoted 2026-08-04** out of spine #640, and the second promotion here that was not archaeology — the anchor was opened at the fourth rhyming issue and closed at the fifth, with the roster and the measurements in hand. Its falsifier resolved *against* retirement: #249 could not land without a rule, and it additionally forced an **ADR-0019 amendment** (Totality's first combination no layer in that stack can close, because a preedit *supplies* a glyph). What the record itself learned is the part worth carrying: three of its five clauses were **corrected by implementing them**, each caught by measurement after the first implementation had passed review — so it was accepted only once a member had shipped against it, not when it was written. A new question about what a composition does to the screen is a conformance item against D1–D5 |
| **When a coordinate leaves core** (whether it must carry the instant it is true at, or may be answered by a query a re-ask always answers) | **ADR-0029** (accepted) | **Promoted 2026-08-06** out of spine #744, over #490 → #737 → #741 → #742 — four decisions about one pair of participants (*which channel* × *which scalar*). Its triggers were unusually legible: a **chain** rather than an edge (#741 and #742 were both surfaced by #737's own pass), **three separate premises measured false** before their issues could start, and a reference that **cannot arbitrate** the design at all because no reference serialises a coordinate across a boundary. The promotion's content is that the question the cluster kept asking was malformed — not *one* rule with two candidate values, but **two discharges** (carry / re-ask) with a derivation saying which a surface owes. What makes it a record rather than a filing cabinet is that it *derives* five shapes already shipped (`marker_index`, `MarkerCreated`, `tracked_point`, `search`, `command_marks`) instead of listing them. **Its scope is explicitly narrower than its cluster**: it answers *when* a coordinate is true and not *which buffer* it names — the second axis surfaced in #742's pass, deliberately left out so the neighbour is not read as decided. **Accepted 2026-08-07 on #743**, the member it had itself named as the hard case: a *document* coordinate no published scalar dates, so carry would have cost a second coordinate space's dating apparatus and the question was whether the derivation reaches that far. It did, on both conditions, with no clause corrected — which is a *weaker* per-clause signal than ADR-0028's (three of five clauses corrected by implementing them), so what accepts this one is reach rather than repair, and the hard case is why that is enough. Two by-products worth carrying: D3.2 turned out to **exclude** a which-buffer answer without choosing one (emptying a query on the alt screen makes absence ambiguous and forfeits re-ask), and a pin for a document coordinate is **vacuous unless its fixture contains the collapse** — two drafts passed against a deliberately broken engine before the fixtures were rebuilt. A new question about an outbound coordinate's dating is a conformance item against D1–D6 |
| **When GPU work may be attempted** (which liveness predicate each renderer entry point asks, and what a value published to a consumer means) | **ADR-0027** (proposed) | **Promoted 2026-08-03** out of spine #689, and the first promotion in this repo that was *not* archaeology — the anchor was opened at the second rhyming issue and closed at the fourth, with the roster and the measurements still in hand, so the record was nearly a copy. Its falsifier fired on the stronger half: #695 was found by asking the rule of every entry point rather than from a symptom, and the same pass classified two further sites before anyone asked. The corollary that does the work is structural — *a module that can only see reports can only answer report questions* — which is why `context_loss.rs` cannot own the frame decision alone. A new question about a lost-context guard is a conformance item against D1–D4, not a fresh decision |

Naming the areas is the point: a maintainer can see a cluster has been re-decided
long before the person inside it can, so a promotion check starts by asking whether
the work sits in one of these rather than re-deriving that from scratch each pass.

## Step 7 — gate matrix + downstream loop

**core / wasm:**
```
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings   # the `-- -D warnings` is the gate, not decoration
cargo check --manifest-path fuzz/Cargo.toml
cargo build -p justerm-wasm-decode --tests --target wasm32-unknown-unknown
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps   # rustdoc lints ≠ clippy, ≠ doctests
node .github/scripts/check-map-links.mjs docs CLAUDE.md CONTEXT.md README.md
bad=0; for f in docs/map/territory/*.md docs/map/invariant/*.md; do \
  node .github/scripts/check-map-note.mjs "$f" || bad=1; done; exit $bad   # note SCHEMA ≠ note LINKS
node .github/scripts/check-tool-pins.mjs
```
**The last two were missing from this list until 2026-08-03 (#545), and the omission cost a red
CI.** Both are steps of the same `test` job as everything above them, so "I ran the local matrix"
read as complete while two gates had never executed. The one that fired checks each map note's
**section schema** — an invariant note owes `## The fact` · `## Why it is cross-cutting` ·
`## Territories it holds in` · `## What a violation looks like` · `## Discovery history` ·
`## Where it will recur`, and a territory note its own six — plus resolution of every symbol named
under `## Code` **against the source roots only**, so a test-function name cannot resolve there and
belongs in prose. It is a different tool from `check-map-links.mjs` one line above it, which is
exactly why having one in the list made the other look present. Read the workflow, not this list,
when a job goes red on something absent here:
`sed -n '/^  test:/,/^  wasm:/p' .github/workflows/test.yml`.
(the last one is the **prose** counterpart of the rustdoc gate above it: rustdoc
resolves links inside `///` comments, nothing resolved the ones *between* the
markdown docs. It checks `#anchors` too — a missing file 404s loudly, a missing
anchor degrades **silently** to the top of the target, and
`reference-facts.md`'s headings embed issue numbers and verification dates that
get rewritten by routine re-verification. Cheap enough to run on any doubt.)
(`--workspace` blind spots: `cargo fmt --all --check` is pinned 1.96.0;
`justerm-wasm-decode/tests/web.rs` is wasm32-only and 0-compiles on host — its
runtime assertions run only in the browser CI job. Keep version-pinned tests in
sync on host *and* wasm.)

**web:**
```
pnpm typecheck        # 3 tsconfigs: tsconfig.json (src, browser, types:[] → process/Buffer are errors),
                      #   tsconfig.test.json (test+demo+e2e, node types), tsconfig.node.json (*.config.ts).
                      #   Running one silently leaks coverage — verify with `tsc -p <each> --listFiles`.
pnpm test             # full vitest
pnpm build            # tsup — does NOT catch type errors typecheck missed; guards output paths only
pnpm demo             # + pnpm test:e2e if the change is a11y/UI-observable
```
For **visual/DOM side effects**, E2E must assert the DOM state
(`document.activeElement` · `scrollTop`) — announce/signal alone is an unverified
gap (Step 4). CI wired since #341 (`web`, `web-e2e`). Local E2E needs
`pnpm exec playwright install chromium` once.

**renderer** (out of every cargo umbrella — `cargo fmt --all` and `--workspace`
visit **zero** renderer files, #333):
```
cargo fmt   --manifest-path justerm-renderer/Cargo.toml --check
cargo test  --manifest-path justerm-renderer/Cargo.toml                                        # pure layer
cargo clippy --manifest-path justerm-renderer/Cargo.toml --target wasm32-unknown-unknown --all-targets
cargo build  --manifest-path justerm-renderer/Cargo.toml --target wasm32-unknown-unknown       # GL/wasm layer 0-compiles on host
RUSTDOCFLAGS="-D warnings" cargo doc --manifest-path justerm-renderer/Cargo.toml --no-deps --target wasm32-unknown-unknown
cd justerm-renderer && pnpm run test:unit                                                      # demo/proof.js pixel helpers
cd justerm-renderer && pnpm run test:proofs                                                    # ONLY if the GL layer changed (#328/#331)
```
CI wired since #333 (`renderer`, `renderer-proofs`).

**Your local `wasm-pack` must match the pin, or `test:proofs` is not the gate CI runs (#616).** CI
installs the version in `WASM_PACK_VERSION` (`rg -n WASM_PACK_VERSION .github/workflows/`); a local
`wasm-pack --version` that differs means the pixel assertions ran against different codegen and a
different `wasm-opt` than the ones that will judge the PR. This is not hypothetical — it is the state
#616 was filed from: CI on 0.15.0, a maintainer's local proofs on 0.14.0, both green.
`cargo install wasm-pack --locked --version <pinned>` aligns them.

**Gate hygiene:** run each gate **bare, never piped** (`test … | tail -1 &&
commit` always commits — a pipeline's status is `tail`'s). **Never move a
threshold** (coverage floor / lint budget) to turn a build green.

**Worktree / PR / CI:** **worktree, not a bare branch** — every substantive change
starts with `git worktree add ../justerm-wt-<issue> -b <branch>`, **beside** the main
checkout. Never edit on `master` in the main checkout. The location is not cosmetic
and it is not the default: `.claude/worktrees/<slug>` is where the harness's worktree
tool lands and where older trees on disk still sit, and it silently breaks every
`../` path in this document — see "What a worktree breaks" below for the failure and
the entry check. Prefer the sibling; if something puts you under `.claude/worktrees/`
anyway, read that section before using any `../` path here. → `feat(<scope>): … (#issue)` (**no `Co-Authored-By`
trailer**) → squash PR (`Closes #issue`) → confirm CI jobs green:
`test` / `wasm` / `renderer` / `renderer-proofs` / `web` / `web-e2e`. A PR touching
`.github/workflows/**` also gets **`supply-chain`** (path-filtered, so it is absent
otherwise) — reproduce it locally with
`cargo run -- scan --strict <justerm repo root>` in `../just-shield`. **Point it at the
repo root, not `.github/workflows`**: given the wrong path it reports "0 workflows
scanned" *and* a green "no violations", a vacuous pass. Don't watch CI *during*
implementation (local gates mirror it) — except wasm browser `wasm_bindgen_test`,
which runs only in the CI wasm job, so check it once per wasm-decode-changing PR.

**What a worktree breaks: every `../` in this document.** The sibling paths here
(`../.refs/`, `../penterm/`, `../just-shield`) are written relative to the **main
checkout**, and a worktree sits somewhere else — so resolve them against the main
checkout, never against the worktree's own `..`. This fails *silently*, which is why
it is written down: `rg -n <symbol> ../.refs/alacritty` from a worktree is not an
error, it is **zero hits**, and zero hits reads exactly like "no prior art" (Step 1's
"unverified ≠ absent", inverted — the tool answered, the answer was about the wrong
directory). Use an absolute path, or run reference reads from the main checkout.
Two consequences that bite hardest: the penterm `[patch.crates-io]` path must point
at the **worktree you are editing** (Step 4), and `cargo run -- scan --strict <justerm
repo root>` takes the *worktree* root — pointed at nothing it reports "0 workflows
scanned" plus a green "no violations".

**This is why the rule above puts the worktree beside the main checkout** (taken on
#649): from `../justerm-wt-<issue>`, every `../` in this document resolves to the same
place it does from the main checkout, and the hazard simply does not arise. Verify it
once on entry — `git -C ../.refs/xterm.js rev-parse --short HEAD` against the pinned
SHA in Step 1 — because the failure mode is silence, not an error.

**That rule and this reason were separated by twenty-five lines, and for a while they
disagreed**: the paragraph above still prescribed `.claude/worktrees/<slug>` while this
one called it the thing to avoid. A reader who took the first instruction never reached
the second, which is the exact shape of the failure it describes — a wrong answer with
no error. Keep the prescription in one place and the reasoning here; if the location
ever changes again, it changes in the paragraph above and this one only explains it.

**The web E2E has the same shape, one layer up, and it bit on #649.**
`playwright.config.ts` sets `reuseExistingServer: !process.env.CI`, so a `pnpm demo`
already listening on **5173 from another checkout** is silently adopted: the worktree's
specs then run against *that* checkout's `demo/` and `src/`. It reads exactly like a
broken change — a probe the spec calls comes back `undefined`, a new assertion fails on
old behaviour — and the code under test is fine. Two ways it gets left running: a
`pnpm demo` in another session/worktree, and a background `pnpm demo` whose task was
stopped (killing the `pnpm` wrapper can leave the `vite` child holding the port).
Before trusting a red E2E from a worktree, check who owns the port —
`netstat -ano | grep 5173` — and kill the stale listener rather than debugging the
symptom. The same trap makes a *green* run untrustworthy when the other checkout
happens to contain the fix.

**The same shape once broke cargo itself, and the fix is the general lesson (#608).** A
worktree does not only break `../` in prose — it breaks any fact stated as a path in a
*parent* file, because the parent a tool finds is not the parent you meant. `justerm-renderer`
was excluded from the workspace by the **root** manifest only, and cargo resolves a crate's
workspace by walking *upward*: from a worktree it climbed past the worktree root (which
excludes the crate) into the main checkout's manifest, matched the crate against an `exclude`
list of paths that were not this one, and refused to build. Every renderer gate — fmt, test,
clippy, build, doc — plus `wasm-pack build` failed **before starting**, in the workflow this
same section prescribes. The repair was to state the fact where it cannot be resolved against
the wrong file: an empty `[workspace]` table in the crate itself, which `fuzz` had carried all
along. Generalise before assuming this one was the last: **a fact about a directory belongs in
that directory**. Unlike the `../` hazard above this one fails loudly, which is the only reason
it was found in an afternoon rather than by silent absence.

**Release tracks (tag-driven, all inert until a tag is pushed):** `v*` → `justerm-core`
(crates.io) + `justerm-wasm-decode` (npm), lockstep; `renderer-v*` → `justerm-renderer`
(npm); `web-v*` → `justerm-web` (npm, #466 — **published since `web-v0.7.0`**).
**The current version is deliberately not written here any more.** Ask instead — it is one command,
and it is the only answer that cannot be stale:

```
npm view justerm-web version && git tag -l 'web-v*' --sort=v:refname | tail -1
```

**This line has now rotted twice, and the second time it was the warning itself that failed.** It
said `0.7.1` when the registry said `0.9.0`, so it was corrected to `0.9.0` *and given a warning* —
*"a version written here is a registry fact and rots silently … re-measure before leaning on it"* —
after a claim about reach had been made from the wrong number during #743. By 2026-08-24 it said
`0.9.0` and the registry said **`0.11.0`**, and #802 was routed to the maintainer as a semver
decision on the strength of it, when the field in question had never been published at all. A
warning next to a number is read as a caveat on a fact; the number wins. So the number is gone and
the query is in its place — the same repair `docs/map/README.md` made when it replaced its stored
"what has no note yet" list with the command that derives it. Each publish workflow gates on tag-version == package-version, **and on the
README carrying no expiring maturity claim**
(`.github/scripts/check-published-readme.mjs` — see Step 6; it runs before the build
so it fails fast, but it still fails *after* the tag is pushed, which is a re-tag).
Details in `docs/agents/release.md`.

**Downstream loop (after release — full cross-repo).** A root fix that ships but
leaves consumers on their old workarounds has only *relocated* the divergence.
Once a fixed `justerm-core`/`justerm-wasm-decode` is published (tag-driven, see
release.md):
- *In-repo* — bump / de-workaround `justerm-web` and `justerm-renderer` (e.g. the
  #297 VS16 renderer workaround must go once the core fix ships).
- *Cross-repo (penterm)* — raise `../penterm/src-tauri/Cargo.toml`
  (`justerm-core = "…"`) and the webview's npm `justerm-wasm-decode` to the fixed
  version, **remove the now-unnecessary workarounds**, and **flip the penterm
  tests that pinned the old bug** (the same ones that broke under the Step 4
  patch-link). penterm's manifest already tracks this contract history (wire
  VERSION bumps: justerm#38/#41/#81; the #100 rename was API/wire-invariant, a
  drop-in). Leave any workaround that was *never* bug-avoidance, with a comment
  saying why. A purely additive release (new option/constructor) obliges penterm
  to do nothing — say so explicitly.

## War-story index (rules with teeth)

- **No consumer workaround / contract≠defect** — #297/#300 (VS16 FE0F renderer workaround blocked, root → #301); the core per-char width & theme-agnostic color are contracts.
- **Concept ≠ mechanism** — #150 (accessible-view: VSCode concept, xterm.js extraction mechanism).
- **Never drop a corpus** — #113/#144/#207 (alt-screen cross-buffer via `abs_floor()`); #158 ("fix is small → skip the reference" caught). Note what #158 actually was: a *corpus* was dropped, not an agent merged — the precedent never spoke to how many subagents read it, which is why merging to one lens over both corpora (2026-07-24) does not contradict it. The event itself lives only in conversation; the issue body and comments carry no record of it, so this line is the whole durable trace.
- **A divergence is not a direction** — **the rule now lives in Step 5 above**, not
  here, because it was measured to be unreachable from an index: #547 paid ~40% of one
  pass's main-thread calls re-deriving by hand a call this file already documented, and
  the index is not read while a pass is being briefed. That cost is also what retired
  the corpus split — a lens holding both sides adjudicates direction itself. This entry
  stays only as the evidence pointer — #396 vs #399, deferrals #398/#400, closed #272
  with zero silent gaps. A rule whose only home is the war-story index is a rule that
  fires after the cost, not before it.
- **A reference cannot erect a claim about our design — #490 (2026-08-04), and the
  failure was that every piece of the rule was already present.** Working #490, a
  refuting lens returned `CONFIRMED` that ghostty stores OSC-133 marks as a 2-bit row
  field (`page.zig:1976`) and that a pin serialises to an origin-relative number
  (`PageList.zig:5066`) — both true, both verified. It then proposed *splitting the
  marker populations* as a peer option, and I carried that to the maintainer as one.
  It was never a candidate: ADR-0015 had already decided a marker carries identity +
  kind + exit + column, none of which a row bit can hold, and the **Wire / frame / API
  shape** row of the tie-breaker above gives the reference no authority on that layer
  at all. The maintainer caught it in one sentence — *"우리가 xterm 을 안 따르기로 한
  곳에 xterm 걸 가져오면 곤란하다"*. What makes this worth an entry is that nothing was
  missing: the tie-breaker table existed, the `DELIBERATE` grade existed, and the
  skill's *"classify findings against the record before reporting them"* existed. The
  harvest simply never asked, because no step owned the question. Two repairs, at the
  seam the skill declares: the **test** went to the skill (restate the finding without
  naming the reference — if it cannot be removed from the sentence it is a design
  proposal, not a defect), the **list** went to the deliberate-divergence table at the
  top of this file. The same pass's genuine defects all survive that test with the
  reference deleted from the argument, which is the tell to look for.
  **A third repair landed on 2026-08-08, and the gap it closed is the reason to distrust
  a repair that reads complete.** Both of the above put the check where it could only
  run *after* the finding arrived — the test on the main thread at harvest, the list in a
  file the lens is not handed. So the pass still produced reference-shaped proposals at
  full price; only their disposal got cheaper. The missing half was **entry**: Step 1's
  routing table now carries a *"what the reference's word is worth here"* column (four of
  its rows are *no vote*), and Step 5's brief carries that row plus the divergence table
  as its sixth item, so the lens grades itself instead of being graded. The portable half
  — *the brief owes the lens the list, or the test can only fire on your main thread* —
  went back into the skill beside the restatement test. Generalise the shape, not the
  case: **a repair that only makes a bad finding cheaper to dismiss has not stopped the
  finding**, and the two are easy to confuse because both show up as less time spent.
- **Real round-trip / visual side effects** — #166 (reveal-focus headless miss), #172 (live MCP path), #223 (browser verify skipped).
- **Probe a runtime fact / readPixels≠screenshot** — #328/#331 (dpr≠1 coord bug green on dpr-1), #352, #337 (tautology); #369 (a throwaway `rustc` probe pinned that an unclamped `+inf` fraction saturates `cursor_thickness`'s `u32` cast to `u32::MAX` — correcting a PR rationale that had credited `frac.max(0.0)`; the setter's `[0,1]` clamp is the load-bearing defence, `frac.max(0.0)` only neutralises `NaN`).
- **Test-trust gate** — #355 (both RED = you broke the proof; re-run baseline GREEN, remove guards one at a time). **#639 is the third bar's evidence and the more uncomfortable case**: RED→GREEN, side conditions, and a placement mutation were *all* done and green, and the fix was still wrong — its guard asked an event-driven flag about a synchronous state, and the proof awaited that very event before testing, so it never entered the window where the two candidate predicates differ. A guard and a test written against the same wrong model agree with each other; only mutating the *predicate* separates them. Found by the Step 5 lens, not by the gate.
- **Defer / negative results = the issue is the durable record** — #317 (deferral left in PR body only, caught); seed measured numbers + rejected alternatives + cleared-concern validity conditions up front.
- **Out-of-workspace / formatter / typecheck blind spots** — #333 (renderer unformatted + proofs CI), #341 (web CI + e2e tsconfig), #343/#344 (typecheck vs build).
- **Behavior-surface drift** — #129/#135 (`mouseWantedEvents` reached `types.ts` only at S16 — grep the wire mirror).
- **The backlog is a surface too (pivot sweep + file-time conflict check)** — 2026-07-21 sweep of all 22 open issues: one pivot (#273) had falsified premises in 4 of them (#398 names a file deleted in #407 and an acceptance box whose comparand is gone; #249/#317 §2 defer to a beamterm/"shared shader" layer that no longer exists; #325 still says "blocked by #273"), and 3 more pairs/clusters were live conflicts nobody had cross-linked (#440↔#490 wire channel; #494/#495/#496 = one branch's entry condition/fg/bg decided separately; #437↔#441 one port capability). Nothing fails when an issue's *premise* dies — it survives as a reason not to act, or points at a deleted file. Sweep the open backlog after a pivot; grep it by touched artifact before filing a follow-up; correct by comment, never by rewriting the body.
- **A cluster that keeps re-deciding itself = a missing model (Step 5 promotion)** — the 2026-07 cell-composition cluster. Of its 20 issues **17 were surfaced by another issue in the same set** (`#453 → {#494, #495, #496}`, `#494 → {#506, #507, #508}`); one pair — *a tile glyph's ink vs a background-ish layer* — was decided **8 separate times** (#241, #398, #430③, #453, #494, #496, #507, #508); **11** decisions contradicted or narrowed an earlier one (#453 measured *both* of its own body's premises false before starting); and xterm could not arbitrate the last four (silent #494, self-contradictory across its own call sites #495, judged the outlier #459, demoted to ADR-0017 grounds #458). Every one was filed and doc-commented exactly as this flow prescribes — **the sink was wrong, not the discipline**: an issue holds one decision with its rejected alternatives and a doc-comment pins a rule to one branch, so neither can hold a rule that *spans* decisions (#494's rationale reached 80 lines of comment on a single `if`). Promoted to **ADR-0019**, which *derives* #430 and #494 instead of restating them, and settles #507 as an implementation choice and #398 as won't-fix-with-a-reason. **How the promotion then went wrong is the more useful half.** Its first amendment generalised "a bg-only layer replaces a background-class glyph" across every route, reclassified three pins as conformance defects and spawned #496/#511 to flip them; the branch reached green host + GL proofs before two lenses and a wider prototype showed the rule erases box-drawing and shading whenever a user drags a selection over them or cycles search matches. Retracted the same day and replaced by **rule 5** (*an interaction highlight does not remove content; a declared decoration may*) — the pins were right, #496/#511 closed won't-do, no renderer change. Two lessons, both cheap to miss: a model can be internally coherent and still be reporting a defect in itself, and the tell was available early — the rule had **no user-facing benefit** anyone could name, only symmetry. Both references (xterm's flat `$fg` over a blended `$bg`, alacritty's explicit `"Reveal inversed text when fg/bg is the same"` guard) had said so from the start and were waved off with "our model governs"; it does, but a reference agreeing *with another reference* against you is signal, not noise. The trigger to notice next time is the shape, not the subject: re-deciding a known pair, a consequence *chain* rather than an edge, an earlier premise measured false, a reference that cannot arbitrate, two artifacts in this repo requiring opposite things.
- **The throughline needs a home before it earns an ADR — the spine (Step 5 / Step 6).** Both records above were archaeology: **ADR-0019** out of 20 issues, **ADR-0025** out of 9 (#521/#528/#530/#532/#533/#534/#535/#538/#540 plus the wire half of #7, filed verb-by-verb before their shared root — a row/pair property a whole-cell write silently mutates — was named). The rung below the ADR bar was the *void*, so the model had nowhere to accrete until the cluster was already big enough to promote. **What the first attempt to *use* the rung taught, before any spine existed:** both clusters that looked like candidates already had a home — the wide-spacer one under ADR-0025 (proposed), the marker-payload one (#440/#490) under ADR-0020 (accepted) — so the record table above *is* the preemption check, and a **proposed record already does a spine's job** (hypothesis + roster + an explicit not-yet-decided list). At that rung the read/write-back round trip is real and observed: #535 and #533 were worked out of ADR-0025's roster (PRs #546, #548) rather than their own bodies, and #546 amended the record back when D4 answered a combination the draft had not anticipated. **Two uses so far, and they proved different halves — which is the useful record, not the count.** `#552` (2026-07-25 → 2026-07-28) ran the rung **in reverse**: its record already existed, so the anchor was opened only to take the half ADR-0025 kept badly, the *live roster* — a hand-copied roster inside the ADR went stale in five places within three days while D1–D4 needed no edit. What that proved is not the hypothesis-holding half this rung was designed for; it is that a **roster wants a mutable home and a rule wants an immutable one**, so they separate even after promotion. `#605` (2026-07-29, `justerm-web`'s ambient work having no lifecycle owner) is the first use in the designed direction — opened *before* any record, holding a suspected root, a two-item roster and an explicit not-yet-decided list — so whether the hypothesis half pays is still open, and the falsifier is written into that issue. **`#744` (2026-08-06) is the first one where the hypothesis half paid and can be checked: it opened holding a suspected root and closed into ADR-0029 with the roster and the measurements still warm, so the record's Context is close to a transcription.** Two things it taught that the design did not anticipate. Its **falsifier fired on a clause nobody was watching** — it named two promotion conditions, neither of which happened, and closed anyway because the *other* half of the same sentence ("with nothing core learns from either") failed: #742 resolved as a *derivation* rather than the one-line contract statement the falsifier assumed. A falsifier is a conjunction, and the clause that decides is not always the one it was written for. And **its exclusion list did real work at the moment of promotion**: the roster was copied through it, keeping #746 (same subtree, different root) out of a record's evidence. (This paragraph read *"no spine issue has been opened in this repo yet"* until 2026-07-29, three days after one had closed. Prefer naming what each use taught over counting them: a count is a status claim with nothing gating it, which is what this file's own Step 6 warns about.)
- **External/registry facts** — web consumes *published* wasm (new binding `undefined` until republish); clean-room worktree only, regex discriminators `=x` / `(?i)abc` / `(?<name>x)`.
- **Downstream contract history** — penterm wire VERSION bumps justerm#38/#41/#81; #100 rename API/wire-invariant drop-in.

(A repo-wide evidence log could live in `docs/agents/lessons.md`; for now these
precedents index inline.)

## Refs

- Contract spec: `docs/architecture.md` (cells · damage · viewport/scroll · cadence · selection · serialization; §"Hidden VT state"). It deliberately does **not** carry the engine API list any more — that shape belongs to `justerm-core/src/lib.rs` / docs.rs, and the prose copy had drifted to advertising a method that never existed.
- Decisions: `docs/adr/` — 0005 (encode/decode round-trip), 0008 (decode boundary), 0013/0014 (viewport state in header), 0017 (mechanism core / policy consumer), 0018 (justerm-renderer pivot), 0019 (renderer cell composition — layered, per-channel, total; xterm is a design input, not a validator), 0020 (what qualifies for the per-frame snapshot — state / not derivable / viewport-bounded), 0021 (one GL context draws N grids as viewports; the resource tier rule), 0022 (the cell is the ink box of the font's `█`, and what derives from it), 0023 (a spacing setting is CSS px because `font_size` is), 0024 (a decoration is colours + a mark; span projection and precedence), 0025 (a row/wide-pair property has one owner and one set/clear/repair lifecycle, not a per-verb rule).
  The five most recent govern axes this flow keeps landing on. **0020–0023 were promoted to accepted
  on 2026-07-22** (`039c1cb`), and **0024 followed on 2026-08-18** once the last of the three open
  items it named (#502) was answered — a record that carries an open question inside a *rule* cannot
  be adjudicated, which is why it lagged its siblings by four weeks. Each ADR's own `Status:` line
  stays authoritative; this sentence is a pointer, not a copy:
  **0020** — what qualifies for the per-frame snapshot (state not occurrence · not derivable by the
  consumer · viewport-bounded). Read it *before* proposing a wire group: 0013–0016 each admitted one
  group on its own merits and four more versions followed with no ADR at all, which is the gap 0020
  closes. It also named `markerLines` as its one stated violation (#482/#490); v16 removed that group, and the amendment records what remains.
  **0021** — one WebGL2 context, N grids as viewports (`TerminalSurface`), with the tier rule that
  assigns every renderer **field**. Adjudicated 2026-08-18 (#768) — the contradiction it carried was a
  *category error*, not a wrong clause, so **route a new setter by D1**: the *selector* (whose state
  decides this — per-grid whenever a consumer can set it per terminal) and the *resource* (where the
  selected thing lives — per-config only when one instance can serve two grids **and** rebuilding it
  repays keying) are two questions, and the old rule answered them with one sentence. A setter is
  neither: it writes a per-grid selector *and* re-keys a per-config resource (D3). D4 is the clause
  that stops `cell_size` reading two ways — a tier names **ownership, not residency**, so a per-grid
  cached copy of a per-config value is expected and owes what
  [the cell size is derived state](../map/invariant/cell-size-is-derived-state.md) already states.
  *This entry said the clause was unresolved and instructed "don't route a new setter by that clause"
  — the reverse of what it now is; corrected with the adjudication rather than left to age.*
  **0022** — the grid cell is the ink box of the font's `█`, and the atlas cell, glyph quad, cursor box,
  CSS cell and (via `builtin`) the tile class all derive from it. Carries an invariant nothing enforces:
  no glyph this crate draws may enter measurement.
  **0023** — a consumer-facing spacing setting is CSS px, because `font_size` is: a setting in the same
  space as an existing logical one uses that space. Both references take device px and so split the
  units of one font description; read it before adding any metric setter.
  **0024** — a decoration is colours plus a mark, not an object; cell precedence is registration order,
  ruler order partitions by position class first, `anchor` moves the colour span. Note its grading habit:
  most of xterm's behaviour here is only inferable from source, so it is cited as "the implementation
  does X (file:line)", never as "xterm specifies X".
- Identity & invariants: `CLAUDE.md`. Glossary: `CONTEXT.md`. Release: `docs/agents/release.md`.
