import { describe, it, expect, vi } from "vitest";
import {
  Terminal,
  rendererNotifyingSink,
  routeWheel,
  textareaMove,
  wheelGoesToApp,
  wheelScrollTarget,
  preeditIntent,
} from "../src/terminal";
import { StubFrameSource } from "../src/frame-source";
import { MouseEvents, StubInputSink } from "../src/input";
import type { Intent } from "../src/input";
import type { DecodedFrame } from "../src/types";
import type { Renderer } from "../src/renderer";

/** A test-double renderer: records what the widget hands it, no WebGL. */
class FakeRenderer implements Renderer {
  applied: DecodedFrame[] = [];
  renderCount = 0;
  /** #606: counted rather than flagged, so "disposed twice" is a distinguishable failure. */
  disposeCount = 0;
  applyFrame(frame: DecodedFrame): void {
    this.applied.push(frame);
  }
  render(): void {
    this.renderCount++;
  }
  dispose(): void {
    this.disposeCount++;
  }
}

const emptyFrame = (cols: number, rows: number): DecodedFrame => ({
  cols,
  rows,
  kind: 0, // Full
  codepoints: [],
  fg: [],
  bg: [],
  flags: [],
  extra: [],
  spans: [],
  sideTable: [],
});

describe("Terminal wiring", () => {
  it("forwards a source frame to the renderer and presents it", () => {
    const source = new StubFrameSource();
    const renderer = new FakeRenderer();
    const term = new Terminal(source, renderer);
    term.mount();

    const frame = emptyFrame(80, 24);
    source.push(frame);

    expect(renderer.applied).toEqual([frame]);
    expect(renderer.renderCount).toBe(1);
  });

  it("stops rendering after dispose", () => {
    const source = new StubFrameSource();
    const renderer = new FakeRenderer();
    const term = new Terminal(source, renderer);
    term.mount();
    term.dispose();

    source.push(emptyFrame(80, 24));

    expect(renderer.applied).toEqual([]);
    expect(renderer.renderCount).toBe(0);
  });

  // #606: unsubscribing stops *frames* reaching the renderer, which is what the test above
  // asserts — but the renderer keeps its own clocks (a rAF blink loop, a `prefers-reduced-motion`
  // listener that draws), so a disposed widget could still repaint its canvas. The widget is
  // handed the renderer, so the widget ends it: the same rule xterm.js applies to a
  // consumer-constructed addon (`AddonManager.dispose()` calls `instance.dispose()` on each), and
  // the same one `FrameSource` already gets here through the `Unsubscribe` it returns.
  it("disposes the renderer it was handed (#606)", () => {
    const source = new StubFrameSource();
    const renderer = new FakeRenderer();
    const term = new Terminal(source, renderer);
    term.mount();
    term.dispose();

    expect(renderer.disposeCount).toBe(1);
  });

  it("disposes the renderer even when the widget was never mounted", () => {
    // `dispose()` is documented safe to call on an unmounted widget, and a consumer that builds a
    // Terminal and bails before mounting still handed over a renderer that may already have started
    // its loop (`JustermRenderer.create` applies the consumer's blink options before any frame).
    const renderer = new FakeRenderer();
    new Terminal(new StubFrameSource(), renderer).dispose();

    expect(renderer.disposeCount).toBe(1);
  });

  it("does not dispose the renderer twice", () => {
    // The port requires `dispose()` to be idempotent, but the widget must not lean on that: the
    // consumer may hold the same renderer and call it too. xterm.js buys idempotence by wrapping
    // the addon's own method (`AddonManager.ts:28-37`); this side simply does not call twice.
    const renderer = new FakeRenderer();
    const term = new Terminal(new StubFrameSource(), renderer);
    term.mount();
    term.dispose();
    term.dispose();

    expect(renderer.disposeCount).toBe(1);
  });

  it("refuses to re-mount after dispose, rather than half-working (#606)", () => {
    // `dispose()` is END OF LIFE, not unmount — declared because the alternative is already broken:
    // `textareaCell` + `cursorAnchor` survive dispose, so a remounted widget parks the IME candidate
    // window at the previous mount's anchor (since #631, until the next focus or composition start
    // re-syncs it — shorter, not gone), and a re-mounted renderer would have lost its
    // reduced-motion listener permanently (its only `addEventListener` is in a private constructor).
    // xterm.js draws the same line — its disposable store latches and `open()` has no re-entry.
    const term = new Terminal(new StubFrameSource(), new FakeRenderer());
    term.mount();
    term.dispose();

    expect(() => term.mount()).toThrow(/disposed/i);
  });

  // #117: the event channel is independent of the DOM group, so it wires with no
  // element (an output-only widget) — testable in node.
  it("routes source events to the consumer handlers", () => {
    const source = new StubFrameSource();
    const onTitle = vi.fn();
    const onBell = vi.fn();
    const term = new Terminal(source, new FakeRenderer(), { events: { onTitle, onBell } });
    term.mount();

    source.pushEvent({ type: "title", title: "make — build" });
    source.pushEvent({ type: "bell" });

    expect(onTitle).toHaveBeenCalledWith("make — build");
    expect(onBell).toHaveBeenCalledTimes(1);
  });

  it("stops routing events after dispose", () => {
    const source = new StubFrameSource();
    const onTitle = vi.fn();
    const term = new Terminal(source, new FakeRenderer(), { events: { onTitle } });
    term.mount();
    term.dispose();

    source.pushEvent({ type: "title", title: "x" });

    expect(onTitle).not.toHaveBeenCalled();
  });
});

