import type { BeamtermRenderer as Backend, Batch, Cell, CellStyle } from "@beamterm/renderer";
import type { Palette } from "justerm-wasm-decode/colors.js";
import { CellMirror } from "./cell-mirror";
import { CursorBlink, cursorOp } from "./cursor";
import type { DecorationRect } from "./decorations";
import { type HighlightRect, highlightRects } from "./overlay";
import { composeOverlayDraws, cursorCellDraw } from "./overlay-compose";
import type { DrawOp, FlagBits } from "./render-core";
import { makeRenderPolicy } from "./render-policy";
import type { DecodedFrame } from "./types";
import type { Renderer } from "./renderer";

/** Theme colours (packed `0xRRGGBB`). The engine stays ignorant of these — the
 * consumer owns them and the renderer resolves cell refs against them. */
export interface Theme {
  /** The 16 ANSI colours (slots `0..15`); `buildPalette` fills `16..255`. */
  ansi: number[];
  defaultFg: number;
  defaultBg: number;
  /** The cursor colour (cell-invert fill / underline). Defaults to `defaultFg`. */
  cursorColor?: number;
  /** Selection highlight background (`0xRRGGBB`). A placeholder until #115's
   * focused/inactive blend; defaults to a muted slate. */
  selectionBg?: number;
  /** Search-match highlight background (`0xRRGGBB`). Defaults to a muted amber. */
  matchBg?: number;
  /** Selection background when the terminal is UNFOCUSED (`0xRRGGBB`). xterm's
   * selectionInactiveBackgroundOpaque; a dimmer tint. Defaults to a muted slate. */
  selectionInactiveBg?: number;
  /** Minimum fg/bg contrast ratio (WCAG, 1..21). Below it the renderer lightens
   * or darkens the fg to stay legible (#115). Defaults to 1 (off, like xterm). */
  minimumContrastRatio?: number;
  /** Draw bold text in the bright (8-15) ANSI colour — xterm's
   * drawBoldTextInBrightColors (#223). Defaults to true (xterm's default). */
  boldToBright?: boolean;
}

export interface BeamtermOptions {
  /** CSS selector of the canvas to attach to, e.g. `"#term"`. */
  canvasSelector: string;
  fontFamily: string;
  fontSize: number;
  theme: Theme;
}

/** The beamterm cell/style factories, captured from the dynamically-imported
 * module so `applyFrame` can build cells without re-importing. */
interface Factory {
  style(): CellStyle;
  cell(symbol: string, style: CellStyle): Cell;
}

/** `0xRRGGBB` → `0xRRGGBBAA` (opaque), the format beamterm's `clear` expects. */
const opaque = (rgb: number): number => ((rgb << 8) | 0xff) >>> 0;

/** Monotonic clock for the blink phase (ms). */
const now = (): number => performance.now();

/**
 * The real {@link Renderer}: wraps `@beamterm/renderer` (WASM + WebGL) and feeds
 * it the draw ops {@link frameToDrawOps} computes from a {@link DecodedFrame}.
 *
 * The two WASM modules (`@beamterm/renderer` + `justerm-wasm-decode`) are loaded
 * with **dynamic `import()`** inside {@link create}, mirroring penterm's working
 * terminal-native integration. Static top-level imports of two wasm-bindgen
 * "bundler"-target modules race their `__wbindgen_start()` during module-graph
 * evaluation and the second fails to instantiate (`__wbindgen_externrefs`
 * undefined); deferring to runtime import lets vite-plugin-wasm instantiate each
 * cleanly.
 *
 * Not exercised by the vitest suite — it needs a GL context and the WASM. All
 * the per-cell logic (span walk, colour resolve, flags, wide-char, grapheme)
 * lives in the pure {@link frameToDrawOps}, which the suite covers with golden
 * frames; this adapter only translates ops into beamterm calls.
 */
