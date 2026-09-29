import type { Palette } from "justerm-wasm-decode/colors.js";
import { CursorBlink, type CursorStyle, cursorCommand, resolveCursorShape } from "./cursor";
import { FrameLoop } from "./frame-loop";
import { type DecorationRect, decorationWire } from "./decorations";
import { MINIMUM_COLS, MINIMUM_ROWS, gridForBox } from "./fit";
import {
  TerminalSurface,
  type FontWeight,
  type GridLease,
  type SurfaceBackend,
} from "./terminal-surface";

import type { Renderer } from "./renderer";
import {
  asU16,
  asU32,
  blinkPhaseHeader,
  carriesBlink,
  damageHeader,
  retainU32,
} from "./renderer-wire";
import { TextBlink } from "./text-blink";
import type { DecodedFrame, FlagBits, UnderlineStyle, UnderlineStyles } from "./types";

/**
 * The two decoder members the widget **carries** rather than lets a consumer re-import (#862).
 *
 * Extracted so it can be tested: the wiring is one the type system cannot check, so the test
 * checks it by **reference identity**. Generic in the value map so a test can hand it a plain object
 * with no cast. Why: `docs/map/territory/published-surface.md` § The decoder members the widget
 * carries.
 */
export function cellStyleContext<S>(decoder: {
  underlineStyle: (flags: number) => UnderlineStyle;
  UnderlineStyle: S;
}): { underlineStyleOf: (flags: number) => UnderlineStyle; styleValues: S } {
  // Free functions, no receiver — taken by reference rather than wrapped.
  return { underlineStyleOf: decoder.underlineStyle, styleValues: decoder.UnderlineStyle };
}

/** Theme colours (packed `0xRRGGBB`). The engine stays ignorant of these — the consumer owns them
 * and the renderer resolves cell refs against them.
 *
 * **A theme is a complete description, not a patch**: {@link JustermRenderer.setTheme} pushes
 * **every** member, so an unset one *resets* to its default rather than keeping whatever the
 * previous theme set. Why, and what each member's placement rests on:
 * [`docs/map/territory/colour-policy.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/colour-policy.md) § `Theme` is colours plus the
 * two colour policies. */
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
   * `activeMatchBackground`, painted above selection and the other matches.
   * Defaults to a dark orange, distinct from both {@link selectionBg} and
   * {@link matchBg}.
   * On a cell that is both selected and the active match, {@link
   * selectionForeground} paints over THIS background (#430, xterm's channel
   * independence) — pick the two to read on each other, or set {@link
   * minimumContrastRatio} (it corrects against the final composited bg). */
  activeMatchBg?: number;
  /** Selection background when the terminal is UNFOCUSED (`0xRRGGBB`). xterm's
   * selectionInactiveBackgroundOpaque; a dimmer tint. Defaults to a muted slate. */
  selectionInactiveBg?: number;
  /** Optional fg for SELECTED cells (`0xRRGGBB`), xterm's `selectionForeground`. Unset
   * keeps each cell's own fg. Selection-only (never a search match), focus-independent.
   * Selection is a property of the cell, not of the bg winner: where the ACTIVE search
   * match covers a selected cell, this fg paints over {@link activeMatchBg} — pick the two to
   * read on each other, or set {@link minimumContrastRatio}. */
  selectionForeground?: number;
  /** Minimum fg/bg contrast ratio (WCAG, 1..21). Defaults to 1 (off, like xterm) (#225).
   *
   * Corrects against the background the cell composites to *within the canvas* — a highlight or
   * decoration bg wins over the cell's own. It does **not** account for
   * {@link JustermRendererOptions.bgAlpha}: the correction runs on the nominal opaque colour, so
   * under a translucent background the real contrast against whatever is behind the canvas may be
   * lower than the ratio asked for — the references share that limit ([`docs/map/territory/colour-policy.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/colour-policy.md)
   * § Minimum contrast is a faithful port). */
  minimumContrastRatio?: number;
  /** Minimum WCAG contrast between the **cursor** and the cell it sits on (#580, consumer half of
   * #368). Below it the cursor inverts to the terminal's default fg/bg, so a {@link cursorColor}
   * that happens to match the cell underneath never makes the caret vanish. Defaults to
   * `1.5` ({@link DEFAULT_CURSOR_CONTRAST}); pass `1` — the floor of the ratio range — to switch the guard
   * off, which is xterm.js's behaviour (it has no cursor guard at all).
   *
   * **A separate knob from {@link minimumContrastRatio}, deliberately.** That one corrects a cell's
   * *text* against its background; this one rescues an *overlay* against the cell it covers. They
   * run on different comparands and either can be set without the other.
   *
   * Out-of-range values are the renderer's to clamp (`[1, 21]`) and are not re-clamped here. Its
   * runtime path is {@link JustermRenderer.setTheme}, like every other policy on this interface. */
  cursorContrast?: number;
  /** Draw bold text in the bright (8-15) ANSI colour — xterm's
   * drawBoldTextInBrightColors. Defaults to true (xterm's default). */
  boldToBright?: boolean;
}

