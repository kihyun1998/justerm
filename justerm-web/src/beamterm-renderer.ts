import { BeamtermRenderer as Backend, main } from "@beamterm/renderer";
import type { DecodedFrame } from "./types";
import type { Renderer } from "./renderer";

export interface BeamtermOptions {
  /** CSS selector of the canvas to attach to, e.g. `"#term"`. */
  canvasSelector: string;
  fontFamily: string;
  fontSize: number;
  /** Background (0xRRGGBBAA) used to clear on full frames (consumer's theme). */
  defaultBg: number;
}

/**
 * The real {@link Renderer}: wraps `@beamterm/renderer` (WASM + WebGL).
 *
 * Not exercised by the vitest suite — it needs a GL context, so it's verified
 * by the manual harness (`index.html`) in a browser. The init + draw pattern
 * mirrors penterm's working terminal-native integration: `main()` to init the
 * WASM, `withDynamicAtlas` to attach, and `batch()` → draw → `render()` per
 * frame (clear only on `full`; `partial` retains untouched cells = damage-only).
 *
 * S1 paints an empty grid (clear on full). S2 (#105) fills the batch with the
 * frame's cells via resolveRgb + flags + wide-char handling.
 */
export class BeamtermRenderer implements Renderer {
  private constructor(
    private readonly backend: Backend,
    private readonly defaultBg: number,
  ) {}

  static async create(opts: BeamtermOptions): Promise<BeamtermRenderer> {
    await main(); // beamterm WASM init (idempotent)
    const backend = Backend.withDynamicAtlas(
      opts.canvasSelector,
      [opts.fontFamily, "monospace"],
      opts.fontSize,
      false, // auto_resize_canvas_css: external CSS controls sizing
    );
    return new BeamtermRenderer(backend, opts.defaultBg);
  }

  applyFrame(frame: DecodedFrame): void {
    const batch = this.backend.batch();
    if (frame.kind === "full") {
      batch.clear(this.defaultBg);
    }
    // S2 (#105): walk the frame's SoA cells → batch.cell(x, y, …) here.
  }

  render(): void {
    this.backend.render();
  }
}
