import type { InputSink, TextareaLike } from "./input";

/** ASCII DEL (C0 `\x7f`) — a backspace while an IME is active shortens the
 * textarea, which we report as one delete (xterm's `C0.DEL`). */
const DEL = "\x7f";

/**
 * IME composition → committed text, ported from xterm's `CompositionHelper`.
 *
 * the renderer's canvas can't receive composition events, so a hidden `<textarea>`
 * over the cursor is the real input target; this controller is driven by that
 * textarea's composition events and reads its VALUE (never the event `data`) to
 * recover the committed text. `data` is unreliable on Chromium, and for Korean an
 * ending consonant (종성) can migrate to the next syllable when the following
 * input is a vowel — so the last `compositionupdate` data misdescribes the
 * character. Reading `textarea.value` after the native event settles is the fix.
 *
 * The read is deferred (`setTimeout(0)` in the browser) because composition events
 * fire BEFORE the textarea mutates on most browsers; the `defer` seam is injected
 * so tests flush it deterministically. Committed text goes out as a `text` intent
 * (raw, unbracketed) on the shared {@link InputSink}.
 */
export class CompositionController {
  private isComposing = false;
  private isSendingComposition = false;
  private start = 0;
  private end = 0;
  private suffix = "";
  private dataAlreadySent = "";
  private pendingTextareaChange = false;

  constructor(
    private readonly textarea: TextareaLike,
    private readonly sink: InputSink,
    private readonly defer: (fn: () => void) => void = (fn) => {
      setTimeout(fn, 0);
    },
  ) {}

  /** Whether a composition is in progress or its committed text is still pending —
   * the glue reads it to know when it's safe to clear the textarea.
   *
   * Not the same question as {@link CompositionController.composing}, and the difference is
   * load-bearing: this stays true across the deferred commit read, which is the window a
   * continuous-CJK `compositionstart` lands in. */
  get active(): boolean {
    return this.isComposing || this.isSendingComposition;
  }

  /** Whether the OS currently owns a candidate window over the textarea — true from
   * `compositionstart` until `compositionend`, and no longer.
   *
   * This is the predicate the IME anchor is frozen on (#637/#649): the OS re-reads the anchor
   * while a composition is open, so moving it walks the candidate window away from the text
   * being composed. {@link CompositionController.active} is deliberately NOT that predicate —
   * it outlives the candidate window by one deferred read, and gating the anchor on it would
   * swallow #631's `compositionstart` re-sync in ordinary Korean/Japanese typing. */
  get composing(): boolean {
    return this.isComposing;
  }

  /** A composition began — anchor the start at the caret (selection, not length,
   * so screen-reader mode's textarea prefill doesn't skew it). */
  compositionStart(): void {
    this.isComposing = true;
    const value = this.textarea.value;
    const s = this.textarea.selectionStart ?? value.length;
    const e = this.textarea.selectionEnd ?? s;
    this.start = Math.min(s, e);
    this.end = Math.max(s, e);
    this.suffix = value.substring(this.end);
    this.dataAlreadySent = "";
  }

  /** In-progress composition text (drives the on-screen view; NOT the source of
   * the committed text). Tracks the composition END through the caret once the
   * textarea settles, so a synchronous finalize (Enter) has the right range. */
  compositionUpdate(_data: string): void {
    this.defer(() => {
      this.end = Math.max(this.start, this.textarea.selectionEnd ?? this.textarea.value.length);
    });
  }

  /** The composition ended — read the committed text once the textarea settles. */
  compositionEnd(): void {
    this.finalize(true);
  }

  /** Route a keydown during/after composition. Returns whether the caller should
   * still process it as a key (`false` = the IME swallowed it). While composing, a
   * composition/modifier key continues the composition; any other key (Enter)
   * finalizes it FIRST — synchronously — so the committed text is sent before the
   * command runs, then the key itself is still handled (`true`). */
  keydown(keyCode: number): boolean {
    if (this.isComposing || this.isSendingComposition) {
      // 229 = composition character, 20 = CapsLock, 16/17/18 = Shift/Ctrl/Alt.
      if (keyCode === 20 || keyCode === 229) return false;
      if (keyCode === 16 || keyCode === 17 || keyCode === 18) return false;
      this.finalize(false);
    }
    if (keyCode === 229) {
      this.handleAnyTextareaChanges();
      return false;
    }
    return true;
  }

  /** A non-composition character was typed while an IME was active (keyCode 229
   * with no composition). The character lands in the textarea after this event, so
   * diff the value once it settles: a longer value = inserted text, shorter = a
   * delete (send DEL), same length but changed = a replacement. Coalesced by the
   * pending flag so one keystroke diffs once. */
  private handleAnyTextareaChanges(): void {
    if (this.pendingTextareaChange) return;
    this.pendingTextareaChange = true;
    const oldValue = this.textarea.value;
    this.defer(() => {
      this.pendingTextareaChange = false;
      if (this.isComposing) return; // a composition started since — let it own the input
      const newValue = this.textarea.value;
      const diff = newValue.replace(oldValue, "");
      this.dataAlreadySent = diff;
      if (newValue.length > oldValue.length) this.sink.send({ kind: "text", text: diff });
      else if (newValue.length < oldValue.length) this.sink.send({ kind: "text", text: DEL });
      else if (newValue !== oldValue) this.sink.send({ kind: "text", text: newValue });
    });
  }

