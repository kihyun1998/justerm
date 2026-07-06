import type { DecodedFrame, FrameSource, Unsubscribe } from "./types";
import type { TermEvent } from "./events";

/**
 * An in-memory {@link FrameSource} you push frames (and events) into by hand.
 *
 * Used by tests and demos to drive the widget without a backend or engine —
 * the simplest concrete source behind the seam.
 */
export class StubFrameSource implements FrameSource {
  private readonly listeners = new Set<(frame: DecodedFrame) => void>();
  private readonly eventListeners = new Set<(event: TermEvent) => void>();

  subscribe(listener: (frame: DecodedFrame) => void): Unsubscribe {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  subscribeEvents(listener: (event: TermEvent) => void): Unsubscribe {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  /** Emit a frame to every current subscriber. */
  push(frame: DecodedFrame): void {
    for (const listener of this.listeners) {
      listener(frame);
    }
  }

  /** Emit a fire-and-forget event (#117) to every current event subscriber. */
  pushEvent(event: TermEvent): void {
    for (const listener of this.eventListeners) {
      listener(event);
    }
  }
}
