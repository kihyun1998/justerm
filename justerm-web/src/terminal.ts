import type { FrameSource, Unsubscribe } from "./types";
import type { Renderer } from "./renderer";

/**
 * The browser terminal widget: wires a {@link FrameSource} to a {@link Renderer}.
 *
 * It owns no transport and no GL — both are injected. Each frame from the
 * source is handed to the renderer and presented. This keeps the widget
 * source-agnostic (frame mode / in-wasm) and renderer-agnostic (real beamterm
 * / fake), which is what makes it testable without a backend or a canvas.
 */
export class Terminal {
  private unsubscribe: Unsubscribe | undefined;

  constructor(
    private readonly source: FrameSource,
    private readonly renderer: Renderer,
  ) {}

  /** Begin consuming frames from the source. */
  mount(): void {
    this.unsubscribe = this.source.subscribe((frame) => {
      this.renderer.applyFrame(frame);
      this.renderer.render();
    });
  }

  /** Stop consuming frames; safe to call more than once. */
  dispose(): void {
    this.unsubscribe?.();
    this.unsubscribe = undefined;
  }
}
