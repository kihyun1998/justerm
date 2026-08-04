import type { Palette } from "justerm-wasm-decode/colors.js";
import { ContextLossRelay } from "./context-loss";
import { CursorBlink } from "./cursor";
import { FrameLoop } from "./frame-loop";
import type { DecorationRect } from "./decorations";
import { MINIMUM_COLS, MINIMUM_ROWS } from "./fit";

import type { Renderer } from "./renderer";
import { TextBlink } from "./text-blink";
import type { DecodedFrame, FlagBits } from "./types";

/** Theme colours (packed `0xRRGGBB`). The engine stays ignorant of these — the
 * consumer owns them and the renderer resolves cell refs against them. Carried over
 * verbatim from the beamterm adapter (#273): the theme contract is renderer-neutral. */
export interface Theme {
  /** The 16 ANSI colours (slots `0..15`); the decoder's `buildPalette` fills `16..255`. */
  ansi: number[];
  defaultFg: number;
  defaultBg: number;
  /** The cursor colour (block fill / stroke). Defaults to `defaultFg`. */
  cursorColor?: number;
  /** Selection highlight background (`0xRRGGBB`). Defaults to a muted slate. */
  selectionBg?: number;
  /** Search-match highlight background (`0xRRGGBB`). Defaults to a muted amber. */
  matchBg?: number;
  /** The *active* (current) search match's background (`0xRRGGBB`) — xterm's
   * `activeMatchBackground`, painted above selection and the other matches (#429).
   * Defaults to a dark orange, distinct from both {@link selectionBg} and
   * {@link matchBg} (the Chrome find-in-page yellow-others/orange-active model;
   * alacritty's `focused_match` gold agrees on "brighter, warmer than the rest").
   * On a cell that is both selected and the active match, {@link
   * selectionForeground} paints over THIS background (#430, xterm's channel
   * independence) — pick the two to read on each other, or set {@link
   * minimumContrastRatio} (it corrects against the final composited bg). */
  activeMatchBg?: number;
  /** Selection background when the terminal is UNFOCUSED (`0xRRGGBB`). xterm's
   * selectionInactiveBackgroundOpaque; a dimmer tint. Defaults to a muted slate. */
  selectionInactiveBg?: number;
  /** Optional fg for SELECTED cells (`0xRRGGBB`), xterm's `selectionForeground`. Unset
   * keeps each cell's own fg. Selection-only (never a search match), focus-independent (#227).
   * Selection is a property of the cell, not of the bg winner (#430): where the ACTIVE search
   * match covers a selected cell, this fg paints over {@link activeMatchBg} — pick the two to
   * read on each other, or set {@link minimumContrastRatio}. */
  selectionForeground?: number;
  /** Minimum fg/bg contrast ratio (WCAG, 1..21). Defaults to 1 (off, like xterm) (#225).
   *
   * Corrects against the background the cell composites to *within the canvas* — a highlight or
   * decoration bg wins over the cell's own. It does **not** account for
   * {@link JustermRendererOptions.bgAlpha}: the correction runs on the nominal opaque colour, so
   * under a translucent background the real contrast against whatever is behind the canvas may be
   * lower than the ratio asked for. That is not an oversight to fix here — xterm.js discards the
   * background's alpha byte before computing luminance, and ghostty composites first and still uses
   * only `bg.rgb`. None of the three can know what is behind the window. */
  minimumContrastRatio?: number;
  /** Draw bold text in the bright (8-15) ANSI colour — xterm's
   * drawBoldTextInBrightColors (#223). Defaults to true (xterm's default). */
  boldToBright?: boolean;
}

export interface JustermRendererOptions {
  /** CSS selector of the canvas to attach to, e.g. `"#term"`. */
  canvasSelector: string;
  /** Initial font family + size — a CSS `font-family` string and a size in CSS px, applied to the
   * renderer at `create` (#406/#413, wired #417). Change them at runtime with
   * {@link JustermRenderer.setFontSize}/{@link JustermRenderer.setFontFamily}. Loading a webfont
   * (`@font-face`/`FontFace`) before an unfamiliar `fontFamily` is the consumer's job. */
  fontFamily: string;
  fontSize: number;
  /**
   * Force the cursor to blink (`true`) or stay steady (`false`), overriding the application.
   * Omit (or `undefined`) to **follow the application's** DECSCUSR / `CSI ?12` mode, which is the
   * default and what both references default to (#575).
   *
   * Deliberately here rather than on {@link Theme}: `Theme` is colours plus the two colour
   * policies that resolve against them, and a blink is motion, not a colour. xterm.js draws the
   * same line — `cursorBlink` is an option, its theme is colours only. Change it at runtime with
   * {@link JustermRenderer.setCursorBlink}.
   */
  cursorBlink?: boolean;
  /**
   * How long the cursor keeps blinking with no user input before parking solid, in ms (#593).
   * `0` disables the timeout. Omit for the default — 5 minutes, xterm.js's `CURSOR_BLINK_IDLE_TIMEOUT`.
   *
   * Exposed because the two references disagree by 60x (alacritty stops after **5 seconds**), so the
   * number is a product choice rather than a standard. Change it at runtime with
   * {@link JustermRenderer.setCursorBlinkTimeout}.
   */
  cursorBlinkTimeout?: number;
  /**
   * The half-period of the **SGR 5 (blink) text** phase, in ms (#576). Omit (or `0`) to leave
   * blinking text steadily shown, which is the default.
   *
   * Off by default because that is where the references sit: only xterm.js animates blinking text
   * at all, and its `blinkIntervalDuration` defaults to `0` (`OptionsService.ts:16-17`); alacritty
   * has no text blink and ghostty stores the attribute without ever drawing it. There is therefore
   * no inheritable cadence to default to — the number is the consumer's product choice, so this is
   * an interval rather than a boolean. `prefers-reduced-motion` pins the text visible whatever is
   * set here (#119). Change it at runtime with
   * {@link JustermRenderer.setTextBlinkInterval}.
   */
  textBlinkInterval?: number;
  /**
   * Background opacity: `0` fully transparent, `1` opaque (the default). Makes the terminal
   * see-through to whatever is behind the canvas — the page, or a Tauri window's desktop — while
   * glyph pixels stay fully opaque (#577, consumer half of #298).
   *
   * **The canvas must also have something to be see-through *to*.** The renderer only stops writing
   * opaque background pixels; a page that paints an opaque colour behind the canvas will look
   * exactly as it does today. Making the page/window transparent is consumer CSS, not the widget's,
   * and it is the first thing to check when this appears to do nothing.
   *
   * Only cells carrying the **default** background are affected — a cell with an explicit SGR
   * background, and the cursor cell, stay opaque. That is alacritty's rule (`compute_bg_alpha`
   * returns `0.` only for the named background colour), and it is what keeps coloured output
   * readable over an arbitrary desktop.
   *
   * **Widget chrome is not alpha-aware.** The scrollbar thumb is a translucent white
   * (`rgba(255,255,255,0.25)`) over a track with no background of its own, which reads well against
   * an opaque terminal and may not against a light desktop showing through at a low `bgAlpha`. Left
   * as-is deliberately rather than fixed blind: no reference can arbitrate it (xterm.js uses the
   * browser's native scrollbar; alacritty and ghostty draw no DOM chrome at all), and the failure
   * needs a combination — low alpha, a transparent page, a light backdrop — that nobody has actually
   * hit. If you build that combination, restyle the scrollbar from your side.
   *
   * Deliberately here rather than on {@link Theme}, though an alpha is the closest thing on this
   * roster to a colour: it changes no palette entry, only how much of what is behind shows through.
   * Both references put it outside the colour scheme too — xterm.js's `allowTransparency` is an
   * option (`OptionsService.ts:47`), alacritty's `opacity` sits under `window`, not `colors`
   * (`config/window.rs:46`). Change it at runtime with {@link JustermRenderer.setBgAlpha}.
   */
  bgAlpha?: number;
  /**
   * Extra space between columns, in **CSS pixels** (#578). Defaults to `0`.
   *
   * CSS px, not device px, because {@link fontSize} is (ADR-0023) — one font description should not
   * speak two units. Both references disagree and take device px, which is why the same setting is a
   * different gap on a Retina display there and moving a window between monitors re-lays-out the
   * text. The renderer applies `round(letterSpacing * dpr)`.
   *
   * May be **negative**, which narrows the cell and crops the glyph rather than condensing it.
   *
   * **Moves the cell**, so see {@link JustermRenderer.setLetterSpacing} for the re-fit obligation
   * this creates — at `create` time it is free, because the first fit has not run yet.
   */
  letterSpacing?: number;
  /**
   * A multiplier on the glyph height, `>= 1` (#578). Defaults to `1`.
   *
   * Unitless by construction, which is why ADR-0023's CSS-px rule does not apply to it — there is no
   * unit to get wrong.
   *
   * **The renderer clamps rather than rejects**, and the value it adopts may be *smaller* than the
   * one asked for: a cell the glyph atlas cannot hold is shrunk to one it can (#359). It also rolls
   * the change back entirely if the atlas re-bake fails. So this is a request, not a setting — read
   * the result back from the cell size rather than assuming it took.
   */
  lineHeight?: number;
  /**
   * Called when the WebGL context has been lost and has **not** come back within
   * {@link contextRestoreTimeout} (#579, consumer half of #327). Omit to be told nothing.
   *
   * **This is a warning, not a verdict, and recovery does not depend on it.** The renderer rebuilds
   * itself on `webglcontextrestored` with no consumer action at all, and Chromium keeps re-attempting
   * a real restore once a second indefinitely — so a context may well come back *after* this fires.
   * What it exists for is the case that has no other signal: a context that never returns leaves a
   * blank canvas, and nothing else tells a consumer to dim the terminal, show a message, or fall back.
   * xterm.js's consumer, VSCode, tears its WebGL renderer down and swaps in a DOM one; what to do here
   * is likewise consumer policy (ADR-0017), so the widget forwards the signal and applies none itself.
   *
   * **Fires at most once per loss**, and never after {@link JustermRenderer.dispose} — matching
   * xterm.js, whose disposable clears the pending restore timeout
   * (`addons/addon-webgl/src/WebglRenderer.ts:161-163`). Change it at runtime with
   * {@link JustermRenderer.setOnContextLoss}.
   *
   * To ask instead of being told — a consumer that attaches late, or polls — use
   * {@link JustermRenderer.isContextLost} / {@link JustermRenderer.isRestoreOverdue}.
   */
  onContextLoss?: () => void;
  /**
   * How long a lost context is given to come back before {@link onContextLoss} fires, in ms (#579).
   * Omit for the renderer's default — **3000**, xterm.js's `_contextRestorationTimeout` value.
   * Negative values are clamped to `0` by the renderer. Applies to the *next* loss; a deadline
   * already armed keeps the duration it was armed with.
   *
   * Exposed rather than left at the default, and that is not a judgement call: `justerm-renderer`
   * declares this one **consumer policy** in as many words — *"only the consumer knows how long a
   * blank terminal is tolerable against how long its GPU takes to recover"* (`context_loss.rs`,
   * `DEFAULT_RESTORE_TIMEOUT_MS`) — and epic #583 settled that every knob the renderer declares
   * consumer policy under ADR-0017 is reachable through the widget, because the widget *is* that
   * consumer. (#579's own body proposed leaving it unwired "until someone needs a non-default
   * deadline"; that predates the answer and is superseded by it.)
   *
   * Deliberately here rather than on {@link Theme}, with {@link cursorBlinkTimeout} and
   * {@link textBlinkInterval}: a duration is not a colour. Change it at runtime with
   * {@link JustermRenderer.setContextRestoreTimeout}.
   */
  contextRestoreTimeout?: number;
  theme: Theme;
}

