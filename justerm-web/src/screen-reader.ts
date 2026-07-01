/**
 * Screen-reader-active gate (#161). VSCode drives its terminal a11y off
 * `IAccessibilityService.onDidChangeScreenReaderOptimized`, and xterm.js only
 * builds its `AccessibilityManager` (row tree + aria-live) while `screenReaderMode`
 * is on — both gate the a11y machinery on whether a screen reader is present.
 *
 * A browser CANNOT reliably detect a screen reader (a physical boundary), so
 * justerm does NOT auto-detect: the host injects the state via {@link setActive}
 * (ADR-0017 — detection is the host's, the gate is policy). It defaults to
 * ACTIVE so a consumer that never wires the seam keeps announcing (defaulting off
 * would silently kill a11y); a host that knows no SR is attached opts INTO
 * suppression to avoid wasted aria-live churn and earcons nobody hears.
 *
 * The gate wraps the announce *sink* rather than short-circuiting the controller,
 * so #119's own bookkeeping stays current while inactive — flipping SR on later
 * then does not replay the backlog of output that arrived while it was off.
 *
 * The command announce/signal (#160) is gated differently: its controller reads
 * {@link isActive} directly through #167's per-outcome `auto` policy state (a
 * blanket signal wrapper couldn't express an `on` override), so there is no
 * `gateSignal` here — only `gateLive`, for #119's output announce.
 */

import type { LiveRegionSink } from "./accessibility";

export class ScreenReaderState {
  private active = true;

  /** Whether a screen reader is currently considered active. */
  isActive(): boolean {
    return this.active;
  }

  /** The host sets this from its own SR detection (e.g. the platform a11y
   * service). Read per announce/signal, so a toggle takes effect immediately. */
  setActive(active: boolean): void {
    this.active = active;
  }

  /** Wrap a {@link LiveRegionSink} so `announce` is a no-op while inactive.
   * `clear` still forwards — it's cleanup, not output. */
  gateLive(sink: LiveRegionSink): LiveRegionSink {
    return {
      announce: (text) => {
        if (this.active) sink.announce(text);
      },
      clear: () => sink.clear(),
    };
  }
}
