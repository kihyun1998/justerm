# Territory — caret report

## What it is

What the consumer is told about the caret so it can draw one: visibility, shape, blink *mode*, and
the position it sits at. **The engine reports; it never draws and never animates.** This is the
consumer-facing half of the cursor, a different concept from [cursor position](cursor-position.md)
— the engine's own idea of where the next glyph goes — even though one is derived from the other.
The default shape the consumer falls back to is part of this report's contract and lives in
`justerm-web`.

## Governing decisions

**None.**

- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  establishes that per-frame scalars ride the header, the *mechanism* these fields use, not a
  decision about the caret.
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  supplies the principle the blink split and the default-shape placement follow, without naming
  either.

## Design model

Status: no record governs it; the rules below come from the source (read at 7bb31f7) and from
maintainer calls recorded on #927.

- **Mode, never phase.** `visible` (DEC ?25), `shape` (DECSCUSR, **unset** by default) and `blink`
  (att610 ?12) are *state* on `Cursor`. The blink phase is the consumer's; the engine has no timer.
  This is not a caret rule — it is ADR-0017's split applied to time-varying presentation, and it has
  a second instance one attribute over: SGR 5 *text* blink stores a cell flag, the renderer conceals
  on the phase it is handed, and the consumer owns the clock (#282 → #576). Two clocks, not one: the
  caret's restarts on user input, the text's does not.
- **Hidden while scrolled up.** `Term::frame` reports `cursor.visible && display_offset == 0`: a
  caret drawn on a frozen viewport would sit where the user is not looking (#48).
- **That bit gates the DRAWING and nothing else — `cursor_row`/`cursor_col` stay true while it is
  false.** They are grid coordinates sampled in the same `Term::frame` body and do not move with
  `display_offset`, so what the wire carries while hidden is the cell the cursor occupies once the
  view returns to the bottom. A consumer that reads the bit as "this coordinate is not usable"
  silently discards an answer it was handed — #921, where the IME anchor froze for a whole
  scrolled-up excursion. Pinned by `tests/cursor_coordinate_while_hidden.rs`.
- **Position rides the header**, not the cell content: the caret moves on almost every frame and a
  consumer cannot derive it from cell damage.
- **A caret move damages two cells, old and new.** `Term::frame_damage` folds the current cell and
  `prev_cursor` (the cell at the consumer's last `reset_damage`, i.e. its last ack, not wall time)
  into the content damage, only when the cursor moved; `Term::damage` stays content-only (#38,
  mirrors alacritty's `last_cursor`).
- **The engine has no caret primitive.** How it is drawn is the renderer's choice
  ([caret drawing](caret-drawing.md)); the wire carries only these scalars.
- **DECSCUSR `0` means "the application has not spoken"**, not "steady block", because the
  consumer's own setting is the fallback for exactly that state. Since #927 the shape is
  `Option<CursorShape>`: `CSI 0 SP q` (with or without the `0`), DECSTR (`Term::soft_reset`) and RIS
  write `None` and turn the blink mode off — DECSTR's blink half since #927 too, as xterm and xterm.js
  reset it on a soft reset — and the wire carries `None` as its own byte (`0xFF`, v17). **An
  explicit `2` is a block, not unset.**
- **An omitted DECSCUSR parameter resets, like `0`** — measured (#927): `CSI 6 SP q` then `CSI SP q`
  reports `None` and no blink, because `vte` hands the dispatcher an explicit `0`. The dispatcher's
  `unwrap_or(1)` fallback (after xterm.js's `params.length === 0 ? 1`) does not decide this form.
- **The consumer's default shape lives in the widget, not in core** (#927, maintainer call).
  `JustermRendererOptions.cursorStyle` / `JustermRenderer.setCursorStyle` resolve
  `resolveCursorShape(appShape, style)` = `appShape ?? style`, beside `cursorBlink`. Decided against
  `Term` holding the default (alacritty's and ghostty's placement) on ADR-0017 (a default shape is
  neither VT-parsed nor a whole-buffer computation) and on the first consumer's cost: PenTerm wires
  its setting straight to the renderer (penterm 7dadc3c95
  `src/blocks/terminal/lib/nativePaneView.ts` — `renderer.setCursorStyle`). **What it gave up**: a
  DECRQSS ` q` reply in core cannot report the drawn shape, because core never learns the default; no
  such reply exists today. **The precedence runs opposite to blink's**: blink's consumer setting
  forces over the application (#575), shape's is a fallback under it.
- **Leaving the alternate screen restores the shape from before it — deliberately unlike every
  reference** (#927, maintainer call on a capture). `save_alt_cursor` copies the whole `Cursor`, so
  the shape rides the 1049 save; xterm, alacritty and xterm.js keep the style outside the saved
  cursor. The reason is measured: nvim under `TERM=xterm-256color` emits both its `CSI 2 SP q`
  (terminfo `Se=\E[2 q`) **inside** the alternate screen (bytes 202 and 246, between `?1049h` at 112
  and `?1049l` at 287), so restoring on leave is what returns the consumer's default after nvim
  exits. A program that resets the shape *outside* the alternate screen with `2` still pins a block —
  as an xterm.js pane does.
- **The 1049 decision reaches two cases nobody ruled on.** `?1048h/l` goes through the same
  `save_alt_cursor` / `restore_alt_cursor`, so it restores the shape too, while DECSC/DECRC
  (`SavedCursor`, which holds row/col/pen/wrap/origin/charsets and no shape) do not — two "save
  cursor" verbs that disagree. And DECSTR clears `cursor.shape` but not `saved_cursor`, so a shape set
  before `?1049h` returns at `?1049l` even when a DECSTR ran in between.
- **Outside input**: none of unbounded size — the report is five fixed-width scalars.

## Code

- `justerm-core/src/cursor.rs` — `CursorShape`, `Cursor` (`visible`, `shape`, `blink`)
- `justerm-core/src/term.rs` — `Term::frame` (the `display_offset == 0` gate), `Term::frame_damage`, `Term::set_cursor_style`, `Term::soft_reset`, `Term::save_alt_cursor`, `Term::restore_alt_cursor`, `SavedCursor`
- `justerm-core/src/serialize.rs` — `Frame`'s `cursor_row` / `cursor_col` / `cursor_visible` / `cursor_shape` / `cursor_blink`, `CURSOR_SHAPE_UNSET`
- `justerm-web/src/cursor.ts` — `resolveCursorShape`; also resolves the blink policy and owns the phase
- `justerm-web/src/justerm-renderer.ts` — `JustermRenderer.setCursorStyle`, `JustermRendererOptions`
- `justerm-core/tests/cursor_style.rs` · `cursor_coordinate_while_hidden.rs`

## Reference behaviour

- [Cursor blink — who decides](../../agents/reference-facts.md#cursor-blink--who-decides-575-verified-2026-07-28)
  — both references resolve blink from the same two inputs (application mode, user setting), the
  side with an explicit intent wins, and they differ only in which side carries the three-state.
  justerm follows alacritty's placement (ADR-0017, no wire change). Also: `CSI ?12 h/l` is ignored
  in xterm.js unless a quirk is enabled.
- [The caret's default shape](../../agents/reference-facts.md#the-carets-default-shape--where-it-lives-and-what-resets-to-it-927-verified-2026-09-17)
  — three of four references keep the default in the terminal layer; xterm's `0` is a blinking block
  rather than the default; the nvim capture that kept the 1049 restore.
- [Text blink — SGR 5](../../agents/reference-facts.md#text-blink--sgr-5-576-verified-2026-07-29)
  — the sibling clock; only one of three references animates blinking text, with the interval
  defaulting to `0`. Linked for the *split* it demonstrates.

## Cross-cutting invariants

**None.**

## Blast radius

- [cursor position](cursor-position.md) — the reported position *is* that position; anything that
  changes where the cursor sits or how it is clamped changes what is reported.
- [damage](damage.md) — the old+new fold assumes `prev_cursor` is the cell the consumer last drew;
  a change to when the ack resets it leaves a ghost caret.
- [frame](frame.md) — five header scalars; adding a sixth is an ADR-0020 question and a wire bump.
- [caret drawing](caret-drawing.md) — draws it; its policies consume only what is reported here and
  rely on `None` meaning "use the consumer default".
- [input encoding](input-encoding.md) — the IME anchor reads `cursor_row`/`cursor_col` while the
  caret is hidden (#921); a change that stops them being true while hidden breaks it.
- [alt screen](alt-screen.md) — the shape rides the 1049/1048 cursor save; a change to what that
  save holds changes the post-nvim caret.
- [widget lifecycle](widget-lifecycle.md) — the blink policy and the default shape resolve in
  `justerm-web`, so this territory spans the crate boundary.

## Known holes / open

- **Zero governing records** for the report's shape, though its behaviour is verified against
  references. Someone checked, nobody decided.
- **The old+new damage fold is a caret rule living in the damage code**, and only this note says so.
- **What `0` means is a real split, settled by the #927 direction rather than the spec proxy.** xterm
  makes `CSI 0 SP q` a blinking block; the three implementations make it "the default". The blink half
  keeps today's value — `0` turns the application's blink mode off — which matches neither xterm
  (blinking) nor xterm.js (back to the user's option).
- **DECSC/DECRC and `?1048` disagree on whether the shape is saved**, and DECSTR does not clear the
  saved shape — both unruled (above).