// --- S16 (#133): wheel routing — app vs scrollback (the #129 mask) ---

describe("wheelGoesToApp", () => {
  // The app tracks wheel only when the WHEEL bit (2) is set on the frame's
  // mouseWantedEvents mask (#129). Then a notch reports to the app; else it
  // stays local (scrollback). Normal (?1000) and up include WHEEL.
  it("is true only when the WHEEL bit is set", () => {
    expect(wheelGoesToApp(MouseEvents.Wheel)).toBe(true);
    expect(wheelGoesToApp(MouseEvents.Down | MouseEvents.Up | MouseEvents.Wheel)).toBe(true);
  });

  it("is false with no mask, or a mask without WHEEL (X10 = DOWN only)", () => {
    expect(wheelGoesToApp(0)).toBe(false);
    expect(wheelGoesToApp(MouseEvents.Down)).toBe(false); // X10: press only, no wheel
    expect(wheelGoesToApp(undefined)).toBe(false); // frame omitted the field
  });
});

describe("wheelScrollTarget", () => {
  // WheelScroller yields lines where positive = down/newer; displayOffset is
  // lines UP from the bottom (0 = following), so scrolling newer LOWERS it.
  it("scrolling down (positive lines) lowers the offset", () => {
    expect(wheelScrollTarget(3, 10, 100)).toBe(7);
  });

  it("scrolling up (negative lines) raises the offset", () => {
    expect(wheelScrollTarget(-3, 10, 100)).toBe(13);
  });

  it("clamps to the bottom (0) and to scrollbackLen", () => {
    expect(wheelScrollTarget(5, 2, 100)).toBe(0); // 2-5 = -3 → 0 (can't pass the live edge)
    expect(wheelScrollTarget(-5, 98, 100)).toBe(100); // 98+5 = 103 → 100 (top of history)
  });

  it("returns null for a zero-line notch (nothing to request)", () => {
    expect(wheelScrollTarget(0, 10, 100)).toBeNull();
  });

  // #675 — the clamp here is the same `Math.max(0, Math.min(len, x))` shape #672
  // fixed one file over: it passes `NaN` straight through. This function is
  // exported, so it owes its own totality rather than trusting its one in-repo
  // caller — and the offset it is *given* can already be poisoned, which is how
  // the widget stayed broken after the scroller itself had recovered.
  it("returns null rather than a non-finite target (#675)", () => {
    expect(wheelScrollTarget(NaN, 10, 100)).toBeNull();
    expect(wheelScrollTarget(Infinity, 10, 100)).toBeNull();
    expect(wheelScrollTarget(3, NaN, 100)).toBeNull(); // a poisoned offset coming back in
    expect(wheelScrollTarget(3, 10, NaN)).toBeNull();
  });
});

// --- S16 (#133): routeWheel — the whole app/alt/scrollback decision (adversarial
// pass, 2-lens: xterm gates the app path AND the alt-buffer cursor-key path on the
// SAME accumulated `lines`; the local path clamps like the scrollbar drag). ---