/**
 * The cursor-contrast threshold applied when {@link Theme.cursorContrast} is unset — the renderer's
 * own default (alacritty's `MIN_CURSOR_CONTRAST`), restated here because a `Theme` field must
 * reset to a named value. Why restated, and the drift it risks:
 * [`docs/map/territory/colour-policy.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/colour-policy.md) § `Theme` is colours plus the
 * two colour policies.
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
   * The weight regular text is drawn at, and the weight bold (SGR 1) text is drawn at. Omit
   * for `"normal"` / `"bold"`. Change them at runtime with {@link JustermRenderer.setFontWeight} /
   * {@link JustermRenderer.setFontWeightBold}; a weight the renderer refuses leaves the default.
   *
   * Neither moves the cell: it is measured at `"normal"` whatever these are.
   */
  fontWeight?: FontWeight;
  fontWeightBold?: FontWeight;
  /**
   * Draw text with per-channel (LCD / subpixel) coverage where the browser produces it, which reads
   * sharper on a subpixel display. Omit for `false` (grayscale). Change it at runtime with
   * {@link JustermRenderer.setSubpixelAntialiasing}.
   *
   * It applies only where the cell's background is opaque: under a {@link JustermRendererOptions.bgAlpha} below 1, cells on
   * the default background keep grayscale, since one alpha cannot carry three coverages. It does not
   * move the cell.
   */
  subpixelAntialiasing?: boolean;
  /**
   * Force the cursor to blink (`true`) or stay steady (`false`), overriding the application.
   * Omit (or `undefined`) to **follow the application's** DECSCUSR / `CSI ?12` mode, which is the
   * default and what both references default to.
   *
   * Change it at runtime with {@link JustermRenderer.setCursorBlink}.
   */
  cursorBlink?: boolean;
  /**
   * The caret shape drawn while the application has not chosen one. DECSCUSR
   * (`CSI Ps SP q`) overrides it, and `CSI 0 SP q`, DECSTR and RIS hand it back. Omit for `"block"`.
   * Change it at runtime with {@link JustermRenderer.setCursorStyle}.
   */
  cursorStyle?: CursorStyle;
  /**
   * How long the cursor keeps blinking with no user input before parking solid, in ms.
   * `0` disables the timeout. Omit for the default — 5 minutes, xterm.js's `CURSOR_BLINK_IDLE_TIMEOUT`.
   *
   * Change it at runtime with {@link JustermRenderer.setCursorBlinkTimeout}.
   */
  cursorBlinkTimeout?: number;
  /**
   * The cursor's stroke thickness as a **fraction of the cell width** (#580, consumer half of
   * #369) — the width of a bar, an underline, or a hollow block's outline. Omit for the renderer's
   * default, `0.15` (alacritty's `cursor.thickness`).
   *
   * **A block ignores it.** A block cursor recolours its cell and draws no stroke, so this changes
   * nothing for a block — a bar or underline (DECSCUSR, or {@link cursorStyle}) shows it.
   *
   * The renderer resolves it as `(frac * cell_w).round().max(1)` device px, so it tracks dpr **and**
   * font size, and even `0` leaves a one-pixel stroke rather than no cursor. Out-of-range values are
   * the renderer's to clamp (`[0, 1]`) and are not re-clamped here. Change it at runtime with
   * {@link JustermRenderer.setCursorThickness}.
   */
  cursorThickness?: number;
  /**
   * The half-period of the **SGR 5 (blink) text** phase, in ms. Omit (or `0`) to leave
   * blinking text steadily shown, which is the default.
   *
   * `prefers-reduced-motion` pins the text visible whatever is set here. Change it at runtime with
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
   * background, and the cursor cell, stay opaque, which keeps coloured output readable over an
   * arbitrary desktop.
   *
   * **Widget chrome is not alpha-aware.** The scrollbar thumb defaults to a translucent white
   * (`rgba(255,255,255,0.25)`) over a track with no background of its own, which reads well against
   * an opaque terminal and may not against a light desktop showing through at a low `bgAlpha`. If you
   * build that combination, set the thumb's colour from your side through the
   * `--justerm-scrollbar-thumb` custom properties that `Scrollbar` reads.
   *
   * Change it at runtime with {@link JustermRenderer.setBgAlpha}.
   */
  bgAlpha?: number;
  /**
   * Extra space between columns, in **CSS pixels**. Defaults to `0`.
   *
   * CSS px, not device px, because {@link fontSize} is — one font description speaks one unit
   * ([ADR-0023](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0023-spacing-settings-are-css-pixels.md)).
   * The renderer applies `round(letterSpacing * dpr)`.
   *
   * May be **negative**, which narrows the cell and crops the glyph rather than condensing it.
   *
   * **Moves the cell**, so see {@link JustermRenderer.setLetterSpacing} for the re-fit obligation
   * this creates — at `create` time it is free, because the first fit has not run yet.
   */
  letterSpacing?: number;
  /**
   * A multiplier on the glyph height, `>= 1`. Defaults to `1`.
   *
   * Unitless by construction, which is why [ADR-0023](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0023-spacing-settings-are-css-pixels.md)'s CSS-px rule does not apply to it — there is no
   * unit to get wrong.
   *
   * **The renderer clamps rather than rejects**, and the value it adopts may be *smaller* than the
   * one asked for: a cell the glyph atlas cannot hold is shrunk to one it can. It also rolls
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
   * What to do is consumer policy, so the widget forwards the signal and applies none itself.
   *
   * **Fires at most once per loss**, and never after {@link JustermRenderer.dispose} on a terminal that
   * composed its surface — on {@link JustermRenderer.attach} the handler belongs to the surface and
   * outlives the widget. Change it at
   * runtime with
   * {@link JustermRenderer.setOnContextLoss}.
   *
   * To ask instead of being told — a consumer that attaches late, or polls — use
   * {@link JustermRenderer.isContextLost} / {@link JustermRenderer.isRestoreOverdue}.
   */
  onContextLoss?: () => void;
  /**
   * How long a lost context is given to come back before {@link onContextLoss} fires, in ms.
   * Omit for the renderer's default — **3000**, xterm.js's `_contextRestorationTimeout` value.
   * Negative values are clamped to `0` by the renderer. Applies to the *next* loss; a deadline
   * already armed keeps the duration it was armed with.
   *
   * Consumer policy the renderer declares as such, and so reachable through the widget. Change it at
   * runtime with {@link JustermRenderer.setContextRestoreTimeout}.
   */
  contextRestoreTimeout?: number;
  theme: Theme;
}

/** The empty cell columns a phase-only re-issue passes (#576) — shared, since they are never
 * written and allocating them twice a second would be pure garbage. */
const EMPTY_U32 = new Uint32Array(0);
const EMPTY_U16 = new Uint16Array(0);

/**
 * The full renderer surface this adapter drives — **the per-grid half plus everything
 * {@link SurfaceBackend} carries**, which is why it extends it rather than restating those members.
 * Declared as an interface (not the imported wasm type) so the wiring is unit-testable behind a fake
 * with no GL context; method names match wasm-bindgen's output (snake_case where there is no
 * `js_name`, camelCase where there is). A call naming a `grid` acts on one terminal, and a call
 * naming none acts on what every terminal shares — the renderer's own 0.15.0 split. How this is
 * gated against the published renderer:
 * [`docs/map/territory/published-surface.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/published-surface.md) § The widget's
 * renderer seam is gated at `build`.
 */