/** The wire sentinel for an absent decoration bg/fg override — mirrors the renderer's
 * `NO_REF` (`u32::MAX`). A decoration colour is a 24-bit `0xRRGGBB` (top byte `0`), so this
 * can never collide with a real colour. */
const NO_REF = 0xffffffff >>> 0;

/** `u32`s per decoration rect in the flat wire: `row, left, right, layer, bg, fg`
 * (mirrors the renderer's `DECORATION_STRIDE`). */
const DECORATION_STRIDE = 6;

/** The empty cell columns a phase-only re-issue passes (#576) — shared, since they are never
 * written and allocating them twice a second would be pure garbage. */
const EMPTY_U32 = new Uint32Array(0);
const EMPTY_U16 = new Uint16Array(0);

/** The subset of `justerm-renderer`'s `JustermRenderer` this adapter drives. Declared as an
 * interface (not the imported wasm type) so the wiring is unit-testable behind a fake with no
 * GL context — the injected-seam pattern the beamterm adapter used via the `Renderer` port. The
 * real wasm instance is assigned to this in {@link JustermRenderer.create}, so a signature drift
 * is a compile error there. Method names match wasm-bindgen's output (snake_case where there is
 * no `js_name`, camelCase where there is).
 *
 * That declaration gates this mirror against the published *renderer* only. The other published
 * package this widget consumes is gated separately, in `test/published-seam.types.ts` (#646),
 * which asserts that the decoder's columns can feed these parameters — the pairing #627 broke. */
export interface RendererBackend {
  /** Scatter a decoded frame's damage into the persistent grid, then re-pack. Header is
   * `[cols, rows, kind, hasScroll, scrollTop, scrollBottom, scrollCount, blinkOn]` (#285). */
  apply_damage(
    header: Uint32Array,
    spans: Uint32Array,
    codepoints: Uint32Array,
    fg: Uint32Array,
    bg: Uint32Array,
    flags: Uint16Array,
    /** Per-cell 1-based grapheme-cluster index — **u32, not u16** (#621/#627). A `u16` cannot
     * number one cluster per cell of a viewport the frame header's own `cols`/`rows` permit, so
     * the column widened at the decoder; narrowing it back here truncated silently above
     * `u16::MAX` (65536 → `0` = "no cluster", 65537 → the *wrong* cluster) and cost an
     * unconditional per-frame copy. `flags` above stays u16 — only this column moved. */
    extra: Uint32Array,
    sideTable: string[],
    /** Per-cell underline colour column (SGR 58, #520) — trailing/optional, so an older
     * renderer build still satisfies this seam. Tagged u32 like fg/bg (`0` = Default). */
    underlineColors?: Uint32Array,
  ): void;
  /** Retain the selection/match spans + blend colours; re-pack the grid (#271). */
  setOverlay(
    selectionSpans: Uint32Array,
    matchSpans: Uint32Array,
    selectionBg: number,
    matchBg: number,
  ): void;
  /** Retain the ACTIVE search match's spans + colour (#427) — additive beside
   * `setOverlay`, ranked above selection; empty spans clear it. */
  setActiveMatch(activeSpans: Uint32Array, activeMatchBg: number): void;
  /** The in-progress IME composition, or an empty run to clear it (#249). Returns the caret /
   * anchor column — one past the run, after any right-edge shift.
   *
   * **Optional because the published package decides, not this file.** `justerm-web` consumes
   * `justerm-renderer` from npm, so a binding added in the repo is absent at runtime — and from the
   * `.d.ts` — until a `renderer-v*` tag publishes it. Required here would make the widget
   * un-typecheckable against every renderer that predates it, and calling it unguarded would be a
   * `TypeError` rather than a missing feature. A renderer without it is preedit-blind, which is the
   * state every consumer was in before #249. */
  setPreedit?(col: number, row: number, codepoints: Uint32Array): number;
  /** Retain the flat decoration directory `[row, left, right, layer, bg, fg]…` (#393). */
  setDecorations(spans: Uint32Array): void;
  /** Place the cursor: shape `0` block / `1` underline / `2` bar / `3` hollow (#270). */
  setCursor(col: number, row: number, shape: number, color: number, textColor: number): void;
  /** Remove the cursor — hidden (DECTCEM) or the blink's off phase. */
  clearCursor(): void;
  setBoldToBright(enabled: boolean): void;
  setMinimumContrastRatio(ratio: number): void;
  setSelectionForeground(color: number | undefined): void;
  /** Background cell opacity, `0`..`1` (#298). The renderer clamps; it is read at *draw* time
   * (the clear colour and a shader uniform), not at pack time, so unlike `setBoldToBright` this
   * needs no re-pack — a bare `render` presents it. */
  setBgAlpha(alpha: number): void;
  /** Re-bake the atlas at a new font size (CSS px) / family (#406/#413). The cell size moves, so the
   * consumer must re-fit. A no-op if unchanged; a non-finite / `<1` size is guarded by the renderer. */
  setFontSize(cssPx: number): void;
  setFontFamily(family: string): void;
  /** Extra space between columns in **CSS px** (ADR-0023 — the space `fontSize` already speaks), and
   * a multiplier on the glyph height (`>= 1`). Both move the cell, so the consumer must re-fit; both
   * are clamped or rolled back by the renderer, so the result is read back rather than assumed (#338,
   * #359). */
  setLetterSpacing(cssPx: number): void;
  setLineHeight(multiplier: number): void;
  /** Swap the palette + default fg/bg for a live theme change (#405): re-resolve every retained
   * cell against the new scheme. `paletteColors` is the 256 pre-built indexed colours. */
  setPalette(paletteColors: Uint32Array, defaultFg: number, defaultBg: number): void;
  /** Size the drawing buffer to a `cols`×`rows` grid (device px = grid × cell). */
  resize(cols: number, rows: number): void;
  /** The columns/rows the last [`resize`] actually adopted — may be fewer than requested if the
   * browser clamped the drawing buffer (#339), so the consumer must read these back, not assume. */
  cols(): number;
  rows(): number;
  /** The cell width/height in **device** pixels. */
  cell_width(): number;
  cell_height(): number;
  /** The cell width/height in **CSS** pixels, unrounded (#331/#335). */
  cssCellWidth(): number;
  cssCellHeight(): number;
  /** The drawing buffer's size in **CSS** pixels — what the canvas display box must be set to. */
  cssWidth(): number;
  cssHeight(): number;
  /** Whether a WebGL context loss has been **reported** to the renderer (#269). Deliberately the
   * event-driven view rather than `gl.isContextLost()`: a browser destroys a context synchronously
   * and only *queues* `webglcontextlost`, so this answers `false` for a window in which every GL
   * call is already dead. Read it as *"was I told"*, never as *"is the GPU usable"* — the renderer
   * branches on its own internal predicate, which this is not (ADR-0027 D4). */
  isContextLost(): boolean;
  /** Whether a lost context has missed its restore deadline (#327) — the poll counterpart of
   * `setOnContextLoss`, for a consumer that attaches late. Cleared by a late
   * `webglcontextrestored`, which also heals the renderer. */
  isRestoreOverdue(): boolean;
  /** Register the single function the renderer calls when a lost context has not come back within
   * the deadline. There is **no unset** — the parameter is a `Function` — which is why the adapter
   * registers an indirection once rather than the consumer's handler directly (`context-loss.ts`;
   * named in prose rather than linked, because that type is internal to this package). */
  setOnContextLoss(callback: () => void): void;
  /** The grace period, in ms, before the callback above fires. Consumer policy (ADR-0017): the
   * renderer times, the consumer decides how long a blank terminal is tolerable. Applies to the
   * *next* loss; a deadline already armed keeps the duration it was armed with. */
  setContextRestoreTimeoutMs(ms: number): void;
  render(): void;
}

