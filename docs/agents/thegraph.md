# thegraph build (justerm)

The compiled graph for the `thegraph` skill — which nodes justerm actually has, how many of
each, what data each one reads, and which are extracted as agents and scripts. The skill holds
the **method** (node-type catalog, four invariants, reasoning habits); this file holds justerm's
**graph**. Built by `/grill-the-graph` from [`theflow.md`](theflow.md), which stays the owner of
the two tables the whole graph is graded against (tie-breaker, deliberate divergences) and of the
war-story index.

**Build stamp:** `thegraph` as of 2026-08-24. Generated artifacts carry the same stamp; the skill
warns when it is behind and never rebuilds on its own.

**`theflow` is not retired.** It remains a valid sibling discipline callable as `/theflow`, and it
owns data this file points at rather than copies. The default for a substantive change is
`thegraph`.

> **Three things this build supersedes in `theflow.md`.** Read them here, not there.
>
> 1. **The spine roster is a tracker relation, not a body list.** `theflow.md`'s spine paragraph
>    says GitHub's defaults are *"`part of #<spine>` tracked-by, an edited body for the roster"*.
>    Measured 2026-08-24: `gh api repos/kihyun1998/justerm/issues/630/sub_issues` returns `#631`,
>    `#632`, `#808` — a real parent/child relation. The catalog forbids the body copy outright
>    (*"What never goes into the body is the roster itself"*). See `### spine`.
> 2. **This file owns the reference-pin table.** `theflow.md` carries an older four-tree copy, and
>    it is an *input* to this build, which does not edit its input. `.github/scripts/cite.mjs`
>    reads **this** file (`--pins`), so the pin refreshed here is the pin that is checked. Adding a
>    tree means: a row here, `TREES` in `cite.mjs`, and nothing in `theflow.md`.
> 3. **`downstream`'s consumer verdict is derived, never stored.** `theflow.md` carries a dated
>    *"the npm packages have no known consumer"*. That is a derivable fact with a timestamp — the
>    exact shape that file already repented of for the published version number. Only the
>    **derivation correction** survives here. See `### downstream`.

---

## Node roster

Every node type in the catalog exists in justerm. That is unusual and worth stating: a repo that
publishes nothing has no `downstream`, and a repo with no territory map has no `map`. justerm has
both, so **no absence needs a reason.**

| Node | Count | What settled it |
|---|---|---|
| `classify` | 1 | catalog |
| `spine` | 1 | catalog; tracker has real parent/child |
| `map` | 1 | `docs/map/` (hub [`README.md`](../map/README.md)) |
| `reference` | **2 source classes** | see `### reference` — the runtime probe is an *instrument*, not a class |
| `enumerate` | 1 | `docs/architecture.md` §"Hidden VT state" — a **write** surface |
| `boundary` | 1 | ADR-0017 |
| `implement` | **3** | one per layer: core/wasm · web · renderer |
| `proof` | **3** | same layers |
| `verify` | **2** | 1 + refuter, because the sacred-path list is non-empty |
| `sweep` | 1, fanning to **11 surfaces** | see `### sweep` |
| `gate` | 1 | **22 commands in 4 groups**, 21 of them pass/fail |
| `search` | once per candidate | 10 areas already carry a record |
| `batch` | 1 or more | catalog |
| `stop` | edge-triggered, **2 guards** | see `### stop` |
| `decide` | edge-triggered | catalog |
| `promote` | 1 | `docs/adr/NNNN-<kebab>.md` |
| `downstream` | 1 | publishes to crates.io + npm across 3 tag tracks |

### Back-edges

The catalog declares two. **This build declares a third**, because justerm's proofs fail routinely
and in kind-specific ways and invariant ② forbids a run inventing an edge when it needs one.

| Edge | Guard | Bound |
|---|---|---|
| `verify` → `implement` | a `CONFIRMED` finding | catalog's |
| `gate` → `implement` | any command exited non-zero | three consecutive failures with the same signature → `decide` |
| **`proof` → `implement`** | the layer's proof did not hold — a golden reddened, the eyeball shows wrong pixels, or the consumer suite broke somewhere other than the pinned test | **three consecutive failures with the same signature → `decide`.** Mirrors `gate`'s deliberately: the third failure of one proof usually means the **proof** is wrong, not the code — the case `### proof`'s golden-selection trap is about |

---

## Per-node data

### `classify`

Nothing project-specific beyond the catalog's three outs. justerm's **trivial** examples, for the
one out that needs them: a typo, a comment, a rename the compiler fully checks. Note that a rename
is *not* trivial when it crosses a workspace exclusion — `cargo test --workspace` does not build
`justerm-renderer`, `fuzz` or `justerm-facade`, so a public-path change there compiles nowhere
until its own gate runs.

### `spine`

**The tracker is GitHub with sub-issues, and the roster is that relation.** Supersedes
`theflow.md` (see the box at the top).

```
gh api repos/kihyun1998/justerm/issues/<spine>/sub_issues \
  --jq '.[] | "\(.number) \(.state) \(.title)"'
```

- **Enrol, do not announce.** A new sibling joins through the relation; a comment saying it exists
  is not a roster entry.
- **`.parent` is absent from the REST payload** — to learn an issue's parent, list the candidate
  parent's `sub_issues` rather than reading the child.
- **Flush split**, so nothing lands in the wrong half: judgements → **body edit** (they read as
  current state, and a stale one is a false status report); evidence → **append-only comment** (it
  is a belief record); relations → **the tree**.
- The area's spine, or the record that preempts it, is read **before** the issue's own body.
  Conventions for the `gh` calls themselves are in [`issue-tracker.md`](issue-tracker.md).

### `map`

