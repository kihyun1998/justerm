# ADR-0010: All-prefixed crate naming (`justerm` → `justerm-core`, `justerm-wasm` → `justerm-wasm-decode`)

Status: accepted (2026-06-26, v0.6.0, #100)

## Context

The `-term` family is **polyglot and multi-artifact**: a Rust engine, one (soon more) WASM binding, and
a forthcoming TS renderer-side package (`justerm-web`). With the engine crate named the bare `justerm`,
the name is ambiguous — does "justerm" mean *the Rust engine* or *the whole family*? Two conventions
resolve it:

- **Flagship-bare** — the flagship crate keeps the bare family name (`justerm`), siblings are prefixed
  (`justerm-wasm`). The flagship is privileged; the family name and the engine name collide.
- **All-prefixed** — every crate carries the family prefix + a role suffix (`justerm-core`,
  `justerm-wasm-decode`, …). The bare `justerm` names the *family*, never a single crate.

Named prior art settles it: the sibling renderer project **beamterm is already all-prefixed**
(`beamterm-core` / `beamterm-renderer` / `beamterm-data`, with a virtual-manifest root). Adopting the
same scheme removes a cross-repo inconsistency in the same `-term` family and makes each justerm crate
self-describing. The decode-vs-engine split is also about to matter: a future in-wasm `feed` binding
(`justerm-wasm-engine`, ADR-0008) would be indistinguishable from the decoder under the bare
`justerm-wasm` name; `-decode` / `-engine` suffixes make the split legible *in the name*.

**Timing — now is cheapest.** At v0.5.0 the only consumer is PenTerm (first-party); external adoption is
effectively nil. The rename is also the *foundation* for `justerm-web`: landing it first means the new
package is born into the correct scheme rather than retrofitted.

## Decision

Adopt **all-prefixed** naming.

- `justerm` → **`justerm-core`** (crates.io); `justerm-wasm` → **`justerm-wasm-decode`** (npm).
- The workspace root becomes a **virtual manifest** (`[workspace]` only); the crates are **flat
  siblings** (`justerm-core/`, `justerm-wasm-decode/`), mirroring beamterm.
- Reserve **`justerm-wasm-engine`** for a future in-wasm `feed` binding (ADR-0008); `-decode` now names
  this crate's scope explicitly.
- **Version → 0.6.0.** The `v0.6.0` tag is the *first* publish of both new names. Continuity over reset:
  the engine is the same 0.5.0-mature code, so it keeps the family version line rather than restarting
  at 0.1.0 (which would misrepresent maturity) or jumping to 1.0.0 (premature — VT compliance is a
  moving long-tail).

### Tombstoning the old names (the irreversible part)

crates.io and npm names are permanent and cannot be deleted. The old names are signposted, not
abandoned:

- **npm** — `npm deprecate justerm-wasm@"*" "Renamed to justerm-wasm-decode"`. npm `deprecate` attaches
  a message that surfaces on install.
- **crates.io** — a **one-shot `justerm` 0.5.1 facade**: `pub use justerm_core::*;` + crate-level
  `#![deprecated = "renamed to justerm-core"]` + a redirect README. **Not** a yank.

**Why npm and crates.io differ — the asymmetry.** npm `deprecate` carries a *message*. crates.io has no
equivalent per-version deprecation message: `yank` historically carries no reason on stable *and* breaks
existing builds, and a published version's page/README can never be edited (`justerm` 0.5.0 cannot be
re-published). So on crates.io the *only* way to (a) make the "renamed" story visible and (b) keep a
stray `justerm = "0.5"` dependant compiling is to publish a **new** 0.5.1 that re-exports the core. The
facade carries on crates.io the same signal `deprecate` carries on npm.

**Shared-cause caveat (why "facade = best practice" is adopted carefully, not blindly).** The Rust
community consensus favours a facade because it protects *many* existing dependants whose migration
takes years. justerm has ~0 external adopters, so that protective value is ~0 here — importing the
consensus wholesale would be a surface analogy, not a shared cause. The facade is adopted anyway because
its *cost* is also ~0 (a one-shot, never-maintained, perfect drop-in — a pure rename means the API is
identical) and, with cost and benefit both ~0, the tiebreaker is **least-surprise to a future reader**:
a `justerm` 0.5.1 that says "renamed to justerm-core" is legible where a frozen 0.5.0 or a silent yank
is not.

The facade lives at **`justerm-facade/`**, **excluded** from the workspace — it must not be dragged by
the version-lockstep that moves `justerm-core` / `justerm-wasm-decode` together; it is frozen at 0.5.1
forever. It is published **manually, after** `justerm-core` 0.6.0 is live on crates.io (it depends on
`justerm-core = "0.6"`), so it cannot be built or gated inside the rename PR.

## Consequences

- **Self-describing, family-aligned names.** Each crate's name states its role; the scheme matches
  beamterm; the decode/engine split is reserved (`justerm-wasm-engine`).
- **Root is a virtual manifest.** Bare `cargo run --example …` no longer has a default package — use
  `cargo run -p justerm-core --example …`. `cargo test --workspace` still gates both members.
- **Two permanent tombstone names + one extra crate dir.** `justerm` (crates.io) and `justerm-wasm`
  (npm) remain owned by us forever; `justerm-facade/` is a permanent one-shot crate in the tree.
- **Old `justerm` 0.5.0 stays installable.** Migration signals are the 0.5.1 facade (crates.io) and the
  npm `deprecate` (justerm-wasm) — not a build-breaking yank.
- **Glossary shift.** `CONTEXT.md` now distinguishes `justerm` (family) from `justerm-core` (engine
  crate); docs and `CLAUDE.md` follow.

## Alternatives considered

- **Flagship-bare (keep `justerm` as the engine).** Rejected — it *is* the family/engine ambiguity this
  ADR exists to remove, and it is misaligned with beamterm's all-prefixed scheme.
- **Reset to 0.1.0 / jump to 1.0.0.** Rejected — 0.1.0 misrepresents the engine's 0.5.0 maturity; 1.0.0
  over-promises API stability while VT compliance is still an accreting long-tail.
- **`cargo yank justerm@0.5.0`.** Rejected — breaks existing builds and carries no message on stable;
  the facade is strictly more humane and more legible.
- **Skip the facade (README-only guidance).** Rejected — crates.io 0.5.0 cannot be re-published, so its
  page README can never be edited; "README guidance" is invisible to a `justerm` user. Only a *new*
  0.5.1 is visible.
- **In-crate `wasm` feature instead of separate crates.** Out of scope here — already rejected in
  ADR-0008 (Axis 1); this ADR only renames the existing crate layout.