/** Assemble the flat `apply_damage` header from a decoded frame. Pure (no backend), so the
 * wire assembly — scroll presence, the negative `scrollCount` that rides a `u32` slot as
 * two's complement, the blink flag — is unit-testable. `blinkOn` gates SGR-blink cells (#282):
 * the adapter passes {@link TextBlink}'s current phase (#576), and the `true` default keeps a
 * caller that has no phase — a test, a hand-built fixture — showing blinking text rather than
 * hiding it. */
export function damageHeader(frame: DecodedFrame, blinkOn = true): Uint32Array {
  const hasScroll =
    frame.scrollTop !== undefined &&
    frame.scrollBottom !== undefined &&
    frame.scrollCount !== undefined &&
    frame.scrollCount !== 0;
  const h = new Uint32Array(8);
  h[0] = frame.cols;
  h[1] = frame.rows;
  h[2] = frame.kind;
  h[3] = hasScroll ? 1 : 0;
  h[4] = frame.scrollTop ?? 0;
  h[5] = frame.scrollBottom ?? 0;
  h[6] = frame.scrollCount ?? 0; // a negative shift wraps to u32; the renderer reads it `as i32 as i16`.
  h[7] = blinkOn ? 1 : 0;
  return h;
}

/**
 * The header for a **phase-only** re-issue: an empty damage that carries nothing but the new SGR-5
 * blink phase (#576).
 *
 * The renderer takes `blink_on` in the damage header and keeps it (`webgl.rs` `last_blink_on`), so
 * this is how a consumer flips the phase between frames — scatter no cells, re-pack the retained
 * grid at the new phase. No renderer or wire change is needed for text blink, which is why the
 * whole feature lands in the widget.
 *
 * `kind` is **Partial**, and that is the load-bearing value: a Full header wipes the grid *before*
 * scattering, and this damage scatters nothing, so a Full flip would blank the terminal instead of
 * re-drawing it. `cols`/`rows` must be the grid the renderer currently holds — a mismatch makes it
 * allocate a fresh (empty) grid, with the same result.
 */
export function blinkPhaseHeader(cols: number, rows: number, blinkOn: boolean): Uint32Array {
  const h = new Uint32Array(8);
  h[0] = cols;
  h[1] = rows;
  h[2] = 1; // Partial — never Full; see above
  h[7] = blinkOn ? 1 : 0;
  return h;
}

/** Whether any cell in a frame's flag column carries `blinkBit` (#576). Pure, so the gate on the
 * phase re-pack is unit-testable without a backend — and separate from the frame it came from, so
 * a `number[]` fixture and the decoder's `Uint16Array` are the same code path. */
export function carriesBlink(flags: ArrayLike<number>, blinkBit: number): boolean {
  for (let i = 0; i < flags.length; i++) {
    if (((flags[i] ?? 0) & blinkBit) !== 0) return true;
  }
  return false;
}

/** Flatten projected decoration rects into the renderer's stride-6 wire
 * `[row, left, right, layer(0=bottom/1=top), bg, fg]…`. `bg`/`fg` are absolute `0xRRGGBB`
 * used verbatim (the consumer already resolved its theme — #393); an absent override becomes
 * {@link NO_REF}. Pure, so the layer mapping + the `undefined → NO_REF` encoding are testable. */
export function decorationWire(rects: readonly DecorationRect[]): Uint32Array {
  const out = new Uint32Array(rects.length * DECORATION_STRIDE);
  rects.forEach((r, i) => {
    const o = i * DECORATION_STRIDE;
    out[o] = r.row;
    out[o + 1] = r.left;
    out[o + 2] = r.right;
    out[o + 3] = r.layer === "top" ? 1 : 0;
    out[o + 4] = r.bg ?? NO_REF;
    out[o + 5] = r.fg ?? NO_REF;
  });
  return out;
}

/** The `cols`×`rows` grid that fits a CSS-pixel box, given the cell's CSS size. Pixel→cell is
 * consumer policy (ADR-0017) and the renderer takes a *grid* (#331), so the adapter owns this
 * division — the same `floor(box / cell)` xterm's FitAddon does. Pure, so the fractional-DPR
 * rounding is testable.
 *
 * Floored at {@link MINIMUM_COLS}×{@link MINIMUM_ROWS}, not at one cell (#547). "A grid must have
 * a cell" was the old reason and it under-shot: the engine clamps `resize(1, r)` up to two
 * columns, so a 1-column proposal is a grid it can never be in. Driving the engine at 1 while it
 * holds 2 puts every span of the frame outside this grid and the surface silently stops updating.
 * The clamp is pull-only on the core side — a consumer reads the width back, it is not told — so
 * agreeing with the floor here is what keeps the two in step.
 *
 * `undefined` when there is nothing to propose, matching what `proposeDimensions` refuses (#632): an
 * **unmeasured cell** (either axis `0`) and a **non-finite box** (`NaN` from a detached or unlaid-out
 * element, `Infinity` from a degenerate one). Refusing is deliberate and is not the same as clamping —
 * a zero-sized *box* still yields the floors, because a container that measured as empty is a real
 * answer, while a non-finite one means *"not measured"*, exactly when the terminal must not be shrunk.
 * The floor agreement above and this refusal are two axes of the same invariant, and this axis was
 * missing: `Math.max(2, Math.floor(NaN / 8))` is `NaN`, so `backend.resize(NaN)` coerced to `0` and the
 * terminal came back 1×1 — through the path that actually reaches the renderer, while the guarded path
 * was the one nothing calls.
 *
 * **One check covers both conditions, and that is measured rather than assumed.** A separate
 * `cellCss* === 0` guard was written first, mirroring the sibling's, and a mutation test showed it
 * could not fail: a zero cell makes the quotient `±Infinity` (or `NaN` for a zero box over a zero
 * cell), so `Number.isFinite` already rejects every one of those inputs. It was removed rather than
 * kept for symmetry — a branch that cannot change an outcome is untestable by construction, and the
 * test below asserts the zero-cell *behaviour*, which is the part that must hold. (The sibling in
 * `fit.ts` carries the same redundancy, inherited from xterm's `cell.width === 0` guard; left alone
 * because it is working code and changing it would alter nothing.) */
export function gridForBox(
  cssWidth: number,
  cssHeight: number,
  cellCssWidth: number,
  cellCssHeight: number,
): { cols: number; rows: number } | undefined {
  const cols = Math.max(MINIMUM_COLS, Math.floor(cssWidth / cellCssWidth));
  const rows = Math.max(MINIMUM_ROWS, Math.floor(cssHeight / cellCssHeight));
  if (!Number.isFinite(cols) || !Number.isFinite(rows)) return undefined;
  return { cols, rows };
}

/** What a frame says to do with the cursor, as a pure decision (no blink/state): `none` = the
 * frame carries no cursor info (leave it); `clear` = hidden (DECTCEM); `set` = place it. Extracted
 * so the visible/hidden branch + the field defaults — the spot an off-by-one or wrong default would
 * hide — are unit-testable without the blink loop. Shape `0` block / `1` underline / `2` bar. */
export type CursorCommand =
  | { kind: "none" }
  | { kind: "clear" }
  | { kind: "set"; col: number; row: number; shape: number };

export function cursorCommand(frame: DecodedFrame): CursorCommand {
  if (frame.cursorRow === undefined && frame.cursorVisible === undefined) return { kind: "none" };
  if (!(frame.cursorVisible ?? false)) return { kind: "clear" };
  return {
    kind: "set",
    col: frame.cursorCol ?? 0,
    row: frame.cursorRow ?? 0,
    shape: frame.cursorShape ?? 0,
  };
}

/** Coerce a decoder array to the exact typed array wasm-bindgen's `&[u32]`/`&[u16]` expect.
 * The decoder's getters already return the right typed array (fast path: identity — a real
 * `Uint32Array` passes through by reference, not copied); the fallback covers a plain-array
 * frame (test/demo fixtures, e.g. `demo/fake-search.ts`).
 *
 * The fallback `Uint32Array.from` REINTERPRETS an out-of-range value, it does not reject it:
 * a negative wraps to its two's-complement, `NaN`/±`Infinity` land as `0`, and `>= 2**32` wraps
 * mod 2**32 (#467, pinned in the renderer test — the same class as the #457 decoration wire). A
 * span source feeding this (`selectionSpans` / `matchSpans` / `activeMatchSpans`) MUST clip to
 * valid u32 range itself, as `decorationsForFrame` and the demo's span producers do; this
 * coercion knows nothing of a value's meaning or geometry and so cannot validate — the producer
 * owns validity. Deliberately not rejected here (#467): a per-frame coercion is the wrong layer.
 *
 * Exported for the seam test only; not re-exported from the package `index.ts`. */
export const asU32 = (a: ArrayLike<number>): Uint32Array =>
  a instanceof Uint32Array ? a : Uint32Array.from(a);
/** The u16 sibling of {@link asU32} (feeds `flags` — and no longer `extra`, which widened to u32
 * at #621/#627), with the same contract: the
 * fallback `Uint16Array.from` REINTERPRETS an out-of-range value (a negative or `>= 2**16` wraps
 * mod 2**16, `NaN`/±`Infinity` → `0`), it does not reject — the producer must clip, this cannot
 * validate (#467). Exported for the seam test only; not re-exported from `index.ts`. */
