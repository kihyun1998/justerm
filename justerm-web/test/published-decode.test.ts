/**
 * The adapter, driven by a frame the **published decoder actually produced** (#657).
 *
 * Every other test in this package hands the adapter a plain object or a hand-made typed array.
 * That is deliberate — a frame arrives through `FrameSource.push` from a consumer on any decoder
 * version, so the shape is source-agnostic by contract (`src/types.ts:1-15`) — but it means the
 * columns production actually receives had never been through this code. #627 is what that costs:
 * the seam fixture was a hand-written `Uint16Array`, so it took `asU16`'s identity branch, which is
 * the branch production had *stopped* taking, and a truncating narrow went unseen for a release.
 *
 * #646 gated the seam's **types**. This is the half no type on it can reach — what the values mean,
 * and whether the columns pass through by reference or are copied.
 *
 * ## The fixture
 *
 * `fixtures/frame-wire-v16.jt` is bytes, checked in rather than generated here, because neither CI
 * job that runs this package installs Rust (`.github/workflows/test.yml:156`, `:188` — the `web`
 * job's own comment says it "needs no Rust"). They come from a real `Engine`:
 *
 *     cargo run -p justerm-wasm-decode --example gen_engine_frame -- \
 *       justerm-web/test/fixtures/frame-wire-v<N>.jt
 *
 * That example holds the VT input the expectations below are read off, so this file never guesses
 * a value. **Regenerate — and rename — when the wire version moves**; the first assertion below is
 * what tells you, and it fails closed.
 *
 * ## What this depends on
 *
 * Node's WebAssembly-ESM import, which is still flagged experimental and prints a warning. The
 * published package ships only the wasm-bindgen *bundler* target, so there is no plain-CommonJS
 * entry to fall back to. If a CI runner's Node cannot do it, this file fails loudly rather than
 * silently skipping — which is the reason it is acceptable to depend on: the failure names itself.
 * The browser has no such caveat, so the same fixture is the natural input if this ever has to move
 * to `e2e/`.
 */
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { asU16, asU32, damageHeader, retainU32 } from "../src/justerm-renderer";
import { osc8Links } from "../src/links";
import type { DecodedFrame } from "../src/types";

/** The wire version `fixtures/frame-wire-v16.jt` was produced at. Regenerated with the
 * engine each time the wire moves — v14 -> v16 here (#490), where the decoder started
 * rejecting the old bytes outright, which is the version check doing its job. */
const FIXTURE_WIRE_VERSION = 16;

const decoder = await import("justerm-wasm-decode");

/** The decoder's own `DecodedFrame`, concrete widths and all — not this package's structural view.
 * The two are kept apart on purpose: an assertion about `.buffer` or `.byteOffset` is about what
 * the decoder returns, while an assertion about what the adapter does with it goes through
 * {@link asWebFrame} and gets only what `src/types.ts` declares. Blurring them would let a test
 * lean on a concrete width the widget's contract deliberately does not promise. */
function decodeFixture() {
  const bytes = new Uint8Array(
    readFileSync(new URL(`./fixtures/frame-wire-v${FIXTURE_WIRE_VERSION}.jt`, import.meta.url)),
  );
  return decoder.decodeFrame(bytes);
}

/** Narrow the decoder's class to this package's interface. That the assignment compiles at all is
 * itself a check: a column the decoder stopped declaring would not satisfy `DecodedFrame`. It is a
 * declaration rather than a cast for the same reason `JustermRenderer.create` binds the renderer
 * that way (#646). */
const asWebFrame = (f: ReturnType<typeof decodeFixture>): DecodedFrame => f;

