import type { Palette } from "justerm-wasm-decode/colors.js";
import { CursorBlink } from "./cursor";
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
  /** Minimum fg/bg contrast ratio (WCAG, 1..21). Defaults to 1 (off, like xterm) (#225). */
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
 * no `js_name`, camelCase where there is). */
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
    extra: Uint16Array,
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
  /** Retain the flat decoration directory `[row, left, right, layer, bg, fg]…` (#393). */
  setDecorations(spans: Uint32Array): void;
  /** Place the cursor: shape `0` block / `1` underline / `2` bar / `3` hollow (#270). */
  setCursor(col: number, row: number, shape: number, color: number, textColor: number): void;
  /** Remove the cursor — hidden (DECTCEM) or the blink's off phase. */
  clearCursor(): void;
  setBoldToBright(enabled: boolean): void;
  setMinimumContrastRatio(ratio: number): void;
  setSelectionForeground(color: number | undefined): void;
  /** Re-bake the atlas at a new font size (CSS px) / family (#406/#413). The cell size moves, so the
   * consumer must re-fit. A no-op if unchanged; a non-finite / `<1` size is guarded by the renderer. */
  setFontSize(cssPx: number): void;
  setFontFamily(family: string): void;
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
 * agreeing with the floor here is what keeps the two in step. */
export function gridForBox(
  cssWidth: number,
  cssHeight: number,
  cellCssWidth: number,
  cellCssHeight: number,
): { cols: number; rows: number } {
  return {
    cols: Math.max(MINIMUM_COLS, Math.floor(cssWidth / cellCssWidth)),
    rows: Math.max(MINIMUM_ROWS, Math.floor(cssHeight / cellCssHeight)),
  };
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
/** The u16 sibling of {@link asU32} (feeds `flags` / `extra`), with the same contract: the
 * fallback `Uint16Array.from` REINTERPRETS an out-of-range value (a negative or `>= 2**16` wraps
 * mod 2**16, `NaN`/±`Infinity` → `0`), it does not reject — the producer must clip, this cannot
 * validate (#467). Exported for the seam test only; not re-exported from `index.ts`. */
export const asU16 = (a: ArrayLike<number>): Uint16Array =>
  a instanceof Uint16Array ? a : Uint16Array.from(a);

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
  private rafId: number | undefined;
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
    // Font family + size (#406/#413, wired #417). Applied before the first fit, so the initial grid
    // is computed at the consumer's cell. Each is a no-op at the renderer's default (monospace/16).
    backend.setFontFamily(opts.fontFamily);
    backend.setFontSize(opts.fontSize);

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

  /** Swap the colour scheme at runtime (#420) — rebuild the 256-colour palette from the new ANSI
   * colours and push it (+ the theme's policy colours) to the renderer, which re-resolves every
   * retained cell in wasm. No re-fit needed (the cell geometry is unchanged); it presents on the
   * render below. The a11y cell mirror reads only text, so it needs no re-notification. */
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
   * device-px buffer displays at twice its size on a Retina screen. */
  resize(cssWidth: number, cssHeight: number): void {
    const { cols, rows } = gridForBox(
      cssWidth,
      cssHeight,
      this.backend.cssCellWidth(),
      this.backend.cssCellHeight(),
    );
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
    this.lastSelectionSpans = asU32(frame.selectionSpans ?? new Uint32Array(0));
    this.lastMatchSpans = asU32(frame.matchSpans ?? new Uint32Array(0));
    this.lastActiveMatchSpans = asU32(frame.activeMatchSpans ?? new Uint32Array(0));
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
      asU16(frame.extra),
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
    // A move (or first appearance) restarts the blink so the cursor shows at once.
    if (!this.cursor || cmd.col !== this.cursor.col || cmd.row !== this.cursor.row) {
      this.blink.restart(now());
    }
    this.cursor = { col: cmd.col, row: cmd.row, shape: cmd.shape };
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
      EMPTY_U16,
      EMPTY_U16,
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
    if (this.rafId !== undefined) return;
    const tick = (): void => {
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
      this.rafId = requestAnimationFrame(tick);
    };
    this.rafId = requestAnimationFrame(tick);
  }

  /** Stop the blink loop and detach the reduced-motion listener. Both are draw paths now (#576):
   * the listener re-packs and presents, so a disposed widget that kept it would still repaint its
   * canvas. Note that nothing calls this yet — `Terminal.dispose` does not, and `dispose` is not on
   * the `Renderer` port — so the loop currently outlives the widget either way. */
  dispose(): void {
    if (this.rafId !== undefined) {
      cancelAnimationFrame(this.rafId);
      this.rafId = undefined;
    }
    this.motionQuery.removeEventListener("change", this.onMotionChange);
  }
}
