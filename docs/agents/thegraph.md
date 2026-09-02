# thegraph build (justerm)

The compiled graph for the `thegraph` skill — which nodes justerm actually has, how many of
each, what data each one reads, and which are extracted as agents and scripts. The skill holds
the **method** (node-type catalog, four invariants, reasoning habits); this file holds justerm's
**graph**. First compiled by `/grill-the-graph` from [`theflow.md`](theflow.md) on 2026-08-24. That
input is **spent**: this file now owns the three tables the whole graph is graded against — the
tie-breaker, the deliberate divergences and the war-story index.

**Build stamp:** `thegraph` at **`18edd61`** (`kihyun1998/kihyun-skills`). Generated artifacts carry
the same stamp; the skill warns when it is behind and never rebuilds on its own.

**What moved since the previous stamp (`bf223be`), and what did not.** Ten commits touched
`thegraph/`. **Three inputs decide whether a roster changes at all, and all three came back
byte-identical** — the schema (*"What the build must supply"*), the node-type catalog table, and the
invariant/build split. Diffed on 2026-09-02, which is what licenses the rest of this update being a
confirmation rather than a re-derivation, and what carries the unowned walk below forward by
construction rather than by re-walking it.

The largest move — `thegraph` splitting into `SKILL.md` + `NODES.md` + `BUILD_CONTRACT.md` — **cost
this build zero artifact edits.** Grepped all nine artifacts and this file for those three names:
zero hits. That is the thin-artifact rule paying out exactly as designed: an artifact that never
names *where* a method lives is indifferent to the method moving. It is worth recording as a
measured result, because a rule whose payout is invisible is one a later build trims.

Two commits did oblige something, and only one of them reached a slot:

- **`58cef7c` — a delegated node's licence is its tool grant, not its brief.** Read-only became the
  enforced **default**, moved only by a `**Runs:**` declaration naming each write-capable tool. All
  four agents needed the declaration; **none needed a narrower grant**, because every brief here
  already named its commands. Upstream's war story was four agents with shells no brief asked for —
  the opposite case, and checking rather than pattern-matching is what told them apart. The rule also
  buys a new artifact: `scripts/thegraph/grants.mjs`, in the `gate` list.
- **`1799dbb` — `catalog_gaps` got its own slot.** A state slot is the catalog's to fix, so nothing
  in the roster moved. What *is* this build's is where the slot goes when it leaves the run, and it
  leaves to a **different tracker**. See `## Run state`.

**A revision named as history is not restamped.** Only the **Build stamp** line above carries this
build's current revision. Every other `bf223be` in this file is a fact about the past — the previous
stamp, and the walk performed at it — and stays as written. A blanket find-and-replace over a
revision id rewrites the past into the present, and afterwards the two read identically, which is
the one thing a stamp exists to prevent.

**Verify the stamp against a fetched remote, not a local clone.** The walk of 2026-08-31 first
reported this stamp *current* by comparing it to `git log -1` in an unfetched checkout of
`kihyun-skills`, which was 22 commits behind; the stamp was stale and the run said it was not. The
tell was in the same run's own output — the coverage check found two slots *"placed upstream"*, which
is only possible when the installed skill is newer than the stamp — and it was read as a result
rather than as evidence about the stamp.

**It is a commit, not a date, and the reason is this build's own first day.** The stamp started as
`2026-08-24`; `thegraph` then moved *that same day* — with the six gaps below — and a date stamp
could not tell the two revisions apart, which is the one job a stamp has. A revision identifier can
be wrong about *when*; a date can be silently right about the wrong thing.

**`theflow` is retired, and the file it left behind is not.** Measured 2026-08-31: there is no
`theflow` skill installed, so `/theflow` does not resolve, and `grill-the-flow` — the only thing
that ever authored `theflow.md` — is retired with it. The **file** survives as a *cited corpus*
rather than as a route: about 40 inbound citations, several of them from doc-comments that ship to
docs.rs (`justerm-core/src/term/walk.rs`, `justerm-renderer/src/webgl.rs`), from ADRs 0019/0021/0026,
from eight `docs/map/` notes, and from `cite.mjs`. It is therefore **not deletable on the strength of
being spent**, and nothing reads it as a method. `thegraph` is the only discipline for a substantive
change.

