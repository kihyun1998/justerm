# CONTEXT — justerm glossary

The shared vocabulary for justerm. Glossary only — no implementation detail, no spec. When code,
issues, or ADRs name a concept here, use this term and not a synonym.

## justerm (family)

`justerm` names the `-term`-family umbrella, not a single crate: the **Engine** (crate `justerm-core`),
its WASM decoder (`justerm-wasm-decode`), the browser **widget** (`justerm-web`), and the first-party
**Renderer** (`justerm-renderer`). `justerm-facade` is a one-shot tombstone for the pre-rename crate
name. When the core engine crate specifically is meant, say `justerm-core`; "justerm" alone is the
family. The family pivoted from an engine-only library to a **first-party full-stack** terminal
(ADR-0018). (The name was `justerm` for the bare engine until v0.6.0 renamed it for this
disambiguation — ADR-0010.)

## Engine

The core component that consumes a VT byte stream and owns the authoritative terminal state (grid,
scrollback, cursor, selection); its crate is **`justerm-core`**. It produces a viewport snapshot,
damage, and scroll ops. It does no I/O, no IPC, no rendering, and is theme-agnostic. Distinct from the
**Renderer**.

## Renderer

The component that turns the engine's output into pixels (WebGL2). The engine never draws; it hands
state to the renderer. Renderer-side, distinct from the **Engine** — but now a **family member**, not
an external dependency: originally the third-party `beamterm`, the family now builds its own first-party
`justerm-renderer` (ADR-0018). beamterm stays the active renderer until the switch (#273); `justerm-core`
still does not render either way.

## Consumer

An application that embeds the engine — feeds it bytes, transports its output, and pairs it with a
renderer. The first consumer is PenTerm.

## WASM decoder

The canonical web decoder: the engine's wire-format `decode` compiled to WASM and published to npm
(as `justerm-wasm-decode`), so a web **Consumer** shares one decoder with the native backend instead of
re-implementing it. It yields reference cells (with **Color references**) and the format-level
helpers to read them; the theme values that resolve those references — the **Palette** — and the
render policy that feeds the **Renderer** stay the consumer's adapter. Version-locked to the engine.

## Cell

One character position in the grid: a grapheme, foreground/background **color references**, text
attributes, and a width (1 or normal, 2 for wide). The unit the grid is made of.

## Grapheme

A user-perceived character — possibly several Unicode code points (a base plus combining marks). A
cell's content is a grapheme, not a single code point.

## Color reference

How a cell names its colour without committing to a pixel value: Default, an indexed palette slot, or
a direct RGB triple. The engine stores references only; resolving a reference to an actual colour —
against a **Palette** — is the consumer/renderer's job (it owns the theme). Keeps the engine
theme-agnostic.

## Palette

The consumer-supplied table that resolves **Color references** to actual colours — the 16 base ANSI
colours plus default foreground/background, i.e. the theme's values. Indexed slots 16–255 are not
theme values but follow a fixed standard. The engine never holds a palette (it stores references);
the consumer owns it because it owns the theme.

## Grid

The two-dimensional array of cells representing the current screen (rows × columns).

## Scrollback

The lines that have scrolled off the top of the screen, retained for scroll-back. Owned by the
engine (not the consumer), so history survives view remounts and can be searched/copied in full.

## Viewport

The slice of the grid currently shown — normally the bottom of the active screen, or a window into
scrollback when scrolled up. What the engine emits to be displayed.

## Band

A transient over-scan window (the viewport plus a margin of rows above and below) that a renderer may
cache so small scrolls are instant without asking the engine each time. A cache, not ownership — the
engine remains authoritative over scrollback.

## Damage

What changed since the last emitted frame, expressed as line ranges each carrying a changed column
span. The minimal description the engine sends downstream so the whole screen need not be re-sent.

## Scroll op

A first-class "shift rows by N (and these new rows)" instruction, distinct from marking every row
damaged — lets a moderate scroll move existing content rather than redraw everything.

## Selection

A highlighted region the user has chosen, owned by the engine: a type (character, word, line, or
block) and its anchors. The engine computes the highlighted range and extracts its text (across
scrollback). The renderer only draws the highlight.

## Cursor

The current input position and its style/visibility. Part of the emitted state. Its blink is a
renderer-local animation, not an engine concern.

## Alt-screen

The alternate screen buffer a full-screen application switches to (no scrollback while active). An
internal mode of the engine; transparent to consumers — the engine simply emits whichever screen is
current.