describe("routeWheel", () => {
  it("a zero-line notch is `none` regardless of mode (accumulator gates first)", () => {
    expect(routeWheel(MouseEvents.Wheel, 0, false, 10, 100)).toEqual({ kind: "none" });
    expect(routeWheel(0, 0, true, 10, 100)).toEqual({ kind: "none" });
  });

  it("routes to the app when it tracks the wheel — direction only, by line sign", () => {
    expect(routeWheel(MouseEvents.Wheel, 3, false, 10, 100)).toEqual({ kind: "app", direction: "down" });
    expect(routeWheel(MouseEvents.Wheel, -2, false, 10, 100)).toEqual({ kind: "app", direction: "up" });
  });

  // #675 — `lines === 0` is the existing gate and `NaN === 0` is false, so a
  // non-finite count reaches every branch below it. The app branch is the one
  // that fails *quietly*: direction comes from `lines < 0`, which is false for
  // NaN, so a poisoned scroller reports a **fabricated `down`** to the
  // application rather than reporting nothing. The local branch is the loud one
  // — it hands the consumer a non-finite offset, which is what latched.
  it("a non-finite line count is `none` on every branch, not a fabricated direction (#675)", () => {
    expect(routeWheel(MouseEvents.Wheel, NaN, false, 10, 100)).toEqual({ kind: "none" }); // app
    expect(routeWheel(0, NaN, true, 10, 100)).toEqual({ kind: "none" }); // altKeys
    expect(routeWheel(0, NaN, false, 10, 100)).toEqual({ kind: "none" }); // local scroll
    expect(routeWheel(0, Infinity, false, 10, 100)).toEqual({ kind: "none" });
  });

  // A poisoned offset arriving from a frame must not become a scroll request
  // either — the same reason, one argument over.
  it("a non-finite display offset is `none` rather than a non-finite request (#675)", () => {
    expect(routeWheel(0, 3, false, NaN, 100)).toEqual({ kind: "none" });
  });

  it("the app path wins over the alt-buffer path (a wheel-tracking TUI on alt)", () => {
    expect(routeWheel(MouseEvents.Wheel, -2, true, 10, 100)).toEqual({ kind: "app", direction: "up" });
  });

  it("on the alt screen (no scrollback), a non-tracking app gets cursor keys (xterm)", () => {
    expect(routeWheel(0, -2, true, 10, 100)).toEqual({ kind: "altKeys", direction: "up" });
    expect(routeWheel(0, 4, true, 10, 100)).toEqual({ kind: "altKeys", direction: "down" });
  });

  it("normal buffer, no tracking → a clamped local scrollback request", () => {
    expect(routeWheel(0, 3, false, 10, 100)).toEqual({ kind: "scroll", displayOffset: 7 });
    expect(routeWheel(0, -3, false, 10, 100)).toEqual({ kind: "scroll", displayOffset: 13 });
    expect(routeWheel(0, 5, false, 2, 100)).toEqual({ kind: "scroll", displayOffset: 0 }); // clamp bottom
    expect(routeWheel(0, -5, false, 98, 100)).toEqual({ kind: "scroll", displayOffset: 100 }); // clamp top
  });
});

// --- S16 (#133): the renderer-notifying sink — typing restarts the blink (the S5
// #107 deferral), focus/blur drives the renderer's focus state (blink + selection
// tint gating, the sibling-lens gap: setFocused was built but unwired). ---