[`docs/map/`](../map/README.md), read **before `boundary`**, never at `verify`.

- `## Blast radius` is the sibling list — it is also the corpus-① brief for `verify`, pre-computed.
- `## Cross-cutting invariants` are the facts that hold **outside** the territory you are standing
  in, which is exactly why they are not visible from it.
- An empty `## Governing decisions` is a valid entry, not a defect.

### `reference` — 2 source classes, **none summarized**

The summarizing route is **banned by name**: `WebFetch` drops method bodies from large files (an
`InputHandler.ts` handler that *is* there reads as absent), and whole-file `gh api` costs a 10K-line
fetch for an 8-line fact *and* leaves the file in context for every later turn. Measured on the
switch to local trees: four cited facts resolved in **0.35 s** total.

**Class 1 — pinned reference trees**, in `../.refs/`, read with `rg`, resolved against the **main
checkout** (see `## Environment preconditions`).

**This table is the authoritative pin record, and `.github/scripts/cite.mjs --pins` parses it.** It
reads the tree name from column 1 and the SHA from column 3 — keep that shape, or `--pins` silently
reports every tree as unpinned. `theflow.md` carries an older four-tree copy; it is an *input* to
this build and is deliberately not edited, so **this** is the one to change.

| Tree | Path | Pin | Scope |
|---|---|---|---|
| alacritty | `../.refs/alacritty` | `852e971cddfabe222d2d5bcda466e130f53af207` | sparse: `alacritty_terminal`, `alacritty/src` |
| ghostty | `../.refs/ghostty` | `e6e26e165ab143f087761cee9f8a479801a27ba7` | sparse: `src` |
| xterm.js | `../.refs/xterm.js` | `699f5537b0232e444cb98261b8b3991c3cfecb5e` | sparse: `src`, `addons`, `test`, `typings` |
| three.js | `../.refs/three.js` | `83d8667898fd32a6a0f1af92f6d91065db272ce2` | sparse: `src/renderers`, `examples` |
| xterm | `../.refs/xterm` | `6380a3eaed857c182ea6cfa78cd706966b2628d0` | tag **`xterm-410`**; **whole tree** — flat repo, 111 files, 8.8M, sparse buys nothing |

```bash
cd ../.refs && git clone --depth 1 --filter=blob:none --branch xterm-410 \
  https://github.com/ThomasDickey/xterm-snapshots xterm
```

**Why xterm was added (2026-08-24), and what it is *not*.** ADR-0004 gives **the spec** the top
authority on the whole VT layer — above every implementation including ours — and that layer had
**no reach at all**: the Step 1 routing row named only xterm.js, alacritty and this repo's
siblings, and ADR-0004 itself cites *"the DEC spec (DECSC/DECRC save set)"* in prose with no
document reference. With the summarizing route banned, the only remaining route was recollection,
which is summarized by definition and can never yield `CONFIRMED`. So the highest-authority row in
the tie-breaker was unreachable.

Read the substitution precisely, because the obvious reading is wrong:

- **`ctlseqs.txt` is an index, not a semantics document.** DECSC is one line — `ESC 7  Save Cursor
  (DECSC), VT100.` (`ctlseqs.txt:399`) — which cannot settle *what is in the save set*, the exact
  question ADR-0004 turns on.
- **xterm's C source is what settles it, and it carries the DEC manual references.**
  `cursor.c:428-452` cites *"Page 270 of the VT420 manual (2nd edition)"*, the VT520 manual page
  5-120 and DEC 070 (29-June-1990) pages 5-186..5-191, lists the save set including **"State of
  origin mode (DECOM)"** — ADR-0004's claim, confirmable — and then records that *"some of the
  documentation is incorrect"*, the manuals disagreeing about the wrap flag. A source that
  documents where the spec contradicts itself is more useful than one that does not.
- **This is a proxy, and naming the gap is the point.** It is not ECMA-48 and not DEC's original
  manuals. What it reaches: xterm's interpretation, its sequence index, and its citations *into*
  those documents. What it does not: a formal definition nobody has transcribed. A question this
  tree cannot settle is `UNADJUDICATED`, not absent.
- **A bonus worth stating so it gets used**: xterm's *implementation* has never been a justerm
  reference — only `xterm.js` was. `charproc.c`, `cursor.c`, `VTPrsTbl.c`, `wcwidth.c` are now
  greppable, and they are the thing xterm.js is itself a port of.

**Class 2 — registry / published-package state.** `npm view <pkg> version`, crates.io, and a
**clean-room worktree** with `npm pack` when the question is *what a consumer actually receives*.
A local pkg-swap pollutes the pnpm store and `--frozen-lockfile` does not repair it. `justerm-web`
consumes the **published** `justerm-wasm-decode`, so a new binding is `undefined` at runtime until
republished.

**Not a class — an instrument: the throwaway runtime probe.** For a fact the code cannot answer
— a real coordinate, a call order, an actually-emitted event — write a disposable probe, read the
number, **delete the probe, record the number in the issue**. Renderer probes go through
`demo/proof.js`; `cell_width()` is **device px**. Reading code is not observing it: every dpr≠1
coordinate bug was *green* on a dpr-1 machine.

It is deliberately **not** counted as a source class, and the distinction is the catalog's: a probe
resolves to *our own measurement*, which the tie-breaker governs — it is not an external source that
could disagree with us. It belongs to `reference` because that is the node that goes and gets a fact,
and it is the only route left when neither a pinned tree nor a registry holds one.

**The class's accumulated cache — start here, not from a blank tree.**
[`reference-facts.md`](reference-facts.md) holds what each reference actually does, every row
`file:line` at the pinned SHA. Three of its rows exist because the obvious grep hit gives the
**wrong** answer.

