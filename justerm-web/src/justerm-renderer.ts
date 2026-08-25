import type { Palette } from "justerm-wasm-decode/colors.js";
import { CursorBlink } from "./cursor";
import { FrameLoop } from "./frame-loop";
import type { DecorationRect } from "./decorations";
import { MINIMUM_COLS, MINIMUM_ROWS } from "./fit";
import { TerminalSurface, type GridLease, type SurfaceBackend } from "./terminal-surface";

import type { Renderer } from "./renderer";
import { TextBlink } from "./text-blink";
import type { DecodedFrame, FlagBits } from "./types";

/** Theme colours (packed `0xRRGGBB`). The engine stays ignorant of these — the
 * consumer owns them and the renderer resolves cell refs against them. Carried over
 * verbatim from the beamterm adapter (#273): the theme contract is renderer-neutral.
 *
 * **A theme is a complete description, not a patch**, and a field added here inherits that:
 * {@link JustermRenderer.setTheme} pushes **every** member, so an unset one *resets* to its default
 * rather than keeping whatever the previous theme set. Adding a field therefore means adding an
 * unconditional push there too — which is the opposite of how the optional members of
 * {@link JustermRendererOptions} are wired (read once at `create`, pushed only when present), and
 * the two conventions sit close enough together to swap by accident. Nothing but a test catches it:
 * the wrong shape type-checks, and it only misbehaves on the *second* `setTheme`. Stated here rather
 * than at one field because it is a property of the interface. (#580 — see
 * `DEFAULT_CURSOR_CONTRAST` for what it costs when a default is not a neutral identity.) */
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
  /** Minimum WCAG contrast between the **cursor** and the cell it sits on (#580, consumer half of
   * #368). Below it the cursor inverts to the terminal's default fg/bg, so a {@link cursorColor}
   * that happens to match the cell underneath never makes the caret vanish. Defaults to
   * {@link DEFAULT_CURSOR_CONTRAST}; pass `1` — the floor of the ratio range — to switch the guard
   * off, which is xterm.js's behaviour (it has no cursor guard at all).
   *
   * **A separate knob from {@link minimumContrastRatio}, deliberately.** That one corrects a cell's
   * *text* against its background; this one rescues an *overlay* against the cell it covers. They
   * run on different comparands and either can be set without the other.
   *
   * Out-of-range values are the renderer's to clamp (`[1, 21]`) and are not re-clamped here — two
   * layers holding the same bound is how they drift apart.
   *
   * Deliberately here rather than on {@link JustermRendererOptions}, though **both references put
   * their contrast knob outside the colour scheme** — xterm.js's `minimumContrastRatio` is an option
   * (`common/services/OptionsService.ts:43`) and alacritty's `MIN_CURSOR_CONTRAST` is not
   * configurable at all (`alacritty/src/display/content.rs:22`). What governs a consumer-facing API
   * shape here is this API's own coherence, not theirs: the thing this defends — {@link cursorColor}
   * — is on `Theme`, and so is the sibling policy {@link minimumContrastRatio}, so a consumer would
   * have to remember that one contrast ratio lives with the colours and the other does not. Its
   * runtime path is {@link JustermRenderer.setTheme}, like every other policy on this interface. */
  cursorContrast?: number;
  /** Draw bold text in the bright (8-15) ANSI colour — xterm's
   * drawBoldTextInBrightColors (#223). Defaults to true (xterm's default). */
  boldToBright?: boolean;
}

/**
 * The cursor-contrast threshold applied when {@link Theme.cursorContrast} is unset — the renderer's
 * own default (alacritty's `MIN_CURSOR_CONTRAST`, `alacritty/src/display/content.rs:22`), restated
 * here.
 *
 * **Restated deliberately, unlike {@link JustermRendererOptions.cursorThickness}'s default, and the
 * asymmetry follows from where each one lives.** A `Theme` is a *complete* description rather than a
 * patch: {@link JustermRenderer.setTheme} pushes every field it carries, so an unset one must
 * **reset** to the default and not silently keep whatever the previous theme set. That obliges this
 * file to name a value, where an option — read once at `create`, never re-applied — can simply not
 * call the setter and leave the number where the renderer documents it.
 *
 * The cost of naming it is a second copy that can drift from the renderer's, which is why it is a
 * named constant rather than a literal in two call sites. The renderer publishes no getter for its
 * own default, so nothing checks the two against each other.
 */
export const DEFAULT_CURSOR_CONTRAST = 1.5;

/**
 * A terminal attached to a surface someone else built ({@link JustermRenderer.attach}) — everything
 * {@link JustermRendererOptions} carries **except the canvas**, which belongs to the surface.
 *
 * Omitting `canvasSelector` is the whole difference, and it is the point: a terminal sharing a
 * surface has no canvas of its own to name. Where it sits on the shared one is a *rect*, supplied
 * with {@link JustermRenderer.setViewportRect} and re-supplied whenever its overlay moves.
 *
 * **Two of its members reach the whole surface rather than this terminal** — `onContextLoss` and
 * `contextRestoreTimeout`. There is one context per surface, so there is one loss and one deadline;
 * passing either on a second `attach` **replaces** what the first terminal set, for every terminal on
 * the canvas. Set them once, on the surface
 * ({@link TerminalSurface.setOnContextLoss} / {@link TerminalSurface.setContextRestoreTimeout}), and
 * leave them out of per-terminal options. They are kept in the type rather than removed because the
 * single-terminal path reaches it too, and there they are exactly per-terminal.
 */