> **Four things this build supersedes in `theflow.md`.** Read them here, not there.
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
> 4. **This file owns the tie-breaker, the deliberate divergences and the war-story index.** The
>    schema names all three as the *build's* obligation, and the skill that maintained `theflow.md`
>    is retired. They are reproduced below **verbatim** at the absorption — and this build does not
>    edit its input, so `theflow.md`'s copies are superseded where they disagree, exactly as the pin
>    table above already is. Removing them there is the maintainer's act, not this build's.
>    Absorbing also caught the drift a pointer hides: this file said the divergence table had **10**
>    rows and it had **12** at that moment (#792 and #810 added one each after the first build).
>    Written in the past tense on purpose — a live count in prose is the same defect one turn later,
>    and the table has since grown again (#823).

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
| `reference` | **3 source classes** | see `### reference` — the runtime probe is an *instrument*, not a class |
| `enumerate` | 1 | `docs/architecture.md` §"Hidden VT state" — a **write** surface |
| `boundary` | 1 | ADR-0017 |
| `place` | 1 | catalog. The **tree rule** is `### place`, authored by `plat` against 7 maintainer-confirmed peers |
| `implement` | **3** | one per layer: core/wasm · web · renderer |
| `proof` | **3** | same layers |
| `verify` | **2** | 1 + refuter, because the sacred-path list is non-empty |
| `sweep` | 1, fanning to **12 surfaces** | see `### sweep` |
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
- **`.parent` is absent from the REST payload, and GraphQL has it.** Measured 2026-08-31, on #823:

  ```
  gh api graphql -f query='{repository(owner:"kihyun1998",name:"justerm"){
    issue(number:823){ parent{ number title } } }}'   ->  parent #47
  ```

  Use that. The fallback — listing each candidate parent's `sub_issues` — still works and is what to
  reach for when GraphQL is unavailable, but it costs one query per *candidate*, so on a backlog of
  any size it is the expensive answer to a cheap question.

  **Read a missing field as a missing field, not as an answer.** `gh api repos/…/issues/<n> --jq
  '.parent.number // "none"'` returns `none` for every issue in this repo, including the eight that
  have a parent, because REST does not carry the field at all. #823's `spine` node reported "no
  parent, no siblings" on the strength of it and a duplicate anchor was filed before a `422 Sub
  issue may only have one parent` exposed it — the write was the only thing that would have. A query
  that returns nothing for a relation that exists is indistinguishable from a clean result.
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

**If no territory matches, that is an exit this node owes an answer for — and the hub already has
it.** Read [`README.md` § *Current coverage*](../map/README.md#current-coverage): it settles whether
an absent note is a **correct state** (the area is only a plan; the epic is its roster, and a
territory appears when the first slice lands) or a **finding** (you are changing code nobody has
mapped, and you are the first person standing there with the evidence). The finding does not block —
coverage may lag — so it is carried as a candidate to `batch`, never written unasked.

Two failure shapes worth naming before you read an empty result as clean:

- **Your search may be the thing that was empty.** Search by the **artifact** — the module, the wire
  field, the predicate — never by the feature name, for the same reason `search` states it: a
  territory rarely shares the vocabulary of the change that lands on it.
- **The invariant half does not lag.** A cross-cutting fact this change reveals is `sweep`'s
  obligation and it *blocks*. Coverage is a report; promotion is a constraint. Do not let the first
  excuse the second.

### `reference` — 3 source classes, **none summarized**

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

**Class 3 — layout peers**, read for `### place` and nothing else. **Named, never stored**: a copy
of somebody else's tree is a derivable fact that rots, so what this build keeps is the *names* and
the rule they produced. Read on demand as a **real tree** at a stated depth — a layout described on a
documentation site is summarized and can never confirm anything, while a repository's tree can.

| Peer (named at the package, not the repository) | The role it is a peer for | How it is read |
|---|---|---|
| `alacritty_terminal` | the engine crate | Class 1, pinned |
| `ghostty/src/terminal` | the engine crate | Class 1, pinned |
| `xterm.js` — `src/common` · `src/browser` · `src/headless` · `addons/*` | the engine ↔ widget seam | Class 1, pinned |
| `three.js/src/renderers` | renderer internals | Class 1, pinned |
| `wezterm-term` (the `term/` directory of `wezterm/wezterm`) | a Rust multi-crate workspace's seam | `gh api repos/<r>/contents/<path>` — a **tree listing**, never a file fetch |
| `tree-sitter/lib/binding_web` | the wasm-binding package | same |
| `junkdog/beamterm` | the wasm WebGL2 renderer crate | same |

**Confirmed by the maintainer on 2026-08-31**; a peer set the build chose alone would be authority it
invented and then deferred to. Four of the seven are already Class 1 trees. The other three carry
**no pin**, which bounds them exactly: a tree listing settles a *layout* question and can never
settle a semantics one.

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

### `place` — the tree rule

**Read before `implement`, never after.** In a repo whose identity is a boundary the directory
boundary is where the seam is *physically* expressed, so a file written to the wrong one breaks the
seam while producing **no error, no failing test and no warning** — and everything the rule would
have said arrives later as rework: the imports, the module wiring, the history.

**Which input won: the declarations, and nothing violates them.** Counted 2026-08-31 — ADR-0010's
crate-prefix rule holds 5/5 with one *recorded* exception (`justerm-facade/` → package `justerm`, the
tombstone); the shape ↔ `Term`-half rule is declared in the doc-comments at both ends and holds 3/3
(`search`, `selection`, `logical`); each of the three workspace exclusions carries its reason in
`Cargo.toml`. So the tree is not in conflict with the declarations — it is a **sharper** statement of
them, which is why the rule below is written finer than `CLAUDE.md` states it.

**Four suspected splits, all sorted by content rather than by filename**, because two directories
holding the same *kind* of file by name routinely hold two different *roles*:

| Suspected split | What the sort was on | Count |
|---|---|---|
| `justerm-core/src/<x>.rs` vs `src/term/<x>.rs` | the returned **shape** and the coordinate model it documents / the **`Term` half**, cell-aware and needing the whole buffer | 3 / 3 |
| core: 4 inline `mod tests` vs 78 files in `tests/` | module-internal unit / through the public API | 4 + 78, overlap 0 |
| renderer: 26 inline vs **no** `tests/` directory | pure module / `webgl.rs` + `rasterizer.rs`, wasm32-only and 0-compiling on host, so their proof is `e2e/` | 26 + 2 |
| three script homes | CI's / the graph build's / the package's own | clean by owner |

None survives as a question. Where the sort comes out clean the measured rule **is** the rule.

#### The tree rule — concrete paths

A rule stated as a layer cannot be matched against a diff, so every line below is a path.

```
justerm-core/          the engine crate — VT parsing, grid, scrollback, cursor, selection, serialize.
                       No I/O, no drawing
justerm-wasm-decode/   the wasm binding crate and the npm artifact built from it
justerm-renderer/      the WebGL2 renderer crate (wasm32-only; carries its own [workspace])
justerm-web/           the browser widget TS package
justerm-facade/        the frozen `justerm` name tombstone. Directory name != package name — the
                       single recorded exception (ADR-0010)
fuzz/                  cargo-fuzz targets (own [workspace], package `justerm-fuzz`)
bench/<name>/          a cross-implementation comparison harness belonging to no crate
scripts/thegraph/      the scripts this build generates
.claude/agents/        the agents this build generates
.github/scripts/       CI's scripts
teach/                 the learning-course workspace

<crate>/src/*.rs                one module per concern, flat
justerm-core/src/<x>.rs         the returned shape, and the coordinate model it documents
justerm-core/src/term/<x>.rs    the `Term` half — cell-aware logic needing the whole buffer
<crate>/src/<x>.rs + <x>/       a module root beside the directory it roots
<crate>/tests/*.rs              integration through the public API, one file per behaviour
<crate>/tests/fixtures/         recorded material — captured VT streams (*.raw), screen-dump
                                goldens (*.golden), and the capture-*.sh that record them.
                                .gitattributes pins each kind; it is material, not test source
<crate>/tests/*.proptest-regressions   proptest's committed failure corpus, written by the runner
inline #[cfg(test)] mod tests   module-internal units
<crate>/benches/*.rs            criterion
<crate>/examples/*.rs           cargo example binaries
justerm-wasm-decode/js/         hand-written JS shipped with the binding
justerm-wasm-decode/pkg*/       generated, git-ignored

<pkg>/src/**.ts                 shipped source ONLY — `tsconfig.json` includes exactly `src`
<pkg>/test/*.test.ts            vitest units
<pkg>/e2e/*.spec.ts             Playwright browser proofs
<pkg>/demo/                     the runnable browser harness pages
<pkg>/scripts/*.mjs             that package's own tooling

docs/architecture.md            the authoritative contract spec
docs/adr/NNNN-<kebab>.md        decision records
docs/map/{README.md,territory/,invariant/}   the territory graph
docs/agents/*.md                agent-read bindings and accumulated reference facts
docs/perf/*.md                  measurement write-ups (their harness lives in bench/<name>/)

LICENSE-* · rust-toolchain.toml · .mcp.json · .github/dependabot.yml · <pkg>/.gitignore
                                toolchain and repo plumbing. No seam runs through it — claimed
                                anyway, because an unclaimed path is the guard's FINDING, and a
                                rule that does not name plumbing reports a licence edit as a new
                                top-level area
```

**The rule above is what `place.mjs` matches, and running it is what completed it.** Against all
**499** tracked files the first draft left **54** unclaimed, and every one was a role the rule had
simply not named — the generated agents, the recorded capture material, proptest's corpus, the
plumbing. None was a new area. Prose does not execute, so it cannot demonstrate that it is lying.

**`<pkg>/src` is a gate, not a convention.** `justerm-web/tsconfig.json` includes exactly `src` and
`tsconfig.test.json` includes `test`, `demo`, `e2e` — which is what makes `process` and `Buffer` a
compile error in shipped code. Co-locating a `*.test.ts` under `src/` breaks nothing visible and
dissolves that gate, which is this node's argument in one line.

**Guard mechanism: `scripts/thegraph/place.mjs` over the diff, never a recollection.** The rule is a
path list and a diff is a path list, so the check is a match. It **reports**; it does not adjudicate.

**Out-edge to `decide`. Guard:** the change needs a **new top-level area** (the last one added
nothing declared — see the unclassified row in the divergence list), or the tree rule and a named
peer disagree and the tie-breaker does not settle it. **Writes `triggers`** when the same placement
is argued twice.

### `implement` / `proof` — 3 layers

| Layer | Members | `proof` method | What that proof structurally cannot see |
|---|---|---|---|
| **core / wasm** | `justerm-core`, `justerm-wasm-decode` | `encode→decode` round-trip (ADR-0005) · `vttest` · **real PTY capture** from the RHEL 9 VM | A capture proves only what its **golden** asserts *and* what its **material** contains. `check_capture` pins the char grid *and* the logical lines, because a soft-wrap link is not a character. A TUI capture cannot supply soft-wrap material at all — every row is positioned with CUP — so that half comes from `capture-softwrap.sh`. And a corpus can supply an **axis** and still miss its **combination**: after soft-wrap material landed, all six captures still observed the wide-wrap artefact **zero** times |
| **web** | `justerm-web` | `pnpm demo` in a real browser + `pnpm test:e2e` (Playwright headless, real wasm + controller round-trip). a11y proven via **SR-consumed proxies** — announce = aria-live `textContent`, signal = console log; **the suppression proof is that with SR off, neither appears** | A green headless run proves only what it **consumes**. A visual or DOM side effect — focus, scroll, reveal — needs the DOM state asserted directly or a live drive. **Two pages, and which one a claim belongs on is not a preference**: `/` is the single-terminal harness ~69 assertions are calibrated against; `/shared-surface.html` is the only place a shared-surface claim can be proven, since the adapter's `composedSurface === false` branch runs nowhere else. A new spec file needs **its own** warm-up `beforeAll` — it is per file per worker and inherits nothing |
| **renderer** | `justerm-renderer` | **Two tools, neither substituting for the other.** *Gate*: `pnpm run build:wasm && pnpm exec playwright test` over `demo/*.html` × dpr **1 / 1.1 / 1.5 / 2**, reading `window.__proof.ok`. *Eyeball*: **Playwright MCP against a real browser** — `pnpm build:wasm` → `node scripts/serve.mjs` (:8269) → navigate a scratch `demo/*.html` → `browser_evaluate` → screenshot, then delete the scratch page (both runners auto-collect `demo/*.html`) | The gate asserts pixels the compositor **never touched**; the eyeball is the only way to see what a person sees. `readPixels` ≠ a screenshot — headless SwiftShader composites a fractional-CSS canvas to white, and a blur metric then reads that as "sharpest". Wanting to *look* at renderer output is not a reason to screenshot the headless run; it is the reason to open a real browser |

**The strongest proof runs in a real consumer, and it attaches to every layer as its strongest
form.** penterm: `[patch.crates-io] justerm-core = { path = "<worktree>/justerm-core" }` in
**`../penterm/Cargo.toml` — the workspace root, not `src-tauri/Cargo.toml`.** **Point it at the
worktree you are editing** — `../justerm/…` builds master and the proof passes for the wrong reason.
Run penterm's **full** suite; the strongest evidence is a penterm test that *pinned the old bug as
expected* now **breaking** while the rest stays green. For a wasm/web change, link through a
**clean-room worktree**.

**Two ways this silently does not link, both measured on #823 (2026-08-31), both exit 0.**

1. **Wrong manifest.** In `src-tauri/Cargo.toml` — where this entry said to put it until #823 —
   cargo prints `warning: patch for the non root package will be ignored, specify patch at the
   workspace root` and **succeeds**. The patch is a no-op and the "proof" then runs against the
   published crate. Note this does **not** contradict `### downstream`, which correctly says
   penterm's *dependency* is declared in `src-tauri/Cargo.toml`: the declaration and the patch live
   in different files, and fixing one must not "correct" the other.
2. **Semver-incompatible pin.** At the root it applies but cargo reports `patch justerm-core
   v0.15.0 … was not used in the crate graph` — penterm requires `^0.6.0` and this workspace is
   `0.15.0`. **Check the precondition before claiming the proof**: `grep -n '^justerm-core'
   ../penterm/src-tauri/Cargo.toml` against this workspace's version.

   As of 2026-08-31 that precondition **fails**, so the strongest proof is unavailable for any core
   change until penterm raises its requirement. That is a fact to *report*, not to route around: say
   the proof is unavailable and why, and do not upgrade penterm's pin to manufacture one — a nine-
   minor jump tests nine releases of drift, so a failure under it is unattributable to your slice.

**Verify the link took, never assume it.** `cargo metadata` and read the source back:

```
cargo metadata --format-version 1 | \
  node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>console.log(
    JSON.parse(s).packages.filter(p=>p.name.startsWith('justerm'))
      .map(p=>p.name+' '+p.version+' <- '+(p.source||'LOCAL PATH')).join('\n')))"
```

`LOCAL PATH` is the link; a `registry+https://…` source is the no-op above wearing a green run.

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

### `sweep` — 12 surfaces

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
| 12 | The accumulated reference cache — [`reference-facts.md`](reference-facts.md) | **A write surface, and the one whose rows go *false* rather than merely stale.** `### reference` covers reading it; this covers the other direction: a change that measures a reference **differently from what a row says** amends that row in the same change, and a change that reads a reference this file has no row for **adds** one. `rg '<the symbol>' docs/agents/reference-facts.md` by artifact, never by feature name. Two rules the `reference` node already states apply verbatim here and are the whole cost of the surface: re-open every `file:line` with `cite.mjs` rather than copying it out of a lens report, and a row asserting a reference has **no** behaviour is the most expensive kind to get wrong |

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
- **A delegated agent's tool grant is a silent failure, and this is the only position before the
  damage.** Invariant ① licenses delegating `verify`, `sweep` and `reference`-fetch on the grounds
  that they read without adjudicating, and the licence is the **grant**, never the brief's claim. A
  corrupted grant errors nowhere: the agent simply holds a tool nobody declared, and it surfaces
  when that agent mutates the worktree a second one is reading — upstream, the second then reported
  a failure it could not reproduce and graded the run `UNADJUDICATED`, correctly, from inside
  evidence the first had manufactured. `grants.mjs` is in the `gate` list too, but a gate runs
  **after** the work and CI runs after the **merge**, and neither sees the file while it is
  uncommitted. Ordering is the whole value: `node scripts/thegraph/preflight.mjs` before starting.
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
- **`catalog_gaps`** — a defect in **`thegraph` itself**, not in this file. It leaves the run to a
  **different tracker**: `batch` files it against `kihyun1998/kihyun-skills`, never against this
  repo. That destination is the only part of the slot this build owns, and it is the part a run
  cannot derive — every other destination a run can reach belongs to justerm, so a catalog defect
  written to `build_gaps` arrives at a party structurally unable to act on it, and a misrouted item
  is indistinguishable from a filed one. Telling the two apart: if a re-grill of *this* repo would
  not make the cause stop happening, it is the catalog's.
- **`dropped`** — candidates dismissed with their ground, which go into the PR body so dropping is
  itself recorded.

**Do not edit this file from inside a run.** That is a build write, and build writes pass through the
maintainer exactly as a rebuild does.

---

## Tie-breaker, deliberate divergences, war stories

**Absorbed from `theflow.md` on 2026-08-31 and owned here.** The schema names all three as the
*build's* obligation and the skill that maintained that file is retired, so a pointer would leave
three required slots in a document nothing routes to. Reproduced **verbatim** at the absorption; the
copies still sitting in `theflow.md` are superseded where they disagree, and this build does not edit
its input.

**The divergence list is co-authored.** The project-scoped rows below are the build's. A run's own
`human` calls arrive through the issue contract and are never written here.

**Read `Step N` as a node.** The absorbed text is verbatim, so it still speaks in `theflow`'s seven
steps; they were not rewritten, because re-typing twenty dense rows is exactly how five wrong
citations landed here in two days. The mapping: **1** -> `reference`, **2** -> `boundary`,
**3** -> `implement`, **4** -> `proof`, **5** -> `verify`, **6** -> `sweep`, **7** -> `gate` and
`downstream`.

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

| XTWINOPS `CSI 22 t` / `CSI 23 t` honour the **second** parameter (the axis) and **ignore the third**, so `CSI 22;2;3t` is an ordinary push | the **spec** defines the third parameter: a value in 1..10 gives *"direct access to the stack … store the title into the stack or retrieve the title from the stack **without pushing/popping**"* (`ctlseqs.txt:1698`), and xterm implements it (`charproc.c:9272` → `misc.c:7988`) | #823, on the **spec** row of the tie-breaker above — and it is the one row where that authority was *not* followed, so the grounds are the measurement rather than a competing reference. Reach of the slot form is **zero on five independent axes**: no occurrence across seven programs recorded under real ptys with a control group (vim · nvim · less · man · htop · tmux · top); **zero files** under `/usr/bin` + `/usr/lib64` matching `\[2[23];[0-9]+;([1-9]\|10)t`; **no terminfo capability emits `CSI 22/23 t` at all** under `xterm-256color`, `tmux-256color` or `screen-256color`, so unlike #825/#826/#828 terminfo cannot rank it either way and applications hardcode the sequence; **zero adoption elsewhere** (xterm.js ignores it, alacritty's dispatch never reads past `params[0]`, ghostty carries the index then drops the command as *"Unimplemented"*); and it entered xterm only in **patch #385 (2023-10-01)**, whose changelog gives its reason as symmetry *"like the XTPUSHCOLORS and XTPOPCOLORS feature"* rather than an application asking — an indexing bug in it was fixed two weeks later in #387. Cost of following the spec instead: xterm's model is one ring of `{icon, window}` pairs with a walk back through older slots, which is a different primitive from two stacks and shares its depth budget differently. A lens reporting "the spec defines this and we do not" is `DELIBERATE` with this row. **Falsifier**: a measurement showing anything emitting the 1..10 form reopens it, and nothing else does |

| DA2 (`CSI > c`) reports `Pp = 1` / `Pc = 0`, and the handler matches the **whole** intermediates slice | alacritty reports `Pp = 0` / `Pc = 1` (`alacritty_terminal/src/term/mod.rs:1267` @ `852e971`) and answers `CSI > $ c`, because its dispatch passes `intermediates.first()` rather than matching the slice (`vte-0.15.0/src/ansi.rs:1572` — the published crate, not a pinned tree) | #824. Three divergences from one reference on three different grounds, and **not one of them is ADR-0004's spec branch** — that branch covers a reference which *omits or under-implements*, and alacritty here **contradicts**, which is a third case its text does not classify; the **spec** row of the tie-breaker above is what covers it. `ctlseqs.txt:839` does not settle it either: its *"always zero"* is said of a **DEC terminal**, describing hardware rather than binding an emulator. So each field rests on its own ground. **`Pc`**: first principles — the field registers a ROM cartridge, this engine has none, `0` is the absence value, which is how both implementations that comment it read it (`charproc.c:4267` *"options (none)"*, `device_attributes.zig:88` *"Always 0 for emulators"*); 3-1 head count. **`Pp`**: our own DA1 already advertises level 62 (= VT220, `ctlseqs.txt:778`) and justerm implements neither DECTID nor DECSCL, so nothing here can decouple the level from the device type — alacritty's `?6c`/`Pp = 0` is that same rule at VT102, not a counterexample. **The slice match**: 3-1 (xterm `VTPrsTbl.c:4747`, ghostty `stream.zig:1612`, xterm.js `InputHandler.ts:233` all drop the form), with **zero measured reach** — no producer of `CSI > $ c` exists in any pinned corpus, so it is pinned by a test rather than by need. A lens reporting any of the three as "alacritty does otherwise" is `DELIBERATE` with this row. **Falsifier**: a producer of `CSI > $ c`, or a consumer that reads `Pc` as anything but a cartridge id, reopens the third and the first |
| An **empty** `OSC 52` target field names the **system clipboard** | the **spec** says it names `s0` — *"If the parameter is empty, xterm uses s 0 , to specify the configurable primary/clipboard selection and cut-buffer 0"* (`ctlseqs.txt:2161`), which `misc.c:3359` implements verbatim; and since `selectToClipboard` defaults to false (`charproc.c:472`), **xterm as shipped resolves that to PRIMARY** | #828, on the **spec** row of the tie-breaker — the second row after #823 where that authority is not followed. The grounds are that the spec's answer names two things this engine does not have: a cut buffer, which is unmodelled, and `s`, which xterm resolves through a *user resource* (`button.c:2081`) — policy ADR-0017 assigns to the consumer. Precisely the shape #834 names one family over, where xterm's trigger (*a colour that failed to parse*) is a condition this engine structurally cannot observe. Of the two readings that **are** representable, three independent lineages give the same answer, and the count is smaller than it first looks: alacritty never sees an empty field because `vte` substitutes `b'c'` first (`vte-0.15.0/src/ansi.rs:1488`), so alacritty+vte are **one**; ghostty is a second, by an explicit byte-scan branch pinned under a test (`clipboard_operation.zig:24`, `:64`); xterm.js's clipboard addon is a third, reaching it by ignoring the selector entirely (`addons/addon-clipboard/src/ClipboardAddon.ts:77`). Reach: `tmux` 3.2a emits exactly this form under `set-clipboard on`, captured on the RHEL 9 VM and checked in as `justerm-core/tests/fixtures/tmux_clipboard.raw`, and tmux's own manual documents the action as *"terminal clipboard"* — never primary. Terminfo cannot rank the axis (`Ms=\E]52;%p1%s;%p2%s\007` — the caller fills `Pc`), so the binaries were scanned instead: on that box exactly two files under `/usr/bin` + `/usr/lib64` carry an OSC 52 literal (`nvim`, `tmux`), the only `Pc` forms in them are the `%p1%s` template and a literal **empty** field, and there are **zero** multi-character targets and **zero** occurrences of `s0` — the very form the spec names as xterm's own default. **Falsifier**: an emitter measured sending `s0`, or a decision to model DECSET 1041 (`ctlseqs.txt:1008`), which sets `selectToClipboard` from the stream and would make the spec's answer representable after all |

| A payload that is **not base64 relays nothing** | the **spec** says the selection is **cleared** — *"If the second parameter is neither a base64 string nor ? , then the selection is cleared"* (`ctlseqs.txt:2174`) | #828, same row and same slice as above. What makes this narrower than the spec sentence reads: **xterm has no validator to disagree with.** `AppendToSelectionBuffer` (`button.c:4679`) decodes one character at a time and `return`s on any byte outside the alphabet (`:4698`), so it *filters* — `Zm9v-Zm9v` yields `foofoo` there — and because the store path clears the buffer before appending (`misc.c:3410`), "cleared" is what falls out when the filter keeps nothing. So the disagreement is about what to **accept**, not what to do on refusal. Counted at the source, the family is **three drop** (alacritty `…/term/mod.rs:1717`; ghostty, returning on a decode failure at `src/Surface.zig:2186` and stating the rule beside its test as *"Read requests and malformed base64 must never reach the callback"*, `src/terminal/c/terminal.zig:2961`; and this), **one clears** (xterm.js's addon, deliberately: *"Clear clipboard if text is not a base64 encoded string"*, `ClipboardAddon.ts:55`), **one filters** (xterm). The reference-free ground is the one that decides it: clearing is a *destructive* act, so inferring one from bytes the engine could not parse lets line noise wipe what a user copied by hand — and a filter is worse still, since it hands the consumer text assembled from bytes the application did not send. **Falsifier**: a measured emitter whose malformed payload is *meant* as a clear, which would make the spec's reading intentional rather than emergent |

Add a row when a decision *chooses against* a reference; that is cheaper than
re-defending it, and the cost of the empty slot is measured — see the #490 entry in
the war-story index.

**Layout rows**, added 2026-08-31 by `plat` against the 7 confirmed peers in `### reference`
Class 3. They are what `### place`'s tree rule is defended by, difference by difference — a tree
nobody can defend difference by difference is not a rule but the shape the repo happened to grow.

| We do | The references do | Decided by |
|---|---|---|
| The seam is a **crate wall**, not a directory | xterm.js splits `src/common` from `src/browser`; ghostty keeps `src/terminal` | `plat`, 2026-08-31, on ADR-0017. The boundary has to hold *outside* this repo — penterm consumes `justerm-core` alone — and a directory boundary is not enforced across a publish |
| No `public/` directory per layer | xterm.js gives each of `common` / `browser` / `headless` a `public/` | `plat`, 2026-08-31. `lib.rs`'s `pub use` plus rustdoc already carry the public surface; a directory would be a second copy of it |
| Directory name **equals** package name | wezterm's engine crate is `term/` and publishes as `wezterm-term` | `plat`, 2026-08-31, on ADR-0010. In a `-term` *family* the directory is the discoverable member. One exception is recorded: `justerm-facade/` → `justerm` |
| 78 topic files in `justerm-core/tests/` | alacritty runs one `tests/ref.rs` over recorded fixtures | `plat`, 2026-08-31, on `CLAUDE.md`'s cumulative-conformance rule. One file per VT behaviour is how the long tail gets **named**; a ref harness records that a capture matched and never which behaviour it was |
| **No** co-located `*.test.ts` under `src/` | xterm.js co-locates units as `src/**/X.test.ts` | `plat`, 2026-08-31, **measured**: `tsconfig.json` includes exactly `src`, and that is the gate making `process`/`Buffer` a compile error. Co-locating dissolves it while breaking no test |
| The binding's JS lives **inside the binding crate** | beamterm keeps `js/` at the repository root | `plat`, 2026-08-31, on ADR-0008. The binding is a crate and its npm artifact is built from that crate, so the shims are that crate's surface |
| The renderer has **no** `tests/`; 26 of 28 modules test inline | alacritty mixes inline with `tests/`; wezterm uses `src/test/` | `plat`, 2026-08-31. The 26 pure modules are host-testable; `webgl.rs` and `rasterizer.rs` are wasm32-only and 0-compile on host, so a `tests/` directory could not build and `e2e/` is the proof |
| **Three** script homes | beamterm keeps one root `scripts/` | `plat`, 2026-08-31. Split by owner: CI's, the graph build's, the package's |
| The widget is **one** package, not core plus addons | xterm.js publishes 13 `addons/*` | ADR-0017 already decided it: xterm's addons hold **in-process buffer access**, so that split buys them something a frame-mode consumer cannot have |
| ⚠ **UNCLASSIFIED** — `docs/research/` holds prior art read from real source with `file:line` citations, and so does `docs/agents/reference-facts.md` | — (internal; no reference involved) | **Nobody has decided this.** Left visible rather than resolved by majority. The cost is concrete: `reference`'s route names only `reference-facts.md`, so a survey filed in `docs/research/` is invisible to the exact path that exists to stop an agent starting from a blank tree |

Nothing was **adopted**: no peer difference produced a move whose reason named justerm's boundary.
Exactly one difference is **unclassified**, and it is the last row.

### War-story index (rules with teeth)

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

**Architecture prior art stays in [`theflow.md`](theflow.md) § "Architecture prior art"** — the two
lineages frame-mode composes, and the prior-art *gaps*. It is not a schema slot, it is read only at
`boundary` for an engine-vs-renderer or state-sync question, and absorbing it would buy nothing: the
file is not deletable either way.

---

## Extraction plan

**4 agents** in `.claude/agents/`, **6 scripts** in `scripts/thegraph/` (node ESM, matching
`.github/scripts/*.mjs`; kept separate because those are CI's and these are not). Each artifact
carries **only justerm's data** and defers the method to `thegraph` — thin, so it survives the skill
gaining a paragraph. Each carries the build stamp.

| Artifact | Node | Carries |
|---|---|---|
| `.claude/agents/thegraph-lens.md` | `verify` | corpora paths · the tie-breaker row for the layer · the deliberate-divergence list · the six brief items |
| `.claude/agents/thegraph-refuter.md` | `verify` (2nd) | the same, opposing stance. Exists **only** because sacred paths do |
| `.claude/agents/thegraph-reference.md` | `reference` (fetch only) | the 2 source classes and how each is reached. **Returns the tree path and the raw hit; it is never the source of a `file:line` anyone copies** — the delegation would otherwise buy a wrong citation at full confidence |
| `.claude/agents/thegraph-sweep.md` | `sweep` | surfaces **1–8 and 12**. Surfaces 9–11 (⛔ above) stay on the main thread: they write to the tracker or adjudicate. The line the split falls on is *which* write, not whether there is one — 1–8 and 12 are doc surfaces this node amends in place, which is why its grant declares `Edit` |
| `scripts/thegraph/grants.mjs` | invariant ① over the four agents above | the read-only **default** and the `**Runs:**` declaration that moves it. Asserts the default, **never the claim**: a check keyed on a description saying *"read-only"* is dodged by rephrasing, and was. Run from **two positions** — `preflight` before the run, `gate` after it — and the first is the one that matters, because the damage is to a live worktree. It is not a CI step, so `gates.mjs`'s CI cross-check does not see it |
| `scripts/thegraph/gates.mjs` | `gate` | the command list, each invoked **bare**, taking a **scope argument** (`core` · `web` · `renderer` · `all`) because two are expensive and conditional. **Asserts its list against `.github/workflows/test.yml`** rather than restating it |
| `scripts/thegraph/place.mjs` | `place`'s guard, and `gate` again on the final diff | the tree rule as a **path list**, matched against the changed paths. It **reports**; the choice between the rule and a named peer is `place`'s and stays on the main thread |
| `scripts/thegraph/triggers.mjs` | the `verify` guard | the sacred-path **globs**, matched against the diff. Never a call-site list |
| `scripts/thegraph/search.mjs` | `search` | the tracker query by artifact + the 10-area preemption table. **Query only** — conflict adjudication is not `code` |
| `scripts/thegraph/preflight.mjs` | `## Environment preconditions` | worktree location · `../.refs` pins (delegating to `cite.mjs --pins`) · port 5173's owner · local `wasm-pack` vs `WASM_PACK_VERSION` · the `just-shield` argument · the agents' tool grants (delegating to `grants.mjs`) |

**Already extracted, not regenerated:** `.github/scripts/cite.mjs` is `reference`'s citation tool and
already exists; `readme_pins.rs` and `check-published-readme.mjs` are `sweep` surface 4's mechanized
halves.

**Refusal check.** No adjudicating node is delegated (invariant ①): the four agents are `verify`,
`verify`, `reference`-fetch and `sweep`, all delegable by the catalog; the six scripts are `code`
conditions. No `implement`, `boundary`, `place`, `enumerate`, `proof`, `batch`, `stop`, `decide` or `promote`
artifact exists — `place.mjs` is its **guard**, a `code` condition matching two path lists, not the
node. No path reaches the tracker without passing `batch` (invariant ③) — which is
exactly what restricting the sweeper to surfaces 9–11 buys. Every back-edge declares a guard **and** a
bound (invariant ②).

**And invariant ① is now checked rather than asserted.** Every one of the four grants was measured
against its own brief on 2026-09-02: all four carry `Bash`, the sweeper also `Edit`, and **every
brief names its commands** — `cite.mjs` in all four, plus `git rev-parse` for a pin, `npm view` /
`git tag -l` / `npm pack` for the registry class, and `rg` for the sweeper's widen pattern. So no
grant was narrowed; four `**Runs:**` declarations were added, and `grants.mjs` now fails the build
if one goes missing. Its own discriminating power was measured the same day, against the two dodges
the invariant names: a description rewritten to *"proposes edits rather than making them"* with no
declaration, and a `**Runs:**` naming no tool. **Both reddened, and the baseline came back green in
the same run** — one red is not a mutation test.

---

## Unowned slots — **none; the walk passes**

The coverage check `/grill-the-graph` runs once per build: walk *"What the build must supply"* and
confirm each entry is placed on one side of the invariant/build split. **Walked 2026-08-31 against
`thegraph` at `bf223be`. Every entry is placed.**

**The walk of 2026-09-02 carries that result forward rather than repeating it, and the licence is a
measurement.** Both of the walk's inputs — the schema and the split — were diffed byte-for-byte
between `bf223be` and `18edd61` and are **identical**; the schema only moved file, out of `SKILL.md`
into `BUILD_CONTRACT.md`. A walk is a comparison of those two texts, so an identical pair cannot
produce a different verdict. Recorded as a derivation and not as a second walk, because the two are
worth different amounts: this one is void the moment either input moves, and the stamp is what says
whether it has.

A passing check is deliberately thin here — no per-slot owner column, because which side of the
split owns a slot is `thegraph`'s fact rather than this project's data, and a column of them would be
stale the day it re-places one. What is kept is the **lineage**, because it is the argument for
walking at all:

| Slot | Opened | Closed |
|---|---|---|
| Tracker capability | earlier build | placed upstream; found by the walk of 2026-08-31 |
| War-story index | earlier build | same walk, same commit upstream |
| `proof` method per layer, and this project's tautological-proof traps | earlier build | `kihyun-skills#24` |
| `search`'s areas already carrying a decision record | **found** by the walk of 2026-08-31 | `kihyun-skills#24`, same day |

**Nobody remembered to re-look at any of the four.** The first two were closed upstream while this
build still listed them as pending; the walk found them. The last two were found *by* a walk, filed
upstream by hand, and closed within the day — and the hand is the part worth noting: a
`pending — needs a thegraph change` marker is read by the **next build of this same repository**,
which is structurally the party that cannot act on it, so nothing routes it anywhere. That gap is
`kihyun-skills#20`, and this build is its second measured instance.

**The fix upstream was a generalisation, not a longer list** — the split's build side now reads
*"each node's data — everything it reads and everything it checks against"* rather than enumerating
four list kinds, with an explicit carve-out that a node's **bound** is not its data. Two of the four
rows above exist because an enumeration stood in for a generalisation twice; the third occurrence is
what bought the change.

---

## Method gaps — **six closed upstream at `89b477a`, one open**

### Open — `sweep`'s contract licenses its delegation on a property the node does not have

Found 2026-09-02, building against `18edd61`. `NODES.md` § `### sweep` says both of these, one
paragraph apart:

> **Writes** the surfaces, and `swept`.

> **Delegable, fanning out one instance per surface** — … and **it is read-only**, so invariant ①
> permits it.

Before `58cef7c`, *"read-only"* there was loose prose that could be read as *"does not adjudicate"*.
That commit made read-only the **enforced default** with a checkable tool grant, so the word now
names the thing the check measures — and the same contract asks this node to edit documents and
forbids it the tool for doing so. **The property the delegation is licensed on is one the node
violates by definition.**

Routed as a **catalog** gap, not a build gap, by the discriminator: the defect is in a skill file,
and a re-grill of justerm would face the identical ambiguity, so the cause does not clear. It does
**not** block this build — invariant ① supplies the mechanism, and `thegraph-sweep`'s `Edit` is
licensed by declaration. The suggested fix is a **wording** change, not a rule change: the
delegation is licensed by the node not *adjudicating*, which is what invariant ① actually requires,
and *"read-only"* is the wrong word for it now that the word is load-bearing.

**Not yet filed against `kihyun1998/kihyun-skills`** — recorded here so the next build does not
rediscover it.

### Closed

All six were accepted into `thegraph` on the day this build was compiled, so **none is `pending` and
a run substituting judgement for one is now working from a stale copy of the skill.** Kept rather
than deleted, because *where each one landed* is what a later reader needs — and because the shape
is the reusable part: every one was found by **building the artifacts and running them**, not by
reading the method. Prose does not execute, so it cannot demonstrate that it is lying.

| # | The gap | Where it landed |
|---|---|---|
| 1 | `proof` had no discriminating-power bar — `implement`'s two bars govern the *test*, and nothing governed the artifact a proof is **read from** | `### proof`, as *"name which of its assertions can observe this change; then turn the fix off and confirm **that** one reddens and the others do not"* |
| 2 | The test-trust gate stopped at placement, and had no re-baseline | `### implement`'s self-loop, now **three** bars: the third is *mutate the predicate*, with the re-baseline folded into the first and two corollaries added (assert the window **exists**; suspect a proxy condition) |
| 3 | `search`'s decider was `code` while one of its four outs required judgement — invariant ④ pointing at itself | The catalog row: **`code` to query, AI to adjudicate a conflict**, delegable **query only**, plus the node body saying which out cannot be counted |
| 4 | Nothing said where a promoted record's roster may **not** live, and the anchor-preemption rule covered records only | `### promote` (copy the roster into the record's *context*, never as a list) and `### search` (**only** a decision record preempts an anchor) |
| 5 | `CONFIRMED` reads as though its `file:line` were the evidence | `### verify`'s grade table, as *"a candidate, not the evidence"* — re-opened at the source or produced by a tool, never transcribed, including out of a report you asked for |
| 6 | A sweep judged by hit count; a gate list mirroring CI as a silent third copy | `### sweep` and `### gate` |

**What was deliberately not upstreamed:** the harvest's five-call cap. The *policy* half already
lives at `batch`, and the number is one a repo may set differently without breaking the method — so
by the split rule it is build data. The **ordering** (`CONFIRMED` first, `INERT`/`DELIBERATE` last at
one line each) and *"the lens is read-only, so start it at GREEN — wall clock is `max`, not the
sum"* did go up, because neither is inferable from the node ordering.

**This section is now a `sweep` surface like any other.** If a later `/grill-the-graph` finds a new
gap, it is added here as `pending` with the same two columns; if `thegraph` moves again, the stamp
above is what says so.
