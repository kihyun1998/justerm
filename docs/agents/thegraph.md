# thegraph build (justerm)

What `thegraph` needs and **no file in this repo answers**. Everything else a run needs is already
in the repo and is read there, not copied here: `CLAUDE.md` for the identity and the boundary,
`docs/architecture.md` for the contract, `docs/adr/` for the decisions (ADR-0030 for the tree rule),
`docs/map/` for the wiring, `.github/workflows/test.yml` for the gates, and
[`reference-facts.md`](reference-facts.md) for what has already been read out of the trees below.

## What this project is

A pure terminal engine in Rust — VT bytes in, grid + scrollback + damage out, never drawn — plus the
first-party family around it. `CLAUDE.md` is authoritative; read it rather than this line.

## References

Every source is read **raw**: the tree itself under `../.refs/<source>` at the SHA in column 3,
grepped with `rg`, resolved against the **main checkout** (`../` is `.claude/worktrees/` from a
worktree). There is no summarizing route — `WebFetch` and a whole-file `gh api` drop method bodies,
so a handler that *is* there reads as absent.

**`.github/scripts/cite.mjs --pins` parses this table**: source name in column 1, backticked 40-hex
in column 3. Keep that shape, or every tree silently reports as unpinned.

| Source | Informs | Reached by | Binding |
|---|---|---|---|
| xterm | how it works | `6380a3eaed857c182ea6cfa78cd706966b2628d0` | **binding — but a proxy**, see below |
| alacritty | how it works · where files go | `852e971cddfabe222d2d5bcda466e130f53af207` | example |
| ghostty | how it works · where files go | `e6e26e165ab143f087761cee9f8a479801a27ba7` | example |
| xterm.js | how it works · where files go | `699f5537b0232e444cb98261b8b3991c3cfecb5e` | example |
| three.js | how it works | `83d8667898fd32a6a0f1af92f6d91065db272ce2` | example |
| wezterm-term | where files go | a `gh api repos/<r>/contents/<path>` **tree listing** — raw, unpinned | example |
| tree-sitter · `lib/binding_web` | where files go | the same | example |
| junkdog · beamterm | where files go | the same | example |
| crates.io and npm | how it works — what a consumer actually receives | `npm view`; `npm pack` in a **clean-room worktree** | binding |

**Sparse scopes**, so `../.refs/` can be rebuilt: alacritty → `alacritty_terminal`, `alacritty/src`.
ghostty → `src`. xterm.js → `src`, `addons`, `test`, `typings`. three.js → `src/renderers`,
`examples`. xterm → **whole tree** at tag `xterm-410` (flat repo, 111 files, sparse buys nothing):
`git clone --depth 1 --filter=blob:none --branch xterm-410 https://github.com/ThomasDickey/xterm-snapshots xterm`.

**xterm binds, but it is a proxy and the gap is the point.** ADR-0004 gives *the spec* top authority
over the whole VT layer, above every implementation including ours — and the spec itself is not
transcribed anywhere reachable. This tree is the nearest thing: `ctlseqs.txt` is an **index**, not a
semantics document (DECSC is one line), while the C source is what settles a question and carries the
DEC manual references (`cursor.c` cites the VT420 manual p. 270, VT520 p. 5-120, DEC 070). So what it
reaches is xterm's interpretation and its citations *into* those documents, never a formal definition.
A question it cannot settle is `UNADJUDICATED`, not absent. Four decisions have gone *against* it on
measured reach — #823, #828 (twice) and #826 — each recorded on its own issue with a falsifier; those
are the exceptions that prove the row rather than evidence it is an example.

**Refreshing a pin is a deliberate act.** There is no periodic pull: a pin that moves silently makes
every recorded citation unverifiable at once, and nothing breaks visibly — the line numbers are still
numbers, with different code on them. Refresh only when a slice needs a fact the pinned copy does not
have; update the SHA here in the *same* change and re-verify the `reference-facts.md` rows it moved.
Widening a sparse checkout is not a refresh — it exposes paths already at that SHA.

**The layout peers were confirmed by the maintainer on 2026-08-31**, and a peer set chosen alone would
be authority invented and then deferred to. The three unpinned ones are bounded by that: a tree
listing settles a *layout* question and can never settle a semantics one.