  /** Extract and send the committed text. `waitForPropagation` false sends it
   * synchronously (a non-composition keystroke like Enter arrived first, so the
   * composition must go out before the command runs); true defers the read until
   * the native compositionend settles the textarea. */
  private finalize(waitForPropagation: boolean): void {
    this.isComposing = false;
    if (waitForPropagation) {
      this.finalizeDeferred();
      return;
    }
    this.isSendingComposition = false;
    this.emit(this.textarea.value.substring(this.start, this.end));
  }

  /** Read + send the committed text after the textarea settles. Snapshots the
   * range because a new composition may start before the deferred read runs. */
  private finalizeDeferred(): void {
    const startSnapshot = this.start;
    const suffixSnapshot = this.suffix;
    this.isSendingComposition = true;
    this.defer(() => {
      if (!this.isSendingComposition) return; // superseded / cancelled
      this.isSendingComposition = false;
      const value = this.textarea.value;
      // Skip a prefix already sent by a keydown (Issue #3191).
      const from = startSnapshot + this.dataAlreadySent.length;
      if (this.isComposing) {
        // A NEW composition started before this read ran (continuous CJK). Stop at its
        // start, else this commit leaks the new composition's in-progress text — which
        // its own compositionend would then send again (xterm CompositionHelper L186-188).
        this.emit(value.substring(from, Math.max(from, this.start)));
        return;
      }
      // Keep any pre-existing suffix out of the commit so it isn't resent.
      const valueEnd =
        suffixSnapshot.length > 0 && value.endsWith(suffixSnapshot)
          ? value.length - suffixSnapshot.length
          : value.length;
      this.emit(value.substring(from, Math.max(from, valueEnd)));
    });
  }

  /** Send committed text as a raw `text` intent — nothing for an empty commit. */
  private emit(text: string): void {
    if (text.length > 0) this.sink.send({ kind: "text", text });
  }
}

/** Where the hidden textarea is anchored, in cells — the cursor cell a frame reported. */
export interface TextareaAnchor {
  col: number;
  row: number;
}

/**
 * Whether the hidden textarea must move, and the cache key to remember (#631).
 *
 * The key is the cursor's **cell coordinate**, but the anchor is computed from the **geometry** —
 * so a cell-size change (`setFontSize`, `setFontFamily`, `setLetterSpacing`, `setLineHeight`, and
 * `setDevicePixelRatio` once it is wired) moves what the anchor should be while leaving the
 * coordinate identical. A coordinate-keyed cache structurally cannot express "the geometry moved",
 * which is why `force` exists: the callers that sit at a moment something actually *reads* the
 * anchor override the cache instead of trying to keep it fresh at all times.
 *
 * Why not simply drop the cache and re-read every frame, which is what all three references do
 * (xterm.js `_syncTextArea`, ghostty `imePoint`, alacritty `update_ime_position` — none of them
 * caches)? Because their cell is a **stored field** they push to (`dimensions.css.cell`,
 * `size.cell`, `size_info`), while ours arrives through a consumer-supplied
 * {@link TerminalOptions.getGeometry} callback whose cost we do not control — the demo's and the
 * README's both do a `getBoundingClientRect()`. Per-frame is cheap for them and a forced layout
 * read per output flush for us. Valid only as long as `getGeometry` stays a pull-based consumer
 * callback; if the widget ever holds a pushed cell, prefer the reference's no-cache shape.
 *
 * `composing` suppresses **every** path, forced or not (#637 established the rule, #649 closed the
 * `force` exemption). While a composition is open the OS re-reads the anchor to keep its candidate
 * window placed — measured with a real Korean IME: the Hanja window follows the anchor down as
 * unsolicited output moves the cursor, walking away from the text being composed.
 *
 * `force` originally won over `composing` so that #631's `compositionstart` re-sync could not be
 * gated by its own guard. It never needed that: `onStart` re-anchors *before* telling the controller,
 * so it sees `composing === false` either way. What the exemption actually bought was a second
 * entrance to the same harm — `element` mousedown → `onDown` → `Terminal.focus()` → a forced re-sync
 * onto the superseded cursor cell, reachable through the public `focus()` from any consumer, not only
 * a pointer (#649). So `force` now means one thing and one thing only: *override the coordinate
 * cache*. It says nothing about the composition rule.
 *
 * **`composing` is `isComposing`, never `active`.** `active` stays true through the deferred commit
 * read, and a continuous-CJK `compositionstart` lands inside exactly that window — so keying this on
 * `active` would swallow #631's re-sync in the ordinary Korean/Japanese typing pattern, while looking
 * equivalent at the call site. `composition.test.ts` pins the two diverging there.
 *
 * Suppression returns `undefined`, which the caller treats as a decided no-op that leaves the cache
 * alone — so the move is not recorded as applied and the anchor catches up on the first frame after
 * the composition ends, rather than waiting for the cursor to move again.
 *
 * **Freezing is this codebase's form of the rule, not the rule itself.** All three references converge
 * on *the anchor tracks where the user's composition is, never where the output cursor went* — and two
 * of the three do that by **actively re-aiming during the composition**, not by suppressing:
 *
 * - ghostty folds the preedit's width into the IME rect it pushes on every key event
 *   (`src/Surface.zig:2108`, used at `:2151`) — no gate at all
 * - alacritty picks the point *from* the preedit when there is one and from the cursor when there is
 *   not (`alacritty/src/display/mod.rs:1136-1142`, `:1215`) — also no gate
 * - xterm.js is the only one that suppresses, and only the **involuntary** writer: `_syncTextArea`
 *   bails while composing (`browser/CoreBrowserTerminal.ts:338`, gating all three of its callers) while
 *   `CompositionHelper.updateCompositionElements` deliberately rewrites `left`/`top` every render
 *   (`browser/input/CompositionHelper.ts:273-274`)
 *
 * **justerm-web now re-aims too, and this guard is unchanged — which is the part worth reading.**
 * Until #249 there was no preedit view here, so *"where the composition is"* collapsed to *"where it
 * started"* and freezing was the only available expression of the shared rule. #249 supplied the
 * missing representation, and the prediction recorded here was that this guard *"is what has to
 * give"*. It did not, and the reason is ADR-0028 D4: the writer that knows where the composition is
 * does not pass through the guard that exists for the writers that do not. The preedit's re-aim goes
 * straight to {@link Terminal.writeTextareaAnchor}, exactly as xterm's `updateCompositionElements`
 * never goes through `_syncTextArea`. So this stays a rule about the **involuntary** writers — the
 * frame stream (#637) and the focus path (#649) — which is what both of them actually measured.
 */