export type AttachedRendererOptions = Omit<JustermRendererOptions, "canvasSelector">;

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
   * The cursor's stroke thickness as a **fraction of the cell width** (#580, consumer half of
   * #369) — the width of a bar, an underline, or a hollow block's outline. Omit for the renderer's
   * default, `0.15` (alacritty's `cursor.thickness`, `alacritty/src/config/cursor.rs:31`).
   *
   * **A block ignores it.** A block cursor recolours its cell and draws no stroke, so this changes
   * nothing for the default shape — set a bar/underline/hollow shape (DECSCUSR) to see it.
   *
   * A *fraction*, not a length, because the renderer resolves it as
   * `(frac * cell_w).round().max(1)` device px — so it tracks dpr **and** font size. That is
   * alacritty's rule, which #270 chose over xterm.js's `cursorWidth` in CSS px
   * (`common/services/OptionsService.ts:19`) because a fixed length gives a 32px font the same
   * hairline caret as a 12px one. ADR-0023 does not apply: a fraction carries no unit to get wrong.
   * The `.max(1)` floor means even `0` leaves a one-pixel stroke rather than no cursor.
   *
   * Out-of-range values are the renderer's to clamp (`[0, 1]`) and are not re-clamped here.
   *
   * Deliberately here rather than on {@link Theme}, and **both references agree**: a thickness is
   * geometry, not a colour, and each keeps it out of its colour scheme (alacritty under `cursor`,
   * xterm.js as an option beside `cursorWidth`'s siblings). Change it at runtime with
   * {@link JustermRenderer.setCursorThickness}.
   */
  cursorThickness?: number;
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
/**
 * The full renderer surface this adapter drives — **the per-grid half plus everything
 * {@link SurfaceBackend} carries**, which is why it extends it rather than restating those members.
 *
 * The two halves are split on a criterion the renderer already compiled in at 0.15.0 (#773): a call
 * naming a `grid` acts on one terminal, a call naming none acts on the thing every terminal shares.
 * Extending is what turns that from a comment into a gate — a member that drifts between the two
 * interfaces fails to compile where {@link TerminalSurface.open}'s `TerminalSurface<PublishedRenderer>`
 * is passed to {@link JustermRenderer.build}, which takes a `TerminalSurface<RendererBackend>`. That
 * parameter is this package's drift gate against the published renderer; before #775 the same job was
 * done by a `const backend: RendererBackend` assignment in `create`, which this change removed.
 */
export interface RendererBackend extends SurfaceBackend {
  /** Scatter a decoded frame's damage into the persistent grid, then re-pack. Header is
   * `[cols, rows, kind, hasScroll, scrollTop, scrollBottom, scrollCount, blinkOn]` (#285). */
  apply_damage(
    grid: number,
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
    grid: number,
    selectionSpans: Uint32Array,
    matchSpans: Uint32Array,
    selectionBg: number,
    matchBg: number,
  ): void;
  /** Retain the ACTIVE search match's spans + colour (#427) — additive beside
   * `setOverlay`, ranked above selection; empty spans clear it. */
  setActiveMatch(grid: number, activeSpans: Uint32Array, activeMatchBg: number): void;
  /** The in-progress IME composition, or an empty run to clear it (#249). Returns the caret /
   * anchor column — one past the run, after any right-edge shift.
   *
   * **Optional because the published package decides, not this file.** `justerm-web` consumes
   * `justerm-renderer` from npm, so a binding added in the repo is absent at runtime — and from the
   * `.d.ts` — until a `renderer-v*` tag publishes it. Required here would make the widget
   * un-typecheckable against every renderer that predates it, and calling it unguarded would be a
   * `TypeError` rather than a missing feature. A renderer without it is preedit-blind, which is the
   * state every consumer was in before #249. */
  setPreedit?(grid: number, col: number, row: number, codepoints: Uint32Array): number;
  /** Retain the flat decoration directory `[row, left, right, layer, bg, fg]…` (#393). */
  setDecorations(grid: number, spans: Uint32Array): void;
  /** Place the cursor: shape `0` block / `1` underline / `2` bar / `3` hollow (#270). */
  setCursor(
    grid: number,
    col: number,
    row: number,
    shape: number,
    color: number,
    textColor: number,
  ): void;
  /** Remove the cursor — hidden (DECTCEM) or the blink's off phase. */
  clearCursor(grid: number): void;
  /** The cursor's minimum WCAG contrast with the cell under it (#368) and its stroke thickness as a
   * fraction of the cell width (#369). Both are read at *draw* time (a shader uniform and a
   * comparison against the resolved cell), so neither needs a re-pack — but the cursor has to be
   * re-issued for the change to present, which is what `redrawCursor` is for. The renderer clamps
   * each (`[1, 21]` / `[0, 1]`). */
  setCursorContrast(grid: number, threshold: number): void;
  setCursorThickness(grid: number, frac: number): void;
  setBoldToBright(grid: number, enabled: boolean): void;
  setMinimumContrastRatio(grid: number, ratio: number): void;
  setSelectionForeground(grid: number, color: number | undefined): void;
  /** Background cell opacity, `0`..`1` (#298). The renderer clamps; it is read at *draw* time
   * (the clear colour and a shader uniform), not at pack time, so unlike `setBoldToBright` this
   * needs no re-pack — a bare `render` presents it. */
  setBgAlpha(grid: number, alpha: number): void;
  /** Re-bake the atlas at a new font size (CSS px) / family (#406/#413). The cell size moves, so the
   * consumer must re-fit. A no-op if unchanged; a non-finite / `<1` size is guarded by the renderer. */
  setFontSize(grid: number, cssPx: number): void;
  setFontFamily(grid: number, family: string): void;
  /** Extra space between columns in **CSS px** (ADR-0023 — the space `fontSize` already speaks), and
   * a multiplier on the glyph height (`>= 1`). Both move the cell, so the consumer must re-fit; both
   * are clamped or rolled back by the renderer, so the result is read back rather than assumed (#338,
   * #359). */
  setLetterSpacing(grid: number, cssPx: number): void;
  setLineHeight(grid: number, multiplier: number): void;
  /** Swap the palette + default fg/bg for a live theme change (#405): re-resolve every retained
   * cell against the new scheme. `paletteColors` is the 256 pre-built indexed colours. */
  setPalette(
    grid: number,
    paletteColors: Uint32Array,
    defaultFg: number,
    defaultBg: number,
  ): void;
  /** Record a grid's dimensions in cells. **It sizes nothing** — since renderer 0.15.0 the drawing
   * buffer is the *surface's* (`resizeSurface`), because a canvas holding N grids in M font
   * configurations has no cell it can be a multiple of (#773). */
  resizeGrid(grid: number, cols: number, rows: number): void;
  /** Place a grid on the shared buffer, in **device px**, top-left origin. A grid draws only once
   * placed; for a one-terminal widget the rect is the whole buffer. */
  setViewport(grid: number, x: number, y: number, width: number, height: number): void;
  /** Stop drawing a grid **without unregistering it** (#770) — the hidden-workspace state. Every
   * byte survives: packed instances, upload baseline, palette, cursor, overlays and the
   * configuration's atlas. The renderer's draw loop skips an unplaced grid *before* the re-pack, so
   * a hidden grid pays neither the pack nor the upload nor the draw. Placing it again re-packs it
   * once, from the state it already had. */
  clearViewport(grid: number): void;
  /** Whether a grid currently has a viewport, i.e. whether it draws (#770). Throws on an id the
   * registry does not hold, like every other per-grid call. */
  isGridDrawn(grid: number): boolean;
  /** The columns/rows a grid was last given by `resizeGrid` — an **echo** since 0.15.0. Nothing
   * clamps a grid any more; a request the buffer cannot hold shows up in `cssWidth`/`cssHeight`,
   * which report what the browser actually granted. */
  cols(grid: number): number;
  rows(grid: number): number;
  /** The cell width/height in **device** pixels, for the font configuration this grid selects into.
   * Per grid since 0.15.0 — two terminals in two fonts have two cells on one canvas. */
  cell_width(grid: number): number;
  cell_height(grid: number): number;
  /** The cell width/height in **CSS** pixels, unrounded (#331/#335). */
  cssCellWidth(grid: number): number;
  cssCellHeight(grid: number): number;
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
 * The wasm modules are loaded with **dynamic `import()`**: the renderer's by
 * {@link TerminalSurface.open} (constructing it is what binds the context to a canvas) and the
 * decoder's here. Two top-level
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
  /**
   * Where this grid sits on the shared drawing buffer, in **device px**, top-left origin — the
   * origin half of the rect {@link setViewportRect} sets; the extent half is always re-derived from
   * the grid and the cell, so it is not stored beside it (#775).
   *
   * `(0, 0)` for a sole tenant, which is what makes the single-terminal arrangement the special case
   * of the general one rather than a second code path. For a shared surface it is the terminal's DOM
   * overlay measured against the canvas — and it has to be re-supplied whenever that box moves,
   * because WebGL binds one context to one canvas, so a terminal is a transparent overlay over its
   * viewport and nothing inside the GL layer can observe the overlay drifting away from it.
   */
  private rect = { x: 0, y: 0 };
  /**
   * Whether the host has taken this terminal off the surface — **state consulted at every placement,
   * not a command issued once** (#801).
   *
   * The distinction is the whole of this field's justification, and it is measured rather than
   * chosen. Seven entry points re-derive this grid's placement — a density change or context
   * restore through `onReapply`, all four font/spacing setters, {@link resize} and
   * {@link setViewportRect} — and every one funnels into {@link applyGrid}, which holds the only
   * `setViewport` call site in this package. A hide implemented as a single `clearViewport` would
   * therefore be undone by the next density change, silently and with no consumer call behind it.
   *
   * Both references keep the same shape for the same reason and neither issues a one-shot: xterm.js
   * holds `_isPaused` and consults it in `refreshRows`
   * (`src/browser/services/RenderService.ts:140-153`), ghostty holds `flags.visible` and consults it
   * at draw (`src/renderer/Thread.zig:528`, *"If we're invisible, we do not draw"*). The convergence
   * is a cross-check; the derivation above stands without it.
   */
  private hidden = false;
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
  private constructor(
    private readonly backend: RendererBackend,
    /**
     * The surface this terminal draws on — the canvas, the context, the grid registry, the display
     * density and context-loss recovery (#775). **Everything the renderer scopes to the surface
     * rather than to a grid now lives there**, which is what lets N terminals share one context.
     *
     * Before #775 this object held all of it directly, and that was the defect rather than a
     * simplification: `dispose()` stopped the density watcher, detached the restore listener and
     * closed the loss channel — all three surface-scoped — so the *first* terminal to end would have
     * taken density tracking and context recovery away from every sibling sharing its canvas.
     */
    private readonly surface: TerminalSurface<RendererBackend>,
    /**
     * **Whether this terminal composed the surface it draws on.** One fact, and everything this
     * class does differently between its two entry points is derived from it — which is why it is a
     * single flag rather than three (#802).
     *
     * `JustermRenderer.create` composes the surface and keeps it in a private field with no
     * accessor, so **it is the surface's only possible tenant**: nobody else can obtain it to attach
     * to. `attach` receives a surface a host opened, which by construction may have siblings.
     *
     * The three consequences, each following from that one fact rather than being a separate policy:
     *
     * | | composed it (`create`) | given it (`attach`) |
     * |---|---|---|
     * | sizes the drawing buffer to its own grid | yes — it is the only tenant, so #331's exactness is available | no — the host sized it |
     * | presents | synchronously — one tenant, nothing to coalesce | through the surface's loop, coalesced with siblings |
     * | ends the surface on dispose | yes | no — ending a shared surface takes down its siblings |
     *
     * The last row is the general rule, written down once in
     * `docs/map/invariant/a-layer-ends-what-it-exclusively-holds.md`: a layer ends what it
     * exclusively holds. Composing is simply how this object comes to be the only holder.
     *
     * **"Sole tenant" throughout this file means exactly this flag being true** — a terminal that
     * composed its surface and is therefore the only one on it. The term used to be defined by the
     * `ownsExtent` option and its guard; #802 deleted both, so it is anchored here instead. It
     * describes a state that holds **by construction**, not one anything checks at runtime.
     *
     * **This used to be mirrored on the surface** as an `ownsExtent` option, with a guard refusing a
     * second tenant. #802 deleted both: the guard defended a state that cannot be constructed, since
     * the surface it protected is unreachable. `test/published-seam.types.ts` §3 pins that
     * unreachability, so the deletion is falsifiable rather than assumed.
     */
    private readonly composedSurface: boolean,
    /**
     * This widget's claim on the surface (#805). A `Terminal` is one terminal, so it is a constant
     * for the object's life — what changed at renderer 0.15.0 is that the grid has to be *named* on
     * every call that acts on a terminal rather than on the surface.
     *
     * **A lease rather than the bare id**, and the difference is what happens after teardown: an id
     * can outlive the grid it names, so holding one meant every registry call had to cope with a
     * stale one, silently. A lease knows whether it is still valid, so the question does not arise —
     * see {@link GridLease}, which carries why that matters rather than being a preference.
     */
    private readonly lease: GridLease,
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

    /**
     * **The two events that move this grid's cell with no consumer call behind them** — a density
     * change and a context restore — reach this terminal through one registration, because from
     * here they are the same obligation: re-ask for the geometry at whatever the cell has just
     * become. The surface owns the *when* (both signals are surface-scoped and it holds both); only
     * this object knows the *what*, which is its own grid.
     *
     * **Why a restore is one of them** (#325). `restore()` re-reads the **live** device pixel ratio
     * and re-bakes at it (`webgl.rs`, #269) — deliberately, because a DPR notification arriving
     * while the context is lost is *dropped* rather than queued. So a density that moved while the
     * context was dead is adopted there, with no setter behind it and no other signal that it
     * happened.
     *
     * **Since renderer 0.15.0 that adoption moves the CELL and not the buffer** (#773), so this
     * re-derivation is owed rather than optional. Caught by the #325 e2e rather than by reading: on
     * 0.14.0 the renderer re-derived the buffer from the grid it was holding, so only the box had to
     * be re-read; on 0.15.0 the buffer belongs to no grid, and dpr 1 -> 2 across a loss left a
     * `1369`-tall grid inside a `703`-tall buffer.
     *
     * Measured on the older shape, for the failure this originally fixed: dpr 1 -> 2 across a loss
     * left the buffer at `2556x1369` under a canvas still styled `1278x703`; the width was
     * accidentally right (the cell doubled exactly) and the height was `703` against a correct
     * `684.5`, so the browser stretched the terminal ~2.7% vertically.
     *
     * The present-before and present-after that make the cell readable and the resized buffer
     * non-blank are the **surface's**, since they present every grid at once — see
     * {@link TerminalSurface}'s restore handler for why that order is load-bearing.
     */
    this.lease.onReapply(() => this.reapplySurface());
    // And how to END this terminal, so a host that disposes the SURFACE ends the widgets on it rather
    // than retiring their grids under them. Without it this object keeps its blink loop and its
    // reduced-motion listener while holding an id the renderer has retired, and every per-grid call
    // then throws `UnknownGrid` on a timer.
    this.lease.onEnd(() => this.dispose());
  }

  /**
   * Attach a terminal to an existing {@link TerminalSurface} — the multi-terminal entry point
   * (Epic #287 S7, #775).
   *
   * **What differs from {@link create}, and all of it follows from who composed the surface.** This
   * terminal claims no sole tenancy, so it neither sizes the shared drawing buffer nor ends the
   * surface when it is disposed; the host does both. It draws where
   * {@link JustermRenderer.setViewportRect} puts it, and until that is called it sits at the origin
   * — where a sole tenant also sits, which is what makes the single-terminal arrangement the special
   * case of this one rather than a second path through the code.
   *
   * Everything else is identical, deliberately: the widget experience is unchanged and the only new
   * noun is the surface (ADR-0021).
   *
   * The consumer still owns the DOM overlay — the hidden IME textarea, the a11y tree, the scrollbar
   * — and one canvas means every terminal shares one stacking plane, so arbitrary DOM cannot be
   * interleaved between two of them. Accepted knowingly: it is what one context binding to one
   * canvas costs.
   */
  static async attach(
    surface: TerminalSurface<RendererBackend>,
    opts: AttachedRendererOptions,
  ): Promise<JustermRenderer> {
    return JustermRenderer.build(surface, false, opts);
  }

  static async create(opts: JustermRendererOptions): Promise<JustermRenderer> {
    // The surface is composed HERE, which is what makes this the single-terminal convenience path
    // rather than the general one (#775): a host wanting two terminals builds the surface itself
    // with `TerminalSurface.open` and attaches to it with `attach` above. What this method composes,
    // the object it returns ends — see the `composedSurface` constructor parameter.
    // **Both wasm modules load in parallel**, as they did before the surface existed. Splitting the
    // old `Promise.all` across two objects made them serial, which is pure startup latency: the
    // decoder's import is started here and `build`'s `await` on it then resolves out of the module
    // registry. Stated because the parallelism is invisible at `build`'s call site.
    const [surface] = await Promise.all([
      TerminalSurface.open(opts.canvasSelector),
      import("justerm-wasm-decode"),
    ]);
    // What this method composes, this method ends — including on the failure path, which is the only
    // place the rule needs saying. A throw after the surface exists would otherwise strand a bound
    // WebGL2 context, a running density watcher and a canvas listener with no handle to any of them;
    // a retry on the same canvas gets the same context back and the orphan's listeners fire beside
    // the new surface's.
    try {
      return await JustermRenderer.build(surface, true, opts);
    } catch (e) {
      surface.dispose();
      throw e;
    }
  }

  /**
   * The one construction path both entry points take, so a difference between a sole tenant and a
   * shared one is a *parameter* rather than a second body that can drift from the first.
   */
  private static async build(
    surface: TerminalSurface<RendererBackend>,
    composedSurface: boolean,
    opts: AttachedRendererOptions,
  ): Promise<JustermRenderer> {
    // Dynamic import (see class doc for the init-race reason). Only the decoder now — the renderer
    // module is the surface's, since constructing it is what binds the context to a canvas.
    const decoder = await import("justerm-wasm-decode");
    const t = opts.theme;
    const paletteColors = decoder.buildPalette(Uint32Array.from(t.ansi));
    const backend = surface.rendererBackend();
    // A renderer arrives holding no terminal since 0.15.0, so this widget's single grid is created
    // here — and its font is named at birth rather than pushed by four setters afterwards. The four
    // selectors key the atlas, so this is **one** bake where the setter route was up to five, each
    // of the first four freed again by the next (#773).
    //
    // The values are the same ones the setters used, defaults included, so the initial fit is still
    // computed at the consumer's final cell.
    //
    const lease = surface.addGrid({
      paletteColors,
      defaultFg: t.defaultFg,
      defaultBg: t.defaultBg,
      fontFamily: opts.fontFamily,
      fontSize: opts.fontSize,
      letterSpacing: opts.letterSpacing ?? 0,
      lineHeight: opts.lineHeight ?? 1,
    });
    try {
      return await JustermRenderer.assemble(surface, composedSurface, opts, lease, decoder, paletteColors);
    } catch (e) {
      // A grid is GPU memory — a VAO, an instance buffer and a refcount on its configuration's atlas
      // (4.2 MiB at an 8x16 cell, 12.8 MiB at 15x30 on a dpr-2 display, measured for #773's follow-up).
      // Nothing holds it if assembly throws, and only `removeGrid` gives it back.
      lease.release();
      throw e;
    }
  }

  /**
   * Everything after the grid exists — the policy setters, the palette and flag tables, the instance
   * and the create-time options.
   *
   * Split from {@link build} for one reason: **it is the error boundary for the grid**. `build` owns
   * a registered grid from `addGrid` onward and has to give it back if anything here throws, and a
   * `try` around the whole remainder is only readable if the remainder is one call.
   */
  private static async assemble(
    surface: TerminalSurface<RendererBackend>,
    composedSurface: boolean,
    opts: AttachedRendererOptions,
    lease: GridLease,
    decoder: typeof import("justerm-wasm-decode"),
    paletteColors: Uint32Array,
  ): Promise<JustermRenderer> {
    const t = opts.theme;
    const backend = surface.rendererBackend();
    // Policy setters (consumer-injected, ADR-0017) — set once; they rarely change.
    backend.setBoldToBright(lease.id, t.boldToBright ?? true);
    backend.setMinimumContrastRatio(lease.id, t.minimumContrastRatio ?? 1);
    backend.setSelectionForeground(lease.id, t.selectionForeground);
    // Unconditional, and with the default named (#580): a `Theme` is a complete description, so
    // this has to push the same value `setTheme` pushes for an unset field — see
    // `DEFAULT_CURSOR_CONTRAST` for why that obliges this file to own a number the renderer already
    // has one of. No cursor exists yet, so there is nothing to redraw.
    backend.setCursorContrast(lease.id, t.cursorContrast ?? DEFAULT_CURSOR_CONTRAST);
    // Background opacity (#577). Set unconditionally at the renderer's own default, so the value the
    // renderer holds is the one this object states rather than one nobody wrote down. No `render`
    // here — nothing has been drawn yet, and the first frame presents it.
    backend.setBgAlpha(lease.id, opts.bgAlpha ?? 1);
    // Font family, size and both spacing options (#406/#413/#578) went into `addGrid` above.
    //
    // **The ordering question this block used to answer is gone with the four calls**, and the
    // answer is worth keeping because it is why moving them was safe. It once claimed font had to
    // precede spacing ("both derive the cell from the glyph metrics those establish") — a dependency
    // the renderer had already removed: every path that changes the glyph box, the DPR or either
    // spacing value funnels through one function reading all four together (`recompute_cell` up to
    // renderer 0.14.x, `bake_config` after, which builds a whole font *configuration* rather than a
    // cell, #772). Since 0.15.0 those four are simply the key that function is called with, so there
    // is no order left to get wrong — they arrive together or not at all.
    // Cursor stroke thickness (#580) — conditional, unlike the four pushes above, and for the same
    // reason `contextRestoreTimeout` is: their defaults are neutral identities (`0` / `1`) this file
    // costs nothing to restate, while `0.15` is a value borrowed from alacritty with a rationale the
    // renderer documents. Naming it here would make this the second owner of a number nothing
    // reconciles. An option can afford that where `cursorContrast` above cannot, because options are
    // read once here and never re-applied — there is no reset for an unset one to get wrong.
    if (opts.cursorThickness !== undefined) {
      backend.setCursorThickness(lease.id, opts.cursorThickness);
    }

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
    const instance = new JustermRenderer(
      backend,
      surface,
      composedSurface,
      lease,
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
    // The relay itself is registered with the renderer by the surface's constructor, unconditionally
    // and exactly once (#579) — it is surface-scoped, since one context means one loss. This only
    // installs the consumer's handler behind it.
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
    return { width: this.backend.cell_width(this.lease.id), height: this.backend.cell_height(this.lease.id) };
  }

  /** Change the font size (CSS px) at runtime (#406/#417) — re-bakes the atlas. The cell size moves,
   * so **the consumer must re-fit** (recompute its grid + `resize`) after calling. A no-op at the
   * current size. */
  setFontSize(cssPx: number): void {
    this.backend.setFontSize(this.lease.id, cssPx);
    this.reapplySurface();
  }

  /** Change the font family at runtime (#413/#417) — a CSS `font-family` string, re-bakes the atlas.
   * As with {@link setFontSize}, the cell size can move, so **the consumer must re-fit** after. Load
   * a webfont before an unfamiliar family (the browser silently falls back otherwise). */
  setFontFamily(family: string): void {
    this.backend.setFontFamily(this.lease.id, family);
    this.reapplySurface();
  }

  /**
   * Change the letter spacing (CSS px) / line height (multiplier `>= 1`) at runtime (#578). The live
   * counterparts of {@link JustermRendererOptions.letterSpacing} / {@link
   * JustermRendererOptions.lineHeight}, whose docs carry the units and the clamping.
   *
   * **The consumer must re-fit afterwards**, exactly as for {@link setFontSize}/{@link
   * setFontFamily}: call {@link resize} with the CSS box. These do not do it for you, because this
   * object has no reference to the fit — the widget and the consumer own that (the demo's
   * `setFontSize(); fit(); render();` is the shape, #417). Skipping it is not cosmetic: the **grid**
   * is then a column count derived from the old cell, so the terminal is fitted to a box it no
   * longer occupies. (The *buffer* half is handled — since renderer 0.15.0 these setters re-derive
   * it here, because the renderer stopped doing it; what they cannot derive is the grid, which needs
   * a container measurement this object does not hold.)
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
   * the whole change back to the previous spacing; and **what happens while the GL context is lost
   * depends on which renderer you are on**, so read the cell back rather than assuming either:
   *
   * - **renderer 0.14.x and earlier** — the cell moves immediately and the buffer does not.
   *   `adopt_spacing` ran `recompute_cell()` *before* its lost-context guard, so {@link cellSize}
   *   reported the new cell while the atlas re-bake and the drawing-buffer resize waited for
   *   `webglcontextrestored`. (This bullet used to say the cell "does not move at all", which was
   *   false against that ordering — corrected in #632.)
   * - **after that** — neither moves until the restore. The cell belongs to a *font configuration*
   *   since #772, and a setter arriving on a dead context cannot bake one, so it advances the
   *   selector and defers; `restore` re-selects from the surviving selectors and the cell lands
   *   then. This is the more consistent of the two, and it is the one #632's dedupe wants: there is
   *   no longer a window in which the cell has moved and the buffer has not.
   *
   * **#632's conclusion is unaffected either way**, which is worth stating because the comment it
   * corrected is the bullet above: {@link FitController} dedupes on the cell *and* the grid, and the
   * reason is that a cell change can leave the grid identical — true under both orderings.
   *
   * {@link cellSize} and
   * {@link terminalSize} are the truth afterwards — and `terminalSize` matters as much as the cell,
   * because the renderer's internal re-size adopts what the drawing buffer will actually grant (#339),
   * so a large enough cell shrinks the *grid* as well.
   */
  setLetterSpacing(cssPx: number): void {
    this.backend.setLetterSpacing(this.lease.id, cssPx);
    this.reapplySurface();
  }

  /** See {@link setLetterSpacing} — same cell-moving contract, same re-fit and read-back obligation. */
  setLineHeight(multiplier: number): void {
    this.backend.setLineHeight(this.lease.id, multiplier);
    this.reapplySurface();
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
    this.backend.setBgAlpha(this.lease.id, alpha);
    this.backend.render();
  }

  /**
   * Change the cursor's stroke thickness at runtime (#580) — a fraction of the cell width. The live
   * counterpart of {@link JustermRendererOptions.cursorThickness}, whose doc carries the full
   * contract (why a fraction, which shapes it reaches, and the renderer's clamp).
   *
   * **No re-fit**, unlike {@link setLetterSpacing}/{@link setLineHeight}: this reads the cell, it
   * does not move it, so the grid the consumer drives its engine at is unaffected.
   *
   * **Redraws only when a cursor is on screen**, matching {@link setCursorBlink} rather than
   * {@link setBgAlpha}: the thickness is a stroke uniform and reaches nothing else, so with the
   * cursor hidden (DECTCEM, or the blink's off phase) there is nothing for a present to change. It
   * is picked up by the next redraw either way.
   *
   * There is no `setCursorContrast` beside this. That knob is on {@link Theme}, so
   * {@link setTheme} is its runtime path — the same as every other policy that lives there.
   */
  setCursorThickness(frac: number): void {
    this.backend.setCursorThickness(this.lease.id, frac);
    if (this.cursor) this.redrawCursor();
  }

  /**
   * Adopt a new device pixel ratio (#325, consumer half of #322) — re-bake the atlas at the new
   * density and re-apply the canvas display box. **Called for you** by the widget's own resolution
   * watcher; a consumer needs this only to drive the path in a test, or to serve a density this
   * object cannot observe (a `window` it was not built against).
   *
   * **Two things move and neither of them is the renderer's to finish.** The renderer re-rasterises
   * at the new density and stops: since 0.15.0 it leaves the drawing buffer exactly as it was asked
   * for (a buffer holding N grids belongs to none of them, so it will not re-derive one), and it
   * never touches the DOM. So this method re-derives the buffer from the grid it is holding *and*
   * re-writes the canvas's CSS box from it — without the first the terminal would shrink by the
   * density ratio, and without the second the browser would scale a stale box. The latter is the
   * blur #322 exists to remove, reintroduced one layer out.
   *
   * **The CSS box can move, which is not obvious and is why this is not just a forward.** The device
   * cell is `round(metric * dpr)`, and dividing that back by the new ratio need not land on the old
   * CSS cell. Measured (font 16, 25x6 grid): CSS height `96` at dpr 1 and at dpr 1.5, but `99` at
   * dpr 2 — the cell is 33 device px there, and `33 / 2 = 16.5`.
   *
   * **Whether it moves is font dependent, so do not treat the numbers above as the contract.** It
   * turns on the fractional part of the metric, which differs per font — and equal cells at one
   * density say nothing about the next: this machine's font goes 19 -> 37 device px across dpr 1 -> 2
   * while CI's Linux font goes 19 -> 38, from the *same* 19. An e2e assertion that the box had moved
   * was red on CI for exactly that reason. What always holds, and is what to assert, is
   * `canvas.style x dpr === drawing buffer`.
   *
   * **No re-fit, deliberately.** The grid is left alone, so a terminal in a fixed container can end
   * up a few CSS px larger or smaller than the box that fitted it. Re-deriving the grid needs the
   * container's measurements, which this object does not hold — the consumer owns the fit (#417,
   * #578) — and the reference draws the same line: xterm.js's `handleDevicePixelRatioChange`
   * re-measures the char size, tells its renderer and repaints, and calls no `resize`
   * (`src/browser/services/RenderService.ts:279-290` @ `699f553`); its `FitAddon` stays manual. A
   * consumer that wants the grid re-derived calls {@link resize} with its current CSS box, exactly
   * as it already must after {@link setFontSize} or {@link setLetterSpacing}.
   *
   * A no-op at an unchanged ratio, and **dropped while the GL context is lost** — both inside the
   * renderer, so this is safe to call unconditionally.
   */
  setDevicePixelRatio(dpr: number): void {
    // Through the surface, because the density is the surface's: one canvas is one drawing buffer at
    // one `devicePixelRatio`, so this moves EVERY grid's cell, and the surface re-derives every
    // attached terminal rather than only the one whose consumer happened to call (#775).
    // Re-deriving and presenting are the SURFACE's now, and doing them here too would do both twice
    // per density change — two `resizeSurface` calls (each of which clears the buffer) and two
    // presents. The obligation itself is unchanged and still real: since renderer 0.15.0 the renderer
    // re-bakes the atlases and leaves every device-px measurement exactly as it was given, because
    // those are the consumer's and it will not convert them through its own copy of the density — a
    // copy that lags, since this very notification is dropped while the context is lost (#773).
    // Without the re-ask the buffer would stay put while `cssWidth()` divided it by the new ratio,
    // so a move to a denser monitor would *halve* the displayed terminal. What changed is only WHO
    // pays it: every attached terminal, not just the one whose consumer happened to call.
    this.surface.setDevicePixelRatio(dpr);
  }

  /**
   * Install (or clear, with `undefined`) the handler called when a lost WebGL context has not come
   * back within {@link setContextRestoreTimeout} (#579). The live counterpart of
   * {@link JustermRendererOptions.onContextLoss}, whose doc carries the full contract — what the
   * signal means, what it does *not* mean, and why the widget applies no policy of its own.
   *
   * **This reaches the whole SURFACE, not just this terminal** (#775), and on a shared one that
   * matters: there is one context, so there is one loss and one notification. A second terminal
   * calling this — or attached with `onContextLoss` in its options — **replaces** the first
   * terminal's handler for the entire canvas, last call wins, with no diagnostic. A host driving
   * several terminals should register once, on the surface.
   *
   * **Nothing is re-registered with the renderer here.** The surface holds one relay for the life of
   * the *surface*, because `setOnContextLoss` takes a `Function` and offers no unset; this swaps the
   * handler behind it. That is what makes clearing expressible at all, and it is why a swap cannot
   * leave the renderer holding a stale closure.
   *
   * **No redraw**, unlike {@link setCursorBlink} / {@link setBgAlpha}: this changes who is told
   * about a future event, not anything currently on screen.
   */
  setOnContextLoss(handler: (() => void) | undefined): void {
    this.surface.setOnContextLoss(handler);
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
    this.surface.setContextRestoreTimeout(ms);
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
    return this.surface.isContextLost();
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
    return this.surface.isRestoreOverdue();
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
    this.backend.setPalette(this.lease.id, colors, theme.defaultFg, theme.defaultBg);
    this.backend.setBoldToBright(this.lease.id, theme.boldToBright ?? true);
    this.backend.setMinimumContrastRatio(this.lease.id, theme.minimumContrastRatio ?? 1);
    this.backend.setSelectionForeground(this.lease.id, theme.selectionForeground);
    // The cursor guard travels with the theme (#580), and it has to: what it defends against is a
    // `cursorColor` too close to the cell under it, and this call is the one that just moved both.
    // Omitting it from a theme RESETS it, like every other field here — that completeness is what
    // `DEFAULT_CURSOR_CONTRAST` exists for.
    this.backend.setCursorContrast(this.lease.id, theme.cursorContrast ?? DEFAULT_CURSOR_CONTRAST);
    this.issueOverlay(); // the selection/match blend colours moved
    this.redrawCursor(); // re-push the cursor with its new colour, then present (one pack, #421)
  }

  /** Fit a `cols`×`rows` grid to a CSS-pixel box and size the renderer + canvas display box to
   * it. Unlike beamterm (which took CSS px and computed the grid itself), the renderer takes a
   * grid, so the adapter divides here (pixel→cell is consumer policy) and sets the canvas CSS box
   * from what the renderer reports it must be (`cssWidth`/`cssHeight`) — forget that and the
   * device-px buffer displays at twice its size on a Retina screen.
   *
   * **A call that lands while the GL context is lost is provisional** (#717). The renderer commits
   * the buffer you asked for but defers reading it back, because a dead context answers `0` and
   * adopting that would floor the surface to one pixel (#639). That read — and therefore any browser
   * clamp (#339) — settles inside `restore()`, which runs on the next {@link render}, not when
   * `webglcontextrestored` fires.
   *
   * **It no longer has to be repeated, and that changed in this package rather than in the
   * renderer.** The `webglcontextrestored` handler now renders (which runs `restore` and settles the
   * clamp), then re-derives the buffer and re-writes the display box from what was actually granted.
   * So the provisional numbers are replaced without a consumer call.
   *
   * Measured on **renderer 0.14.x**, where nothing did that (headless Chromium, `MAX_TEXTURE_SIZE`
   * 8192, cell 9 device px), asking for 4000 columns during a loss — kept because it is the shape of
   * the failure, and because the middle column is still what happens:
   *
   * | | grid | `cssWidth()` | `canvas.style.width` |
   * |---|---|---|---|
   * | during the loss | 4000 | 36000 | `36000px` |
   * | after the restoring `render()` | **910** | **8190** | `36000px` ← now `8190px` |
   *
   * The display box described a buffer 4.4x wider than the one that existed and the browser
   * stretched to fit. Reachable only when the requested grid exceeds the browser's buffer limits, so
   * most consumers will never see either version. */
  resize(cssWidth: number, cssHeight: number): void {
    const grid = gridForBox(
      cssWidth,
      cssHeight,
      this.backend.cssCellWidth(this.lease.id),
      this.backend.cssCellHeight(this.lease.id),
    );
    // Nothing to propose — an unmeasured cell or a non-finite box (#632). Leave the renderer and the
    // canvas box exactly as they are: resizing to a guess is how an unlaid-out container turned into
    // a 1x1 terminal, and the CSS box below must not describe a buffer we did not ask for.
    if (!grid) return;
    this.applyGrid(grid.cols, grid.rows);
  }

  /**
   * Give the renderer a `cols`×`rows` grid **and** the surface to draw it on, then re-apply the
   * canvas display box.
   *
   * **This is what `backend.resize(cols, rows)` was until renderer 0.15.0**, assembled here because
   * the renderer stopped being able to do it. A drawing buffer shared by N grids in M font
   * configurations has no cell it can be a multiple of, so it is sized in device px by whoever knows
   * which grid it is holding (#773). This widget holds exactly one, so it can — and every obligation
   * the renderer handed back is discharged in this one method rather than at each of its callers.
   *
   * **It asks for `cols * cell_width(grid)` rather than scaling a CSS box by the ratio**, and that
   * is #331's exactness kept rather than re-derived: both are integers the renderer hands back, so
   * nothing rounds between the grid the shader lays out and the buffer that has to hold it. The
   * browser may still grant less (#339), which is why the display box is written from `cssWidth()`
   * afterwards rather than from the numbers asked for.
   */
  private applyGrid(cols: number, rows: number): void {
    this.backend.resizeGrid(this.lease.id, cols, rows);
    // Sizing the shared buffer is the SOLE TENANT's alone (#775). Asking for `cols * cell` is #331's
    // exactness — both are integers the renderer hands back, so nothing rounds between the grid the
    // shader lays out and the buffer holding it — and it is available only while this grid is the
    // one thing on the canvas. A terminal sharing a surface leaves the buffer to whoever measured
    // the container, and takes its rect from `setViewportRect` instead.
    if (this.composedSurface) {
      this.surface.resizeSurface(
        cols * this.backend.cell_width(this.lease.id),
        rows * this.backend.cell_height(this.lease.id),
      );
    }

    // **Read the grant back and adopt it** (#339). WebGL is free to give a smaller drawing buffer
    // than asked for, and until renderer 0.15.0 the renderer read that back itself and shrank the
    // grid — which is what made {@link terminalSize} "the grid actually adopted". It cannot now: a
    // buffer belongs to no grid, so it clamps the *surface* and leaves the grid saying what it was
    // told. This widget is the one place holding both the grant (`cssWidth`) and the grid, so the
    // read-back lands here, and `justerm-web`'s contract is unchanged across the renderer break.
    //
    // Without it the failure is silent in the way this repo treats as worst: nothing errors, the
    // grid keeps the columns it asked for, and the cells past the buffer's edge are clipped by the
    // scissor — drawn nowhere, with `terminalSize()` still reporting them.
    //
    // `cssWidth()` is the granted buffer, in CSS px, which is the space `gridForBox` divides in. On
    // a lost context it reports the *committed* request rather than a grant (the read-back is
    // deferred to the restore, #639), so this shrinks nothing then — which is right, and the restore
    // path re-runs this whole method.
    //
    // **It protects a SOLE TENANT, and only that** (#775). `cssWidth()` is the whole canvas while
    // `cols` is this tenant's share of it, so for a pane smaller than the surface the comparison is
    // between quantities of different scope and the clamp cannot fire: measured on the two-terminal
    // drive, a 450 CSS-px pane at an 8 CSS-px cell fits 56 columns while this computes 112. That is
    // not a wrong answer for a sole tenant, where the two are the same box by construction — it is a
    // check that has nothing to say about a shared one. The grant on a shared surface belongs to
    // whoever asked for the buffer: the host reads `TerminalSurface.cssSize()` back after
    // `resizeSurface` and re-places its panes. Where the per-grid check should live once a host
    // actually tiles is an open question, deliberately not answered here.
    const granted = gridForBox(
      this.backend.cssWidth(),
      this.backend.cssHeight(),
      this.backend.cssCellWidth(this.lease.id),
      this.backend.cssCellHeight(this.lease.id),
    );
    if (granted !== undefined && (granted.cols < cols || granted.rows < rows)) {
      this.backend.resizeGrid(this.lease.id, granted.cols, granted.rows);
    }

    // **A hidden terminal is placed nowhere, and this is the only site that can enforce that**
    // (#801). Every path that re-derives a placement arrives here, so consulting `hidden` once
    // covers all seven — including the two that carry no consumer call at all, a density change and
    // a context restore, which is precisely where a one-shot `clearViewport` would have been undone
    // without anyone calling anything.
    //
    // `resizeGrid` above still ran, deliberately: a host may re-fit a hidden pane, and the grid has
    // to adopt it so that coming back stays a placement rather than a resize-and-repack. What is
    // withheld is only the rect.
    if (this.hidden) {
      this.backend.clearViewport(this.lease.id);
      this.present();
      return;
    }

    // A grid draws only where it is placed, and for a one-terminal widget that is the whole buffer.
    // Re-issued on every call because a rect is device px: the cell may have just moved under it,
    // and the grid may have just been shrunk to the grant.
    const { cols: fitted, rows: fittedRows } = granted ?? { cols, rows };
    this.backend.setViewport(
      this.lease.id,
      this.rect.x,
      this.rect.y,
      Math.min(cols, fitted) * this.backend.cell_width(this.lease.id),
      Math.min(rows, fittedRows) * this.backend.cell_height(this.lease.id),
    );
    this.present();
  }

  /**
   * Present, because a placement change is **a change to what is on screen** and nothing else on
   * this path will draw it (#801).
   *
   * Found by looking at the compositor rather than at the drawing buffer, which is the only
   * instrument that can see it: every reading in this package's own probes is taken after a forced
   * `present()`, so a `readPixels` assertion is structurally blind here. On an idle two-terminal page
   * — timers stopped, which is exactly the state a host is in when the user has just switched tabs —
   * hiding a pane left its pixels on the shared canvas, and showing one left it unpainted, until some
   * unrelated event happened to present.
   *
   * The rule it violated is this file's own, stated at {@link setOnContextLoss}: a call that changes
   * *"who is told about a future event, not anything currently on screen"* owes no redraw — and by
   * that division a call that moves a grid onto or off the buffer plainly does owe one.
   *
   * It sits at the end of {@link applyGrid} rather than in `hide` / `setViewportRect` because that is
   * where all seven placement paths already meet, and the same argument covers a rect that merely
   * moved: an overlay dragged across the canvas with no frame behind it had the same gap before this
   * change, and it is repaired by the same line. {@link render}'s existing split is what is reused —
   * a sole tenant presents now, a shared tenant coalesces into the surface's one frame — so N
   * terminals re-placed inside one host handler still cost one present.
   */
  private present(): void {
    this.render();
  }

  /**
   * Re-derive the drawing buffer from the grid this widget is already holding, at whatever the cell
   * has just become — the response to anything that moves the cell without moving the grid.
   *
   * Until renderer 0.15.0 the renderer did this itself: every font, spacing and density path ended
   * by re-deriving the buffer from the grid it was holding. It cannot any more — a surface belongs
   * to no grid — so the obligation came back to the consumer, and this widget *is* the consumer.
   * What stays the renderer's is the cell; what stays the application's is the **grid**, which is
   * why {@link setFontSize} and friends still say "re-fit" and still mean the column count.
   *
   * A no-op before the first {@link resize}: a grid is born `0`x`0`, and `resizeGrid` would floor
   * that to one cell — turning a density change that arrives between `create` and the first fit into
   * a one-cell canvas. (The old renderer had no such window: it seeded its implicit grid from the
   * canvas attributes.)
   */
  private reapplySurface(): void {
    const { cols, rows } = this.terminalSize();
    if (cols > 0 && rows > 0) this.applyGrid(cols, rows);
  }

  /**
   * Place this terminal on the shared canvas: the top-left of its viewport, in **device px**
   * (#775). For a terminal sharing a surface with siblings — a sole tenant sits at the origin and
   * never calls this.
   *
   * **The extent is not a parameter, and that is the point.** A viewport's size is `cols * cell` and
   * `rows * cell`, both integers the renderer hands back, so passing a measured box would reintroduce
   * the rounding #331 exists to prevent. What only the host can know is *where* the box is; what only
   * the renderer can know is how big a grid of cells is. So this takes the first and derives the
   * second — pair it with {@link resize} to change how many cells fit.
   *
   * **The host owes this call whenever the overlay's box moves** — a scroll, a layout change, a pane
   * drag — and nothing detects a missed one: the GL viewport simply stays where it was while the DOM
   * overlay (the hidden textarea, the a11y tree, the scrollbar) moves off it. That asymmetry is the
   * forced consequence of one context being bound to one canvas, and it is accepted knowingly
   * (ADR-0021).
   *
   * **And it is owed after a density change, where the box has not moved at all.** A rect is device
   * px, so ADR-0021 D3 invalidates it along with every other device-px quantity the host gave — *"the
   * surface's size as well as every viewport rect, since only the consumer can re-measure them"*.
   * Nothing here can pay that: this object re-issues the rect it was last given, and scaling it by a
   * density it holds a copy of is exactly the conversion the renderer refuses one layer down, for the
   * same reason. Register {@link TerminalSurface.onDensityChange} and re-supply — which also covers
   * the density a **context restore** adopts on its own, since a notification arriving during a loss
   * is dropped rather than queued and the restore re-reads the live ratio (#808). A sole tenant
   * re-derives its own buffer there and needs none of this; a shared one has no other notice.
   */
  setViewportRect(x: number, y: number): void {
    this.rect = { x, y };
    // Giving a rect IS showing (#801) — the same field {@link hide} and {@link show} write, so no
    // pair of bits can disagree about one overlay (the shape #805 reached one issue earlier). A
    // shared tenant returning into a layout goes through here rather than through `show`, because it
    // has to re-supply the origin its missing box took away.
    this.setHidden(false);
  }

  /**
   * Take this terminal off the surface **without ending it** (#801) — the hidden-tab state.
   *
   * Every byte survives: the grid stays registered, its packed instances and upload baseline stay
   * resident, and its font configuration's atlas is not released. Coming back is
   * {@link setViewportRect} — a placement, which re-packs once from the state the grid already had.
   * That is the payoff ADR-0021 states in as many words, and until this method existed a host with a
   * hidden tab had exactly one option: {@link dispose} the terminal and rebuild it on the way back,
   * which is the rebuild Epic #287 exists to remove.
   *
   * **Hiding the DOM overlay is not this, and cannot be** — measured in a real browser rather than
   * reasoned. One WebGL context binds to one canvas, so a terminal's pixels are on the *shared*
   * canvas and not in its overlay: `visibility: hidden` leaves the terminal fully drawn and fully
   * paid for, and `display: none` is worse, because the box it removes used to be re-read as the
   * origin `{ 0, 0 }` and re-placed this grid, at full size, on top of a sibling. That path is
   * closed at its source ({@link viewportOrigin} answers `undefined` for a box with no area), and
   * this is what a host wires it to.
   *
   * Idempotent, and a no-op before the first {@link resize}: a grid with no cells is drawn nowhere
   * already. {@link show} is the way back.
   */
  hide(): void {
    this.setHidden(true);
  }

  /**
   * The one writer of {@link hidden}, so the *transition* has a single site (#801).
   *
   * Setting the field is the easy half; what needs a home is what happens on the way **back**.
   * {@link blinkTick} does no work while hidden, so the phase the renderer holds can drift away from
   * the live clock — a terminal hidden with its blinking cells lit and shown again after the phase
   * flipped would keep drawing them lit until the next flip, up to a whole interval later. Re-syncing
   * once on the true → false edge costs nothing on any other path, which is why it is keyed on the
   * edge and not on the value: {@link setViewportRect} runs through here on every scroll of an
   * ancestor, and re-packing the grid on a scroll would be a new cost in the hot path.
   */
  private setHidden(next: boolean): void {
    const was = this.hidden;
    this.hidden = next;
    this.reapplySurface();
    if (was && !next) {
      // Order matters: the grid is placed by `reapplySurface` above, and `repackAtTextBlinkPhase`
      // refuses while the renderer's grid disagrees with the last frame's.
      this.syncTextBlinkPhase();
      if (this.cursor) this.redrawCursor();
    }
  }

  /**
   * Draw this terminal again, at the rect it already holds — the inverse of {@link hide}.
   *
   * **A sole tenant is why this exists rather than {@link setViewportRect} being the only way back.**
   * That method also shows, and for a *shared* tenant it is the natural call, since a pane returning
   * into a layout has to re-supply its origin anyway — its box is what was taken away. But a sole
   * tenant sits at the origin and is told, on `setViewportRect` itself, that it never calls it. So
   * without this the single-terminal widget — the `create` path, which is what every consumer of this
   * package uses today — could enter the hidden state and leave it only by calling the method its own
   * documentation forbids it. Two doc-comments in this file contradicting each other is the tell, and
   * on this layer the tie-breaker is our own API's internal coherence rather than any reference.
   *
   * A shared tenant that has *moved* while it was away must still call {@link setViewportRect}: this
   * re-places at the last origin given, and a stale origin is exactly the wrong answer the whole
   * `undefined` union upstream exists to prevent. Wiring `observeViewportRect` covers that by
   * construction — a returning box fires it.
   *
   * Idempotent, and a no-op before the first {@link resize}.
   */
  show(): void {
    this.setHidden(false);
  }

  /**
   * Whether this terminal is currently drawn — the renderer's own answer, not a mirror of
   * {@link hide} (#801).
   *
   * Asked of the registry rather than reported from the field above, because the two can differ in
   * the one direction that matters: a grid is registered **not drawn** until its first
   * {@link resize} places it, so a terminal that has never been sized answers `false` here while
   * nothing has hidden it. Reporting the field would answer `true` and be wrong on exactly the case
   * a host uses this to check.
   */
  isDrawn(): boolean {
    return this.backend.isGridDrawn(this.lease.id);
  }


  /** The terminal grid ACTUALLY adopted after the last {@link resize} — not the requested
   * `cols`/`rows`, so a browser drawing-buffer clamp (#339) cannot desync the grid the consumer
   * drives its engine and frames at from the grid the buffer can hold.
   *
   * **That guarantee belongs to a terminal that composed its surface** (#775) — held by
   * construction rather than enforced, see `composedSurface`. A terminal sharing a surface occupies part of a
   * buffer it did not ask for, so the read-back in {@link applyGrid} compares its columns against the
   * whole canvas and never shrinks it; the grant is the host's to read from
   * {@link TerminalSurface.cssSize} after it sizes the surface. This still answers what `resizeGrid`
   * was last given either way — what varies is whether anything clamped it first.
   *
   * **Who adopts it moved at renderer 0.15.0, and this contract did not** (#773). The renderer used
   * to shrink the grid to what the buffer granted; it clamps only the shared *surface* now, because
   * a buffer holding N grids belongs to none of them. So {@link resize} reads the grant back and
   * shrinks the grid here instead — which is why this still answers what it always did. */
  terminalSize(): { cols: number; rows: number } {
    return { cols: this.backend.cols(this.lease.id), rows: this.backend.rows(this.lease.id) };
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
    this.backend.setDecorations(this.lease.id, decorationWire(this.decorationSource?.(frame) ?? []));
    this.updateCursor(frame);
    // Pack at the CURRENT text-blink phase, not forced-on — same reason `updateCursor` draws at
    // the cursor's current phase: a content frame arriving during the off phase must not flash the
    // blinking cells back on until the loop's next flip. `apply_damage` stores this as the
    // renderer's `last_blink_on`, so the two stay in step by construction.
    const textBlinkOn = this.textBlink.isVisible(now());
    this.backend.apply_damage(this.lease.id,
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

  /**
   * Present the canvas — and **which of the two ways follows from whether this terminal is alone on
   * it**, because `render()` takes no grid: one call presents the whole canvas, every registered grid
   * included.
   *
   * - **Sole tenant** (the {@link create} path): synchronous, exactly as before #775. One terminal
   *   means one present per frame either way, so coalescing buys nothing and would only add a frame
   *   of latency — and the whole e2e suite drives a frame and reads pixels in the same turn, so
   *   deferring here would silently change what a large number of unrelated assertions mean.
   * - **Sharing a surface** (the {@link attach} path): a *request* on the surface's loop, coalesced
   *   with every sibling's into one present per frame. N terminals presenting synchronously would
   *   redraw the whole canvas N times a frame — a cost that grows with the number of terminals while
   *   the pixels do not.
   *
   * The two are the same behaviour at N=1, which is what makes this a derivation rather than a mode
   * switch. `Terminal` calls this on every decoded frame and has no way to choose, so a widget on a
   * shared surface would otherwise be unable to reach the loop the surface exists to run — the
   * advice "call `requestRender` instead" was unfollowable while `Terminal.mount` drove this one.
   *
   * A host that needs the canvas drawn *before* it returns — reading pixels, a screenshot — calls
   * {@link TerminalSurface.present} directly.
   */
  render(): void {
    if (this.composedSurface) this.surface.present();
    else this.surface.requestRender();
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
    this.backend.setOverlay(this.lease.id,
      this.lastSelectionSpans,
      this.lastMatchSpans,
      this.activeSelectionBg(),
      this.matchBg,
    );
    this.backend.setActiveMatch(this.lease.id, this.lastActiveMatchSpans, this.activeMatchBg);
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
      this.backend.clearCursor(this.lease.id);
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
      this.backend.setCursor(this.lease.id,
        this.cursor.col,
        this.cursor.row,
        this.cursor.shape,
        this.cursorColor,
        this.cursorTextColor,
      );
    } else {
      this.backend.clearCursor(this.lease.id);
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
    const caretCol = this.backend.setPreedit(this.lease.id, col, row, codepoints);
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
    if (!grid || grid.cols !== this.backend.cols(this.lease.id) || grid.rows !== this.backend.rows(this.lease.id)) {
      return false;
    }
    this.backend.apply_damage(this.lease.id,
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
    // **A terminal nobody is drawing has no phase worth flipping** (#801). Both halves of this tick
    // end in `backend.render()`, which presents the WHOLE canvas — so on a shared surface a hidden
    // pane's blink drives a full redraw of its siblings, twice a second, for pixels that are not on
    // screen. The re-pack itself is already gated one layer down (the renderer's draw loop skips an
    // unplaced grid), which is exactly why this was invisible: what is wasted is the present, not
    // the pack, and no counter at this layer reports presents.
    //
    // The cursor half self-gated on the ordinary path and that is what made this look narrower than
    // it is: `CursorBlink.isVisible` returns solid when `!focused`, and `display: none` blurs the
    // focused textarea. But `TextBlink.isVisible` has no focus gate at all, and `hide()` called
    // without a DOM change — which this package's README recommends for `visibility: hidden`, since
    // no observer fires there — leaves the terminal focused. So the path that needed this is the one
    // the documentation points at.
    //
    // Skipping the work is what makes the phase drift; `setHidden` re-syncs on the way back.
    if (this.hidden) return;
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
   * **It releases this widget's grid, and with it the GPU memory that grid was holding.** The wasm
   * instance itself, the GL context and the canvas context-loss listeners the Rust side owns still
   * survive — those go with the binding's `free()`, which cannot be called while the consumer holds
   * this object.
   *
   * This paragraph said *"stops work, does not release memory"* until #773's follow-up, and its
   * stated reason — that `free()` is the only release and is unreachable — **stopped being true at
   * #770**, which added `removeGrid`. A promise whose grounds have gone is a thing to fix rather
   * than to keep. What it costs to keep it is measured: the glyph atlas is a fixed
   * `tex_storage_3d(RGBA8, paddedW, paddedH * 32, 192)` allocation whose size does not depend on how
   * many glyphs were ever used — **4.2 MiB** at a 8x16 cell and **12.8 MiB** at the 15x30 cell
   * measured at dpr 2 — plus a VAO, an instance buffer, and the rasteriser and glyph cache on the
   * wasm heap. A tabbed application that closes terminals while the page lives (which is the first
   * consumer's shape) held all of it, per closed terminal, until the tab went away.
   *
   * **So a disposed widget has no grid, and every method that acts on one throws afterwards** —
   * `cellSize`, `terminalSize`, `resize`, the font and spacing setters, the frame and cursor paths.
   * That is the honest answer rather than a regression: they would otherwise report or mutate a
   * terminal that has ended. The surface-level readers below keep answering, because the surface is
   * still there.
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
    // No flag here any more (#805). This widget used to carry a `gridReleased` boolean whose stated
    // reason was that `removeGrid` throws on an id it does not know — true when it was written, and
    // false by the time #775 merged, because the guard had moved into the surface. It survived on a
    // reason that had gone, which is exactly what a lease removes: `release` is idempotent because
    // the lease knows its own state, so a second `dispose()` needs nothing to remember for it.
    //
    // It releases THIS grid and nothing else — a sibling on the same surface keeps its cells, its
    // atlas and its viewport (#775).
    this.lease.release();
    // What this object exclusively holds, this object ends — the rule in
    // `docs/map/invariant/a-layer-ends-what-it-exclusively-holds.md`, which is where it is written
    // down rather than here. On the `create` path this object is the surface's only holder; on the
    // `attach` path it is not, and ending a shared surface would take down every sibling drawing on
    // it.
    if (this.composedSurface) this.surface.dispose();
  }
}