export const asU16 = (a: ArrayLike<number>): Uint16Array =>
  a instanceof Uint16Array ? a : Uint16Array.from(a);
/**
 * Like {@link asU32}, but for a column this object **keeps past the current frame** — always a copy,
 * never the argument.
 *
 * The opposite rule from {@link asU32}, and not a contradiction: a column that is forwarded and
 * forgotten wants the zero-copy view (#627), while a column that is *retained* cannot have one. A
 * decoded frame's columns view WASM memory directly and are invalidated when that memory grows —
 * the decoder states it as a contract — so a retained view survives exactly until the next decode
 * large enough to reallocate. Measured (#657): a held view detaches after **one** decode of a
 * 300x220 frame, or 109 small ones held at once, and passing the detached array to any wasm entry
 * point throws `TypeError: … on a detached or out-of-bounds ArrayBuffer` rather than degrading.
 *
 * That throw would land in {@link JustermRenderer.issueOverlay}, which by design runs on a **focus
 * flip with no new frame** — so the visible failure is that clicking away from a terminal with a
 * live selection raises, after the viewport has grown at some earlier point.
 *
 * Cheap: overlay spans are `(row, left, right)` triples for the highlighted rows only, copied once
 * per frame — not a cell column.
 */
export const retainU32 = (a: ArrayLike<number>): Uint32Array => Uint32Array.from(a);

/** Monotonic clock for the blink phase (ms). */
const now = (): number => performance.now();

/**
 * The real {@link Renderer}: wraps the first-party `justerm-renderer` (WASM + WebGL2) and pushes
 * each decoded frame's cells + overlay + cursor + decorations to it, letting the renderer do all
 * compositing **in wasm** (colour resolve, highlight blend, cursor, decorations). This is the
 * pivot's payoff (ADR-0018): the beamterm adapter did that compositing in TypeScript
 * (CellMirror + makeRenderPolicy + composeOverlayDraws) because beamterm has no such concepts;
 * this adapter is a thin translator because the renderer owns them.
 *
 * The overlay/cursor/decoration state is **consumer-pushed every frame** (the renderer retains
 * it as state, exactly like `setCursor` — #273 wiring note): the adapter sets that state *before*
 * `apply_damage`, so the frame packs once with the current overlay, avoiding a redundant re-pack.
 *
 * Both wasm modules are loaded with **dynamic `import()`** in {@link create}: two top-level
 * wasm-bindgen "bundler" imports race their init and the second fails (`__wbindgen_externrefs`
 * undefined), so deferring to runtime lets vite instantiate each cleanly (same reason as the
 * beamterm adapter). Not exercised by the vitest suite — it needs a GL context + the WASM; the
 * pure wire logic ({@link damageHeader}/{@link decorationWire}/{@link gridForBox}) is unit-tested,
 * and the whole path is proven by the demo's headless e2e (a real WebGL boot) + the renderer's
 * own GL proofs.
 */
export class JustermRenderer implements Renderer {
  private readonly blink = new CursorBlink();
  /** The SGR 5 text phase (#576) — a separate clock from the caret's, never restarted by input. */
  private readonly textBlink = new TextBlink();
  /** Last cursor reported by a frame (screen coords), or `undefined` if hidden. */
  private cursor: { col: number; row: number; shape: number } | undefined;
  /** Where the caret sits while a composition is open (ADR-0028 D5), or undefined when none is —
   * retained because every frame re-asserts the engine's cursor, which cannot know about a preedit. */
  private preeditCaret: { col: number; row: number } | undefined;
  private lastBlinkOn = true;
  /** The text-blink phase the renderer was last handed (its `last_blink_on`), so the loop only
   * re-issues on an actual flip. */
  private lastTextBlinkOn = true;
  /** The grid the last applied frame described. A phase flip re-issues a header carrying these,
   * so it must not run while they disagree with the grid the renderer holds — see
   * {@link JustermRenderer.repackAtTextBlinkPhase}. `undefined` until the first frame. */
  private lastFrameGrid: { cols: number; rows: number } | undefined;
  /** Whether any cell the renderer holds may carry `BLINK` — the gate on the phase re-pack (#576).
   * See {@link JustermRenderer.trackBlinkCells} for why it over-approximates and why that is sound. */
  private mayHaveBlinkCells = false;
  /** The blink loop. Owns its own scheduling handle so a throw from the body cannot latch it
   * off — see `frame-loop.ts` (#696). Built lazily because `requestAnimationFrame` is read at
   * construction time and the loop's body closes over `this`. */
  private readonly blinkLoop = new FrameLoop(
    (cb) => requestAnimationFrame(cb),
    (id) => cancelAnimationFrame(id),
    () => this.blinkTick(),
  );
  /** Held so {@link JustermRenderer.dispose} can detach: since #576 this listener *draws* (it
   * re-packs and presents), so leaving it attached lets a disposed widget repaint its canvas. */
  private readonly motionQuery: MediaQueryList;
  private readonly onMotionChange: (e: MediaQueryListEvent) => void;
  /** Focus gates the selection colour (focused → `selectionBg`, blurred → the dimmer
   * `selectionInactiveBg`) and the blink (blurred → solid). xterm's two selection colours (#115). */
  private focused = true;
  /** The current frame's overlay spans, retained so a focus flip (no new frame) can re-issue
   * `setOverlay` with the active/inactive tint. Empty ⇔ nothing highlighted. */
  // Annotated bare (`Uint32Array<ArrayBufferLike>`) so `asU32`'s buffer-agnostic result assigns
  // without the TS5.7 TypedArray-generic friction a `new Uint32Array(0)` initializer would infer.
  private lastSelectionSpans: Uint32Array = new Uint32Array(0);
  private lastMatchSpans: Uint32Array = new Uint32Array(0);
  private lastActiveMatchSpans: Uint32Array = new Uint32Array(0);
  /** Per-frame decoration rects (#120): consumer-side, injected via {@link setDecorationSource}. */
  private decorationSource: ((frame: DecodedFrame) => DecorationRect[]) | undefined;
  /** The consumer's never-restored-context handler, behind an indirection the renderer holds for
   * the life of this object (#579). See {@link ContextLossRelay} for why it is not registered
   * directly — the renderer's setter has no unset, and its own teardown runs at `free()`, which
   * {@link dispose} does not reach. */
  private readonly contextLoss = new ContextLossRelay();

  private constructor(
    private readonly backend: RendererBackend,
    private readonly canvas: HTMLCanvasElement,
    // Retained so `setTheme` (#420) can rebuild the 256-colour table from a new ANSI scheme.
    private readonly buildPalette: (ansi: Uint32Array) => Uint32Array,
    // Theme-derived state is mutable: `setTheme` swaps the whole scheme at runtime (#420).
    private palette: Palette,
    private readonly flagBits: FlagBits,
    private cursorColor: number,
    private cursorTextColor: number,
    private selectionBg: number,
    private matchBg: number,
    private activeMatchBg: number,
    private selectionInactiveBg: number,
  ) {
    // Honour prefers-reduced-motion (#119): suppress the cursor blink AND the SGR-5 text blink
    // (#576), tracking changes live. Text needs two things the cursor does not: a re-sync (a change
    // landing on the off phase would otherwise leave that text invisible until the next frame) and
    // a loop start — releasing reduced motion is the one path that turns blinking on without going
    // through `setTextBlinkInterval`, and with a hidden cursor nothing else would ever start it.
    this.motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    this.blink.setReducedMotion(this.motionQuery.matches);
    this.textBlink.setReducedMotion(this.motionQuery.matches, now());
    this.onMotionChange = (e: MediaQueryListEvent): void => {
      this.blink.setReducedMotion(e.matches);
      this.textBlink.setReducedMotion(e.matches, now());
      this.syncTextBlinkPhase();
      if (this.textBlink.enabled) this.startBlinkLoop();
    };
    this.motionQuery.addEventListener("change", this.onMotionChange);
  }

