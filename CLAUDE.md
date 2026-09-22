# justerm

A **pure terminal engine** (Rust) that folds a VT byte stream into terminal screen state (grid +
scrollback). It is neither a renderer nor an emulator: it never *draws* the screen, it produces and
exposes screen *state and what changed (damage)*.

`justerm` is a **family umbrella name**, not a crate: `justerm-core` plus the first-party stack around
it (`README.md` gives each crate's role). The first consumer is **PenTerm** (a Tauri terminal app), but
`justerm-core` is a reusable standalone crate, not a penterm-specific one.

## Boundary invariant (this is the identity)

justerm **does**: parse the VT stream with vte → hold cell grid + scrollback + cursor + selection state →
expose a *viewport snapshot + damage (row + column range) + scroll op*. It provides text extraction (copy).

justerm **does not** (and pulls none of these in as a dependency):
- **No I/O** — reads no PTY/SSH/socket. The caller pushes bytes in with `feed()`.
- **No IPC** — no Tauri/channel/transport. It provides the binary *format*; *transport* is the consumer's.
- **No rendering** — core does no GPU/canvas/drawing. The family's `justerm-renderer` draws, as a separate
  crate; the full-stack pivot (ADR-0018) left this line where it was.
- **Theme-agnostic** — colours are stored only as *references* (Default / Indexed(u8) / Rgb). Palette →
  actual colour is resolved by the *consumer/renderer* with a frozen scheme. justerm never learns a hex colour.

→ So it is **testable in isolation**, with no PTY, Tauri or GPU (vttest + unit tests).

**Core or consumer (routing rule, ADR-0017)**: a feature's *mechanism* goes in **core** when it ① is VT
parsing, or ② needs the *whole buffer* to be correct (every cell, scrollback, coordinates, wrap, wide
chars) — a frame-mode consumer holds only the viewport and physically cannot. *Policy* (query, regex,
palette) is injected by the consumer, so core stays policy/theme-agnostic (**mechanism in core, policy in
the consumer**). Everything else (colour resolution, hover, pixel→cell, debounce, scrollbar, clipboard,
transport) is the consumer's. **A defect is fixed in the layer that has it**: when a consumer would have
to cover for another layer, stop and tell the user.

## Where to read

| When | Read |
|---|---|
| Before starting any change — what else moves, which decision the code came from | `docs/map/README.md` (editing the map: its § Conventions) |
| Touching the cell · damage · viewport/scroll · cadence · selection · serialization · engine API contract | `docs/architecture.md` (authoritative) |
| You need the reason behind a decision | `docs/adr/` |
| A term is unclear | `CONTEXT.md` |
| A crate's role, or the epic a slice belongs to | `README.md` |

**ADR lists and statuses live only in `docs/adr/`**: the file name is the one-line summary and each
file's `Status:` line is authoritative. A copy has no gate and goes stale silently.

## Tech stack

**`vte`** (Paul-Williams ANSI parser) does the parsing — only *the genuinely hard parsing* is delegated
to a stable crate; grid/scrollback/selection above it are written here. `alacritty_terminal` is a
reference for model design and **not a dependency** (unstable API; ADR-0001).

## Commands

```bash
cargo test --workspace   # core + the justerm-wasm-decode binding
```

The root is a virtual manifest, so `--workspace` is required — and even then the crates in `Cargo.toml`'s
`exclude` and the wasm32-only tests are ***not built at all***. The authoritative gates are the `run:`
lines of `.github/workflows/test.yml`, which say per job what runs and what does not; this file carries
no gate list or count.

## Core rules

- **Language**: comments, `CLAUDE.md`, `CONTEXT.md`, `docs/adr/` and `docs/map/` are English (LLM token
  efficiency — agents read `docs/map/` at *every* start). Other human-facing docs are Korean.
- **What a comment holds**: a comment says what the code *is*. Why it is this way, what it deliberately
  leaves out, the trap and the measured value go to the territory note under `docs/map/`; history goes to
  the commit message. **And a comment on a *published* item is read by someone who cannot open `#721`** —
  a doc-comment ships verbatim to docs.rs and into the generated `.d.ts`, so name the thing or link the
  number, never leave it bare. Which surfaces those are, and why: `docs/map/territory/published-surface.md`.
- **Commit messages**: reference the GitHub issue (`feat: ... (#12)`), with no `Co-Authored-By`
  trailer.
- **Compliance is cumulative**: VT conformance (an 8.6K-SLoC-class long tail) is not written in one pass —
  start from the common 90% and grow the tail as dogfood breaks cases. *The bones (contract/boundary)
  are right from the start.*
- **A directory boundary is where a seam is physically expressed.** A file in the wrong place breaks the
  seam with no error, failing test or warning — read **ADR-0030** (concrete paths) *before* writing one.
- **Releases publish on a `vX.Y.Z` tag push**, to crates.io + npm automatically; a manual
  `cargo publish`/`npm publish` collides with it. Procedure: `docs/agents/release.md`.
- **`docs/agents/theflow.md` stays.** Its discipline is retired, but it is a *cited* corpus — ADRs,
  `docs/map/` notes and doc-comments link into it (`rg -l theflow.md` is the roster).

## Agent skills

**The work discipline is the `thegraph` skill** — run `/thegraph` when starting a substantive change
(core · wasm · web · renderer). The skill owns its node graph, invariants and method; they are not
copied here.

| When | Read |
|---|---|
| Before reading an outside reference (xterm, alacritty, …) | `docs/agents/thegraph.md` — which sources, how each is reached, which bind — then `docs/agents/reference-facts.md` for what is already confirmed |
| Filing, reading or labelling an issue | `docs/agents/issue-tracker.md` · `docs/agents/triage-labels.md` |
| A skill consumes the domain docs | `docs/agents/domain.md` |
| CI `supply-chain` is red, or a workflow action changes | `docs/agents/supply-chain.md` |
| Consumer (penterm) wiring, the proof method per layer, architecture prior art | `docs/agents/theflow.md` § Crate / module map · § Step 4 · "Architecture prior art" |
| `/teach` | `teach/README.md`, first |
