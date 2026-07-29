# Territory — caret report

## What it is

What the consumer is told about the caret so it can draw one: visibility, shape, blink *mode*, and
the position it sits at. **The engine reports; it never draws and never animates.** This is the
consumer-facing half of the cursor, and it is a different concept from
[cursor position](cursor-position.md) — the engine's own idea of where the next glyph goes — even
though one is derived from the other.

## Governing decisions

**None.**

- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  establishes that per-frame scalars ride the header, which is the *mechanism* these fields use, not
  a decision about the caret
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  supplies the principle the blink split follows, without naming it

## Design model

- **Mode, never phase.** `visible` (DEC ?25), `shape` (DECSCUSR, `Block` by default) and `blink`
  (att610 ?12) are *state*. The blink phase — the actual on/off animation — is the consumer's, and
  the engine has no timer.
  **This is not a caret rule** — it is ADR-0017's split applied to time-varying presentation, and it
  has a second instance one attribute over: SGR 5 *text* blink stores a cell flag, the renderer
  conceals on the phase it is handed, and the consumer owns the clock (#282 → #576). Two clocks, not
  one: the caret's restarts on user input, the text's does not, so sharing them would make typing
  reset a blink that no reference ties to input.
- **Hidden while scrolled up.** The frame reports `cursor_visible && display_offset == 0`: a caret
  drawn on a frozen viewport would sit at a position the user is not looking at.
- **Position rides the header**, not the cell content, because the caret moves on almost every frame
  and a consumer cannot derive it from cell damage.
- **The engine has no caret primitive.** How it is drawn — cell inversion, a native overlay quad — is
  entirely the renderer's choice, and the family renderer draws it as an overlay while the wire
  carries only these scalars.
- **DECSCUSR `0` means "the application has not spoken"**, not "steady block" — the difference
  matters because the consumer's own setting is the fallback for exactly that state.

## Code

- `justerm-core/src/cursor.rs` — `CursorShape`, and `Cursor`'s `visible` / `shape` / `blink`
- `justerm-core/src/serialize.rs` — `Frame`'s `cursor_row` / `cursor_col` / `cursor_visible` /
  `cursor_shape` / `cursor_blink`
- `justerm-core/src/term.rs` — `Term::frame` (the `display_offset == 0` gate), `Term::frame_damage`
  (folds the old and current caret cells so it does not ghost)
- `justerm-web/src/cursor.ts` — the consumer half: resolves the blink policy and owns the phase

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row is pinned to a `file:line`
at a recorded SHA; a paraphrase drops the pin).

- [Cursor blink — who decides](../../agents/reference-facts.md#cursor-blink--who-decides-575-verified-2026-07-28)
  — both references resolve blink from the **same two inputs** (the application's mode and the user's
  setting), the side expressing an explicit intent wins, and they differ only in which side carries
  the three-state. justerm follows alacritty's placement because it is what ADR-0017 implies and it
  needs no wire change. Also records that `CSI ?12 h/l` is ignored in xterm.js unless a quirk is
  enabled, because it writes the *user's* option rather than the application channel
- [Text blink — SGR 5](../../agents/reference-facts.md#text-blink--sgr-5-576-verified-2026-07-29)
  — the sibling clock, and a **negative result** worth reading before assuming a default: only one of
  the three references animates blinking text at all, and it ships the interval defaulting to `0`.
  Linked from here because the *split* it demonstrates is this note's rule, not because a text cell
  is a caret

## Cross-cutting invariants

*(none identified yet)*

## Blast radius

- [cursor position](cursor-position.md) — the reported position is that position, so anything that
  moves it moves this
- [damage](damage.md) — a caret move damages **two** cells, old and new, and the "old" is defined by
  the consumer's last ack rather than by wall time
- [frame](frame.md) — five header scalars; adding a sixth is an ADR-0020 question and a wire bump
- [caret drawing](caret-drawing.md) — draws it, and its scalar policies (`setCursorContrast`,
  `setCursorThickness`) consume only what is reported here
- **a11y / web widget** *(no note yet)* — the blink policy resolution lives in `justerm-web`, which
  makes this territory one of the few that genuinely spans the crate boundary

## Known holes / open

- **Zero governing records** for the report's shape, though the *behaviour* it implements is verified
  (§Reference behaviour). Recorded external facts without a decision record is its own state: someone
  checked, nobody decided.
- **The old+new damage fold is a caret rule living in the damage code**, and neither territory's
  documentation owns it.
- **`justerm-web/src/cursor.ts` is the only consumer half mapped anywhere**, and it is named here
  rather than in a note of its own — the renderer and web widget have no territories yet.