  static async create(opts: JustermRendererOptions): Promise<JustermRenderer> {
    // Dynamic import both wasm-bindgen bundler modules (see class doc for the init-race reason).
    const [renderer, decoder] = await Promise.all([
      import("justerm-renderer"),
      import("justerm-wasm-decode"),
    ]);
    const t = opts.theme;
    const paletteColors = decoder.buildPalette(Uint32Array.from(t.ansi));
    // Typed assignment (not a cast): the real class is a structural superset of RendererBackend, so
    // this compiles today AND turns a future signature drift in the published renderer into a compile
    // error here — the drift gate the injected seam exists for.
    const backend: RendererBackend = new renderer.JustermRenderer(
      opts.canvasSelector,
      paletteColors,
      t.defaultFg,
      t.defaultBg,
    );
    // Policy setters (consumer-injected, ADR-0017) — set once; they rarely change.
    backend.setBoldToBright(t.boldToBright ?? true);
    backend.setMinimumContrastRatio(t.minimumContrastRatio ?? 1);
    backend.setSelectionForeground(t.selectionForeground);
    // Background opacity (#577). Set unconditionally at the renderer's own default, so the value the
    // renderer holds is the one this object states rather than one nobody wrote down. No `render`
    // here — nothing has been drawn yet, and the first frame presents it.
    backend.setBgAlpha(opts.bgAlpha ?? 1);
    // Font family + size (#406/#413, wired #417). Applied before the first fit, so the initial grid
    // is computed at the consumer's cell. Each is a no-op at the renderer's default (monospace/16).
    backend.setFontFamily(opts.fontFamily);
    backend.setFontSize(opts.fontSize);
    // Spacing (#578) — before the first fit, so the initial grid is computed at the consumer's final
    // cell. Each is a no-op at the renderer's default (0 / 1.0), and unlike the runtime setters below
    // there is nothing to re-fit here: `create` returns before the consumer has fitted once.
    //
    // **Order relative to the font calls above does not matter**, and this comment used to claim it
    // did ("both derive the cell from the glyph metrics those establish") — a dependency the renderer
    // deliberately removed. Every path that changes the glyph box, the DPR or either spacing value
    // funnels through `recompute_cell`, which reads all four together, and the font path explicitly
    // re-derives the cell from the *surviving* spacing policy. So font-then-spacing and
    // spacing-then-font land on the same cell. Stated because a comment asserting a constraint that
    // does not exist is one someone later "fixes" by reordering, and then has to re-derive why it was
    // safe.
    backend.setLetterSpacing(opts.letterSpacing ?? 0);
    backend.setLineHeight(opts.lineHeight ?? 1);

    const palette: Palette = {
      colors: paletteColors,
      defaultFg: t.defaultFg,
      defaultBg: t.defaultBg,
    };
    const f = decoder.flags();
    const flagBits: FlagBits = {
      bold: f.bold,
      italic: f.italic,
      underline: f.underline,
      strikethrough: f.strikethrough,
      wide_char_spacer: f.wide_char_spacer,
      inverse: f.inverse,
      dim: f.dim,
      hidden: f.hidden,
      blink: f.blink,
    };
    const canvas = document.querySelector<HTMLCanvasElement>(opts.canvasSelector);
    if (!canvas) throw new Error(`justerm-renderer: canvas ${opts.canvasSelector} not found`);
    const instance = new JustermRenderer(
      backend,
      canvas,
      (ansi) => decoder.buildPalette(ansi),
      palette,
      flagBits,
      t.cursorColor ?? t.defaultFg,
      t.defaultBg,
      t.selectionBg ?? 0x45475a,
      t.matchBg ?? 0x6e5c00,
      t.activeMatchBg ?? 0x995200,
      t.selectionInactiveBg ?? 0x30313d,
    );
    // The context-loss relay is registered UNCONDITIONALLY (#579), like `setBgAlpha` above and for
    // the same reason: the function the renderer holds is then the one this object states, rather
    // than one nobody wrote down. It also has to be — the renderer's `setOnContextLoss` takes a
    // `Function` with no unset, so a later `setOnContextLoss(handler)` has nothing to register
    // *with* unless the indirection is already in place. Cheap: an inert relay is one arrow the GC
    // keeps, and the renderer only ever calls it on a loss that outlived its deadline.
    backend.setOnContextLoss(instance.contextLoss.notify);
    if (opts.onContextLoss !== undefined) instance.setOnContextLoss(opts.onContextLoss);
    // The renderer's own default is 3000 (xterm parity), so this is a no-op unless the consumer
    // states one — unlike `setBgAlpha`, because a duration the consumer did not choose is better
    // left where the renderer documents it than restated here at a value this file would then own.
    if (opts.contextRestoreTimeout !== undefined) {
      instance.setContextRestoreTimeout(opts.contextRestoreTimeout);
    }
    // `undefined` is the default (follow the application), so this is a no-op unless set (#575).
    instance.setCursorBlink(opts.cursorBlink);
    if (opts.cursorBlinkTimeout !== undefined) instance.setCursorBlinkTimeout(opts.cursorBlinkTimeout);
    // `0`/omitted = no text blink, the reference default (#576) — a no-op unless the consumer opts in.
    if (opts.textBlinkInterval !== undefined) instance.setTextBlinkInterval(opts.textBlinkInterval);
    return instance;
  }

  /** The cell-decoding context (palette + flag bits) the a11y mirror (#119) reads so it decodes
   * the same cells via its own `CellMirror` without re-importing the decoder. */
  get cellPalette(): Palette {
    return this.palette;
  }
  get cellFlags(): FlagBits {
    return this.flagBits;
  }

  /** Wire marker-anchored decorations (#120): the source projects each frame's rects (typically
   * `(f) => registry.decorationsForFrame(f)`), which the renderer composites under/over the
   * highlight. Pass `undefined` to detach. */
  setDecorationSource(source: ((frame: DecodedFrame) => DecorationRect[]) | undefined): void {
    this.decorationSource = source;
  }

  /** The renderer's cell size in **device** pixels — the consumer divides by `devicePixelRatio`
   * to map pointer coordinates to cells (matches the beamterm adapter's `cellSize`). */
  cellSize(): { width: number; height: number } {
    return { width: this.backend.cell_width(), height: this.backend.cell_height() };
  }

  /** Change the font size (CSS px) at runtime (#406/#417) — re-bakes the atlas. The cell size moves,
   * so **the consumer must re-fit** (recompute its grid + `resize`) after calling. A no-op at the
   * current size. */
  setFontSize(cssPx: number): void {
    this.backend.setFontSize(cssPx);
  }

  /** Change the font family at runtime (#413/#417) — a CSS `font-family` string, re-bakes the atlas.
   * As with {@link setFontSize}, the cell size can move, so **the consumer must re-fit** after. Load
   * a webfont before an unfamiliar family (the browser silently falls back otherwise). */
  setFontFamily(family: string): void {
    this.backend.setFontFamily(family);
  }

  /**
   * Change the letter spacing (CSS px) / line height (multiplier `>= 1`) at runtime (#578). The live
   * counterparts of {@link JustermRendererOptions.letterSpacing} / {@link
   * JustermRendererOptions.lineHeight}, whose docs carry the units and the clamping.
   *
   * **The consumer must re-fit afterwards**, exactly as for {@link setFontSize}/{@link
   * setFontFamily}: call {@link resize} with the CSS box. These do not do it for you, because this
   * object has no reference to the fit — the widget and the consumer own that (the demo's
   * `setFontSize(); fit(); render();` is the shape, #417). Skipping it is not cosmetic: the renderer
   * re-sizes its own drawing buffer to the new cell, so the canvas display box the adapter set from
   * `cssWidth()`/`cssHeight()` immediately describes a buffer that no longer exists.
   *
   * **Call this {@link resize} directly — not `FitController.fit()`**, even if you hold one. The
   * reason is a *signature*, not a bug: `ResizePort.resize(cols, rows)` carries a **grid**, and the
   * canvas display box is set only here, from a **box**. So a flush reaches the consumer's port and
   * stops there; nothing in that chain touches `canvas.style.width/height`. Secondarily, the flush is
   * debounced (100 ms by default), which is 100 ms of displaying a buffer that no longer exists.
   *
   * Until #632 there was a third reason and it was the one written here: the controller deduped on
   * `cols`/`rows` alone, so a cell change that left the grid identical was dropped outright.
   * **That one is fixed** — the key now carries the cell too, so `FitController` is safe to keep
   * using for container resizes across a spacing change. It still is not the thing that re-sizes
   * this canvas.
   *
   * xterm.js draws the same line, which is why this is a shape rather than a preference: an option
   * change there re-lays out at the *current* grid (`RenderService.ts` `handleResize(cols, rows)`) and
   * its `FitAddon` registers no listeners at all — re-deriving the grid from the pixel box stays
   * manual. alacritty auto-re-fits, but it owns its OS window; an embeddable widget does not.
   *
   * **Read the cell back rather than deriving it from what you passed.** `adopt_spacing` can hand you
   * something other than what you asked for in three separate ways, and none of them reports an error:
   * a `lineHeight` whose cell the atlas cannot hold is *shrunk* (#359); a failed atlas re-bake rolls
   * the whole change back to the previous spacing; and while the GL context is lost the **cell moves
   * immediately but the buffer does not** — `adopt_spacing` runs `recompute_cell()` *before* its
   * lost-context guard (`webgl.rs`), so {@link cellSize} reports the new cell while the atlas re-bake
   * and the drawing-buffer resize wait for `webglcontextrestored`. (This sentence used to say the cell
   * "does not move at all", which was false against that ordering — corrected in #632, whose own
   * reasoning depended on it.) {@link cellSize} and
   * {@link terminalSize} are the truth afterwards — and `terminalSize` matters as much as the cell,
   * because the renderer's internal re-size adopts what the drawing buffer will actually grant (#339),
   * so a large enough cell shrinks the *grid* as well.
   */
  setLetterSpacing(cssPx: number): void {
    this.backend.setLetterSpacing(cssPx);
  }

  /** See {@link setLetterSpacing} — same cell-moving contract, same re-fit and read-back obligation. */
  setLineHeight(multiplier: number): void {
    this.backend.setLineHeight(multiplier);
  }

  /**
   * Change the background opacity at runtime (#577) — `0` transparent, `1` opaque. The live
   * counterpart of {@link JustermRendererOptions.bgAlpha}, whose doc carries the full contract
   * (which cells it reaches, and the consumer CSS it depends on to be visible at all).
   *
   * **No re-fit**, unlike {@link setFontSize}/{@link setFontFamily}: the cell geometry does not
   * move, so the grid the consumer drives its engine at is unaffected.
   *
   * Presents immediately, and — unlike {@link setCursorBlink} / {@link setTextBlinkInterval}, which
   * first check that there is a cursor or a frame to redraw — does so unconditionally. Those guards
   * exist because their redraw has nothing to say without retained content; this one always does.
   * The alpha rides the *clear* colour as well as the per-cell one (`webgl.rs` `draw`), so even an
   * empty terminal changes, and a consumer that sets this before the first frame sees it at once
   * rather than at the next output.
   *
   * Out-of-range values are the renderer's to clamp (`set_bg_alpha`, `[0,1]`) and are deliberately
   * not re-clamped here — two layers holding the same bound is how they drift apart.
   */
  setBgAlpha(alpha: number): void {
    this.backend.setBgAlpha(alpha);
    this.backend.render();
  }

