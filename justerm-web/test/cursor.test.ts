import { describe, expect, it } from "vitest";
import { CursorBlink } from "../src/cursor";

// #575 — the widget used to blink unconditionally and never read the frame's blink mode, so an
// application asking for a STEADY cursor (`CSI 2 q`, `CSI ?12 l`) got a blinking one. Blinking is
// now resolved from two inputs, mirroring the two references (both pinned trees, read 2026-07-28):
//
//   xterm.js  `browser/renderer/dom/DomRenderer.ts:531` @ 699f553
//             `decPrivateModes.cursorBlink ?? rawOptions.cursorBlink`  — app wins, option is fallback
//   alacritty `alacritty/src/event.rs:1631` @ 852e971
//             `cursor_style.blinking_override().unwrap_or(terminal_blinking)` — a consumer FORCE
//             wins, otherwise the app decides (`config/cursor.rs:125-131`: Always/Never => Some,
//             On/Off => None)
//
// justerm follows alacritty's placement, because it is the one ADR-0017 already implies: core
// reports the application's intent (mechanism), the consumer holds the three-state override
// (policy). That needs no wire change — core's `cursor_blink` bool is already enough.
describe("CursorBlink", () => {
  // The default is SOLID, not blinking, and this is the behaviour flip #575 is about. Both
  // references default the same way (xterm.js `OptionsService.ts:16` `cursorBlink: false`,
  // alacritty `config/cursor.rs:107` `Shape(shape) => blinking: false`), and core's
  // `Cursor::blink` starts `false` — so the widget's old unconditional blink was the outlier.
  it("is solid until something asks it to blink", () => {
    const blink = new CursorBlink();

    expect([blink.isVisible(0), blink.isVisible(600), blink.isVisible(1200)]).toEqual([true, true, true]);
  });

  // The application's intent arrives on every frame as `cursorBlink` (wire v4, #81), written by
  // BOTH DECSCUSR (`term.rs:2938`) and att610 `?12 h/l` (`term.rs:4505-4506`).
  it("follows the application's blink mode", () => {
    const blink = new CursorBlink();

    blink.setAppBlink(true);
    expect([blink.isVisible(0), blink.isVisible(600), blink.isVisible(1200)]).toEqual([true, false, true]);

    // `CSI 2 q` / `CSI ?12 l` — the app asks for steady, and it is honoured from that moment.
    blink.setAppBlink(false);
    expect([blink.isVisible(0), blink.isVisible(600), blink.isVisible(1200)]).toEqual([true, true, true]);
  });

  // The consumer override is the escape hatch both references have and justerm-web did not.
  // MEASURED, which is why it is not hypothetical (RHEL 9.2, real PTY, TERM=xterm-256color): of six
  // real applications, ZERO emit DECSCUSR, and vim/htop/top all emit `CSI ?12 l` — because
  // terminfo's own `cnorm=\E[?12l\E[?25h` carries a blink-off. So merely quitting vim pins the
  // cursor steady for the rest of the session, with no way back. `undefined` = follow the app.
  it("lets a consumer force blinking on, against the application", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(false); // the app (or terminfo's cnorm) said steady

    blink.setBlinkOverride(true);

    expect([blink.isVisible(0), blink.isVisible(600), blink.isVisible(1200)]).toEqual([true, false, true]);
  });

  it("lets a consumer force blinking off, against the application", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true); // the app asked to blink

    blink.setBlinkOverride(false);

    expect([blink.isVisible(0), blink.isVisible(600), blink.isVisible(1200)]).toEqual([true, true, true]);
  });

  // Clearing the override returns authority to the application rather than latching the last
  // forced value — the `??` semantics, not a copy.
  it("returns authority to the application when the override is cleared", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);
    blink.setBlinkOverride(false);
    expect(blink.isVisible(600)).toBe(true); // forced steady

    blink.setBlinkOverride(undefined);

    expect(blink.isVisible(600)).toBe(false); // the app's "blink" is in charge again
  });

  // xterm BLINK_INTERVAL = 600ms: the cursor shows for the first interval, hides for the next, and
  // so on. Time is injected so the state is testable without real timers.
  it("toggles visibility every 600ms", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);

    expect([blink.isVisible(0), blink.isVisible(599), blink.isVisible(600), blink.isVisible(1200)]).toEqual(
      [true, true, false, true],
    );
  });

  // prefers-reduced-motion (#119): the blink is motion the user asked to avoid, so the cursor stays
  // solid regardless of phase. NEITHER reference has this — xterm.js's `src` has zero
  // `prefers-reduced-motion` hits and alacritty is native — so the precedence is derived, not
  // ported: reduced motion only ever SUBTRACTS motion, so letting it win can never make a steady
  // cursor blink. It therefore outranks an application that explicitly asked to blink.
  it("stays solid when reduced motion is requested, even if the application asked to blink", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);
    blink.setReducedMotion(true);

    expect([blink.isVisible(0), blink.isVisible(600), blink.isVisible(1200)]).toEqual([true, true, true]);
  });

  // ...and it outranks a consumer force too: an accessibility preference is not overridable by the
  // page's own styling choice.
  it("stays solid under reduced motion even when the consumer forces blinking on", () => {
    const blink = new CursorBlink();
    blink.setBlinkOverride(true);
    blink.setReducedMotion(true);

    expect([blink.isVisible(0), blink.isVisible(600), blink.isVisible(1200)]).toEqual([true, true, true]);
  });

  // Typing or moving the cursor restarts the animation: the cursor shows at once and the interval
  // resets from that moment, so it never blinks off right after input (xterm restartBlinkAnimation).
  it("shows immediately and resets the phase on restart", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);
    expect(blink.isVisible(600)).toBe(false); // would be hidden mid-blink

    blink.restart(600);

    expect(blink.isVisible(600)).toBe(true); // shown at once
    expect(blink.isVisible(1199)).toBe(true); // still the first interval after restart
    expect(blink.isVisible(1200)).toBe(false); // hides 600ms later
  });

  // Unfocused terminals stop blinking — the cursor stays solid (xterm pause sets
  // isCursorVisible = true and clears the interval; alacritty gates on `is_focused`,
  // `alacritty/src/event.rs:1643`).
  it("stays solid while unfocused", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);

    blink.setFocused(false);
    expect(blink.isVisible(600)).toBe(true); // would blink off if focused
    expect(blink.isVisible(1800)).toBe(true);

    blink.setFocused(true);
    expect(blink.isVisible(600)).toBe(false); // focused again → blinks
  });
});