export class BeamtermRenderer implements Renderer {
  /** Viewport cell mirror (ADR-0011), (re)built lazily to the frame's size. */
  private mirror: CellMirror | undefined;
  private cols = 0;
  private rows = 0;
  private readonly blink = new CursorBlink();
  /** Last cursor reported by a frame (screen coords). */
  private cursor: { row: number; col: number; shape: number; visible: boolean } | undefined;
  private lastBlinkOn = true;
  private rafId: number | undefined;
  /** Per-frame decoration rects (#120 S2). Decorations are consumer-side (not on
   * the frame wire like selection/match), so the consumer injects a source bound
   * to its {@link DecorationRegistry}; absent → no decorations. */
  private decorationSource: ((frame: DecodedFrame) => DecorationRect[]) | undefined;
  /** The overlay-tinted cell keys (`y·cols + x`) of the LAST frame, for the #140
   * partial-frame delta: a cell whose highlight/decoration membership flipped but
   * that this frame's damage doesn't cover is repainted from the mirror. Reset when
   * the mirror is rebuilt (a resize invalidates the keys). Empty ⇔ nothing tinted. */
  private prevOverlay = new Set<number>();
  /** The last frame's overlay rects, retained so the blink loop's off-phase cursor
   * cell composites the SAME tint (#210) — the loop fires between frames, so it can't
   * read the frame. Selection/decoration only change via a frame, so these stay
   * current. Empty ⇔ nothing tinted. */
  private lastHighlights: HighlightRect[] = [];
  private lastDecorations: DecorationRect[] = [];

  /** Focus gates the selection colour: an unfocused terminal shows the inactive
   * selection tint (xterm's selectionInactiveBackgroundOpaque). #115. */
  private focused = true;

  private constructor(
    private readonly backend: Backend,
    private readonly factory: Factory,
    private palette: Palette,
    private readonly flagBits: FlagBits,
    private readonly buildPalette: (ansi: Uint32Array) => Uint32Array,
    private clearColor: number,
    private cursorColor: number,
    private selectionBg: number,
    private matchBg: number,
    private selectionInactiveBg: number,
    private minimumContrastRatio: number,
    private boldToBright: boolean,
  ) {
    // Honour prefers-reduced-motion (#119): suppress the cursor blink, tracking
    // changes live. Browser-only; the renderer is only built via `create`.
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    this.blink.setReducedMotion(mq.matches);
    mq.addEventListener("change", (e) => this.blink.setReducedMotion(e.matches));
  }

  /** Wire marker-anchored decorations (#120 S2): the source projects the frame's
   * decoration rects (typically `(frame) => registry.decorationsForFrame(frame)`),
   * which `applyFrame` composes into each cell's colour under/over the
   * selection/match highlight. Pass `undefined` to detach. */
  setDecorationSource(source: ((frame: DecodedFrame) => DecorationRect[]) | undefined): void {
    this.decorationSource = source;
  }

  /** The cell-decoding context the renderer resolved from the wasm decoder
   * (palette + flag bits). Exposed so the a11y mirror (#119) reads the same
   * cells via its own {@link CellMirror} without re-importing the decoder. */
  get cellPalette(): Palette {
    return this.palette;
  }
  get cellFlags(): FlagBits {
    return this.flagBits;
  }

