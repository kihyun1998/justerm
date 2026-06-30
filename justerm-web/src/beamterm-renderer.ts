import type { BeamtermRenderer as Backend, Batch, Cell, CellStyle } from "@beamterm/renderer";
import type { Palette } from "justerm-wasm-decode/colors.js";
import { CellMirror } from "./cell-mirror";
import { CursorBlink, cursorOp } from "./cursor";
import { highlightAt, highlightRects } from "./overlay";
import type { DrawOp, FlagBits } from "./render-core";
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

  private constructor(
    private readonly backend: Backend,
    private readonly factory: Factory,
    private readonly palette: Palette,
    private readonly flagBits: FlagBits,
    private readonly clearColor: number,
    private readonly cursorColor: number,
    private readonly selectionBg: number,
    private readonly matchBg: number,
  ) {}

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
    };
    const factory: Factory = { style: beamterm.style, cell: beamterm.cell };
    return new BeamtermRenderer(
      backend,
      factory,
      palette,
      flagBits,
      opaque(opts.theme.defaultBg),
      opts.theme.cursorColor ?? opts.theme.defaultFg,
      opts.theme.selectionBg ?? 0x45475a,
      opts.theme.matchBg ?? 0x6e5c00,
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
      this.mirror = new CellMirror(frame.cols, frame.rows, this.palette, this.flagBits);
    }
    const ops = this.mirror.applyFrame(frame);
    this.updateCursor(frame);

    const batch = this.backend.batch();
    // Full frames repaint the whole viewport; partial frames retain untouched
    // cells (damage-only), so only full clears.
    if (frame.kind === 0) batch.clear(this.clearColor);
    // Blend selection/search highlights into each painted cell's background —
    // beamterm has no overlay layer, so a highlight is a per-cell bg swap (like
    // the cursor's cell-invert). NB: this tints only the cells in `ops`; on a
    // partial frame a cell whose highlight state just flipped isn't repainted —
    // needs old+new overlay-cell damage like the cursor (#140). Correct today
    // only on Full frames (the demo pushes Full).
    const rects = highlightRects(frame);
    for (const op of ops) {
      const kind = rects.length ? highlightAt(rects, op.x, op.y) : null;
      this.drawOp(batch, kind ? { ...op, bg: kind === "selection" ? this.selectionBg : this.matchBg } : op);
    }
    // Overlay the cursor into the same batch (it draws over its mirror cell).
    this.lastBlinkOn = this.blink.isVisible(now());
    this.overlayCursor(batch, this.lastBlinkOn);
    this.startBlinkLoop();
  }

  /**
   * Focus gates the blink: an unfocused terminal stops blinking. xterm draws a
   * *hollow* (outline) cursor when unfocused, but that is sub-cell geometry
   * beamterm can't render (ADR-0012), so we keep a solid cursor — only the blink
   * stops.
   */
  setFocused(focused: boolean): void {
    this.blink.setFocused(focused);
    this.redrawCursor();
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

  /** Draw the cursor cell — styled when `on`, otherwise its plain mirror cell. */
  private overlayCursor(batch: Batch, on: boolean): void {
    if (!this.mirror || !this.cursor || !this.cursor.visible) return;
    const { col, row, shape } = this.cursor;
    if (col >= this.cols || row >= this.rows) return;
    const base = this.mirror.cellAt(col, row);
    this.drawOp(batch, on ? cursorOp(base, shape, this.cursorColor) : base);
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