// #593 — both references stop blinking after a period with no user input, and justerm-web blinked
// forever. Verified in the pinned trees (2026-07-28): alacritty 5s (`config/cursor.rs:34`, reset by
// `on_typing_start` at `event.rs:1201-1213`) and xterm.js 5min
// (`browser/renderer/shared/Constants.ts:12`, reset by `restartBlinkAnimation`). ghostty has none.
//
// It is NOT a latch and needs no timer: `lastInput` is a timestamp the class already had to keep, so
// the whole rule is one more pure read of `(now, lastInput)` — which is what settles #591's doubt
// that a timeout could not fit a chain of pure functions.
describe("CursorBlink — idle timeout (#593)", () => {
  const MIN = 60_000;

  it("blinks normally while the idle timeout has not elapsed", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);

    expect([blink.isVisible(0), blink.isVisible(600), blink.isVisible(1200)]).toEqual([true, false, true]);
  });

  // The default is xterm.js's 5 minutes, not alacritty's 5 seconds. The two disagree by 60x so
  // neither is inheritable on authority; the tie-breaker is that justerm-web's reset set is already
  // exactly xterm.js's (key/text intents + pointer-down), so matching the reference we are
  // structurally identical to is the least arbitrary default available. It is an option regardless.
  it("defaults to five minutes, and goes solid once that passes with no input", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);

    // Just inside: still blinking (5min is an exact multiple of 600ms, so this phase is 'off').
    expect(blink.isVisible(5 * MIN - 600)).toBe(false);
    // Past it: solid, and solid means SHOWN — the cursor parks visible, it does not vanish.
    expect(blink.isVisible(5 * MIN + 1)).toBe(true);
    expect(blink.isVisible(5 * MIN + 601)).toBe(true);
  });

  it("is configurable, and a keystroke restarts the idle clock", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);
    blink.setIdleTimeout(10_000);

    expect(blink.isVisible(10_601)).toBe(true); // idled out

    blink.restartFromInput(10_601); // the user typed

    expect(blink.isVisible(11_201)).toBe(false); // blinking again, phase restarted
    expect(blink.isVisible(20_602)).toBe(true); // …and idles out again 10s later
  });

  // The discriminating case, and the reason `restart` and `restartFromInput` are separate methods.
  // A cursor MOVE is application output — `top` repainting once a second moves the cursor — and both
  // references reset their clock on *user input* only. Feeding the move into the idle clock would
  // mean a terminal running any live TUI never idles out, which is broader than either reference.
  it("a cursor move restarts the phase but NOT the idle clock", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);
    blink.setIdleTimeout(10_000);

    blink.restart(9_000); // the app moved the cursor — phase resets from here

    expect(blink.isVisible(9_300)).toBe(true); // phase was restarted at 9000, so this is 'on'
    expect(blink.isVisible(9_601)).toBe(false); // …and flips 600ms later: still blinking
    expect(blink.isVisible(10_001)).toBe(true); // but the IDLE clock never moved → solid past 10s
  });

  // alacritty's `blink_timeout: 0` disables the timeout entirely (`config/cursor.rs:64-70`).
  it("a timeout of 0 disables the idle stop", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);
    blink.setIdleTimeout(0);

    expect(blink.isVisible(60 * MIN + 600)).toBe(false); // an hour in and still blinking
  });

  // The chain is unchanged above it: an idled-out cursor is solid, and so is one whose application
  // never asked to blink — the timeout cannot resurrect a blink nobody requested.
  it("does not make a non-blinking cursor blink", () => {
    const blink = new CursorBlink();
    blink.setIdleTimeout(10_000);

    expect([blink.isVisible(600), blink.isVisible(10_601)]).toEqual([true, true]);
  });
});