describe("a frame the published decoder produced", () => {
  it("the fixture and the installed decoder agree on the wire version", () => {
    // Fails closed when the pin moves to a decoder speaking a newer wire: `decodeFrame` would
    // throw `BadVersion` below and every expectation would read as a mystery. Regenerate the
    // fixture with the example named in this file's header and rename it to the new version.
    expect(decoder.wireVersion()).toBe(FIXTURE_WIRE_VERSION);
  });

  it("carries what the engine printed", () => {
    const frame = decodeFixture();
    const text = Array.from(frame.codepoints)
      .map((c) => String.fromCodePoint(c))
      .join("");
    expect(text).toBe("ReLINK ");
    // Span directory, stride 5: (line, left, right, cell_offset, count). Pure convention — an
    // identically typed column carries any other layout equally well, so only a real frame
    // asserts it.
    expect(Array.from(frame.spans)).toEqual([0, 0, 6, 0, 7]);
    expect([frame.cols, frame.rows, frame.kind]).toEqual([80, 24, 1]);
  });

  it("passes every coerced column through by reference, never copying", () => {
    const frame = decodeFixture();
    // The load-bearing assertion of this file. `asU32`/`asU16` copy whenever the input is not
    // already the target width, and that copy runs per frame at frame cadence over the whole
    // viewport — #627's real cost, larger than the truncation it is usually remembered for.
    // `toBe` (identity), not `toEqual`: a copy with equal contents is exactly the failure.
    //
    // Each column is read into a local FIRST, and that is not a style choice: a getter read
    // builds a fresh `Uint32Array` *view* every time (same buffer, same byteOffset, new object —
    // measured), so `expect(asU32(frame.x)).toBe(frame.x)` compares two different objects and
    // fails on a code path that is doing exactly the right thing. The identity that matters is
    // between what the adapter received and what it forwards, which is one read.
    const { spans, codepoints, fg, bg, extra, flags, underlineColor } = frame;
    expect(asU32(spans)).toBe(spans);
    expect(asU32(codepoints)).toBe(codepoints);
    expect(asU32(fg)).toBe(fg);
    expect(asU32(bg)).toBe(bg);
    expect(asU32(extra)).toBe(extra);
    expect(asU32(underlineColor!)).toBe(underlineColor);
    // `flags` is the one column still narrower than the rest, and the only remaining `asU16` in
    // production. It takes the identity branch for the same reason and by the opposite width.
    expect(asU16(flags)).toBe(flags);
  });

  it("hands out a NEW view object on every column read, over the same memory", () => {
    const frame = decodeFixture();
    // Recorded because it is invisible with the plain-object fixtures every other test uses, where
    // a property read is free and returns the same array. Here `frame.spans` twice is two objects
    // over one buffer — no data is copied, but a reader that touches a column inside a per-cell
    // loop allocates a view per access. `src/cell-mirror.ts` is the one place that did.
    expect(frame.spans).not.toBe(frame.spans);
    expect(frame.spans.buffer).toBe(frame.spans.buffer);
    expect(frame.spans.byteOffset).toBe(frame.spans.byteOffset);
  });

  it("copies a column it RETAINS, so nothing outlives the memory it viewed", () => {
    const frame = decodeFixture();
    const { spans } = frame;
    // The contrast with the assertion above is the whole rule, and the two live together on
    // purpose. Forwarded and forgotten -> the view (zero copy, #627). Kept past this frame -> a
    // copy, because the view is only valid until the next decode grows WASM memory.
    expect(asU32(spans)).toBe(spans);
    expect(retainU32(spans)).not.toBe(spans);
    expect(retainU32(spans).buffer).not.toBe(spans.buffer);
    expect(Array.from(retainU32(spans))).toEqual(Array.from(spans));
  });

  it("a retained VIEW would have detached — which is why the copy exists", () => {
    // The justification for `retainU32`, kept as a test rather than only as a comment: without it
    // `lastSelectionSpans` holds this, and `issueOverlay` re-reads it on a focus flip that has no
    // new frame. Passing a detached array to any wasm entry point throws rather than degrading.
    const held = decodeFixture().spans;
    expect(held.byteLength).toBeGreaterThan(0);

    // Force WASM memory to grow by holding decoded frames instead of freeing them. Measured at
    // ~109 on this fixture; the cap is 45x that, so a failure here means the invalidation contract
    // changed, not that the loop was too short.
    const holdOpen = [];
    let detachedAfter = -1;
    for (let i = 0; i < 5000; i++) {
      holdOpen.push(decodeFixture());
      if (held.byteLength === 0) {
        detachedAfter = i;
        break;
      }
    }
    expect(detachedAfter, "a held column view never detached — the lifetime contract moved").toBeGreaterThan(-1);
  });

  it("reads the header scalars off getters, not off a plain object", () => {
    const frame = decodeFixture();
    // `damageHeader` has only ever been fed object literals. A getter that started returning a
    // string, or a `kind` that changed meaning, would produce a header this asserts against.
    expect(Array.from(damageHeader(asWebFrame(frame)))).toEqual([80, 24, 1, 0, 0, 0, 0, 1]);
  });

  it("resolves the OSC 8 hyperlink by walking the real span directory", () => {
    const frame = decodeFixture();
    // `osc8Links` walks `spans` in stride-5 chunks and indexes `link` at each span's cell offset.
    // Driving it with hand-made columns proves the arithmetic; driving it with the decoder's proves
    // the arithmetic *and* that the two agree on what the five fields mean.
    expect(osc8Links(asWebFrame(frame))).toEqual([
      {
        uri: "https://example.com",
        cells: [
          [0, 2],
          [0, 3],
          [0, 4],
          [0, 5],
        ],
      },
    ]);
  });

  it("puts only the trailing marks in sideTable, leaving the base character in codepoints", () => {
    const frame = decodeFixture();
    // The clearest case of a meaning no type carries: both readings are `Uint32Array` + `string[]`.
    // A consumer that drew `sideTable[extra - 1]` as the cell's text would render a bare accent.
    // `justerm-renderer/src/frame_grid.rs:41` composes the two, and this is what it composes.
    expect(Array.from(frame.extra)).toEqual([0, 1, 0, 0, 0, 0, 0]);
    expect(frame.sideTable).toEqual(["́"]);
    expect(String.fromCodePoint(frame.codepoints[1]!)).toBe("e");
  });
});
