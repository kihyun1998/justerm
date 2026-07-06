import type { InputSink, TextareaLike } from "./input";

/** ASCII DEL (C0 `\x7f`) — a backspace while an IME is active shortens the
 * textarea, which we report as one delete (xterm's `C0.DEL`). */
const DEL = "\x7f";

/**
 * IME composition → committed text, ported from xterm's `CompositionHelper`.
 *
 * beamterm's canvas can't receive composition events, so a hidden `<textarea>`
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
   * the glue reads it to know when it's safe to clear the textarea. */
  get active(): boolean {
    return this.isComposing || this.isSendingComposition;
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