  /**
   * Install (or clear, with `undefined`) the handler called when a lost WebGL context has not come
   * back within {@link setContextRestoreTimeout} (#579). The live counterpart of
   * {@link JustermRendererOptions.onContextLoss}, whose doc carries the full contract — what the
   * signal means, what it does *not* mean, and why the widget applies no policy of its own.
   *
   * **Nothing is re-registered with the renderer here.** The renderer holds one relay for the life
   * of this object (`create`), because `setOnContextLoss` takes a `Function` and offers no unset;
   * this swaps the handler behind it. That is what makes clearing expressible at all, and it is why
   * a swap cannot leave the renderer holding a stale closure.
   *
   * **No redraw**, unlike {@link setCursorBlink} / {@link setBgAlpha}: this changes who is told
   * about a future event, not anything currently on screen.
   */
  setOnContextLoss(handler: (() => void) | undefined): void {
    this.contextLoss.set(handler);
  }

  /**
   * Change the restore grace period at runtime, in ms (#579) — the live counterpart of
   * {@link JustermRendererOptions.contextRestoreTimeout}, whose doc carries the default and why the
   * knob exists.
   *
   * Applies to the **next** loss. A deadline already armed keeps the duration it was armed with, so
   * shortening this during a loss does not bring that loss's notification forward — the renderer
   * stamps each deadline with the loss it belongs to and never re-arms one (`context_loss.rs`, the
   * `loss_epoch` field).
   */
  setContextRestoreTimeout(ms: number): void {
    this.backend.setContextRestoreTimeoutMs(ms);
  }

  /**
   * Whether a context loss has been **reported** (#579). For surfacing the state — dimming the
   * terminal, showing a badge — not for deciding whether drawing is safe.
   *
   * **It answers *"was I told"*, and that is deliberate rather than an approximation** (ADR-0027
   * D4). A browser destroys a context synchronously and merely *queues* `webglcontextlost`, so for
   * a window this reads `false` while every GL call is already dead. The renderer guards its own
   * work on a different, stricter predicate that consults the context itself; that one is private,
   * because a consumer branching on it would be making a decision the renderer has already made.
   * Recovery needs nothing from you either way — the renderer rebuilds itself on
   * `webglcontextrestored`.
   *
   * Stays truthful after {@link dispose}: disposal stops this object's *work*, and the renderer's
   * canvas listeners belong to the wasm binding, so the state machine behind this keeps tracking.
   * Only the notification is closed.
   *
   * **Watch it for the falling edge if you re-fit**, which is the one thing a consumer has to *do*
   * with this rather than display (#717): a {@link resize} that landed during the loss is
   * provisional, and repeating it once this reads `false` again is what re-syncs the canvas display
   * box to the buffer the browser actually granted. `resize`'s doc carries the measurement and why
   * re-reading {@link terminalSize} does not cover it.
   */
  isContextLost(): boolean {
    return this.backend.isContextLost();
  }

  /**
   * Whether a lost context has missed its restore deadline (#579) — the same fact
   * {@link setOnContextLoss} pushes, available to pull. For a consumer that attached late, or that
   * prefers to poll a status line rather than hold a callback.
   *
   * **Advisory, and it un-sets.** A late `webglcontextrestored` clears it *and* heals the renderer,
   * so a consumer that latched a permanent "GPU lost" state off one reading will be wrong about a
   * terminal that has since recovered. Read it each time.
   */
  isRestoreOverdue(): boolean {
    return this.backend.isRestoreOverdue();
  }

  /** Swap the colour scheme at runtime (#420) — rebuild the 256-colour palette from the new ANSI
   * colours and push it (+ the theme's policy colours) to the renderer, which re-resolves every
   * retained cell in wasm. No re-fit needed (the cell geometry is unchanged); it presents on the
   * render below. The a11y cell mirror reads only text, so it needs no re-notification.
   *
   * **Leaves {@link setBgAlpha} alone**, which is the point of keeping the alpha off {@link Theme}
   * (#577): swapping the colour scheme does not silently make a translucent terminal opaque again.
   * Stated because it is a property of where the field lives rather than of any code here — nothing
   * in this method would have to change for it to be false. */
  setTheme(theme: Theme): void {
    const colors = this.buildPalette(Uint32Array.from(theme.ansi));
    this.palette = { colors, defaultFg: theme.defaultFg, defaultBg: theme.defaultBg };
    this.cursorColor = theme.cursorColor ?? theme.defaultFg;
    this.cursorTextColor = theme.defaultBg;
    this.selectionBg = theme.selectionBg ?? 0x45475a;
    this.matchBg = theme.matchBg ?? 0x6e5c00;
    this.activeMatchBg = theme.activeMatchBg ?? 0x995200;
    this.selectionInactiveBg = theme.selectionInactiveBg ?? 0x30313d;
    // Push the palette + the policy colours a theme can carry; each marks the buffer dirty (#421).
    this.backend.setPalette(colors, theme.defaultFg, theme.defaultBg);
    this.backend.setBoldToBright(theme.boldToBright ?? true);
    this.backend.setMinimumContrastRatio(theme.minimumContrastRatio ?? 1);
    this.backend.setSelectionForeground(theme.selectionForeground);
    this.issueOverlay(); // the selection/match blend colours moved
    this.redrawCursor(); // re-push the cursor with its new colour, then present (one pack, #421)
  }

  /** Fit a `cols`×`rows` grid to a CSS-pixel box and size the renderer + canvas display box to
   * it. Unlike beamterm (which took CSS px and computed the grid itself), the renderer takes a
   * grid, so the adapter divides here (pixel→cell is consumer policy) and sets the canvas CSS box
   * from what the renderer reports it must be (`cssWidth`/`cssHeight`) — forget that and the
   * device-px buffer displays at twice its size on a Retina screen.
   *
   * **A call that lands while the GL context is lost is provisional, and must be repeated once the
   * context comes back** (#717). The renderer commits the grid you asked for but defers reading the
   * drawing buffer back, because a dead context answers `0` and adopting that would floor the grid
   * to one cell (#639). That read — and therefore any browser clamp (#339) — settles inside
   * `restore()`, which runs on the next {@link render}, not when `webglcontextrestored` fires. The
   * two lines below have already run by then with the pre-clamp numbers, and nothing rewrites them.
   *
   * Measured (headless Chromium, `MAX_TEXTURE_SIZE` 8192, cell 9 device px), asking for 4000
   * columns during a loss:
   *
   * | | grid | `cssWidth()` | `canvas.style.width` |
   * |---|---|---|---|
   * | during the loss | 4000 | 36000 | `36000px` |
   * | after the restoring `render()` | **910** | **8190** | `36000px` |
   *
   * So the display box describes a buffer 4.4x wider than the one that exists, and the browser
   * stretches to fit. **Re-reading {@link terminalSize} is not the remedy** — it reports the truth,
   * but the canvas box is written here and only here. Call this method again with your current CSS
   * box; that is the whole fix, and it is idempotent on a live context.
   *
   * Reachable only when the requested grid exceeds the browser's buffer limits, so most consumers
   * will never see it. {@link isContextLost} going `false` is the signal that the repeat is due. */
  resize(cssWidth: number, cssHeight: number): void {
    const grid = gridForBox(
      cssWidth,
      cssHeight,
      this.backend.cssCellWidth(),
      this.backend.cssCellHeight(),
    );
    // Nothing to propose — an unmeasured cell or a non-finite box (#632). Leave the renderer and the
    // canvas box exactly as they are: resizing to a guess is how an unlaid-out container turned into
    // a 1x1 terminal, and the CSS box below must not describe a buffer we did not ask for.
    if (!grid) return;
    const { cols, rows } = grid;
    this.backend.resize(cols, rows);
    this.canvas.style.width = `${this.backend.cssWidth()}px`;
    this.canvas.style.height = `${this.backend.cssHeight()}px`;
  }

  /** The terminal grid the renderer ACTUALLY adopted after the last {@link resize} — read back from
   * the renderer (not the requested `cols`/`rows`), so a browser drawing-buffer clamp (#339) can't
   * desync the grid the consumer drives its engine + frames at from the grid the buffer holds. */
  terminalSize(): { cols: number; rows: number } {
    return { cols: this.backend.cols(), rows: this.backend.rows() };
  }

