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

- **Mode, never phase.** `visible` (DEC ?25), `shape` (DECSCUSR, **unset** by default) and `blink`
  (att610 ?12) are *state*. The blink phase — the actual on/off animation — is the consumer's, and
  the engine has no timer.
  **This is not a caret rule** — it is ADR-0017's split applied to time-varying presentation, and it
  has a second instance one attribute over: SGR 5 *text* blink stores a cell flag, the renderer
  conceals on the phase it is handed, and the consumer owns the clock (#282 → #576). Two clocks, not
  one: the caret's restarts on user input, the text's does not, so sharing them would make typing
  reset a blink that no reference ties to input.
- **Hidden while scrolled up.** The frame reports `cursor_visible && display_offset == 0`: a caret
  drawn on a frozen viewport would sit at a position the user is not looking at.
- **That bit gates the DRAWING and nothing else — `cursor_row`/`cursor_col` stay true while it is
  false.** They are grid coordinates sampled in the same `Term::frame` body and they do not move with
  `display_offset`, so what the wire carries while the caret is hidden is already the cell the cursor
  occupies once the view returns to the bottom. **A consumer that reads the bit as "this coordinate
  is not usable" discards an answer it was handed**, and silently: nothing errors, and with a
  stationary cursor the value it keeps instead is accidentally right. That is #921, where the IME
  anchor froze for a whole scrolled-up excursion. Pinned on this side by
  `justerm-core/tests/cursor_coordinate_while_hidden.rs`, because the consumer's fix depends on it.
- **Position rides the header**, not the cell content, because the caret moves on almost every frame
  and a consumer cannot derive it from cell damage.
- **The engine has no caret primitive.** How it is drawn — cell inversion, a native overlay quad — is
  entirely the renderer's choice, and the family renderer draws it as an overlay while the wire
  carries only these scalars.
- **DECSCUSR `0` means "the application has not spoken"**, not "steady block" — the difference
  matters because the consumer's own setting is the fallback for exactly that state. Since #927 the
  engine holds the shape as `Option<CursorShape>`: `CSI 0 SP q`, DECSTR and RIS write `None`, and the
  wire carries `None` as its own byte (`0xFF`, v17). **An explicit `2` is a block, not unset** — the
  distinction the whole slice exists for.
- **The consumer's default shape lives in the widget, not in core** (#927, a maintainer call).
  `JustermRendererOptions.cursorStyle` / `setCursorStyle` resolve `appShape ?? style` beside
  `cursorBlink`. It was decided against the other placement — `Term` holding the default and
  resolving it, as alacritty and ghostty do — on ADR-0017 (a default shape is neither VT-parsed nor a
  whole-buffer computation) and on the first consumer's cost (penterm's `cursorBlink` path already
  reaches the renderer; nothing of its settings reaches its Rust `Engine`). **What the decision gave
  up**: a DECRQSS ` q` reply in core cannot report the drawn shape, because core never learns the
  default. No such reply exists today. **The precedence runs opposite to blink's**: blink's consumer
  setting forces over the application (#575), shape's is a fallback under it.
- **Leaving the alternate screen restores the shape from before it — deliberately unlike every
  reference** (#927, a maintainer call on a capture). `saved_cursor` is the whole `Cursor`, so the
  override rides the 1049 save; xterm, alacritty and xterm.js all keep the style outside the saved
  cursor. The reason is measured: nvim under `TERM=xterm-256color` emits both of its `CSI 2 SP q`
  (terminfo `Se=\E[2 q`) **inside** the alternate screen (bytes 202 and 246, between `?1049h` at 112
  and `?1049l` at 287), so restoring on leave is what returns the consumer's default after nvim
  exits. Moving the override off `Cursor` to match the references would pin a block there. A program
  that resets the shape *outside* the alternate screen with `2` still pins a block — the same thing
  an xterm.js pane shows, since `2` is explicit there too.
  **The decision covered 1049 only; the same save reaches two cases nobody ruled on.** `?1048h/l`
  goes through the same `save_alt_cursor` / `restore_alt_cursor`, so it restores the shape as well,
  while DECSC/DECRC (`SavedCursor`) do not carry it — two "save cursor" verbs that disagree, true
  since #89. And DECSTR clears `cursor.shape` but not `saved_cursor`, so a shape set before
  `?1049h` returns at `?1049l` even when a DECSTR ran in between.

## Code

- `justerm-core/src/cursor.rs` — `CursorShape`, and `Cursor`'s `visible` / `shape` / `blink`
- `justerm-web/src/justerm-renderer.ts` — `resolveCursorShape`, `JustermRenderer.setCursorStyle`: the
  consumer's default shape under an unset application shape
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
- [The caret's default shape](../../agents/reference-facts.md#the-carets-default-shape--where-it-lives-and-what-resets-to-it-927-verified-2026-09-17)
  — three of four references keep the default in the terminal layer, xterm's `0` is a blinking block
  rather than the default, and the nvim capture that kept the 1049 restore
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
- [widget lifecycle](widget-lifecycle.md) — the blink policy resolution lives in `justerm-web`, which
  makes this territory one of the few that genuinely spans the crate boundary

## Known holes / open

- **Zero governing records** for the report's shape, though the *behaviour* it implements is verified
  (§Reference behaviour). Recorded external facts without a decision record is its own state: someone
  checked, nobody decided.
- **The old+new damage fold is a caret rule living in the damage code**, and neither territory's
  documentation owns it.
- **`justerm-web/src/cursor.ts` is the only consumer half mapped anywhere**, and it is named here
  rather than in a note of its own — the renderer and web widget have no territories yet.
- **What `0` means is a real split, settled by the #927 direction rather than by the spec proxy.**
  xterm makes `CSI 0 SP q` a blinking block; the three implementations make it "the default". The
  blink half keeps today's value — `0` turns the application's blink mode off — which matches
  neither xterm (blinking) nor xterm.js (back to the user's option).
- **An omitted DECSCUSR parameter resets, like `0`** — measured (#927): `CSI 6 SP q` then `CSI SP q`
  reports `None` and no blink, because `vte` hands the dispatcher an explicit `0`. The dispatcher's
  `unwrap_or(1)` fallback (written after xterm.js's `params.length === 0 ? 1`) is therefore not what
  decides this form, and an earlier line in this note that said it was, unmeasured, was wrong.
