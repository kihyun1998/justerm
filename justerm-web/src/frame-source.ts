import type { DecodedFrame, FrameSource, Unsubscribe } from "./types";

/**
 * An in-memory {@link FrameSource} you push frames into by hand.
 *
 * Used by tests and demos to drive the widget without a backend or engine —
 * the simplest concrete source behind the seam.
 */
export class StubFrameSource implements FrameSource {
  private readonly listeners = new Set<(frame: DecodedFrame) => void>();

  subscribe(listener: (frame: DecodedFrame) => void): Unsubscribe {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** Emit a frame to every current subscriber. */
  push(frame: DecodedFrame): void {
    for (const listener of this.listeners) {
      listener(frame);
    }
  }
}