  applyFrame(frame: DecodedFrame): void {
    // Set the retained overlay/decoration/cursor state FIRST, then apply_damage packs the grid
    // once with it (setOverlay's re-pack is a no-op until the first apply_damage, so the first
    // frame is a single pack). The renderer composites them in wasm — no consumer-side overlay
    // walk (the beamterm adapter's composeOverlayDraws) survives the pivot.
    // `retainU32`, not `asU32`: these three outlive the frame (a focus flip re-issues them with no
    // new frame), and a decoder column is a view into WASM memory that the next large decode
    // detaches (#657).
    this.lastSelectionSpans = retainU32(frame.selectionSpans ?? new Uint32Array(0));
    this.lastMatchSpans = retainU32(frame.matchSpans ?? new Uint32Array(0));
    this.lastActiveMatchSpans = retainU32(frame.activeMatchSpans ?? new Uint32Array(0));
    this.issueOverlay();
    this.backend.setDecorations(decorationWire(this.decorationSource?.(frame) ?? []));
    this.updateCursor(frame);
    // Pack at the CURRENT text-blink phase, not forced-on — same reason `updateCursor` draws at
    // the cursor's current phase: a content frame arriving during the off phase must not flash the
    // blinking cells back on until the loop's next flip. `apply_damage` stores this as the
    // renderer's `last_blink_on`, so the two stay in step by construction.
    const textBlinkOn = this.textBlink.isVisible(now());
    this.backend.apply_damage(
      damageHeader(frame, textBlinkOn),
      asU32(frame.spans),
      asU32(frame.codepoints),
      asU32(frame.fg),
      asU32(frame.bg),
      asU16(frame.flags),
      // #627: `asU32`, not `asU16` — renderer >= 0.9.0 takes this column as u32, and narrowing it
      // back truncated silently above `u16::MAX` while copying every frame.
      //
      // Whether the identity branch is taken is **the frame producer's** business, not this
      // package's. `frame` arrives through `FrameSource.push`, so the width of `extra` is decided by
      // whoever decoded it — a consumer on `justerm-wasm-decode` >= 0.12.0 hands a `Uint32Array`
      // (identity, zero JS allocation); one still on 0.11.0 hands a `Uint16Array` and pays one
      // widening copy, correct for every value a u16 can hold. This package's own dependency on the
      // decoder does **not** decide it: the only thing web imports from there is `buildPalette` plus
      // the `Palette` type — `decodeFrame` is called nowhere in `src/`. An earlier version of this
      // comment tied the copy to *our* pin moving at #633 step 5; that was wrong, and the pin bump
      // it predicted has now landed without changing anything here.
      asU32(frame.extra),
      Array.from(frame.sideTable),
      // #520: the underline colour column (SGR 58). Trailing arg on the renderer's apply_damage;
      // the renderer packs it as the base ink of the line channel so an underline draws in its
      // own colour. All compositing stays in the renderer (post-#273/#504) — this is pure forwarding.
      // Optional on the frame (a fixture may omit it) → empty scatters as all-Default.
      asU32(frame.underlineColor ?? new Uint32Array(0)),
    );
    // Everything below records what the renderer NOW holds, so it is committed only after
    // `apply_damage` returns: that call refuses a malformed span directory (`webgl.rs`, #355) and
    // returns *before* it stores the phase, so recording first would leave this side believing it
    // pushed a phase the renderer never took — and the loop, seeing no flip, would not correct it.
    this.lastTextBlinkOn = textBlinkOn;
    this.lastFrameGrid = { cols: frame.cols, rows: frame.rows };
    this.trackBlinkCells(frame);
    // A frame is the only thing that establishes a grid to re-pack, so the loop starts here as
    // well as at the cursor — a terminal with a hidden cursor still blinks its SGR 5 text.
    if (this.textBlink.enabled) this.startBlinkLoop();
  }

  /**
   * Track whether the renderer's grid may hold a `BLINK` cell — xterm.js's `needsBlinkInViewport`
   * (`TextBlinkStateManager.ts:67`), adapted to frame mode (#576).
   *
   * xterm.js answers this exactly, by scanning the viewport it owns. A frame-mode consumer holds
   * damage, not the grid, so the exact question is not answerable here — but the gate only needs to
   * be **conservative**, because a false positive costs one redundant re-pack while a false
   * negative would freeze blinking text. So: a Full frame *replaces* the answer, and a Partial
   * frame can only *add* to it. The result decays only at the next Full frame, which is the safe
   * direction.
   *
   * **What the exactness of the Full case rests on**, cited at the layer that owns it rather than
   * at the renderer's paraphrase of it: core emits full damage as every row at full width
   * (`justerm-core/src/term.rs`, `TermDamage::Full` → `(0..rows).map(|l| (l, 0, cols - 1))`), which
   * `FrameKind::Full` states as its contract (*"Every row is present"*, `serialize.rs`). If a Full
   * frame ever became a subset, this gate would start producing the one error it must not.
   *
   * Without this, a consumer that opts in pays a full re-pack per half-period on every terminal —
   * `resolve_and_pack` walks every cell and `plan_upload` diffs the result — even where no cell has
   * ever carried SGR 5, and the produced buffer is byte-identical. **Measured** (demo, 600ms
   * interval, 3.0s, presenting rAF turns, identical conditions either side): 16 with three blinking
   * cells on screen, 11 with none — a delta of 5, which is exactly the 5 phase flips in that window.
   * The work behind each is proportional to `cols × rows`.
   */
  private trackBlinkCells(frame: DecodedFrame): void {
    const here = carriesBlink(frame.flags, this.flagBits.blink);
    // kind 0 = Full, 1 = Partial (the same encoding `damageHeader` puts on the wire).
    this.mayHaveBlinkCells = frame.kind === 0 ? here : this.mayHaveBlinkCells || here;
  }

  render(): void {
    this.backend.render();
  }

  /** The active selection tint for the current focus state (#115). */
  private activeSelectionBg(): number {
    return this.focused ? this.selectionBg : this.selectionInactiveBg;
  }

  /** Re-issue the retained overlay spans with the focus-gated tint — the single site for the
   * "retained spans + active selection tint" contract, shared by the per-frame push and a focus
   * flip (which has no new frame) so the two can never drift. The active-match channel (#429)
   * rides along: additive renderer state (`setActiveMatch`), pushed with the same cadence so a
   * theme swap re-colours it too. Its tint is NOT focus-gated — xterm has no inactive variant
   * for match colours (only the selection dims on blur). */
  private issueOverlay(): void {
    this.backend.setOverlay(
      this.lastSelectionSpans,
      this.lastMatchSpans,
      this.activeSelectionBg(),
      this.matchBg,
    );
    this.backend.setActiveMatch(this.lastActiveMatchSpans, this.activeMatchBg);
  }

  /** Push the frame's cursor to the renderer (native cursor — #270), or clear it when hidden.
   * The renderer draws the shape (block/underline/bar/hollow) itself: unlike beamterm (which had
   * no cursor and fell a bar back to a block), a bar renders as a real bar. Blink phase stays
   * consumer policy — the blink loop calls `clearCursor`/`setCursor` on the off/on flip. */
  private updateCursor(frame: DecodedFrame): void {
    // The application's blink mode rides every frame (wire v4, #81) — core writes it from both
    // DECSCUSR and `CSI ?12 h/l`. Applied only when the frame actually carries it, so a frame that
    // omits the field (an older backend, a hand-built fixture) leaves the last known mode alone
    // rather than silently forcing steady. `CursorBlink` resolves it against the consumer override.
    if (frame.cursorBlink !== undefined) this.blink.setAppBlink(frame.cursorBlink);
    const cmd = cursorCommand(frame);
    if (cmd.kind === "none") return;
    if (cmd.kind === "clear") {
      this.cursor = undefined;
      this.backend.clearCursor();
      return;
    }
    // ADR-0028 D5 — while a composition is open the caret's POSITION is the composition's end, not
    // the engine cursor the frame carries. Position only: `cursorCommand` above still decides
    // whether there is a caret at all, so an application that hid it keeps it hidden (#592's
    // boundary, which browser ownership does not reach).
    const col = this.preeditCaret?.col ?? cmd.col;
    const row = this.preeditCaret?.row ?? cmd.row;
    // A move (or first appearance) restarts the blink so the cursor shows at once.
    if (!this.cursor || col !== this.cursor.col || row !== this.cursor.row) {
      this.blink.restart(now());
    }
    this.cursor = { col, row, shape: cmd.shape };
    // Draw at the CURRENT blink phase, not forced-on: the decoder emits cursor fields on every
    // frame, so a content frame streaming during blink-off must leave the cursor off (a `restart`
    // above already forces phase-on for a move). Forcing on here would pin the cursor solid and
    // flicker against the rAF loop during output — the beamterm adapter drew at `isVisible` too.
    this.pushCursor(this.blink.isVisible(now()));
    this.startBlinkLoop();
  }

  /** Set (`on`) or clear (`off`) the cursor for the current blink phase. */
  private pushCursor(on: boolean): void {
    this.lastBlinkOn = on;
    if (on && this.cursor) {
      this.backend.setCursor(
        this.cursor.col,
        this.cursor.row,
        this.cursor.shape,
        this.cursorColor,
        this.cursorTextColor,
      );
    } else {
      this.backend.clearCursor();
    }
  }

  /** Re-issue the cursor for the current phase and present (the blink loop + focus/typing paths).
   * The strokes are shader uniforms, so this costs no upload — only the block repaints a cell. */
  private redrawCursor(): void {
    this.pushCursor(this.blink.isVisible(now()));
    this.backend.render();
  }

  /** Show the cursor and reset its blink phase (#107) — the widget calls this on a key intent so
   * the caret stays solid while typing rather than blinking off right after a keystroke. */
  restartCursorBlink(): void {
    // The INPUT path — this is the only caller that means "the user did something", so it is the one
    // that resets the idle clock (#593). `updateCursor`'s move branch calls the phase-only
    // `restart()`, because a cursor move is application output.
    this.blink.restartFromInput(now());
    this.redrawCursor();
  }

  /**
   * How long the cursor keeps blinking with no user input before parking solid, in ms (#593).
   * `0` disables it. Defaults to {@link BLINK_IDLE_TIMEOUT} (5 minutes, xterm.js's value).
   *
   * Consumer policy (ADR-0017), so it is injected rather than assumed — the two references disagree
   * on the number by 60x. The live counterpart of
   * {@link JustermRendererOptions.cursorBlinkTimeout}.
   */
  setCursorBlinkTimeout(ms: number): void {
    this.blink.setIdleTimeout(ms);
    if (this.cursor) this.redrawCursor();
  }

