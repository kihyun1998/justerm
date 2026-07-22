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
| **Wire / frame / API shape** | **this repo's own precedent** | No external authority exists — no reference serializes a terminal state this way (see the architecture prior-art note below: composing a render-free engine with a state wire is justerm's own bet) |
| **Consumer-facing API shape / units** | **our own API's internal coherence** | ADR-0023: `letter_spacing` is CSS px because `font_size` is, though *both* references use device px. A setting expressed in the same space as an existing one must use that space — an API the consumer has to remember two spaces for is incoherent, not merely different. Same posture as the composition row, one layer down |
| **Performance claims** | **our measurement, on a release build** | A claim about our own throughput was wrong because it was measured on a debug build; a number from a consumer's journey is a hypothesis until re-measured here |

A layer not in this table has no recorded tie-breaker — say so and ask, rather
than borrowing a neighbouring row.

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

Read real source with `gh api …/contents/<path> --jq .content | base64 -d`, then
`grep -n` / `sed -n` the actual lines. **WebFetch is banned** — it summarizes and
drops method bodies (e.g. xterm.js `InputHandler.ts`, 3.7K lines: the registry
shows, handler bodies like `setOrReportIndexedColor` get cut).

| Change type | Real source to read |
|---|---|
| **Web feature (concept/UX)** | its real source — usually **xterm.js** (`repos/xtermjs/xterm.js`; e.g. drag-scroll 50px/15, highlightLimit 1000, `_charsToConsume`); for features xterm lacks, the consumer that built it (e.g. **VSCode** `microsoft/vscode` terminal a11y) |
| **Text / coords / VT-semantics (mechanism)** | **xterm.js buffer layer + alacritty real source** + *this repo's siblings* (`docs/architecture.md` §"Hidden VT state" + `search` / `selection` / `logical-lines` cell-walk). Enumerate the hidden state the reference tracks *first* |
| **Wire / format / coord / API shape** | *this repo's sibling fields & precedent* — #129 `mouse_events`, #112 scroll, #108 overlay: how they touch struct→encode→decode→Flat→getter→`types.ts` — plus **ADR-0013/0014** (viewport state in the header) and **ADR-0008** (decode boundary). Mirror the most recent sibling verbatim |
| **Renderer cell composition** (what colour a cell's bg / fg / ink ends up) | **ADR-0019 first** — its layer stack, per-channel declaration, paint modes and ink sources answer the combination *by construction*, so start by asking what the model says. Then this repo's siblings (`overlay.rs`, `frame.rs`, `decoration.rs`, `glyph_class.rs`) for the rules in force. **xterm.js is a design input here, not a validator**: read it for *what problem exists* and how it solved it, never as the tie-breaker — a difference from it is not by itself a defect, and in the four decisions before ADR-0019 it was silent, self-contradictory, the outlier, or demoted. A combination the model cannot answer is an ADR-0019 amendment, not a new decision |

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

## Step 4 — proof method per layer (real round-trip, not a fake)

| Layer | Real proof |
|---|---|
| **core / wasm** | `encode→decode` round-trip (ADR-0005) · `vttest` · **real PTY capture** (the user's RHEL 9 VM, `capture-dogfood.sh` — vim/top/htop; TUI needs a foreground timeout, alt-screen apps snapshot just before `?1049l`) |
| **web** | `pnpm demo` real browser (DPR / coords / render bugs; canvas buffer = CSS×DPR, geometry from `rect.h/ROWS`) + `pnpm test:e2e` (Playwright headless, `webServer` auto-starts `pnpm demo` → real wasm+controller round-trip). a11y proven via **SR-consumed proxies**: announce = aria-live `textContent`, signal = console log; **suppression proof = with SR off, neither appears** |
| **renderer** | `pnpm run build:wasm && pnpm exec playwright test` over `demo/*.html` × dpr **1 / 1.1 / 1.5 / 2**, reading `window.__proof.ok`; coordinates via `demo/proof.js`, `cell_width()` in device px |
| **strongest — real consumer** | **penterm.** Link the local build in: `[patch.crates-io] justerm-core = { path = "../justerm/justerm-core" }` in `../penterm/src-tauri/Cargo.toml`, run penterm's **full** suite. Strongest evidence = a penterm test that *pinned the old bug as expected* now **breaks** while the rest stays green. For a wasm/web change, link via a **clean-room worktree** (a local pkg-swap pollutes the pnpm store) |

Traps this layer must respect:

- **A green headless E2E proves only SR-consumed proxies** (announce · signal) —
  not *visual/DOM* side effects (focus · scroll · reveal). Assert the DOM state
  directly (`document.activeElement` line index, `scrollTop`) **or** drive live
  via Playwright MCP (`browser_evaluate`), then lock the regression into E2E.
  (#166 reveal-focus; #172 live-drive path.)
- **`readPixels` ≠ a screenshot.** Headless SwiftShader composites a
  fractional-CSS canvas to white (#352); a blur metric then reads that as
  "sharpest". Beware tautological proofs (#337) — a check that can only confirm
  its own premise. Don't eyeball at dpr 1 and move on (#328).
- Visual/color changes still need a browser verify even when Step 5 is skipped
  for a closed surface — a synthetic-input unit is not a substitute (#223).

## Step 5 — adversarial two-lens

Lens ① this repo — `architecture.md` §"Hidden VT state" + sibling cell-walk
(search / selection / logical-lines). Lens ② reference — xterm.js / alacritty
real source via `gh api`. Never collapse to one lens even for a small fix (#158).
Precedent: #113 logical-lines (single-buffer view missed the alt-screen
cross-buffer defect; also surfaced the same bug in `search()` → #144; the
`abs_floor()` centralization covers logical_lines/#113 · search/#144 ·
word-sel/#207). Gate on *enumeration risk*, not diff size; a reactive spike that
keeps catching new gaps is the trigger. Record an explicit skip for a closed
surface.

**Unconditional triggers — three paths where both lenses run regardless of the
judgement above.** justerm has no money path, no production mutation and nothing
destructive, so the schema's usual examples do not apply; here a path is sacred
when it is **irreversible** (already published) or **silent** (wrong answer, no
crash, user-visible state quietly corrupted). You do not get to skip these because
the diff is small:

1. **`justerm-core/src/serialize.rs` — the wire, and any `WIRE_VERSION` bump.**
   crates.io and npm are immutable; a consumer decoding a wrong layout gets
   garbage cells, not an error. Touching `struct → encode → decode → Flat →
   getter → types.ts` in one crate and not the others is exactly the failure a
   single lens misses.
2. **The release path — `.github/workflows/*` publish jobs + `docs/agents/release.md`.**
   Publishing is tag-driven and automatic: pushing `vX.Y.Z` ships to both
   registries with no confirmation step, and nothing but a yank comes back.
3. **Absolute-index walks over the concatenated `[scrollback ++ grid]` buffer —
   `abs_floor()` (`term.rs:1842`, 4 call sites) and every reader that indexes
   absolutely.** On the alt screen an unfloored index reads the wrong region and
   returns *plausible* text: selection, search and markers silently disagree with
   the screen. This is on the list because the two-lens pass has found a fresh
   sibling three times — #113 (logical lines) → #144 (`search`) → #207
   (word-selection `prev_pos`) — so "I checked the obvious callers" has a measured
   failure rate here.

## Step 6 — behavior-describing surfaces (sweep by hand)

No change ends at the code; nothing compiles the drift away. Sweep every surface
that *describes* the behavior:

- **Public doc-comments → docs.rs.** `justerm-core`/`justerm-wasm-decode` ship
  their `///` / `//!` comments verbatim as the crate's **docs.rs** API reference
  (core has ~20 in `lib.rs` alone) — the surface most likely to still describe the
  old behavior. Update them in the same change.
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
  checks cover most of it:** a constant quoted in a README (a wire version, a
  stride, a version number) must be greppable against its definition, and a README
  that describes the crate's *maturity* ("under construction", "lands in #N")
  expires at the next publish — re-read it whenever you push a publish tag.
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

### Where a promoted decision record goes, and what earns one

**Destination + format.** `docs/adr/NNNN-<kebab-slug>.md`, **English**, numbered
sequentially, opening with `Status: accepted (YYYY-MM-DD[, #issue])` and following
the house sections: `Context` (with the forcing case) → `Decision` → `Named prior
art` → `Consequences` → `Alternatives considered`. Amend in place rather than
rewriting history: a status-line note when a later change moves the *reason*
(ADR-0011, #504) or realises a direction (ADR-0012→0018), a `supersedes` /
`superseded by` pair when it is replaced (ADR-0002↔0018). An ADR may carry no
issue number — 0018 and 0019 do not.

**What earns one.** The portable bar governs: **two or more Step 5 promotion
triggers**, not one. Below that it is a decision and belongs in the issue as usual.

Areas already known to have hit the bar — check these first, since a new question
in one of them is probably a conformance item under an existing record rather than
a fresh decision:

| Area | Record | State |
|---|---|---|
| Renderer **cell composition** (a cell's bg / fg / ink) | **ADR-0019** | Recorded. Open questions here resolve *against the model*; a combination it cannot answer is an amendment |
| **core ↔ consumer routing** (mechanism vs policy) | **ADR-0017** | Recorded. Its own rejected `(D) keep deciding case by case` is the pattern to watch for |
| **Wire / frame shape** | 0005, 0008, 0013–0016 | Recorded across several, each at a version bump |
| **Span projection / decoration geometry** (viewport clamp, anchor, precedence) | **ADR-0024** (proposed) | **Promoted 2026-07-21**, over #120/#198/#202/#457/#458/#459/#461/#463/#480/#498. The triggers that carried it, kept because they are what the bar looks like in practice: "which decoration wins" was decided at three granularities (#452 per-property within a marker, #458 across markers, #498 ruler marks); #452→#457→#461 is a consequence chain, with #461 recorded as *"the vertical mirror of 457"*; and #457 found a repo test comment asserting the opposite of the behaviour. Kept out of ADR-0019 deliberately — consumer policy under ADR-0017, viewport-only; 0024 opens by placing itself on *"the axis ADR-0019 explicitly put out of its own scope"* |

Naming the areas is the point: a maintainer can see a cluster has been re-decided
long before the person inside it can, so Step 5 starts by asking whether the work
sits in one of these rather than re-deriving that from scratch each pass.

## Step 7 — gate matrix + downstream loop

**core / wasm:**
```
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo check --manifest-path fuzz/Cargo.toml
cargo build -p justerm-wasm-decode --tests --target wasm32-unknown-unknown
```
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
cd justerm-renderer && pnpm run test:unit                                                      # demo/proof.js pixel helpers
cd justerm-renderer && pnpm run test:proofs                                                    # ONLY if the GL layer changed (#328/#331)
```
CI wired since #333 (`renderer`, `renderer-proofs`).

**Gate hygiene:** run each gate **bare, never piped** (`test … | tail -1 &&
commit` always commits — a pipeline's status is `tail`'s). **Never move a
threshold** (coverage floor / lint budget) to turn a build green.

**Branch / PR / CI:** branch → `feat(<scope>): … (#issue)` (**no `Co-Authored-By`
trailer**) → squash PR (`Closes #issue`) → confirm CI jobs green:
`test` / `wasm` / `renderer` / `renderer-proofs` / `web` / `web-e2e`. A PR touching
`.github/workflows/**` also gets **`supply-chain`** (path-filtered, so it is absent
otherwise) — reproduce it locally with
`cargo run -- scan --strict <justerm repo root>` in `../just-shield`. **Point it at the
repo root, not `.github/workflows`**: given the wrong path it reports "0 workflows
scanned" *and* a green "no violations", a vacuous pass. Don't watch CI *during*
implementation (local gates mirror it) — except wasm browser `wasm_bindgen_test`,
which runs only in the CI wasm job, so check it once per wasm-decode-changing PR.

**Release tracks (tag-driven, all inert until a tag is pushed):** `v*` → `justerm-core`
(crates.io) + `justerm-wasm-decode` (npm), lockstep; `renderer-v*` → `justerm-renderer`
(npm); `web-v*` → `justerm-web` (npm, #466 — workflow exists, nothing published yet).
Each publish workflow gates on tag-version == package-version. Details in
`docs/agents/release.md`.

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
- **Two-lens, never collapse** — #113/#144/#207 (alt-screen cross-buffer via `abs_floor()`); #158 ("fix is small → one lens" caught).
- **Two-lens divergence *direction* (which way the fix goes)** — a lens finding "differs from the reference" does NOT by itself say "move to the reference". The *direction* depends on what the two lenses **share**: if only the reference lens (②) diverges (the sibling is reference-correct, this layer alone drifted) you move toward the reference; if BOTH lenses share the divergence (sibling == this layer, both drift from the reference) it is a **family** decision — keep the consumer-neutral behaviour now, track the reference-parity fix as a coordinated multi-layer change. #396 (slice-2 `minimumContrastRatio`: Lens ② found justerm-web's double-pass was a *beamterm-forced* compromise the renderer's own architecture doesn't need → moved to xterm's single pass, MORE correct AND still web-neutral for the common case) vs #399 (slice-4 tile re-tint: Lens ① found `renderer == web` byte-for-byte, Lens ② found *both* diverge from xterm → kept web-neutral for #273, family fix tracked #398). One lens structurally cannot make this call — it sees the divergence but not whether the sibling shares it. Deferrals tracked as issues (#398 tile-retint, #400 search-match-solid), so the closed #272 leaves zero silent gaps.
- **Real round-trip / visual side effects** — #166 (reveal-focus headless miss), #172 (live MCP path), #223 (browser verify skipped).
- **Probe a runtime fact / readPixels≠screenshot** — #328/#331 (dpr≠1 coord bug green on dpr-1), #352, #337 (tautology); #369 (a throwaway `rustc` probe pinned that an unclamped `+inf` fraction saturates `cursor_thickness`'s `u32` cast to `u32::MAX` — correcting a PR rationale that had credited `frac.max(0.0)`; the setter's `[0,1]` clamp is the load-bearing defence, `frac.max(0.0)` only neutralises `NaN`).
- **Test-trust gate** — #355 (both RED = you broke the proof; re-run baseline GREEN, remove guards one at a time).
- **Defer / negative results = the issue is the durable record** — #317 (deferral left in PR body only, caught); seed measured numbers + rejected alternatives + cleared-concern validity conditions up front.
- **Out-of-workspace / formatter / typecheck blind spots** — #333 (renderer unformatted + proofs CI), #341 (web CI + e2e tsconfig), #343/#344 (typecheck vs build).
- **Behavior-surface drift** — #129/#135 (`mouseWantedEvents` reached `types.ts` only at S16 — grep the wire mirror).
- **The backlog is a surface too (pivot sweep + file-time conflict check)** — 2026-07-21 sweep of all 22 open issues: one pivot (#273) had falsified premises in 4 of them (#398 names a file deleted in #407 and an acceptance box whose comparand is gone; #249/#317 §2 defer to a beamterm/"shared shader" layer that no longer exists; #325 still says "blocked by #273"), and 3 more pairs/clusters were live conflicts nobody had cross-linked (#440↔#490 wire channel; #494/#495/#496 = one branch's entry condition/fg/bg decided separately; #437↔#441 one port capability). Nothing fails when an issue's *premise* dies — it survives as a reason not to act, or points at a deleted file. Sweep the open backlog after a pivot; grep it by touched artifact before filing a follow-up; correct by comment, never by rewriting the body.
- **A cluster that keeps re-deciding itself = a missing model (Step 5 promotion)** — the 2026-07 cell-composition cluster. Of its 20 issues **17 were surfaced by another issue in the same set** (`#453 → {#494, #495, #496}`, `#494 → {#506, #507, #508}`); one pair — *a tile glyph's ink vs a background-ish layer* — was decided **8 separate times** (#241, #398, #430③, #453, #494, #496, #507, #508); **11** decisions contradicted or narrowed an earlier one (#453 measured *both* of its own body's premises false before starting); and xterm could not arbitrate the last four (silent #494, self-contradictory across its own call sites #495, judged the outlier #459, demoted to ADR-0017 grounds #458). Every one was filed and doc-commented exactly as this flow prescribes — **the sink was wrong, not the discipline**: an issue holds one decision with its rejected alternatives and a doc-comment pins a rule to one branch, so neither can hold a rule that *spans* decisions (#494's rationale reached 80 lines of comment on a single `if`). Promoted to **ADR-0019**, which *derives* #430 and #494 instead of restating them, and reclassifies #496/#508 as conformance defects, #507 as an implementation choice, #398 as won't-fix-with-a-reason. Three pins contradict the model, all three resting on xterm parity alone — tracked for adjudication, not flipped. The trigger to notice next time is the shape, not the subject: re-deciding a known pair, a consequence *chain* rather than an edge, an earlier premise measured false, a reference that cannot arbitrate, two artifacts in this repo requiring opposite things.
- **External/registry facts** — web consumes *published* wasm (new binding `undefined` until republish); clean-room worktree only, regex discriminators `=x` / `(?i)abc` / `(?<name>x)`.
- **Downstream contract history** — penterm wire VERSION bumps justerm#38/#41/#81; #100 rename API/wire-invariant drop-in.

(A repo-wide evidence log could live in `docs/agents/lessons.md`; for now these
precedents index inline.)

## Refs

- Contract spec: `docs/architecture.md` (cells · damage · viewport/scroll · cadence · selection · serialization · engine API; §"Hidden VT state").
- Decisions: `docs/adr/` — 0005 (encode/decode round-trip), 0008 (decode boundary), 0013/0014 (viewport state in header), 0017 (mechanism core / policy consumer), 0018 (justerm-renderer pivot), 0019 (renderer cell composition — layered, per-channel, total; xterm is a design input, not a validator), 0020 (what qualifies for the per-frame snapshot — state / not derivable / viewport-bounded), 0021 (one GL context draws N grids as viewports; the resource tier rule), 0022 (the cell is the ink box of the font's `█`, and what derives from it), 0023 (a spacing setting is CSS px because `font_size` is), 0024 (a decoration is colours + a mark; span projection and precedence).
  Three more are **proposed** (authored, not adjudicated) and govern axes this flow keeps landing on:
  **0020** — what qualifies for the per-frame snapshot (state not occurrence · not derivable by the
  consumer · viewport-bounded). Read it *before* proposing a wire group: 0013–0016 each admitted one
  group on its own merits and four more versions followed with no ADR at all, which is the gap 0020
  closes. It also names `markerLines` as its one stated violation (#482/#490).
  **0021** — one WebGL2 context, N grids as viewports (`TerminalSurface`), with the tier rule that
  assigns every renderer setter: share byte-for-byte ⇒ per-config, must differ visibly per terminal ⇒
  per-grid, consumer-settable ⇒ per-grid by definition.
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