describe("rendererNotifyingSink", () => {
  const fakeRenderer = () => ({
    applyFrame: vi.fn(),
    render: vi.fn(),
    restartCursorBlink: vi.fn(),
    setFocused: vi.fn(),
  });

  it("restarts the cursor blink on a key intent, then forwards it", () => {
    const inner = new StubInputSink();
    const r = fakeRenderer();
    const sink = rendererNotifyingSink(inner, r);

    const key: Intent = {
      kind: "key",
      event: { key: { type: "char", char: "a" }, mods: 0, action: "press" },
    };
    sink.send(key);

    expect(r.restartCursorBlink).toHaveBeenCalledTimes(1);
    expect(r.setFocused).not.toHaveBeenCalled();
    expect(inner.sent).toEqual([key]);
  });

  it("drives renderer focus state on a focus intent, then forwards it", () => {
    const inner = new StubInputSink();
    const r = fakeRenderer();
    const sink = rendererNotifyingSink(inner, r);

    const blur: Intent = { kind: "focus", focused: false };
    sink.send(blur);

    expect(r.setFocused).toHaveBeenCalledWith(false);
    expect(r.restartCursorBlink).not.toHaveBeenCalled();
    expect(inner.sent).toEqual([blur]);
  });

  it("forwards paste untouched (no renderer notification)", () => {
    const inner = new StubInputSink();
    const r = fakeRenderer();
    const sink = rendererNotifyingSink(inner, r);

    const paste: Intent = { kind: "paste", text: "hi" };
    sink.send(paste);

    expect(r.restartCursorBlink).not.toHaveBeenCalled();
    expect(r.setFocused).not.toHaveBeenCalled();
    expect(inner.sent).toEqual([paste]);
  });

  it("tolerates a renderer that omits the optional hooks (no throw)", () => {
    const inner = new StubInputSink();
    const sink = rendererNotifyingSink(inner, { applyFrame: vi.fn(), render: vi.fn() });

    expect(() =>
      sink.send({ kind: "key", event: { key: { type: "enter" }, mods: 0, action: "press" } }),
    ).not.toThrow();
    expect(() => sink.send({ kind: "focus", focused: true })).not.toThrow();
  });
});

/**
 * #631 — the textarea anchor's dedupe. The cache key is the cursor's **cell coordinate**, but the
 * anchor is computed from the **geometry**, and a cell-size change moves the geometry while leaving
 * the coordinate identical. So the decision needs a way to say "re-read anyway", which is what the
 * point-of-use callers (composition start, focus) pass.
 */
describe("textareaMove (#631)", () => {
  it("moves when the cursor moved to another cell", () => {
    expect(textareaMove({ col: 2, row: 5 }, "2,4", false, false)).toEqual({ col: 2, row: 5, key: "2,5" });
  });

  it("skips an unchanged cell — the per-frame layout read this cache exists to avoid", () => {
    expect(textareaMove({ col: 2, row: 5 }, "2,5", false, false)).toBeUndefined();
  });

  it("moves anyway at an unchanged cell when forced: the geometry may have moved under it", () => {
    // The whole defect. `setLineHeight(1.6)` with a stationary cursor leaves the anchor at
    // `row * oldCellHeight`; the coordinate cache cannot see that, so the caller overrides it.
    expect(textareaMove({ col: 2, row: 5 }, "2,5", true, false)).toEqual({ col: 2, row: 5, key: "2,5" });
  });

  it("stays undefined with no cursor to anchor to, forced or not", () => {
    expect(textareaMove(undefined, "2,5", false, false)).toBeUndefined();
    expect(textareaMove(undefined, "2,5", true, false)).toBeUndefined();
  });

  it("distinguishes a column-only move from a row-only move (the key carries both)", () => {
    expect(textareaMove({ col: 3, row: 5 }, "2,5", false, false)?.key).toBe("3,5");
    expect(textareaMove({ col: 2, row: 6 }, "2,5", false, false)?.key).toBe("2,6");
  });
});

/**
 * #637 — an output frame may not move the anchor while a composition is open.
 *
 * Adjudicated by measurement, not by reading the reference: with a real Korean IME on Windows the
 * Hanja candidate window **follows the anchor down** as unsolicited output moves the cursor, so the OS
 * re-reads the anchor mid-composition and the window walks away from the text being composed.
 *
 * That also dissolves xterm.js's apparent self-contradiction — `_syncTextArea` bailing while composing
 * (`browser/CoreBrowserTerminal.ts:338`) and `CompositionHelper.updateCompositionElements` rewriting
 * `left`/`top` every render (`browser/input/CompositionHelper.ts:273-274`) are not rival rules. Since
 * the OS re-reads, whoever writes the position owns the window: xterm suppresses the INVOLUNTARY writer
 * (the frame stream) and keeps the VOLUNTARY one (tracking its own preedit view). #249 gave this
 * widget that second writer (ADR-0028 D4) — and this guard did NOT have to give, which is what the
 * prediction here got wrong: the preedit writer re-aims through `writeTextareaAnchor` and never
 * consults `textareaMove`, so this stays a rule about the involuntary writers alone.
 *
 * The rule lives here rather than in `positionTextarea` because the callers funnel through one seam.
 *
 * **#649 narrowed it: `composing` now outranks `force`, and `composing` means `isComposing` alone.**
 * `force` originally won so that #631's `compositionstart` re-sync could not be gated by its own guard
 * — but that re-sync does not need the override, because it runs BEFORE the controller is told and so
 * sees `composing === false`. What the override actually bought was a second, unwanted entrance: a
 * pointer-down (or any consumer `Terminal.focus()`) re-anchored to the superseded cursor cell
 * mid-composition, the same harm through a different trigger. So `force` now carries one meaning,
 * "override the coordinate cache", and none of "override the composition rule".
 *
 * The predicate is `isComposing`, never `active`: `active` stays true through the deferred commit read,
 * which is precisely the window a continuous-CJK `compositionstart` lands in. Pinned on both sides —
 * `composition.test.ts` proves the two diverge there, and the callers below prove which one this seam
 * is asking about.
 */
