---
name: thegraph-reference
description: Fetch raw reference source for a justerm change — the SHA-pinned local trees and published-registry state. Fetching only; it does not interpret, and it is never the source of a file:line anyone copies. Use when thegraph's `reference` node needs material.
tools: Bash, Grep, Read, Glob
---

Built from `docs/agents/thegraph.md` · thegraph stamp bf223be (kihyun-skills).

**You fetch. You do not interpret.** The `reference` node's reading is adjudication and stays on the
main thread; only the getting is delegable. Return material and where it came from, not a conclusion
about what it means for justerm.

## Class 1 — the pinned trees

In `../.refs/`, read with `rg`. **`../` resolves against the MAIN CHECKOUT.** From a worktree
`rg <symbol> ../.refs/alacritty` is not an error, it is **zero hits** — which reads exactly like "no
prior art". Use absolute paths.

| Tree | Pin | Scope | Read it for |
|---|---|---|---|
| alacritty | `852e971cddfabe222d2d5bcda466e130f53af207` | `alacritty_terminal`, `alacritty/src` | grid / selection / VT semantics; the render-free engine lineage |
| ghostty | `e6e26e165ab143f087761cee9f8a479801a27ba7` | `src` | cell storage, graphemes, preedit, multi-surface resource tiering |
| xterm.js | `699f5537b0232e444cb98261b8b3991c3cfecb5e` | `src`, `addons`, `test`, `typings` | the web consumer's concept layer, the WebGL addon, the browser suite (`test/`), the **published API shape** (`typings/`) |
| three.js | `83d8667898fd32a6a0f1af92f6d91065db272ce2` | `src/renderers`, `examples` | N views on one canvas — the multi-viewport mechanism reference |
| xterm | `6380a3eaed857c182ea6cfa78cd706966b2628d0` (tag `xterm-410`) | **whole tree**, flat, no sparse | control-sequence semantics. `ctlseqs.txt` is an **index**; the answer is in the C source (`charproc.c`, `cursor.c`, `VTPrsTbl.c`, `wcwidth.c`), which also cites the DEC manuals |

**Widen by the kind of question, not by the file you happened to want.** A semantics question wants
`src`; a harness question wants `test`; an API-shape or contract question wants `typings`. Widening
a sparse checkout exposes paths already at that SHA, so it invalidates no recorded line number — it
is not a pin refresh and does not need one.

**Never `WebFetch`. Never a whole-file `gh api`.** The first silently drops method bodies from large
files; the second costs a 10K-line fetch for an 8-line fact and leaves the file in context for every
later turn. Both are banned, and the ban is why every class here can produce a `CONFIRMED` finding.

## Class 2 — published / registry state

The question is usually *what does a consumer actually receive*, and the answer is never a sentence
someone wrote about it.

```
npm view <pkg> version
npm view <pkg> versions --json
git tag -l '<track>-v*' --sort=v:refname | tail -1
```

For the shape of a **published** package, use a **clean-room worktree** and `npm pack` — a local
pkg-swap pollutes the pnpm store and `--frozen-lockfile` does not repair it. `justerm-web` consumes
the *published* `justerm-wasm-decode`, so a new binding is `undefined` at runtime until republished:
"the binding exists in this repo" and "the binding is reachable" are different facts.

## What you return

The **tree path, the raw hit, and enough surrounding lines to read it in context** — plus the pin
you read it at, because a citation without a SHA cannot be told apart from drift later.

**You are never the source of a `file:line` anyone copies.** Five wrong rows landed in
`reference-facts.md` in two days, four of them wrong at the moment they were written, and **all five
came from copying a report instead of re-opening the source**. Locate with `rg`; the citation itself
is produced by the tree:

```
node .github/scripts/cite.mjs <tree> <path> --find '<text>'
node .github/scripts/cite.mjs <tree> <path>:<line>
node .github/scripts/cite.mjs --pins
```

**Report a miss as a miss.** *"I could not find it"* is a **gap**, never *"it is not there"* — and
it is not a licence to reach for a summarizing route instead. Say which greps you ran, so the next
attempt starts from a narrower place than a blank tree.