export function textareaMove(
  cursor: TextareaAnchor | undefined,
  lastKey: string,
  force: boolean,
  composing: boolean,
): { col: number; row: number; key: string } | undefined {
  if (!cursor) return undefined;
  const key = `${cursor.col},${cursor.row}`;
  if (composing || (!force && key === lastKey)) return undefined;
  return { col: cursor.col, row: cursor.row, key };
}

/**
 * What a `compositionupdate` should do to the drawn run: nothing, or push these codepoints (#249).
 *
 * Pure so the two decisions are testable at all — the widget half that acts on them needs a DOM and
 * the unit suite runs in `environment: "node"`, which is the blind spot #649 measured.
 *
 * - **Unchanged text is dropped.** A real IME emits one settling `compositionupdate` per syllable
 *   carrying the data it already sent (measured on a Windows Korean IME), and each one would
 *   otherwise cost a full re-pack.
 * - **No origin, no push.** The origin is latched at `compositionstart`; a composition that somehow
 *   runs before any frame has reported a cursor has nowhere to draw, and guessing a cell is worse
 *   than drawing nothing.
 *
 * The text itself is split by **code point**, not by UTF-16 unit: a preedit can carry astral
 * scalars, and `Array.from`'s iterator is what makes `"\u{1F600}"` one cell rather than two halves
 * of a surrogate pair.
 */
export function preeditIntent(
  text: string,
  lastText: string,
  origin: TextareaAnchor | undefined,
): { codepoints: Uint32Array; origin: TextareaAnchor } | undefined {
  if (text === lastText || !origin) return undefined;
  return { codepoints: Uint32Array.from(text, (c) => c.codePointAt(0) ?? 0), origin };
}

/**
 * Where a composition that is starting should draw, given what the widget is still holding (#911).
 *
 * `cursorAnchor` is written only by the frame stream, so it can never be fresher than the last frame
 * the engine sent. `lastRunEnd` is where the previous composition's run finished — the renderer's own
 * caret column, one past its last cell.
 *
 * **When a commit is in flight the frame stream is known to be stale, not merely possibly stale.**
 * The committed text leaves one deferred read after `compositionend` (#116) and a continuous-CJK
 * `compositionstart` lands inside that window, so at the latch the engine has not been handed the
 * previous syllable at all — `cursorAnchor` still points at the cell that syllable is about to take.
 * The previous run's end is the one coordinate that does account for it.
 *
 * Pure so the choice is testable at all: the wiring needs a DOM and the unit suite runs in
 * `environment: "node"`. It is the run's END rather than a width because the widget has no
 * `wcwidth` — see {@link Renderer.setPreedit}, whose return exists for that reason.
 */
export function preeditLatch(
  cursorAnchor: TextareaAnchor | undefined,
  lastRunEnd: TextareaAnchor | undefined,
  commitPending: boolean,
): TextareaAnchor | undefined {
  if (commitPending && lastRunEnd) return lastRunEnd;
  return cursorAnchor;
}