**Do not type a line number.** `rg` finds; the citation is produced by the tree:

```
node .github/scripts/cite.mjs <tree> <path> --find '<text>'   # locate
node .github/scripts/cite.mjs <tree> <path>:<line>            # print + emit the Site cell
node .github/scripts/cite.mjs --pins                          # check local trees vs the SHAs above
```

Five wrong rows landed in two days, four of them wrong at the moment they were written, **all five
from copying a lens report instead of re-opening the source**. `cite.mjs` resolves trees from the
**main checkout**, so it is correct from a worktree; bare `../.refs/` is not.

**Refreshing a pin is a deliberate act, not a habit** — and there is deliberately **no periodic
pull**. A pin that moves silently makes every recorded citation unverifiable at once, and nothing
breaks visibly: the line numbers are still numbers, with different code on them. `git fetch --depth
1 && git reset --hard origin/<default>`, update the SHA here **in the same change**, and re-verify
the `reference-facts.md` rows it moved. The trigger is *a slice needs a fact the pinned copy does
not have, for that question* — which is how three.js arrived and why **wezterm still has no tree**.
Widening a sparse checkout is **not** a pin refresh: it exposes paths already at that SHA, so no
recorded line number is invalidated.

### `enumerate` — a write node

The hidden-state list lives in `docs/architecture.md` §"Hidden VT state", and you **add to it
before implementing**, not after. Classics a first-principles model omits: pending-wrap, the
wide-char spacer, the soft-wrap join, BCE.

**Removal is the mirror image.** A value read *incidentally* — feeding a boolean, gating a branch,
computed into something else — is unpinned the moment you delete it. Grep every read site first.

### `boundary` — ADR-0017

A mechanism is **core** iff it is ① VT-parsing, or ② only correct with the *whole buffer* (all
cells, scrollback, coordinates, wrap, wide-char) — a frame-mode consumer holds only the viewport
and physically cannot. **Policy** (query · regex · palette · announce policy) is injected by the
consumer so core stays policy- and theme-agnostic. *Mechanism core, policy consumer.*

**The consumer seams**, concretely: the write seam is `FrameSource`'s siblings — `SelectionPort`,
`SearchPort` and friends; queries are `Promise` IPC; web draws frame overlays but never runs the
engine.

**Owned by the consumer by definition** (not a workaround): colour interpretation, hover,
pixel→cell, debounce, scrollbar, clipboard, transport.

**Contract ≠ defect.** Theme-agnostic colour and per-char `UnicodeWidthChar` width are contracts
justerm *deliberately* holds. A consumer unhappy with one is standing on nothing valid, and "fix at
the root" means fixing the consumer.

**The boundary is a membrane.** A core floor (edition 2024, a future MSRV) rides a compatible range
straight *down* to penterm and web; and a contract change makes a consumer's **rationale** stale,
which is `sweep`'s problem.

### `implement` / `proof` — 3 layers

| Layer | Members | `proof` method | What that proof structurally cannot see |
|---|---|---|---|
| **core / wasm** | `justerm-core`, `justerm-wasm-decode` | `encode→decode` round-trip (ADR-0005) · `vttest` · **real PTY capture** from the RHEL 9 VM | A capture proves only what its **golden** asserts *and* what its **material** contains. `check_capture` pins the char grid *and* the logical lines, because a soft-wrap link is not a character. A TUI capture cannot supply soft-wrap material at all — every row is positioned with CUP — so that half comes from `capture-softwrap.sh`. And a corpus can supply an **axis** and still miss its **combination**: after soft-wrap material landed, all six captures still observed the wide-wrap artefact **zero** times |
| **web** | `justerm-web` | `pnpm demo` in a real browser + `pnpm test:e2e` (Playwright headless, real wasm + controller round-trip). a11y proven via **SR-consumed proxies** — announce = aria-live `textContent`, signal = console log; **the suppression proof is that with SR off, neither appears** | A green headless run proves only what it **consumes**. A visual or DOM side effect — focus, scroll, reveal — needs the DOM state asserted directly or a live drive. **Two pages, and which one a claim belongs on is not a preference**: `/` is the single-terminal harness ~69 assertions are calibrated against; `/shared-surface.html` is the only place a shared-surface claim can be proven, since the adapter's `composedSurface === false` branch runs nowhere else. A new spec file needs **its own** warm-up `beforeAll` — it is per file per worker and inherits nothing |
| **renderer** | `justerm-renderer` | **Two tools, neither substituting for the other.** *Gate*: `pnpm run build:wasm && pnpm exec playwright test` over `demo/*.html` × dpr **1 / 1.1 / 1.5 / 2**, reading `window.__proof.ok`. *Eyeball*: **Playwright MCP against a real browser** — `pnpm build:wasm` → `node scripts/serve.mjs` (:8269) → navigate a scratch `demo/*.html` → `browser_evaluate` → screenshot, then delete the scratch page (both runners auto-collect `demo/*.html`) | The gate asserts pixels the compositor **never touched**; the eyeball is the only way to see what a person sees. `readPixels` ≠ a screenshot — headless SwiftShader composites a fractional-CSS canvas to white, and a blur metric then reads that as "sharpest". Wanting to *look* at renderer output is not a reason to screenshot the headless run; it is the reason to open a real browser |

**The strongest proof runs in a real consumer, and it attaches to every layer as its strongest
form.** penterm: `[patch.crates-io] justerm-core = { path = "<worktree>/justerm-core" }` in
`../penterm/src-tauri/Cargo.toml`. **Point it at the worktree you are editing** — `../justerm/…`
builds master and the proof passes for the wrong reason. Run penterm's **full** suite; the strongest
evidence is a penterm test that *pinned the old bug as expected* now **breaking** while the rest
stays green. For a wasm/web change, link through a **clean-room worktree**.