describe("textareaMove — the mid-composition guard (#637)", () => {
  it("refuses an output-frame move while a composition is open", () => {
    expect(textareaMove({ col: 2, row: 8 }, "2,5", false, true)).toBeUndefined();
  });

  it("still moves on an output frame when no composition is open", () => {
    expect(textareaMove({ col: 2, row: 8 }, "2,5", false, false)).toEqual({
      col: 2,
      row: 8,
      key: "2,8",
    });
  });

  it("refuses a FORCED re-sync too, so a pointer-down cannot re-anchor mid-composition (#649)", () => {
    // `element` mousedown → onDown → Terminal.focus() → syncTextareaAnchor(force: true). The retained
    // cursor cell keeps advancing during a composition (positionTextarea latches it before the guard),
    // so a forced move here lands on exactly the superseded cell #637 exists to stop using.
    expect(textareaMove({ col: 2, row: 8 }, "2,5", true, true)).toBeUndefined();
  });

  it("still lets #631's compositionstart re-sync through — it runs before `composing` flips", () => {
    // The side condition that makes #649's fix safe rather than a regression: onStart re-anchors
    // BEFORE composition.compositionStart(), so this caller sees composing === false and the forced
    // override of the coordinate cache — force's one remaining job — still works.
    expect(textareaMove({ col: 2, row: 5 }, "2,5", true, false)).toEqual({
      col: 2,
      row: 5,
      key: "2,5",
    });
  });

  it("leaves the dedupe cache untouched, so the anchor catches up after the composition ends", () => {
    // The suppressed move must NOT be recorded as applied: the caller keys its cache on the returned
    // move, so a swallowed one has to stay a cache miss or the anchor would sit at the stale cell for
    // as long as the cursor happens not to move again.
    const suppressed = textareaMove({ col: 2, row: 8 }, "2,5", false, true);
    expect(suppressed).toBeUndefined();
    expect(textareaMove({ col: 2, row: 8 }, "2,5", false, false)?.key).toBe("2,8");
  });
});

describe("preeditIntent — what a compositionupdate does to the drawn run (#249)", () => {
  const origin = { col: 3, row: 4 };

  it("pushes the run as code points, not UTF-16 units", () => {
    const got = preeditIntent("한글", "", origin);
    expect(Array.from(got!.codepoints)).toEqual([0xd55c, 0xae00]);
    expect(got!.origin).toEqual(origin);
    // An astral scalar is ONE cell's codepoint; splitting by UTF-16 unit would push two halves of a
    // surrogate pair and draw two replacement boxes.
    expect(Array.from(preeditIntent("\u{1F600}", "", origin)!.codepoints)).toEqual([0x1f600]);
  });

  it("drops an update whose data did not change", () => {
    // A real IME emits one settling update per syllable carrying the data it already sent
    // (measured). Each would otherwise cost a full re-pack.
    expect(preeditIntent("한", "한", origin)).toBeUndefined();
    expect(preeditIntent("한글", "한", origin)).toBeDefined();
  });

  it("pushes nothing without a latched origin", () => {
    expect(preeditIntent("한", "", undefined)).toBeUndefined();
  });

  it("treats the empty run as a real change, so a composition can be cleared", () => {
    // `compositionend` clears by pushing "" — if that were dropped as "no change" the last
    // syllable would stay on screen for the life of the widget.
    expect(preeditIntent("", "한", origin)).toBeDefined();
    expect(Array.from(preeditIntent("", "한", origin)!.codepoints)).toEqual([]);
  });
});