  static async create(opts: BeamtermOptions): Promise<BeamtermRenderer> {
    const [beamterm, decoder] = await Promise.all([
      import("@beamterm/renderer"),
      import("justerm-wasm-decode"),
    ]);
    await beamterm.main(); // beamterm WASM init (idempotent)
    const backend = beamterm.BeamtermRenderer.withDynamicAtlas(
      opts.canvasSelector,
      [opts.fontFamily, "monospace"],
      opts.fontSize,
      false, // auto_resize_canvas_css: external CSS controls sizing
    );
    const palette: Palette = {
      colors: decoder.buildPalette(Uint32Array.from(opts.theme.ansi)),
      defaultFg: opts.theme.defaultFg,
      defaultBg: opts.theme.defaultBg,
    };
    // Cache the flag bits once — they never change within a build (decoder doc).
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
    };
    const factory: Factory = { style: beamterm.style, cell: beamterm.cell };
    return new BeamtermRenderer(
      backend,
      factory,
      palette,
      flagBits,
      (ansi) => decoder.buildPalette(ansi),
      opaque(opts.theme.defaultBg),
      opts.theme.cursorColor ?? opts.theme.defaultFg,
      opts.theme.selectionBg ?? 0x45475a,
      opts.theme.matchBg ?? 0x6e5c00,
      opts.theme.selectionInactiveBg ?? 0x30313d,
      opts.theme.minimumContrastRatio ?? 1,
      opts.theme.boldToBright ?? true,
    );
  }

  /** The renderer's cell size in pixels — the consumer needs it to map pointer
   * coordinates to cells (e.g. for the selection controller's geometry). */
  cellSize(): { width: number; height: number } {
    const s = this.backend.cellSize();
    return { width: s.width, height: s.height };
  }

  /** Resize the backend to a new canvas backing-buffer size (px). The caller
   * sizes the canvas; the next frame rebuilds the cell mirror to the new grid. */
  resize(width: number, height: number): void {
    this.backend.resize(width, height);
  }

  /** The terminal grid the backend currently fits — `{ cols, rows }`. Changes
   * after {@link resize}; the consumer drives its frames at these dimensions. */
  terminalSize(): { cols: number; rows: number } {
    const t = this.backend.terminalSize();
    return { cols: t.cols, rows: t.rows };
  }

  applyFrame(frame: DecodedFrame): void {
    // The mirror applies scroll ops + spans and yields the damaged cells; rebuild
    // it when the viewport size changes (a resize arrives as a Full frame).
    if (!this.mirror || this.cols !== frame.cols || this.rows !== frame.rows) {
      this.cols = frame.cols;
      this.rows = frame.rows;
      this.mirror = new CellMirror(
        frame.cols,
        frame.rows,
        this.palette,
        this.flagBits,
        makeRenderPolicy(this.flagBits, this.minimumContrastRatio),
        this.boldToBright,
      );
      // The mirror is fresh and the old overlay keys (`y·cols + x`) index a different
      // grid — drop them so the #140 delta doesn't repaint stale coordinates.
      this.prevOverlay = new Set();
    }
    const ops = this.mirror.applyFrame(frame);
    this.updateCursor(frame);

    this.paint(ops, highlightRects(frame), this.decorationSource?.(frame) ?? [], frame.kind === 0);
  }

  /**
   * Composite `ops` with the overlay tints (selection/search + #120 decorations)
   * AND the cursor into a batch, then present. `full` clears first — a Full frame
   * or a themeless full repaint (setTheme/setFocused). beamterm has no overlay
   * layer, so highlights/decorations are per-cell colour overrides layered
   * back-to-front (base < bottom < highlight < top); `composeOverlayDraws` also
   * adds the #140 delta for cells whose overlay membership flipped off-damage.
   */
  private paint(ops: DrawOp[], highlights: HighlightRect[], decorations: DecorationRect[], full: boolean): void {
    const batch = this.backend.batch();
    if (full) batch.clear(this.clearColor);
    const { draws, overlay } = composeOverlayDraws({
      ops,
      highlights,
      decorations,
      prevOverlay: this.prevOverlay,
      cols: this.cols,
      rows: this.rows,
      colors: { selectionBg: this.activeSelectionBg(), matchBg: this.matchBg },
      cellAt: (x, y) => this.mirror!.cellAt(x, y),
    });
    for (const d of draws) this.drawOp(batch, d);
    this.prevOverlay = overlay;
    // Retain for the blink loop's off-phase cursor tint (#210).
    this.lastHighlights = highlights;
    this.lastDecorations = decorations;
    // Overlay the cursor into the same batch — it draws over its mirror cell,
    // AFTER the decoration/highlight compose above, and cell-inverts the cell's
    // ORIGINAL (un-composed) colours, taking visual precedence over a decoration.
    this.lastBlinkOn = this.blink.isVisible(now());
    this.overlayCursor(batch, this.lastBlinkOn);
    this.startBlinkLoop();
  }

  /** The selection tint for the current focus state (#115): focused → selectionBg,
   * blurred → the dimmer selectionInactiveBg (xterm's two selection colours). */
  private activeSelectionBg(): number {
    return this.focused ? this.selectionBg : this.selectionInactiveBg;
  }

  /**
   * Swap the theme (#115): rebuild the palette + render policy + overlay colours,
   * then re-resolve every stored cell (the mirror keeps colour refs) and full-
   * repaint. No new frame is needed — a live theme change reflows all colours.
   */
  setTheme(theme: Theme): void {
    this.palette = {
      colors: this.buildPalette(Uint32Array.from(theme.ansi)),
      defaultFg: theme.defaultFg,
      defaultBg: theme.defaultBg,
    };
    this.clearColor = opaque(theme.defaultBg);
    this.cursorColor = theme.cursorColor ?? theme.defaultFg;
    this.selectionBg = theme.selectionBg ?? 0x45475a;
    this.matchBg = theme.matchBg ?? 0x6e5c00;
    this.selectionInactiveBg = theme.selectionInactiveBg ?? 0x30313d;
    this.minimumContrastRatio = theme.minimumContrastRatio ?? 1;
    this.boldToBright = theme.boldToBright ?? true;
    if (!this.mirror) return;
    const ops = this.mirror.recolor(
      this.palette,
      makeRenderPolicy(this.flagBits, this.minimumContrastRatio),
      this.boldToBright,
    );
    this.paint(ops, this.lastHighlights, this.lastDecorations, true);
  }

  /**
   * Focus gates the blink: an unfocused terminal stops blinking. xterm draws a
   * *hollow* (outline) cursor when unfocused, but that is sub-cell geometry
   * beamterm can't render (ADR-0012), so we keep a solid cursor — only the blink
   * stops.
   */
  setFocused(focused: boolean): void {
    this.blink.setFocused(focused);
    if (this.focused === focused) {
      this.redrawCursor();
      return;
    }
    // #115: focus flips the selection colour (active ↔ inactive). No frame changed,
    // so full-repaint the mirror through the overlay compose to re-tint (the cursor
    // is redrawn as part of paint), like a live theme change.
    this.focused = focused;
    if (this.mirror) this.paint(this.mirror.repaintAll(), this.lastHighlights, this.lastDecorations, true);
    else this.redrawCursor();
  }

  /** Stop the blink loop. */
  dispose(): void {
    if (this.rafId !== undefined) {
      cancelAnimationFrame(this.rafId);
      this.rafId = undefined;
    }
  }

  private updateCursor(frame: DecodedFrame): void {
    if (frame.cursorRow === undefined && frame.cursorVisible === undefined) return;
    const next = {
      row: frame.cursorRow ?? 0,
      col: frame.cursorCol ?? 0,
      shape: frame.cursorShape ?? 0,
      visible: frame.cursorVisible ?? false,
    };
    // A move (or first appearance) restarts the blink so the cursor shows at once.
    if (!this.cursor || next.row !== this.cursor.row || next.col !== this.cursor.col) {
      this.blink.restart(now());
    }
    this.cursor = next;
  }

  /** Draw the cursor cell — the cursor-invert when `on`, otherwise the cell's
   * composited overlay tint (#210), so a selected/decorated cursor cell keeps its
   * highlight during the blink-off gap instead of flashing to the bare cell. */
  private overlayCursor(batch: Batch, on: boolean): void {
    if (!this.mirror || !this.cursor || !this.cursor.visible) return;
    const { col, row, shape } = this.cursor;
    if (col >= this.cols || row >= this.rows) return;
    const base = this.mirror.cellAt(col, row);
    if (!base) return; // a wide-char spacer half — the cursor sits on the lead, not here
    const styled = cursorOp(base, shape, this.cursorColor);
    this.drawOp(
      batch,
      cursorCellDraw(base, on, styled, this.lastHighlights, this.lastDecorations, {
        selectionBg: this.activeSelectionBg(),
        matchBg: this.matchBg,
      }),
    );
  }

  /** Re-render just the cursor cell (used by the blink loop + focus changes). */
  private redrawCursor(): void {
    const batch = this.backend.batch();
    this.overlayCursor(batch, this.blink.isVisible(now()));
    this.backend.render();
  }

  /** A rAF loop that re-renders the cursor cell whenever its blink phase flips. */
  private startBlinkLoop(): void {
    if (this.rafId !== undefined) return;
    const tick = (): void => {
      const on = this.blink.isVisible(now());
      if (on !== this.lastBlinkOn) {
        this.lastBlinkOn = on;
        this.redrawCursor();
      }
      this.rafId = requestAnimationFrame(tick);
    };
    this.rafId = requestAnimationFrame(tick);
  }

  private drawOp(batch: Batch, op: DrawOp): void {
    let st = this.factory.style().fg(op.fg).bg(op.bg);
    if (op.bold) st = st.bold();
    if (op.italic) st = st.italic();
    if (op.underline) st = st.underline();
    if (op.strikethrough) st = st.strikethrough();
    batch.cell(op.x, op.y, this.factory.cell(op.symbol, st));
  }

  render(): void {
    this.backend.render();
  }
}