**Recording a capture on the VM** (material acquisition for the core/wasm layer; agent-side, no user
in the loop). `justerm-vm` is a `~/.ssh/config` alias.

```bash
scp justerm-core/tests/fixtures/capture-softwrap.sh justerm-vm:/tmp/
ssh -tt justerm-vm 'stty rows 24 cols 80; stty size; \
    rm -rf /tmp/capout && mkdir /tmp/capout && bash /tmp/capture-softwrap.sh /tmp/capout'
scp justerm-vm:/tmp/capout/'*.raw' "$SCRATCH/"
```

- **`-tt` is not optional and its absence is silent.** No tty means an unsized pty, and the TUI lays
  out to a winsize nobody chose. Print `stty size` **inside the same invocation** and confirm `24
  80` before trusting a byte.
- **Pin `LC_ALL=C.UTF-8`**, not `C`: the same htop recording differs by locale, and `C` strips
  precisely the Unicode material this engine exists to get right.
- **Prove the pipe before trusting a recording.** The deterministic captures are byte-reproducible,
  so re-record and `sha256sum` against the checked-in fixtures — a match proves transfer, locale,
  line endings and `.gitattributes` all round-trip.
- **The box is mutable.** Check `command -v script expect less` on entry rather than trusting any
  sentence written here.

**Before recording, decide which golden can observe the change; after, turn the fix off and confirm
that one reddens and the others do not.** A capture whose golden cannot fail reads as new coverage
while proving nothing — and the harness has had the same defect as the engine, which is how a
golden stayed green in both states by construction.

**An experiment that saturates the workstation is asked about before it is spawned — every time.**
This box is shared with the maintainer and with other agent sessions. Do **not** answer this by
lowering the load: the contention *is* the experiment. Ask first, then **box it** with
`ProcessorAffinity` and scale the load to keep the ratio inside the box, stating that absolute
milliseconds are then incomparable and only the ratio survives. A load generator must **poll its
parent pid and exit** when the parent dies — a `finally` block does not survive a Task Manager kill.
This is `stop`'s second guard.

### `verify` — 2 lenses

**One lens, briefed on both corpora** — never one lens per corpus. A lens holding half the material
can see that two things disagree but not which one is wrong, so every divergence returns to the main
thread to be adjudicated from cold against material the other half had open. Measured: the two
rejected findings of a split pass took **~40% of that pass's main-thread calls** and changed no
code; the reference-side lens, which had to read both sides to compare at all, returned 3 real
defects of 4 against the sibling lens's 2 of 5.

**Start it at GREEN and keep working.** It is read-only, so it is not a barrier — wall clock is
`max(lens, rest)`, not the sum. Sequencing is not the discipline; **harvesting** is, and no merge
happens before the findings are dispositioned.

**Harvest in rank order with a cap.** `CONFIRMED` first; `INERT` / `DELIBERATE` last at one line
each. **If disposing of one finding passes five tool calls without settling, stop** — it goes to
`batch` with what you have and what finishing would cost. A finding that costs more to dismiss than
the change cost to write is the pass telling you it found something worth the maintainer's judgement.

**The brief carries six items.** The catalog fixes four; these two are justerm's and both are
measured:

5. **The open-issue list** (`gh issue list`) — an already-filed gap comes back as one line,
   *"already filed: #534"*, instead of a fresh three-page write-up.
6. **What the last pass on this area found** — a lens that knows the leading-spacer half is already
   fixed looks at the trailing half, which is exactly how #535 was found.

Plus the frontier: the functions the diff touches, one hop of callers and callees, and the
invariants `docs/map/` names for that territory. Anything outside it that the lens notices anyway is
still reported — the frontier is a search order, not a gag.

**A lens's `file:line` is a candidate, not a fact.** Re-open the source or run `cite.mjs`; do not
copy a citation out of a report.

#### Sacred paths — the `code` guard into `verify`, and the refuter's budget

justerm has no money path, no production mutation and nothing destructive. Here a path is sacred
when it is **irreversible** (already published) or **silent** (wrong answer, no crash, user-visible
state quietly corrupted). These run the pass regardless of the enumeration-risk judgement, and they
are the only places the second, refuting lens is worth its cost.

1. **`justerm-core/src/serialize.rs`, and any `WIRE_VERSION` bump.** crates.io and npm are
   immutable; a consumer decoding a wrong layout gets garbage cells, not an error.
2. **The release path** — `.github/workflows/publish-*.yml` + [`release.md`](release.md). Pushing
   `vX.Y.Z` ships to both registries with no confirmation step, and nothing but a yank comes back.
3. **Absolute-index walks over the concatenated `[scrollback ++ grid]` buffer.** On the alt screen
   an unfloored index reads the wrong region and returns *plausible* text.

**The guard is a glob; it is never a call-site list.** Three artifacts each hand-wrote a *different*
affected-site set and on 2026-07-28 **all three were wrong against the code** — the first-ever
discovery missing from two of them while its issue number sat in the same paragraph. The sites are
**derived** by the grep that
[`alt-screen-buffer-floor`](../map/invariant/alt-screen-buffer-floor.md) carries; the guard matches
paths. Grep `abs_floor` **and** raw `scrollback.len()` walks — searching for the helper's name
cannot find the defect this entry exists to catch, because every miss so far was a *fresh* unfloored
walk that never mentioned it.

### `sweep` — 11 surfaces

**Three of these are main-thread only** (marked ⛔): they write to the tracker or adjudicate, and a
delegated sweeper doing either violates invariant ③ or ①.