// #592 — while an IME composition is in progress the caret stays put. Two of the three references do
// this (pinned trees, 2026-07-28): alacritty suppresses the blink as a term in the same expression
// (`alacritty/src/event.rs:1633` @ 852e971), and ghostty — CORRECTED 2026-08-03, #249 — draws no
// terminal caret at all while composing. `cursor.zig:47` does return `.block` for preedit, but
// `rebuildCells` discards the cursor before it is used (`src/renderer/generic.zig:2453` @ e6e26e1,
// after `setCursor(null, null)`), so the block this comment used to cite is unreachable.
// xterm.js has no rule — its only `isComposing` guard near the cursor is `_syncTextArea`.
//
// MEASURED before building (real browser, composition driven through the hidden textarea): with the
// application silent — the default since #575 — the cursor is ALREADY solid during composition, and
// the content cells never change either. So this gate is a no-op in the common case and bites only
// where an application explicitly asked to blink. That narrowness is the point, not a shortcoming.
//
// NOT adopted: the rule that a preedit outranks DECTCEM and *reveals* a cursor the application hid.
// That inverts `cursorCommand`'s contract for a rare case; recorded on #592, and carried forward as
// ADR-0028 D5 (browser ownership governs position and extent, never visibility). Note what the 2026-
// 08-03 correction above does to its GROUNDS: #592 rejected that rule believing ghostty held it, and
// ghostty does not — so the reference reopens nothing, but the answer it DOES hold (suppress the
// caret entirely) was never on the table, and is ADR-0028's alternative (D).
describe("CursorBlink — IME composition (#592)", () => {
  it("stays solid while composing, even though the application asked to blink", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);

    blink.setComposing(true);

    expect([blink.isVisible(0), blink.isVisible(600), blink.isVisible(1200)]).toEqual([true, true, true]);
  });

  it("resumes blinking when the composition ends", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);
    blink.setComposing(true);
    expect(blink.isVisible(600)).toBe(true); // suppressed

    blink.setComposing(false);

    expect([blink.isVisible(600), blink.isVisible(1200)]).toEqual([false, true]);
  });

  // The no-op case the browser measurement found, pinned so a later change cannot quietly make
  // composition *start* a blink that nobody asked for.
  it("does not make a non-blinking cursor blink", () => {
    const blink = new CursorBlink();

    blink.setComposing(true);
    expect([blink.isVisible(0), blink.isVisible(600)]).toEqual([true, true]);

    blink.setComposing(false);
    expect([blink.isVisible(0), blink.isVisible(600)]).toEqual([true, true]);
  });

  // Composing suppresses the blink; it does not touch the idle clock (#593) or the phase. A user
  // mid-composition has plainly not gone idle, and `restartFromInput` is the only thing that speaks
  // for input — keeping the two separate is what stops this gate from acquiring a second job.
  it("leaves the idle clock alone", () => {
    const blink = new CursorBlink();
    blink.setAppBlink(true);
    blink.setIdleTimeout(10_000);

    blink.setComposing(true);
    blink.setComposing(false);

    expect(blink.isVisible(10_601)).toBe(true); // still idled out — composing did not count as input
  });
});