  /**
   * An IME composition started / ended (#592) — the caret stays put for the duration.
   *
   * Redraws immediately: no frame carries this (composition never reaches the engine), so waiting
   * for one would leave the caret mid-phase until the next output — the same reason
   * {@link setFocused} redraws.
   */
  /**
   * Draw the composition into the grid and report where the caret belongs (#249, ADR-0028).
   *
   * Presents immediately rather than waiting for the next frame: a composition produces no frames
   * at all — the engine never sees it — so there is nothing else to ride on. The cursor is re-pushed
   * at the returned column, which is D5's position rule: the caret rides the composition's end,
   * while `cursorCommand` still decides whether it is drawn (an application that hid the caret
   * keeps it hidden, #592's boundary).
   */
  setPreedit(col: number, row: number, codepoints: Uint32Array): number {
    // A renderer published before #249 has no such binding: report the anchor cell unchanged, so
    // the widget's anchor logic still works and only the drawing is missing.
    if (!this.backend.setPreedit) return col;
    const caretCol = this.backend.setPreedit(col, row, codepoints);
    // Retained, because D5 is a rule about every frame and not about this call. Frames keep arriving
    // while a composition is open and each one describes the ENGINE's cursor, which knows nothing
    // about the preedit — so without this the caret snaps back under the composed text on the next
    // output frame, which is #637's harm wearing the cursor's clothes.
    this.preeditCaret = codepoints.length > 0 ? { col: caretCol, row } : undefined;
    if (this.cursor) {
      this.cursor = { ...this.cursor, col: caretCol, row };
      // The CURRENT phase, not the last one pushed. `setComposing(true)` has already told the blink
      // to hold the caret on (#592), and `lastBlinkOn` is whatever the loop happened to push before
      // the composition opened — so re-pushing that lands a *cleared* cursor for the whole
      // composition whenever it began on an off phase. Caught by #592's own probe once it sampled
      // where the caret had moved to.
      this.pushCursor(this.blink.isVisible(now()));
    }
    this.render();
    return caretCol;
  }

  setComposing(composing: boolean): void {
    this.blink.setComposing(composing);
    if (this.cursor) this.redrawCursor();
  }

  /**
   * Force the cursor to blink (`true`) / stay steady (`false`), or `undefined` to follow the
   * application's DECSCUSR / `CSI ?12` mode (#575). The live counterpart of
   * {@link JustermRendererOptions.cursorBlink}.
   *
   * Redraws immediately: a change of blink authority is not carried by any frame, so waiting for
   * one would leave the cursor in the previous phase until the next output — the same reason
   * {@link setFocused} redraws. Guarded on there *being* a cursor, because {@link create} applies
   * the initial value before the first frame and before the first fit: `redrawCursor` presents,
   * and presenting an unsized canvas is a GL call with nothing to draw.
   */
  setCursorBlink(blink: boolean | undefined): void {
    this.blink.setBlinkOverride(blink);
    if (this.cursor) this.redrawCursor();
  }

  /**
   * The half-period of the SGR 5 text blink in ms; `0` disables it (the default). The live
   * counterpart of {@link JustermRendererOptions.textBlinkInterval} (#576).
   *
   * Re-syncs immediately: no frame carries this, so disabling while the phase is off would leave
   * that text invisible until the next output — the failure xterm.js avoids by forcing `blinkOn`
   * true and re-rendering when its interval stops (`TextBlinkStateManager._updateIntervalState`).
   */
  setTextBlinkInterval(ms: number): void {
    this.textBlink.setIntervalMs(ms, now());
    this.syncTextBlinkPhase();
    if (this.textBlink.enabled) this.startBlinkLoop();
  }

  /** Bring the renderer's retained phase back in line with {@link textBlink} and present, if they
   * have drifted — the shared tail of the reduced-motion listener and the interval setter. */
  private syncTextBlinkPhase(): void {
    const on = this.textBlink.isVisible(now());
    if (on !== this.lastTextBlinkOn && this.repackAtTextBlinkPhase(on)) this.backend.render();
  }

  /**
   * Re-pack the retained grid at a new text-blink phase, without a frame (#576). Returns whether
   * the renderer was actually re-issued — the caller presents.
   *
   * Refuses while no frame has been applied, or while the last frame's grid disagrees with the one
   * the renderer holds (a `resize` that is still waiting for its first frame). Both cases would
   * make `apply_damage` allocate a fresh empty grid for the dimensions in the header and the
   * screen would go blank — the flip has nothing to redraw with, since it carries no cells.
   */
  private repackAtTextBlinkPhase(on: boolean): boolean {
    const grid = this.lastFrameGrid;
    if (!grid || grid.cols !== this.backend.cols() || grid.rows !== this.backend.rows()) {
      return false;
    }
    this.backend.apply_damage(
      blinkPhaseHeader(grid.cols, grid.rows, on),
      EMPTY_U32,
      EMPTY_U32,
      EMPTY_U32,
      EMPTY_U32,
      EMPTY_U16, // flags — still u16
      EMPTY_U32, // extra — u32 since #621/#627; `flags` above is the only u16 column left
      [],
      EMPTY_U32,
    );
    this.lastTextBlinkOn = on;
    return true;
  }

  /** Focus gates the blink (blurred → solid) and the selection tint (active ↔ inactive, #115).
   * No frame changed on a focus flip, so re-issue `setOverlay` with the retained spans + the new
   * tint (the renderer re-packs the retained grid) and redraw the cursor. */
  setFocused(focused: boolean): void {
    this.blink.setFocused(focused);
    if (this.focused !== focused) {
      this.focused = focused;
      this.issueOverlay();
    }
    this.redrawCursor();
  }

  /**
   * A rAF loop that re-issues the cursor cell whenever its blink phase flips, and re-packs the
   * grid whenever the SGR 5 text phase flips (#576).
   *
   * The two phases are separate clocks but share one loop and, when they flip together, **one
   * present** — the same "pack once, present once" rule `applyFrame` follows (#421).
   *
   * rAF stops firing for a hidden *document*, so a backgrounded tab costs nothing. That is **not**
   * the same guarantee as xterm.js's `setViewportVisible`, which is fed by an `IntersectionObserver`
   * on the screen element: a terminal scrolled out of view inside a visible page keeps ticking here.
   * The gap applies equally to the cursor half of this loop, so it is a widget-level decision rather
   * than a text-blink one, and is left as it was.
   */
  private startBlinkLoop(): void {
    this.blinkLoop.start();
  }

  /**
   * One blink iteration. Called by {@link FrameLoop}, which owns the scheduling — including the
   * part that matters here: if this throws, the loop stops with no handle left behind, so the next
   * `startBlinkLoop` (which `updateCursor` issues on every decoded frame) restarts it. Before #696
   * the re-arm lived at the bottom of this body and a throw latched the loop off permanently.
   *
   * Must not call {@link JustermRenderer.startBlinkLoop} — see `FrameLoop`'s `run` doc.
   */
  private blinkTick(): void {
    const t = now();
    const cursorOn = this.blink.isVisible(t);
    const cursorFlip = cursorOn !== this.lastBlinkOn;
    const textOn = this.textBlink.isVisible(t);
    // Gated on there being something to conceal: the flip is a full re-pack, and running it over
    // a grid with no BLINK cell produces a byte-identical buffer at the cost of a walk over every
    // cell. The two orders below are interchangeable (the cursor is a shader uniform, not an
    // instance — `webgl.rs` `set_cursor` sets no `needs_repack`), so this one is only convention.
    const textFlip = this.mayHaveBlinkCells && textOn !== this.lastTextBlinkOn;
    const repacked = textFlip && this.repackAtTextBlinkPhase(textOn);
    if (cursorFlip) this.pushCursor(cursorOn);
    if (repacked || cursorFlip) this.backend.render();
  }

  /**
   * Stop the blink loop and detach the reduced-motion listener. Both are draw paths (#576): the
   * listener re-packs and presents, so a widget that kept it would still repaint its canvas after
   * being disposed.
   *
   * **Called by `Terminal.dispose()` since #606** — the sentence that stood here until then said
   * *"nothing calls this yet"*, which was the defect, not a caveat. Idempotent, as the `Renderer`
   * port requires: `cancelAnimationFrame` is guarded and `removeEventListener` is a no-op the
   * second time, so a consumer that also calls it is not punished.
   *
   * **Stops work, does not release memory.** The wasm instance, its retained grid and glyph atlas,
   * the GL context and the canvas context-loss listeners the Rust side owns all survive — they go
   * with the binding's `free()`, which cannot be called while the consumer still holds this object.
   * A consumer tearing down for good should drop its own reference and let the page go.
   *
   * **That is exactly why the context-loss channel is closed here by hand** (#579). It is the one
   * piece of ambient work whose teardown the renderer *does* own but at the wrong end of the
   * object's life: `ContextLossHandler`'s `Drop` clears the callback slot, and `Drop` runs at
   * `free()`, which the sentence above says never happens. So a restore deadline armed moments
   * before disposal would otherwise still deliver to a widget that has ended. The observable
   * contract this restores is the reference's — xterm.js's disposable clears its pending restore
   * timeout (`addons/addon-webgl/src/WebglRenderer.ts:161-163`) — and the renderer's own `Drop`
   * comment names that same behaviour as what it matches.
   *
   * `isContextLost()` / `isRestoreOverdue()` keep answering afterwards; only the *push* stops. They
   * read the state machine the surviving canvas listeners still feed, so silencing them would mean
   * lying about the context rather than ending work.
   */
  dispose(): void {
    this.blinkLoop.stop();
    this.motionQuery.removeEventListener("change", this.onMotionChange);
    this.contextLoss.end();
  }
}