| # | Surface | How it is read |
|---|---|---|
| 1 | `docs/map/` | Two obligations. **Coverage** may lag; **promotion** may not — if the fact this change revealed holds outside its territory, the change does not land until a `docs/map/invariant/` note exists. Answer by grep, not judgement. Also `rg '#<n>' docs/map/` when closing an issue: a note may cite it as *evidence* (still true) or as *status* (false the moment it lands) |
| 2 | Public doc-comments → **docs.rs** | Ship verbatim as the crate's API reference; the surface most likely to still describe a fixed bug as a contract. **Build them** — `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` is a gate, because updating a comment and never rendering it is how public docs end up linking private items |
| 3 | Release notes = **GitHub Releases** | **There is no `CHANGELOG.md`.** Never rewrite a published entry; open a new one |
| 4 | The **published READMEs** (4) | A *behaviour* surface, not a release artifact — snapshotted at publish, and nothing gates prose. Two checks are mechanized: `justerm-wasm-decode/tests/readme_pins.rs` ties a quoted constant to its definition on every PR, and `.github/scripts/check-published-readme.mjs` rejects expiring maturity claims at publish time. A README that starts quoting a new constant gets a new pin |
| 5 | Glossary + decision trail — `CONTEXT.md`, `docs/adr/` | A **write** surface. A change that falsifies a record's premise amends *that record* in the same change. The decision surviving is not a reason to skip it: what rots first is the **grounds**, and grounds are what the next implementer reasons from. See [`domain.md`](domain.md) |
| 6 | The **wire contract mirror** | `struct → encode → decode → Flat → getter → types.ts`. `justerm-web/types.ts` hand-mirrors the wasm getters, so grep it — and note it cannot catch **width** either: every column is `ArrayLike<number>`, so `u16` and `u32` both pass |
| 7 | **A producer API with zero consumer call sites** | Ask what call sites a new published export has downstream and treat *none* as the finding. The widget is the consumer ADR-0017 names, so a knob the renderer declares and the widget cannot reach is a contradiction in the boundary. Cross the renderer's `js_name` list and the decoder's getters against `rg` over `justerm-web/src` |
| 8 | Reclaim **now-false rationale** | Walk recent PR/issue/release reasoning and retract what the new behaviour falsified. Surviving reasons are usually the transitive ones |
| 9 | ⛔ Correct falsified premises in **open issues** | *Episodic, not per-change* — its guard is an architecture pivot, not a diff. One pivot broke premises in **4 of 22** open issues, and nothing fails when a premise dies: it survives as a reason not to act. Corrections go in **comments**, never by rewriting a body — and whatever this produces are `candidate`s for `batch`, not edits |
| 10 | ⛔ **Epic body + labels** | The inverse of #9: an epic body is a live checklist read as current state, so leaving it unedited is a false status report. Tick the box in the slice's own PR. **Sweep the labels with the body** — `blocked` is the one label that decides whether the next agent may pick the work up |
| 11 | ⛔ **Cross-check the backlog against itself** | Before opening a follow-up, grep the open backlog by the **artifact it touches** for anything its proposal would break, and cross-link **both** ways in the same act of filing. This is `search`'s conflict out, reached from the sweep side |

**Judge a sweep by what it cannot see, never by its hit count.** A low count means the pattern is
clean **or** the pattern is narrow, and the two are indistinguishable from the number. When a hit
turns up, **widen the pattern with the phrasing that produced it** before fixing the hit. The
narrower first version of the stale-promise grep returned exactly one hit — which read as
reassurance — while the worse of the two defects sat outside it on both axes.

```
rg -n '(lands in|will land|not yet|planned|open question|tracked in|carried over|once #|until #)' \
   --glob '*.rs' --glob '*.md' --glob '*.ts'
```

### `gate` — 22 commands, 4 groups, each run bare

**core / wasm**

```
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --manifest-path fuzz/Cargo.toml
cargo build -p justerm-wasm-decode --tests --target wasm32-unknown-unknown
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
node .github/scripts/check-map-links.mjs docs CLAUDE.md CONTEXT.md README.md
bad=0; for f in docs/map/territory/*.md docs/map/invariant/*.md; do \
  node .github/scripts/check-map-note.mjs "$f" || bad=1; done; exit $bad
node .github/scripts/check-tool-pins.mjs
```

**web**

```
pnpm typecheck      # 3 tsconfigs — running one silently leaks coverage
pnpm test
pnpm build          # guards output paths only; catches no type error typecheck missed
pnpm demo           # MANUAL verification, not a pass/fail command — see below
pnpm test:e2e       # if the change is a11y/UI-observable
```

**`pnpm demo` is the one entry that is not a gate**, and it is listed rather than dropped because
leaving it out is how a real-browser check stops being owed at all (a visual or colour change needs
one even when `verify` is skipped for a closed surface — a synthetic-input unit is not a
substitute). `gates.mjs` **names it and skips it**, reporting it as owed rather than passed.

**renderer** — outside every cargo umbrella; `cargo fmt --all` and `--workspace` visit **zero**
renderer files.

```
cargo fmt    --manifest-path justerm-renderer/Cargo.toml --check
cargo test   --manifest-path justerm-renderer/Cargo.toml
cargo clippy --manifest-path justerm-renderer/Cargo.toml --target wasm32-unknown-unknown --all-targets
cargo build  --manifest-path justerm-renderer/Cargo.toml --target wasm32-unknown-unknown
RUSTDOCFLAGS="-D warnings" cargo doc --manifest-path justerm-renderer/Cargo.toml --no-deps --target wasm32-unknown-unknown
cd justerm-renderer && pnpm run test:unit
cd justerm-renderer && pnpm run test:proofs     # only if the GL layer changed
```

