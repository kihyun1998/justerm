import { describe, it, expect } from "vitest";
import { CompositionController } from "../src/composition";
import { StubInputSink, type TextareaLike } from "../src/input";

/** A mutable fake textarea — the browser mutates `value`/selection during a
 * real composition, so tests drive those directly (no DOM). */
function fakeTextarea(value = ""): TextareaLike & { value: string; selectionStart: number; selectionEnd: number } {
  return { value, selectionStart: value.length, selectionEnd: value.length };
}

/** A manual-flush stand-in for `setTimeout(0)`, so a test controls exactly when
 * the deferred textarea reads happen (the whole mechanism hinges on reading the
 * textarea AFTER the native composition event completes). FIFO, like the macro
 * task queue. */
function manualDefer(): { defer: (fn: () => void) => void; flush: () => void } {
  const q: Array<() => void> = [];
  return {
    defer: (fn) => q.push(fn),
    flush: () => {
      while (q.length) q.shift()!();
    },
  };
}

describe("CompositionController", () => {
  it("emits the committed text read from the textarea, not the event data", () => {
    const ta = fakeTextarea("");
    const sink = new StubInputSink();
    const { defer, flush } = manualDefer();
    const c = new CompositionController(ta, sink, defer);

    c.compositionStart(); // start = 0 (empty textarea)
    c.compositionUpdate("ㅎ"); // in-progress view text (not what gets committed)
    ta.value = "한"; // the browser commits the syllable into the textarea
    c.compositionEnd();
    flush(); // the deferred read + send fires

    expect(sink.sent).toEqual([{ kind: "text", text: "한" }]);
  });

  it("uses the final textarea value when the jongseong migrated (event data lies)", () => {
    // Korean: an ending consonant can move to the next syllable when the next input
    // is a vowel, so the last compositionupdate `data` ("니") misdescribes the commit
    // ("아니"). Reading the textarea value is the only correct source.
    const ta = fakeTextarea("");
    const sink = new StubInputSink();
    const { defer, flush } = manualDefer();
    const c = new CompositionController(ta, sink, defer);

    c.compositionStart();
    c.compositionUpdate("안");
    c.compositionUpdate("니"); // misleading final view (the jongseong migrated)
    ta.value = "아니"; // the browser committed the whole thing
    c.compositionEnd();
    flush();

    expect(sink.sent).toEqual([{ kind: "text", text: "아니" }]);
  });

  it("excludes text that already followed the cursor (no resend of the suffix)", () => {
    // The caret sits before existing text ("world"); composing inserts before it, so
    // the committed value is "안world" — only "안" is new and must be sent.
    const ta = fakeTextarea("world");
    ta.selectionStart = 0;
    ta.selectionEnd = 0;
    const sink = new StubInputSink();
    const { defer, flush } = manualDefer();
    const c = new CompositionController(ta, sink, defer);

    c.compositionStart(); // start = 0, suffix = "world"
    c.compositionUpdate("안");
    ta.value = "안world";
    c.compositionEnd();
    flush();

    expect(sink.sent).toEqual([{ kind: "text", text: "안" }]);
  });

  it("Enter finalizes the composition synchronously, before the key is processed", () => {
    const ta = fakeTextarea("");
    const sink = new StubInputSink();
    const { defer, flush } = manualDefer();
    const c = new CompositionController(ta, sink, defer);

    c.compositionStart();
    c.compositionUpdate("한");
    ta.value = "한";
    ta.selectionEnd = 1;
    flush(); // the compositionupdate end-tracking settles (end = 1)

    const proceed = c.keydown(13); // Enter — no compositionend event has fired yet

    expect(sink.sent).toEqual([{ kind: "text", text: "한" }]); // sent NOW, no flush
    expect(proceed).toBe(true); // the caller still handles Enter as a key afterwards
  });

  it("swallows composition and modifier keys while composing (no premature finalize)", () => {
    const ta = fakeTextarea("");
    const sink = new StubInputSink();
    const { defer } = manualDefer();
    const c = new CompositionController(ta, sink, defer);

    c.compositionStart();
    ta.value = "한";

    expect(c.keydown(229)).toBe(false); // composition character
    expect(c.keydown(20)).toBe(false); // CapsLock
    expect(c.keydown(16)).toBe(false); // Shift
    expect(c.keydown(17)).toBe(false); // Ctrl
    expect(c.keydown(18)).toBe(false); // Alt
    expect(sink.sent).toEqual([]); // still composing — nothing finalized
  });

  it("sends a non-composition character typed while the IME is active (229, not composing)", () => {
    const ta = fakeTextarea("");
    const sink = new StubInputSink();
    const { defer, flush } = manualDefer();
    const c = new CompositionController(ta, sink, defer);

    const proceed = c.keydown(229); // IME active, but no composition in progress
    ta.value = "1"; // the char lands in the textarea after the event settles
    flush();

    expect(proceed).toBe(false); // 229 is swallowed — handled via the diff, not as a key
    expect(sink.sent).toEqual([{ kind: "text", text: "1" }]);
  });

  it("reports a backspace while the IME is active as a single DEL", () => {
    const ta = fakeTextarea("ab");
    const sink = new StubInputSink();
    const { defer, flush } = manualDefer();
    const c = new CompositionController(ta, sink, defer);

    c.keydown(229); // IME active, non-composing
    ta.value = "a"; // a character was removed
    flush();

    expect(sink.sent).toEqual([{ kind: "text", text: "\x7f" }]); // one DEL
  });

  it("a fresh composition is not truncated by a prior keydown send (dataAlreadySent reset)", () => {
    const ta = fakeTextarea("");
    const sink = new StubInputSink();
    const { defer, flush } = manualDefer();
    const c = new CompositionController(ta, sink, defer);

    // A 229 keydown sends "1" and records it as already-sent.
    c.keydown(229);
    ta.value = "1";
    flush();

    // A brand-new composition then commits "한"; compositionstart must reset the
    // already-sent length, else the commit would be shortened to "" (start over-skipped).
    ta.selectionStart = 1;
    ta.selectionEnd = 1; // caret after "1"
    c.compositionStart();
    c.compositionUpdate("한");
    ta.value = "1한";
    ta.selectionEnd = 2;
    c.compositionEnd();
    flush();

    expect(sink.sent).toEqual([
      { kind: "text", text: "1" },
      { kind: "text", text: "한" },
    ]);
  });

  it("a new composition starting before the deferred read stops the commit at its start", () => {
    // Continuous CJK: the IME commits "가" and immediately starts "나" in the same task,
    // BEFORE the previous commit's deferred read runs. The read must stop at the new
    // composition's start, not run to the value end — else "가"'s commit leaks "나"'s
    // in-progress jamo and "나" is then sent again (double-send / corruption).
    const ta = fakeTextarea("");
    const sink = new StubInputSink();
    const { defer, flush } = manualDefer();
    const c = new CompositionController(ta, sink, defer);

    c.compositionStart(); // A: start = 0
    c.compositionUpdate("가");
    ta.value = "가";
    ta.selectionEnd = 1;
    c.compositionEnd(); // schedules A's deferred read (start snapshot = 0)

    // B starts before A's read flushes; the caret is after "가", B composes a jamo.
    ta.selectionStart = 1;
    ta.selectionEnd = 1;
    c.compositionStart(); // B: start = 1, isComposing = true
    c.compositionUpdate("ㄴ");
    ta.value = "가ㄴ"; // B in progress

    flush(); // A's read runs while B composes — must emit only "가"

    expect(sink.sent).toEqual([{ kind: "text", text: "가" }]);

    // B commits "나".
    ta.value = "가나";
    ta.selectionEnd = 2;
    c.compositionEnd();
    flush();

    expect(sink.sent).toEqual([
      { kind: "text", text: "가" },
      { kind: "text", text: "나" },
    ]);
  });
});
