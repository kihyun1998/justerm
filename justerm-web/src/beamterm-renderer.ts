import type { BeamtermRenderer as Backend, Cell, CellStyle } from "@beamterm/renderer";
import type { Palette } from "justerm-wasm-decode/colors.js";
import { frameToDrawOps } from "./render-core";
import type { FlagBits } from "./render-core";
import type { DecodedFrame } from "./types";
import type { Renderer } from "./renderer";

/** Theme colours (packed `0xRRGGBB`). The engine stays ignorant of these — the
 * consumer owns them and the renderer resolves cell refs against them. */
export interface Theme {
  /** The 16 ANSI colours (slots `0..15`); `buildPalette` fills `16..255`. */
  ansi: number[];
  defaultFg: number;
  defaultBg: number;
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
  private constructor(
    private readonly backend: Backend,
    private readonly factory: Factory,
    private readonly palette: Palette,
    private readonly flagBits: FlagBits,
    private readonly clearColor: number,
  ) {}

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
    return new BeamtermRenderer(backend, factory, palette, flagBits, opaque(opts.theme.defaultBg));
  }

  applyFrame(frame: DecodedFrame): void {
    const batch = this.backend.batch();
    // Full frames repaint the whole viewport; partial frames retain untouched
    // cells (damage-only), so only full clears.
    if (frame.kind === 0) batch.clear(this.clearColor);
    for (const op of frameToDrawOps(frame, this.palette, this.flagBits)) {
      let st = this.factory.style().fg(op.fg).bg(op.bg);
      if (op.bold) st = st.bold();
      if (op.italic) st = st.italic();
      if (op.underline) st = st.underline();
      if (op.strikethrough) st = st.strikethrough();
      batch.cell(op.x, op.y, this.factory.cell(op.symbol, st));
    }
  }

  render(): void {
    this.backend.render();
  }
}