**supply-chain** — path-filtered to `.github/workflows/**`, so absent otherwise. In `../just-shield`:
`cargo run -- scan --strict <justerm repo root>`. **Point it at the repo root**: given the wrong
path it reports "0 workflows scanned" *and* a green "no violations" — a vacuous pass.

**Blind spots, stated because a top-level command does not reach them:** `--workspace` does not even
**build** `fuzz`, `justerm-facade` or `justerm-renderer`, so renames and public-path changes need
their own check; `cargo fmt --all --check` is pinned to 1.96.0;
`justerm-wasm-decode/tests/web.rs` is wasm32-only and 0-compiles on host, its runtime assertions
running only in the CI wasm job.

**All three exclusions are deliberate, and knowing *why* is what stops one being "fixed" by adding it
back:** `fuzz` carries its own `[workspace]` because cargo-fuzz needs nightly and its own profile;
`justerm-renderer` is wasm32-only, so a host `cargo test --workspace` could not build it at all, and
it carries its own `[workspace]` table for the reason under `## Environment preconditions`;
`justerm-facade` is a one-shot tombstone off the version lockstep and is deliberately frozen. What is
**not** deliberate is their gate coverage — each has explicit commands above, and the exclusion is
precisely why.

**This list is a copy of CI and must say so.** Two of the commands above were missing from it until
they cost a red CI, because both are steps of the *same* `test` job — so "I ran the local matrix"
read as complete. The authoritative source is the workflow:

```
sed -n '/^  test:/,/^  wasm:/p' .github/workflows/test.yml
```

The extracted runner **asserts its list against that file** rather than restating it; see
`## Extraction plan`.

**Each gate runs bare, never piped** — `test … | tail -1 && commit` always commits, because a
pipeline's status is `tail`'s. **Never move a threshold to turn a build green.**

**Branch / PR / CI.** Worktree at `../justerm-wt-<issue>` (see `## Environment preconditions`) →
`feat(<scope>): … (#issue)`, **no `Co-Authored-By` trailer** → squash PR closing the issue → confirm
`test` · `wasm` · `renderer` · `renderer-proofs` · `web` · `web-e2e`. Do not watch CI *during*
implementation; the local gates mirror it. **`gh pr checks` can return the previous commit's
results** — match `headSha` before believing them.

### `search` — 10 areas already carry a record

Search by **the artifact** — the module, the wire field, the predicate, the config key — never by
the feature name: a related issue almost never shares your vocabulary. The trigger is **naming**,
not deciding.

**The conflict out is not `code`, and the script must not pretend otherwise.** Deciding that an
existing issue's proposal *would break* is judgement, not counting — justerm's measured cases are
all semantic and never lexical (a wire channel; one branch's entry condition, fg and bg filed as
three independent decisions; one port capability with two symptoms). **`search.mjs` owns the query;
the conflict adjudication stays on the main thread.**

**The preemption check before proposing an anchor** — an area here already has the home a spine
would provide, so a sibling is filed as a **conformance item** under that record and no anchor opens
beside it. A *proposed* record counts: it is doing an anchor's job by construction.