export interface RendererBackend extends SurfaceBackend {
  /** Scatter a decoded frame's damage into the persistent grid, then re-pack. Header is
   * `[cols, rows, kind, hasScroll, scrollTop, scrollBottom, scrollCount, blinkOn]`. */
  apply_damage(
    grid: number,
    header: Uint32Array,
    spans: Uint32Array,
    codepoints: Uint32Array,
    fg: Uint32Array,
    bg: Uint32Array,
    flags: Uint16Array,
    /** Per-cell 1-based grapheme-cluster index — **u32, not u16** (#621/#627); `flags` above stays
     * u16. */
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
   * **`row` is a VIEWPORT row**, like everything else this renderer is handed — it draws the window
   * the user is looking at. It is *not* the grid row core reports the cursor at: those agree only
   * while `display_offset` is 0, and the caller owes the mapping
   * ([justerm#921](https://github.com/kihyun1998/justerm/issues/921)).
   *
   * **Optional**: absent from a renderer until a `renderer-v*` release publishes it, and a renderer
   * without it is preedit-blind. Why optional:
   * [`docs/map/territory/published-surface.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/published-surface.md) § A binding
   * added in the repo is absent until published. */
  setPreedit?(grid: number, col: number, row: number, codepoints: Uint32Array): number;
  /** Retain the consumer's suggestion run ([justerm#972](https://github.com/kihyun1998/justerm/issues/972));
   * an empty run clears it. Optional for the reason {@link setPreedit} is: a `renderer-v*` tag
   * publishes it. */
  setSuggestion?(
    grid: number,
    col: number,
    row: number,
    codepoints: Uint32Array,
    fg: number,
    dim: boolean,
  ): void;
  /** Retain the hovered link's spans (#934), drawn underlined; empty spans clear it. Optional for
   * the reason {@link setPreedit} is: a `renderer-v*` tag publishes it. */
  setLinkHover?(grid: number, spans: Uint32Array): void;
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
   * fraction of the cell width. Both are read at *draw* time (a shader uniform and a
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
  /** Move this grid onto the configuration a new font size (CSS px) / family names (#406/#413),
   * baking one only if no grid holds it. The cell size moves, so the consumer must re-fit. A no-op if
   * unchanged; a non-finite / `<1` size is guarded by the renderer. */
  setFontSize(grid: number, cssPx: number): void;
  setFontFamily(grid: number, family: string): void;
  /** Move this grid onto the configuration a new weight for regular / bold text names (#928). The
   * cell does not move. A weight outside {@link FontWeight} is ignored by the renderer; unchanged is
   * a no-op. */
  setFontWeight(grid: number, weight: FontWeight): void;
  setFontWeightBold(grid: number, weight: FontWeight): void;
  /** Move this grid onto the configuration with or without per-channel (LCD) text coverage. The
   * cell does not move; unchanged is a no-op. */
  setSubpixelAntialiasing(grid: number, on: boolean): void;
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
   * configurations has no cell it can be a multiple of. */
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

/** Monotonic clock for the blink phase (ms). */
const now = (): number => performance.now();

/**
 * The real {@link Renderer}: wraps the first-party `justerm-renderer` (WASM + WebGL2) and pushes
 * each decoded frame's cells, overlay, cursor and decorations to it; the renderer does all
 * compositing in wasm. Overlay, cursor and decoration state is consumer-pushed every frame and set
 * before the frame's damage, so a frame packs once.
 *
 * Build one with {@link JustermRenderer.create} — a terminal on its own canvas — or
 * {@link JustermRenderer.attach} — a terminal on a shared {@link TerminalSurface}. Why it is built
 * this way: [`docs/map/territory/widget-lifecycle.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/widget-lifecycle.md) § How a `JustermRenderer` is built.
 */
export class JustermRenderer implements Renderer {
  private readonly blink = new CursorBlink();
  /** The SGR 5 text phase (#576) — a separate clock from the caret's, never restarted by input. */
  private readonly textBlink = new TextBlink();
  /** Last cursor reported by a frame (screen coords), or `undefined` if hidden. `shape` is the
   * application's, `undefined` while it has set none. */
  private cursor: { col: number; row: number; shape: number | undefined } | undefined;
  /** The consumer's default caret shape (#927). */
  private cursorStyle: CursorStyle = "block";
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
   * off — see `frame-loop.ts`. Built lazily because `requestAnimationFrame` is read at
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
   * origin half of the rect {@link setViewportRect} sets; the extent is re-derived from the grid and
   * the cell. `(0, 0)` for a sole tenant; for a shared surface it is the terminal's DOM overlay
   * measured against the canvas, re-supplied whenever that box moves.
   */
  private rect = { x: 0, y: 0 };
  /**
   * Whether the host has taken this terminal off the surface — **state consulted at every placement**
   * ({@link applyGrid}), not a command issued once. Why: [`docs/map/territory/multi-viewport.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/multi-viewport.md) § Hidden-ness is state the widget
   * consults.
   */
  private hidden = false;
  /** Focus gates the selection colour (focused → `selectionBg`, blurred → the dimmer
   * `selectionInactiveBg`) and the blink (blurred → solid). xterm's two selection colours.
   *
   * **Starts unfocused**, and {@link Terminal} also reports once at mount. Why both:
   * [`docs/map/territory/widget-lifecycle.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/widget-lifecycle.md) § How a `JustermRenderer` is built. */
  private focused = false;
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
     * density and context-loss recovery: everything the renderer scopes to the surface rather than to
     * a grid, which is what lets N terminals share one context.
     */
    private readonly surface: TerminalSurface<RendererBackend>,
    /**
     * **Whether this terminal composed the surface it draws on.** One fact, and everything this
     * class does differently between its two entry points is derived from it — which is why it is a
     * single flag rather than three.
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
     * **"Sole tenant" throughout this file means exactly this flag being true** — a state that holds by
     * construction, not one anything checks at runtime. Why: [`docs/map/territory/widget-lifecycle.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/widget-lifecycle.md) § How a `JustermRenderer` is
     * built.
     */
    private readonly composedSurface: boolean,
    /**
     * This widget's claim on the surface. A `Terminal` is one terminal, so it is a constant
     * for the object's life — what changed at renderer 0.15.0 is that the grid has to be *named* on
     * every call that acts on a terminal rather than on the surface.
     *
     * A lease rather than the bare id, so no registry call has to cope with a stale id — see
     * {@link GridLease}.
     */
    private readonly lease: GridLease,
    // Retained so `setTheme` (#420) can rebuild the 256-colour table from a new ANSI scheme.
    private readonly buildPalette: (ansi: Uint32Array) => Uint32Array,
    // Theme-derived state is mutable: `setTheme` swaps the whole scheme at runtime (#420).
    private palette: Palette,
    private readonly flagBits: FlagBits,
    /**
     * The decoder's underline-style accessor and its named values (#862, #827 story 15).
     *
     * Carried, like `buildPalette`, because the decoder is loaded with a dynamic `import()` — a
     * static re-export would put its wasm init on this module's graph.
     */
    private readonly underlineStyleOf: (flags: number) => UnderlineStyle,
    private readonly styleValues: UnderlineStyles,
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

    // A density change and a context restore both move this grid's cell with no consumer call behind
    // them; the surface owns *when*, this object re-derives its own geometry (#325, #773 —
    // docs/map/territory/gl-context-lifecycle.md holds the measurement).
    this.lease.onReapply(() => this.reapplySurface());
    // And how to END this terminal, so a host that disposes the surface ends the widgets on it rather
    // than retiring their grids under them.
    this.lease.onEnd(() => this.dispose());
  }

  /**
   * Attach a terminal to an existing {@link TerminalSurface} — the multi-terminal entry point.
   *
   * **What differs from {@link create}, and all of it follows from who composed the surface.** This
   * terminal claims no sole tenancy, so it neither sizes the shared drawing buffer nor ends the
   * surface when it is disposed; the host does both. It draws where
   * {@link JustermRenderer.setViewportRect} puts it, and until that is called it sits at the origin
   * — where a sole tenant also sits, which is what makes the single-terminal arrangement the special
   * case of this one rather than a second path through the code.
   *
   * Everything else is identical, deliberately: the widget experience is unchanged and the only new
   * noun is the surface ([ADR-0021](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0021-single-context-multi-viewport.md)).
   * The consumer still owns the DOM overlay — the hidden IME textarea, the a11y
   * tree, the scrollbar — and one canvas means every terminal shares one stacking plane, so arbitrary
   * DOM cannot be interleaved between two of them.
   */
  static async attach(
    surface: TerminalSurface<RendererBackend>,
    opts: AttachedRendererOptions,
  ): Promise<JustermRenderer> {
    return JustermRenderer.build(surface, false, opts);
  }

  static async create(opts: JustermRendererOptions): Promise<JustermRenderer> {
    // The surface is composed here, so this object ends it (the `composedSurface` parameter). Both
    // wasm modules load in parallel: the decoder's import starts here and `build`'s own `await` on it
    // resolves from the module registry.
    const [surface] = await Promise.all([
      TerminalSurface.open(opts.canvasSelector),
      import("justerm-wasm-decode"),
    ]);
    // What this method composes, it ends — on the failure path too, or a throw strands a bound
    // context, a density watcher and a canvas listener.
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
    // Dynamic import (docs/map/territory/widget-lifecycle.md § How a `JustermRenderer` is built). Only
    // the decoder — the renderer module is the surface's.
    const decoder = await import("justerm-wasm-decode");
    const t = opts.theme;
    const paletteColors = decoder.buildPalette(Uint32Array.from(t.ansi));
    const backend = surface.rendererBackend();
    // This widget's one grid, with its font named at birth: one bake, where pushing the selectors by
    // setter afterwards could bake up to eight (#773, #928, #961).
    const lease = surface.addGrid({
      paletteColors,
      defaultFg: t.defaultFg,
      defaultBg: t.defaultBg,
      fontFamily: opts.fontFamily,
      fontSize: opts.fontSize,
      letterSpacing: opts.letterSpacing ?? 0,
      lineHeight: opts.lineHeight ?? 1,
      fontWeight: opts.fontWeight,
      fontWeightBold: opts.fontWeightBold,
      subpixelAntialiasing: opts.subpixelAntialiasing,
    });
    try {
      return await JustermRenderer.assemble(surface, composedSurface, opts, lease, decoder, paletteColors);
    } catch (e) {
      // A grid is GPU memory; nothing holds it if assembly throws, and only `removeGrid` gives it back.
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
    // Unconditional, with the default named (#580): a `Theme` is a complete description
    // (docs/map/territory/colour-policy.md § `Theme` is colours plus the two colour policies).
    backend.setCursorContrast(lease.id, t.cursorContrast ?? DEFAULT_CURSOR_CONTRAST);
    // Background opacity (#577). Set unconditionally at the renderer's own default, so the value the
    // renderer holds is the one this object states rather than one nobody wrote down. No `render`
    // here — nothing has been drawn yet, and the first frame presents it.
    backend.setBgAlpha(lease.id, opts.bgAlpha ?? 1);
    // Font family, size and both spacing options (#406/#413/#578) went into `addGrid` above, together.
    // Cursor stroke thickness (#580) — conditional: an option is read once, so an unset one can leave
    // the renderer's default in place (colour-policy.md, as above).
    if (opts.cursorThickness !== undefined) {
      backend.setCursorThickness(lease.id, opts.cursorThickness);
    }

    const palette: Palette = {
      colors: paletteColors,
      defaultFg: t.defaultFg,
      defaultBg: t.defaultBg,
    };
    // Copied into a plain object rather than held: `Flags` is a wasm resource and the decoder's
    // own guidance is to read it once and destructure. Every member is listed because the type
    // now *requires* all of them (#831) — this used to name nine of eleven and nothing said so.
    const f = decoder.flags();
    const flagBits: FlagBits = {
      bold: f.bold,
      italic: f.italic,
      underline: f.underline,
      strikethrough: f.strikethrough,
      wide_char: f.wide_char,
      wide_char_spacer: f.wide_char_spacer,
      wrapline: f.wrapline,
      inverse: f.inverse,
      dim: f.dim,
      hidden: f.hidden,
      blink: f.blink,
    };
    const styleCtx = cellStyleContext(decoder);
    const instance = new JustermRenderer(
      backend,
      surface,
      composedSurface,
      lease,
      (ansi) => decoder.buildPalette(ansi),
      palette,
      flagBits,
      styleCtx.underlineStyleOf,
      styleCtx.styleValues,
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
    // Conditional for the same reason: the renderer's default (3000) stays its own.
    if (opts.contextRestoreTimeout !== undefined) {
      instance.setContextRestoreTimeout(opts.contextRestoreTimeout);
    }
    // `undefined` is the default (follow the application), so this is a no-op unless set (#575).
    instance.setCursorBlink(opts.cursorBlink);
    if (opts.cursorStyle !== undefined) instance.setCursorStyle(opts.cursorStyle);
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

  /**
   * The underline style a `flags[i]` word carries — `Curly`, `Dotted`, and the four others.
   *
   * **A method, not a member of {@link cellFlags}.** The style is a 3-bit *field*; that map is one
   * bit per question and cannot answer "which of six", which is why the decoder split it the same
   * way. Keeping `cellFlags` a plain object of numbers is also what lets a test pass
   * `{ bold: 1, … }` as a literal.
   *
   * Pass the whole word; the shift and the width appear in no consumer's source. Total — a value
   * outside the enum reads as `Single`, the same normalisation the engine applies.
   */
  underlineStyle(flags: number): UnderlineStyle {
    return this.underlineStyleOf(flags);
  }

  /**
   * The named style values, so a consumer writes `styles.Curly` rather than `3` and never
   * imports `justerm-wasm-decode` to do it.
   *
   * Frozen by the decoder; read once and cache, as with {@link cellFlags}.
   */
  get underlineStyles(): UnderlineStyles {
    return this.styleValues;
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

  /** Change the font size (CSS px) at runtime — this terminal joins the font configuration the new
   * size names. The cell size moves, so **the consumer must re-fit** (recompute its grid +
   * {@link resize}) after calling. A no-op at the current size. */
  setFontSize(cssPx: number): void {
    this.backend.setFontSize(this.lease.id, cssPx);
    this.reapplySurface();
  }

  /** Change the font family at runtime — a CSS `font-family` string; this terminal joins the
   * configuration it names. As with {@link setFontSize}, the cell size can move, so **the consumer
   * must re-fit** after. Load a webfont before an unfamiliar family (the browser silently falls back
   * otherwise). */
  setFontFamily(family: string): void {
    this.backend.setFontFamily(this.lease.id, family);
    this.reapplySurface();
  }

  /**
   * Change the weight regular text is drawn at — the live counterpart of
   * {@link JustermRendererOptions.fontWeight}. Joins the configuration the weight names, and presents.
   *
   * **No re-fit**, unlike {@link setFontFamily}: the cell is measured at `"normal"` whatever the
   * weight, so the grid the consumer drives its engine at is unaffected.
   */
  setFontWeight(weight: FontWeight): void {
    this.backend.setFontWeight(this.lease.id, weight);
    this.backend.render();
  }

  /** Change the weight bold (SGR 1) text is drawn at (#928). See {@link setFontWeight}. */
  setFontWeightBold(weight: FontWeight): void {
    this.backend.setFontWeightBold(this.lease.id, weight);
    this.backend.render();
  }

  /**
   * Turn per-channel (LCD / subpixel) text coverage on or off — the live counterpart of
   * {@link JustermRendererOptions.subpixelAntialiasing}. Joins the configuration it names, and
   * presents. No re-fit:
   * the cell does not move.
   */
  setSubpixelAntialiasing(on: boolean): void {
    this.backend.setSubpixelAntialiasing(this.lease.id, on);
    this.backend.render();
  }

  /**
   * Change the letter spacing (CSS px) / line height (multiplier `>= 1`) at runtime. The live
   * counterparts of {@link JustermRendererOptions.letterSpacing} / {@link
   * JustermRendererOptions.lineHeight}, whose docs carry the units and the clamping.
   *
   * **The consumer must re-fit afterwards**, exactly as for {@link setFontSize}/{@link
   * setFontFamily}: call {@link resize} with the CSS box — **not `FitController.fit()`**, whose port
   * carries a grid and never reaches this canvas's display box (it stays right for container resizes). Skipping it leaves the grid a column
   * count derived from the old cell.
   *
   * **Read the cell back rather than deriving it from what you passed**: a `lineHeight` whose cell
   * the atlas cannot hold is shrunk, a failed atlas re-bake rolls the whole change back, and on a lost
   * context the change lands at the restore. {@link cellSize} and {@link terminalSize} are the truth
   * afterwards — a large enough cell shrinks the *grid* as well. Why each:
   * [`docs/map/territory/fit.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/fit.md) § The widget re-fits on `resize`, and only there.
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
   * Change the background opacity at runtime — `0` transparent, `1` opaque. The live
   * counterpart of {@link JustermRendererOptions.bgAlpha}, whose doc carries the full contract
   * (which cells it reaches, and the consumer CSS it depends on to be visible at all).
   *
   * **No re-fit**, unlike {@link setFontSize}/{@link setFontFamily}: the cell geometry does not
   * move, so the grid the consumer drives its engine at is unaffected.
   *
   * Presents immediately and unconditionally — the alpha rides the clear colour as well as each
   * cell's, so even an empty terminal changes. Out-of-range values are the renderer's to clamp
   * (`[0,1]`).
   */
  setBgAlpha(alpha: number): void {
    this.backend.setBgAlpha(this.lease.id, alpha);
    this.backend.render();
  }

  /**
   * Change the cursor's stroke thickness at runtime — a fraction of the cell width. The live
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
   * Adopt a new device pixel ratio: every atlas re-bakes at the new density and every terminal on
   * this surface is re-placed at the new cell. A terminal that composed its surface also re-derives
   * the drawing buffer and the canvas display box; a shared surface's buffer is the host's to re-size
   * on {@link TerminalSurface.onDensityChange}. **Called for you** by the
   * widget's own resolution watcher; a consumer needs this only to drive the path in a test, or to
   * serve a density this object cannot observe (a `window` it was not built against).
   *
   * **The CSS box can move** — the device cell is `round(metric * dpr)`, and dividing that back need
   * not land on the old CSS cell — and whether it does is font dependent. What always holds is
   * `canvas.style x dpr === drawing buffer`.
   *
   * **No re-fit**: the grid is left alone, so a terminal in a fixed container can end up a few CSS px
   * larger or smaller than the box that fitted it; call {@link resize} with the current CSS box to
   * re-derive it. A no-op at an unchanged ratio, and **dropped while the GL context is lost** — so
   * this is safe to call unconditionally. Why each: [`docs/map/territory/cell-geometry.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/cell-geometry.md) § A density change moves the cell and may move the CSS box.
   */
  setDevicePixelRatio(dpr: number): void {
    // Through the surface: one canvas is one density, so this moves every grid's cell and the surface
    // re-derives every attached terminal (#775; docs/map/territory/cell-geometry.md § A density
    // change moves the cell and may move the CSS box).
    this.surface.setDevicePixelRatio(dpr);
  }

  /**
   * Install (or clear, with `undefined`) the handler called when a lost WebGL context has not come
   * back within {@link setContextRestoreTimeout}. The live counterpart of
   * {@link JustermRendererOptions.onContextLoss}, whose doc carries the full contract — what the
   * signal means, what it does *not* mean, and why the widget applies no policy of its own.
   *
   * **This reaches the whole SURFACE, not just this terminal**, and on a shared one that
   * matters: there is one context, so there is one loss and one notification. A second terminal
   * calling this — or attached with `onContextLoss` in its options — **replaces** the first
   * terminal's handler for the entire canvas, last call wins, with no diagnostic. A host driving
   * several terminals should register once, on the surface.
   *
   * Nothing is re-registered with the renderer here: the surface holds one relay for its life and this
   * swaps the handler behind it, which is what makes clearing expressible.
   *
   * **No redraw**, unlike {@link setCursorBlink} / {@link setBgAlpha}: this changes who is told
   * about a future event, not anything currently on screen.
   */
  setOnContextLoss(handler: (() => void) | undefined): void {
    this.surface.setOnContextLoss(handler);
  }

  /**
   * Change the restore grace period at runtime, in ms — the live counterpart of
   * {@link JustermRendererOptions.contextRestoreTimeout}, whose doc carries the default and why the
   * knob exists.
   *
   * Applies to the **next** loss. A deadline already armed keeps the duration it was armed with, so
   * shortening this during a loss does not bring that loss's notification forward.
   */
  setContextRestoreTimeout(ms: number): void {
    this.surface.setContextRestoreTimeout(ms);
  }

  /**
   * Whether a context loss has been **reported**. For surfacing the state — dimming the
   * terminal, showing a badge — not for deciding whether drawing is safe.
   *
   * **It answers *"was I told"*, and that is deliberate rather than an approximation** ([ADR-0027](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0027-liveness-is-answered-by-the-source-that-owns-it.md)
   * D4). A browser destroys a context synchronously and merely *queues* `webglcontextlost`, so for
   * a window this reads `false` while every GL call is already dead. The renderer guards its own
   * work on a different, stricter predicate that consults the context itself; that one is private,
   * because a consumer branching on it would be making a decision the renderer has already made.
   * Recovery needs nothing from you either way — the renderer rebuilds itself on
   * `webglcontextrestored`.
   *
   * Stays truthful after {@link dispose}: disposal stops this object's *work*, and the renderer's
   * canvas listeners belong to the wasm binding, so the state machine behind this keeps tracking.
   * On a terminal that composed its surface the notification is closed with it.
   *
   * A {@link resize} that landed during the loss is provisional; on a terminal that composed its
   * surface the widget re-syncs it at the restore with no call from you (see {@link resize}). A host
   * sharing a surface is told through {@link TerminalSurface.onDensityChange} when a restore adopted a
   * new density.
   */
  isContextLost(): boolean {
    return this.surface.isContextLost();
  }

  /**
   * Whether a lost context has missed its restore deadline — the same fact
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
   * **Leaves {@link setBgAlpha} alone**: the alpha is not on {@link Theme}, so swapping the colour
   * scheme does not make a translucent terminal opaque again. */
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

  /** Fit a `cols`×`rows` grid to a CSS-pixel box and size the renderer and the canvas display box
   * to it; the display box is written from what the renderer reports (`cssWidth`/`cssHeight`), since
   * the device-px buffer would otherwise display at twice its size on a Retina screen.
   *
   * **A call that lands while the GL context is lost is provisional**: the renderer commits the
   * buffer asked for and settles any browser clamp at the restore, after which this widget re-derives
   * the buffer and the display box by itself — no consumer call is needed. Why:
   * [`docs/map/territory/gl-context-lifecycle.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/gl-context-lifecycle.md) § A resize during a loss is provisional. */
  resize(cssWidth: number, cssHeight: number): void {
    const grid = gridForBox(
      cssWidth,
      cssHeight,
      this.backend.cssCellWidth(this.lease.id),
      this.backend.cssCellHeight(this.lease.id),
    );
    // Nothing to propose — an unmeasured cell or a non-finite box (#632): leave the renderer and the
    // canvas box exactly as they are.
    if (!grid) return;
    this.applyGrid(grid.cols, grid.rows);
  }

  /**
   * Give the renderer a `cols`×`rows` grid **and** the surface to draw it on, then re-apply the
   * canvas display box.
   *
   * The one site every placement path reaches. It asks for `cols * cell_width(grid)` device px (a
   * sole tenant only), reads the browser's grant back and shrinks the grid to it, and places or
   * withholds the rect. Why each: [`docs/map/territory/multi-viewport.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/multi-viewport.md) § `applyGrid` is the widget's one placement site.
   */
  private applyGrid(cols: number, rows: number): void {
    this.backend.resizeGrid(this.lease.id, cols, rows);
    // Sizing the shared buffer is the sole tenant's alone (#775; #331's exactness).
    if (this.composedSurface) {
      this.surface.resizeSurface(
        cols * this.backend.cell_width(this.lease.id),
        rows * this.backend.cell_height(this.lease.id),
      );
    }

    // Read the grant back and adopt it (#339) — for a sole tenant; on a lost context `cssWidth()` is
    // the committed request, so nothing shrinks (#639). docs/map/territory/multi-viewport.md § `applyGrid`.
    const granted = gridForBox(
      this.backend.cssWidth(),
      this.backend.cssHeight(),
      this.backend.cssCellWidth(this.lease.id),
      this.backend.cssCellHeight(this.lease.id),
    );
    if (granted !== undefined && (granted.cols < cols || granted.rows < rows)) {
      this.backend.resizeGrid(this.lease.id, granted.cols, granted.rows);
    }

    // A hidden terminal is placed nowhere (#801); `resizeGrid` above still ran, so coming back stays a
    // placement. Only the rect is withheld.
    if (this.hidden) {
      this.backend.clearViewport(this.lease.id);
      this.present();
      return;
    }

    // Re-issued on every call: a rect is device px, the cell may have just moved and the grid may
    // have just been shrunk to the grant.
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
   * this path will draw it: a sole tenant presents now, a shared tenant coalesces into the surface's
   * one frame. Why: [`docs/map/territory/multi-viewport.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/multi-viewport.md) § `applyGrid` is the widget's one placement site.
   */
  private present(): void {
    this.render();
  }

  /**
   * Re-derive the drawing buffer from the grid this widget is already holding, at whatever the cell
   * has just become — the response to anything that moves the cell without moving the grid.
   *
   * A no-op before the first {@link resize}: a grid is born `0`x`0`, and re-deriving from that would
   * floor it to one cell.
   */
  private reapplySurface(): void {
    const { cols, rows } = this.terminalSize();
    if (cols > 0 && rows > 0) this.applyGrid(cols, rows);
  }

  /**
   * Place this terminal on the shared canvas: the top-left of its viewport, in **device px**.
   * For a terminal sharing a surface with siblings — a sole tenant sits at the origin and
   * never calls this.
   *
   * The extent is not a parameter: it is `cols * cell` × `rows * cell`, derived here — pair this with
   * {@link resize} to change how many cells fit.
   *
   * **The host owes this call whenever the overlay's box moves** — a scroll, a layout change, a pane
   * drag — and nothing detects a missed one: the GL viewport stays where it was while the DOM overlay
   * moves off it. **It is owed after a density change too**, where the box has not moved: a rect is
   * device px, so register {@link TerminalSurface.onDensityChange} and re-supply it (a context
   * restore that adopts a new density fires it as well). A sole tenant needs none of this. Why:
   * [`docs/map/territory/multi-viewport.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/multi-viewport.md) § `applyGrid` is the widget's one placement site.
   */
  setViewportRect(x: number, y: number): void {
    this.rect = { x, y };
    // Giving a rect IS showing (#801) — the same field `hide` and `show` write.
    this.setHidden(false);
  }

  /**
   * Take this terminal off the surface **without ending it** — the hidden-tab state.
   *
   * Every byte survives: the grid stays registered, its packed instances and upload baseline stay
   * resident, and its font configuration's atlas is not released. Coming back is
   * {@link setViewportRect} — a placement, which re-packs once from the state the grid already had —
   * or {@link show}.
   *
   * **Hiding the DOM overlay is not this**: one WebGL context binds to one canvas, so a terminal's
   * pixels are on the shared canvas, not in its overlay — `visibility: hidden` leaves it drawn and
   * paid for.
   *
   * Idempotent, and a no-op before the first {@link resize}: a grid with no cells is drawn nowhere
   * already. {@link show} is the way back.
   */
  hide(): void {
    this.setHidden(true);
  }

  /**
   * The one writer of {@link hidden}, so the *transition* has a single site.
   *
   * On the way back (the true → false edge) it re-syncs the text-blink phase and the cursor, which
   * drift while hidden; keyed on the edge because {@link setViewportRect} runs through here on every
   * ancestor scroll.
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
   * For a sole tenant this is the way back; a *shared* tenant that moved while it was away calls
   * {@link setViewportRect} instead, since this re-places at the last origin given — wiring
   * `observeViewportRect` does that for you.
   *
   * Idempotent, and a no-op before the first {@link resize}.
   */
  show(): void {
    this.setHidden(false);
  }

  /**
   * Whether this terminal is currently drawn — the renderer's own answer, not a mirror of
   * {@link hide}.
   *
   * A terminal that has never been sized answers `false` here while nothing has hidden it: a grid is
   * registered not drawn until its first {@link resize} places it.
   */
  isDrawn(): boolean {
    return this.backend.isGridDrawn(this.lease.id);
  }


  /** The terminal grid ACTUALLY adopted after the last {@link resize} — not the requested
   * `cols`/`rows`, so a browser drawing-buffer clamp cannot desync the grid the consumer
   * drives its engine and frames at from the grid the buffer can hold.
   *
   * **That guarantee belongs to a terminal that composed its surface.** A terminal sharing a surface
   * occupies part of a buffer it did not ask for, so nothing clamps its grid; the grant is the host's
   * to read from {@link TerminalSurface.cssSize}. */
  terminalSize(): { cols: number; rows: number } {
    return { cols: this.backend.cols(this.lease.id), rows: this.backend.rows(this.lease.id) };
  }

  applyFrame(frame: DecodedFrame): void {
    // Set the retained overlay/decoration/cursor state first, so `apply_damage` packs once with it.
    // `retainU32`: these three outlive the frame (a focus flip re-issues them) (#657).
    this.lastSelectionSpans = retainU32(frame.selectionSpans ?? new Uint32Array(0));
    this.lastMatchSpans = retainU32(frame.matchSpans ?? new Uint32Array(0));
    this.lastActiveMatchSpans = retainU32(frame.activeMatchSpans ?? new Uint32Array(0));
    this.issueOverlay();
    this.backend.setDecorations(this.lease.id, decorationWire(this.decorationSource?.(frame) ?? []));
    this.updateCursor(frame);
    // Pack at the current text-blink phase, not forced on, so a frame arriving in the off phase does
    // not flash blinking cells back on.
    const textBlinkOn = this.textBlink.isVisible(now());
    this.backend.apply_damage(this.lease.id,
      damageHeader(frame, textBlinkOn),
      asU32(frame.spans),
      asU32(frame.codepoints),
      asU32(frame.fg),
      asU32(frame.bg),
      asU16(frame.flags),
      // #627: u32. Whether this is a zero-copy identity is the frame producer's decoder version
      // (docs/map/territory/frame-adapter.md § The damage entry point's arguments).
      asU32(frame.extra),
      Array.from(frame.sideTable),
      // #520: the underline colour column (SGR 58), forwarded; omitted → all Default.
      asU32(frame.underlineColor ?? new Uint32Array(0)),
    );
    // Recorded only after `apply_damage` returns: it refuses a malformed frame before storing the
    // phase (#355), and recording first would leave the loop believing a flip it never made.
    this.lastTextBlinkOn = textBlinkOn;
    this.lastFrameGrid = { cols: frame.cols, rows: frame.rows };
    this.trackBlinkCells(frame);
    // Started here too: a terminal with a hidden cursor still blinks its SGR 5 text.
    if (this.textBlink.enabled) this.startBlinkLoop();
  }

  /**
   * Track whether the renderer's grid may hold a `BLINK` cell — conservatively: a Full frame
   * replaces the answer and a Partial one can only add to it, so it decays only at the next Full
   * frame. Why: [`docs/map/territory/widget-lifecycle.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/widget-lifecycle.md) § The blink loop and the present.
   */
  private trackBlinkCells(frame: DecodedFrame): void {
    const here = carriesBlink(frame.flags, this.flagBits.blink);
    // kind 0 = Full, 1 = Partial (the same encoding `damageHeader` puts on the wire).
    this.mayHaveBlinkCells = frame.kind === 0 ? here : this.mayHaveBlinkCells || here;
  }

  /**
   * Present the canvas. `render()` takes no grid, so one call presents the whole canvas: a sole
   * tenant ({@link create}) presents synchronously, and a terminal sharing a surface ({@link attach})
   * requests a present coalesced with its siblings' into one per frame. A host that needs the canvas
   * drawn *before* it returns — reading pixels, a screenshot — calls {@link TerminalSurface.present}
   * directly. Why: [`docs/map/territory/widget-lifecycle.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/widget-lifecycle.md) § The blink loop and the present.
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
   * flip (which has no new frame) so the two can never drift. The active-match channel
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

  /** Push the frame's cursor to the renderer (native cursor — #270), or clear it when hidden. The
   * renderer draws the shape; the blink phase stays consumer policy — the blink loop calls
   * `clearCursor`/`setCursor` on the off/on flip. */
  private updateCursor(frame: DecodedFrame): void {
    // The application's blink mode (wire v4, #81), applied only when the frame carries it.
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
    // Draw at the current blink phase, not forced on: cursor fields ride every frame, so forcing on
    // would pin the caret solid during output.
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
        resolveCursorShape(this.cursor.shape, this.cursorStyle),
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
    // The input path resets the idle clock (#593); a cursor move is output and restarts the phase only.
    this.blink.restartFromInput(now());
    this.redrawCursor();
  }

  /**
   * How long the cursor keeps blinking with no user input before parking solid, in ms.
   * `0` disables it. Defaults to {@link BLINK_IDLE_TIMEOUT} (5 minutes, xterm.js's value).
   *
   * The live counterpart of
   * {@link JustermRendererOptions.cursorBlinkTimeout}.
   */
  setCursorBlinkTimeout(ms: number): void {
    this.blink.setIdleTimeout(ms);
    if (this.cursor) this.redrawCursor();
  }

  /** Underline the hovered link's cells, or clear it with empty spans (#934). Drawn at the next
   * present; a renderer published before the binding draws no underline. */
  setLinkHover(spans: Uint32Array): void {
    this.backend.setLinkHover?.(this.lease.id, spans);
  }

  /**
   * Draw the composition into the grid and report where the caret belongs (#249, [ADR-0028](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0028-composition-surfaces-have-one-writer-each.md)).
   *
   * `row` is a **viewport** row — see the binding's own doc. The caller maps the composition's grid
   * origin through the display offset and withholds the call when the result is off screen.
   *
   * Presents immediately rather than waiting for the next frame: a composition produces no frames
   * at all — the engine never sees it — so there is nothing else to ride on. The cursor is re-pushed
   * at the returned column, which is D5's position rule: the caret rides the composition's end,
   * while `cursorCommand` still decides whether it is drawn (an application that hid the caret
   * keeps it hidden, #592's boundary).
   */
  setPreedit(col: number, row: number, codepoints: Uint32Array): number {
    // A renderer without the binding: report the anchor cell unchanged, so only the drawing is missing.
    if (!this.backend.setPreedit) return col;
    const caretCol = this.backend.setPreedit(this.lease.id, col, row, codepoints);
    // Retained: every later frame re-asserts the engine's cursor, which knows nothing of the preedit
    // (docs/map/invariant/composition-is-browser-owned-state.md).
    this.preeditCaret = codepoints.length > 0 ? { col: caretCol, row } : undefined;
    if (this.cursor) {
      this.cursor = { ...this.cursor, col: caretCol, row };
      // The current phase, not the last one pushed (#592).
      this.pushCursor(this.blink.isVisible(now()));
    }
    this.render();
    return caretCol;
  }

  /**
   * Retain the consumer's suggestion at viewport cell `(col, row)`
   * ([justerm#972](https://github.com/kihyun1998/justerm/issues/972)); an empty run clears it. Does
   * not present: the widget re-sends the anchor inside a frame it is about to render, so a present
   * here would draw twice.
   */
  setSuggestion(col: number, row: number, codepoints: Uint32Array, color: number, dim: boolean): void {
    this.backend.setSuggestion?.(this.lease.id, col, row, codepoints, color, dim);
  }

  /**
   * An IME composition started / ended — the caret stays put for the duration.
   *
   * Redraws immediately: no frame carries this (composition never reaches the engine), so waiting
   * for one would leave the caret mid-phase until the next output — the same reason
   * {@link setFocused} redraws.
   */
  setComposing(composing: boolean): void {
    this.blink.setComposing(composing);
    if (this.cursor) this.redrawCursor();
  }

  /**
   * Force the cursor to blink (`true`) / stay steady (`false`), or `undefined` to follow the
   * application's DECSCUSR / `CSI ?12` mode. The live counterpart of
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
   * The caret shape drawn while the application has not chosen one. The live counterpart of
   * {@link JustermRendererOptions.cursorStyle}. Redraws immediately when a cursor is on screen, as
   * {@link setCursorBlink} does.
   */
  setCursorStyle(style: CursorStyle): void {
    this.cursorStyle = style;
    if (this.cursor) this.redrawCursor();
  }

  /**
   * The half-period of the SGR 5 text blink in ms; `0` disables it (the default). The live
   * counterpart of {@link JustermRendererOptions.textBlinkInterval}.
   *
   * Re-syncs immediately: no frame carries this, so disabling while the phase is off would otherwise
   * leave that text invisible until the next output.
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
   * Re-pack the retained grid at a new text-blink phase, without a frame. Returns whether
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
   * tint (the renderer re-packs the retained grid) and redraw the cursor.
   *
   * It presents even with no caret on screen when the tint moved — the retained spans exist so a
   * focus flip with no new frame can be drawn. Why: [`docs/map/territory/widget-lifecycle.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/widget-lifecycle.md) § The blink loop and the present. */
  setFocused(focused: boolean): void {
    this.blink.setFocused(focused);
    const changed = this.focused !== focused;
    if (changed) {
      this.focused = focused;
      this.issueOverlay();
      // Arriving must not hide the caret (#912): re-anchor the phase, not the idle clock
      // (docs/map/territory/caret-drawing.md § Focus-in re-anchors the blink phase).
      if (focused) this.blink.restart(now());
    }
    if (this.cursor) this.redrawCursor();
    else if (changed) this.backend.render();
  }

  /**
   * A rAF loop that re-issues the cursor cell whenever its blink phase flips, and re-packs the
   * grid whenever the SGR 5 text phase flips.
   *
   * The two phases are separate clocks but share one loop and, when they flip together, **one
   * present** — the same "pack once, present once" rule `applyFrame` follows.
   *
   * rAF stops firing for a hidden *document*, so a backgrounded tab costs nothing; a terminal
   * scrolled out of view inside a visible page keeps ticking. Why: [`docs/map/territory/widget-lifecycle.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/widget-lifecycle.md) § The blink loop and the present.
   */
  private startBlinkLoop(): void {
    this.blinkLoop.start();
  }

  /**
   * One blink iteration. Called by {@link FrameLoop}, which owns the scheduling — including the
   * part that matters here: if this throws, the loop stops with no handle left behind, so the next
   * `startBlinkLoop` (which `updateCursor` issues on every decoded frame) restarts it.
   *
   * Must not call {@link JustermRenderer.startBlinkLoop} — see `FrameLoop`'s `run` doc.
   */
  private blinkTick(): void {
    // A hidden terminal flips no phase (#801): both halves end in a whole-canvas present
    // (widget-lifecycle.md § The blink loop and the present). `setHidden` re-syncs on the way back.
    if (this.hidden) return;
    const t = now();
    const cursorOn = this.blink.isVisible(t);
    const cursorFlip = cursorOn !== this.lastBlinkOn;
    const textOn = this.textBlink.isVisible(t);
    // Gated on there being a BLINK cell: the flip is a full re-pack. The order of the two halves is
    // convention (the cursor is a uniform, not an instance).
    const textFlip = this.mayHaveBlinkCells && textOn !== this.lastTextBlinkOn;
    const repacked = textFlip && this.repackAtTextBlinkPhase(textOn);
    if (cursorFlip) this.pushCursor(cursorOn);
    if (repacked || cursorFlip) this.backend.render();
  }

  /**
   * Stop the blink loop and detach the reduced-motion listener. Both are draw paths: the
   * listener re-packs and presents, so a widget that kept it would still repaint its canvas after
   * being disposed.
   *
   * Called by `Terminal.dispose()`. Idempotent, as the `Renderer` port requires.
   *
   * **It releases this widget's grid**, and with it the GPU memory that grid holds (its VAO, its
   * instance buffer and its share of its font configuration's atlas); a terminal that composed its
   * surface ({@link create}) also ends the surface, and with it the context-loss notification.
   *
   * **So a disposed widget has no grid, and every method that acts on one throws afterwards** —
   * `cellSize`, `terminalSize`, `resize`, the font and spacing setters, the frame and cursor paths.
   * `isContextLost()` / `isRestoreOverdue()` keep answering: they read the state machine the canvas
   * listeners still feed. Why
   * each: [`docs/map/territory/widget-lifecycle.md`](https://github.com/kihyun1998/justerm/blob/master/docs/map/territory/widget-lifecycle.md) § The blink loop and the present.
   */
  dispose(): void {
    this.blinkLoop.stop();
    this.motionQuery.removeEventListener("change", this.onMotionChange);
    // Idempotent because the lease knows its own state (#805); it releases this grid only — a sibling
    // keeps its cells, its atlas and its viewport (#775).
    this.lease.release();
    // What this object exclusively holds, it ends
    // (docs/map/invariant/a-layer-ends-what-it-exclusively-holds.md).
    if (this.composedSurface) this.surface.dispose();
  }
}
