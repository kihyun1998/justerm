import { describe, it, expect } from "vitest";
import { Terminal } from "../src/terminal";
import { StubFrameSource } from "../src/frame-source";
import type { DecodedFrame } from "../src/types";
import type { Renderer } from "../src/renderer";

/** A test-double renderer: records what the widget hands it, no WebGL. */
class FakeRenderer implements Renderer {
  applied: DecodedFrame[] = [];
  renderCount = 0;
  applyFrame(frame: DecodedFrame): void {
    this.applied.push(frame);
  }
  render(): void {
    this.renderCount++;
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
});