| Area | Record |
|---|---|
| Renderer cell composition (a cell's bg / fg / ink) | **ADR-0019** |
| Renderer resource ownership / tiering | **ADR-0021** (accepted) |
| core ↔ consumer routing (mechanism vs policy) | **ADR-0017** |
| Wire / frame shape | 0005, 0008, 0013–0016 |
| Row / wide-pair state ownership | **ADR-0025** |
| Span projection / decoration geometry | **ADR-0024** |
| An out-of-range coordinate handed in from outside | **ADR-0026** (proposed) |
| When GPU work may be attempted | **ADR-0027** (proposed) |
| What an IME composition puts on screen | **ADR-0028** |
| When a coordinate leaves core | **ADR-0029** |

**ADR-0022 and ADR-0023 are deliberately excluded**, though they are the same *shape* as 0021:
nothing has re-decided them. Listing 12 would over-preempt anchors.

**A `docs/map/invariant/` note does not preempt an anchor**, however much it looks like one. A map
note is *descriptive* and belongs in a file; a roster is *current state* and belongs somewhere
editable. The live case is [the cell size is derived
state](../map/invariant/cell-size-is-derived-state.md) with spine **#630** — both, deliberately.

Label conventions for a filed issue are in [`triage-labels.md`](triage-labels.md), which carries a
measured negative worth knowing: the five triage labels sat on **zero** open issues, so an
unlabelled issue must not be read as already triaged.

### `batch`

Catalog's envelope, unchanged. justerm's standing rule that reinforces it: **prefer fewer, verified,
currently-real issues**, and say in the PR which candidates were dropped and why, so dropping is
itself recorded.

### `stop` — 2 guards

1. **The upstream-defect interrupt** (catalog's). The urge to make a consumer test pass by
   compensating for a deeper layer's defect *is* the signal. Precedent: a core VS16 width gap worked
   around in the renderer — blocked, root fix tracked instead.
2. **The workstation-saturation interrupt** (justerm's; see `### proof`). Reached from `proof`, and
   answered by asking, never by lowering the load.

### `promote`

`docs/adr/NNNN-<kebab-slug>.md`, **English**, numbered sequentially, opening
`Status: accepted (YYYY-MM-DD[, #issue])`, house sections `Context` → `Decision` → `Named prior art`
→ `Consequences` → `Alternatives considered`. Amend in place: a status-line note when a later change
moves the *reason*, a `supersedes` / `superseded by` pair when replaced. An ADR may carry no issue
number.

**A record earns its place by deriving decisions already taken, not by listing them.** The tests it
reproduces are its evidence; the ones it contradicts are its findings — adjudicate them, do not
quietly flip them.

**A promoted record must not hold the roster.** A hand-copied roster inside ADR-0025 went stale in
**five places in three days** while D1–D4 needed no edit. A roster wants a mutable home and a rule
wants an immutable one, so they separate **even after promotion**: the record keeps the rule, the
spine keeps who is on the list. Copy the roster **through the spine's exclusion list** at promotion
— the subtree is provenance and may hold more than the cluster.

### `downstream`

**Publishes**, tag-driven, all inert until a tag is pushed: `v*` → `justerm-core` (crates.io) +
`justerm-wasm-decode` (npm), lockstep · `renderer-v*` → `justerm-renderer` (npm) · `web-v*` →
`justerm-web` (npm).

**Derive the consumer list at that moment; never store it.** The correction that makes the
derivation work — and the only part of the old note that survives here:

> **penterm's Rust dependency lives under `src-tauri/Cargo.toml`, not the repo-root manifest.** A
> top-level `grep` misses it and falsely reports "no consumer".

Check both manifests, every time:

```
grep -n justerm ../penterm/src-tauri/Cargo.toml
node -e "const d=require('../penterm/package.json');console.log(Object.keys({...d.dependencies,...d.devDependencies}).filter(k=>k.includes('justerm')))"
```

Then, per consumer: raise the constraint, **remove the workarounds the fix made unnecessary**, and
**flip the tests that pinned the old bug**. Leave any workaround that was never bug-avoidance, with
a comment saying why. **A purely additive release obliges consumers to do nothing — say so
explicitly.**

---

## Environment preconditions

Not one node's data — these gate `reference`, `proof` and `gate` together, and **every one fails
silently**, which is why they are extracted as a script (`preflight.mjs`).

- **A worktree breaks every `../` path in this file.** The sibling paths (`../.refs/`, `../penterm/`,
  `../just-shield`) are written relative to the **main checkout**. From a worktree,
  `rg <symbol> ../.refs/alacritty` is not an error — it is **zero hits**, which reads exactly like
  "no prior art". Put the worktree at `../justerm-wt-<issue>`, **beside** the main checkout, and the
  hazard does not arise; `.claude/worktrees/<slug>` is where the harness's tool lands and it breaks
  all of them. Verify on entry: `git -C ../.refs/xterm.js rev-parse --short HEAD` against the pin.
- **Port 5173 is shared.** `playwright.config.ts` sets `reuseExistingServer: !process.env.CI`, so a
  `pnpm demo` already listening from **another checkout** is silently adopted — and it makes a
  *green* run untrustworthy as readily as a red one. `netstat -ano | grep 5173` before believing
  either.
- **Local `wasm-pack` must match the CI pin** or `test:proofs` is not the gate CI runs — different
  codegen, different `wasm-opt`, both green. `check-tool-pins.mjs` compares the **workflows against
  each other** and does not look at the local binary. `rg -n WASM_PACK_VERSION .github/workflows/`,
  then `cargo install wasm-pack --locked --version <pinned>`.
- **A fact about a directory belongs in that directory.** `justerm-renderer` was excluded by the
  **root** manifest only, and cargo resolves a workspace by walking *upward* — from a worktree it
  climbed past the worktree root into the main checkout's manifest and refused to build, failing
  every renderer gate **before starting**. The repair was an empty `[workspace]` table in the crate
  itself. Unlike the `../` hazard this one fails loudly, which is the only reason it was found in an
  afternoon.
- **`/tmp` is Git-Bash-only.** A file bash writes there, Windows `python`/`node` resolve as
  `C:\tmp`. Anything two toolchains exchange goes in the scratchpad.

---

## Run state

`/.thegraph/` at the repo root — append-only during a run, **ignored by version control**. It is a
**cache**: it may be deleted at any moment and rebuilt by re-reading the issue, because the issue is
the durable record and each node flushes to it on exit. A cache that reaches a review is noise,
which is why the ignore line exists rather than a cleanup step.

The catalog fixes *what* the slots are and who writes each; it names no path, so this does. Two of
them are worth naming here because they leave the run:

- **`build_gaps`** — a slot this build failed to answer *mid-run*: a layer added since, a gate
  command CI gained, a diff touching a path the sacred list never named. Resolve it by judgement,
  **say which slot you are substituting for**, and write it here. It reaches the maintainer at
  `batch` as a re-grill request, and the **next `/grill-the-graph` reads it first**. The build stamp
  catches `thegraph` drifting ahead of this file; *nothing* catches the **repo** drifting ahead of
  it, so this record is the only detector there is.
- **`dropped`** — candidates dismissed with their ground, which go into the PR body so dropping is
  itself recorded.

**Do not edit this file from inside a run.** That is a build write, and build writes pass through the
maintainer exactly as a rebuild does.

---

## Boundary rule, tie-breaker, divergences, war stories — by pointer

These four stay in [`theflow.md`](theflow.md) and are **not copied here**. A copy is the divergence
seed, and this repo has measured that cost: a hand-copied roster went stale in five places in three
days, and a `Status:` line copied into `CLAUDE.md` said "proposed" for five days after four ADRs
were accepted.

| What | Where | Read it at |
|---|---|---|
| **Tie-breaker** — what wins when prior art and justerm's evidence disagree, **by layer** (7 rows; four of them give the reference *no vote*) | `theflow.md` § "Tie-breaker" | `reference` (which tree), **and** `verify`'s brief (what a divergence found in it is *worth*) |
| **Deliberate divergences** — where justerm does not follow its named prior art, on purpose (10 rows, each with the record that decided it) | `theflow.md` § "Deliberate divergences" | `verify`'s brief — it is what the reference-free restatement test is checked against |
| **Architecture prior art** — the two lineages frame-mode composes, and the prior-art *gaps* | `theflow.md` § "Architecture prior art" | `boundary`, for an engine-vs-renderer or state-sync question |
| **War-story index** — the precedents that give each rule teeth | `theflow.md` § "War-story index" (inline; **there is no `lessons.md`**) | Not during a run. Its own verdict: *"a rule whose only home is the war-story index is a rule that fires after the cost, not before it"* — which is why the rules that earned their place have been moved into the nodes above |

---

## Extraction plan

**4 agents** in `.claude/agents/`, **4 scripts** in `scripts/thegraph/` (node ESM, matching
`.github/scripts/*.mjs`; kept separate because those are CI's and these are not). Each artifact
carries **only justerm's data** and defers the method to `thegraph` — thin, so it survives the skill
gaining a paragraph. Each carries the build stamp.

| Artifact | Node | Carries |
|---|---|---|
| `.claude/agents/thegraph-lens.md` | `verify` | corpora paths · the tie-breaker row for the layer · the deliberate-divergence list · the six brief items |
| `.claude/agents/thegraph-refuter.md` | `verify` (2nd) | the same, opposing stance. Exists **only** because sacred paths do |
| `.claude/agents/thegraph-reference.md` | `reference` (fetch only) | the 2 source classes and how each is reached. **Returns the tree path and the raw hit; it is never the source of a `file:line` anyone copies** — the delegation would otherwise buy a wrong citation at full confidence |
| `.claude/agents/thegraph-sweep.md` | `sweep` | surfaces **1–8 only**. Surfaces 9–11 (⛔ above) stay on the main thread: they write to the tracker or adjudicate |
| `scripts/thegraph/gates.mjs` | `gate` | the command list, each invoked **bare**, taking a **scope argument** (`core` · `web` · `renderer` · `all`) because two are expensive and conditional. **Asserts its list against `.github/workflows/test.yml`** rather than restating it |
| `scripts/thegraph/triggers.mjs` | the `verify` guard | the sacred-path **globs**, matched against the diff. Never a call-site list |
| `scripts/thegraph/search.mjs` | `search` | the tracker query by artifact + the 10-area preemption table. **Query only** — conflict adjudication is not `code` |
| `scripts/thegraph/preflight.mjs` | `## Environment preconditions` | worktree location · `../.refs` pins (delegating to `cite.mjs --pins`) · port 5173's owner · local `wasm-pack` vs `WASM_PACK_VERSION` · the `just-shield` argument |

**Already extracted, not regenerated:** `.github/scripts/cite.mjs` is `reference`'s citation tool and
already exists; `readme_pins.rs` and `check-published-readme.mjs` are `sweep` surface 4's mechanized
halves.

**Refusal check.** No adjudicating node is delegated (invariant ①): the four agents are `verify`,
`verify`, `reference`-fetch and `sweep`, all delegable by the catalog; the four scripts are `code`
conditions. No `implement`, `boundary`, `enumerate`, `proof`, `batch`, `stop`, `decide` or `promote`
artifact exists. No path reaches the tracker without passing `batch` (invariant ③) — which is
exactly what restricting the sweeper to surfaces 1–8 buys. Every back-edge declares a guard **and** a
bound (invariant ②).

---

## Method gaps — `pending`, needs a `thegraph` change

Each is a rule that is **general**, not justerm data, and that the catalog claims as fixed while
lacking it. A build cannot supply these; they are recorded here so the next `/grill-the-graph` finds
them, and so a run knows it is substituting judgement.

1. **`proof` has no discriminating-power bar.** The catalog puts both mechanical bars on
   `implement`'s self-loop and gives `proof` only a *warning* about tautology. justerm has measured
   twice that the **fixture** is where it fails: a capture passed with the repair it guards
   disabled, and a golden was green in both states *by construction* while the **harness carried the
   same defect as the engine**. The missing rule: *before recording a proof artifact, name which of
   its assertions can observe the change; after, turn the fix off and confirm **that** one reddens
   and the others do not.*
2. **The test-trust gate needs a third bar and a re-baseline.** *(a)* **Mutate the predicate, not
   only the placement.** Moving or deleting a guard shakes *where* it runs and says nothing about
   whether it asks the right question — and a guard and a test written against the same wrong model
   confirm each other. Measured: RED→GREEN, side conditions and a placement mutation were all green
   and the defect survived verbatim, because the guard asked an event-driven flag about a
   synchronous state and the proof awaited that very event. Two cheap corollaries: **assert the
   window exists** before asserting behaviour inside it, and **suspect any condition that is a
   proxy** — a flag for a state, an event for a transition, a successful return for liveness.
   *(b)* **Re-run the baseline GREEN in the same pass**: both red means you broke the proof, not that
   the mutation worked. Remove guards one at a time and check a new guard fires before the old one.
   *(c)* Generalised: **for each assertion, name the mutation that should redden it and run it.**
3. **`search`'s conflict out is labelled `code` but requires judgement** — invariant ④ pointing at
   itself, the same argument the catalog itself makes for `classify`.
4. **A promoted record must not hold the roster**, and **a descriptive cross-cutting note does not
   preempt an anchor** — both measured here, neither in the catalog's promotion section.
5. **A lens's `file:line` is a candidate, not a fact.** `CONFIRMED` is defined as *"reproduced, with
   `file:line`"*, which reads as if the citation is the evidence. Five wrong rows landed in two
   days, all five from copying a lens report.
6. **Lower reach, recorded for completeness:** a sweep's hit count is not its result (widen the
   pattern with the phrasing that produced a hit, before fixing the hit); a `gate` list that mirrors
   CI should name its authoritative source rather than becoming a third copy — two repos
   independently routed around this, which is the catalog's own signal test; the harvest cap and
   rank order exist as policy but with no ordering and no number.
