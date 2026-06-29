import { describe, it, expect, vi } from "vitest";
import { StubFrameSource } from "../src/frame-source";
import type { DecodedFrame } from "../src/types";

const emptyFrame = (cols: number, rows: number): DecodedFrame => ({
  cols,
  rows,
  kind: "full",
});

describe("FrameSource contract (StubFrameSource)", () => {
  it("delivers a pushed frame to a subscriber", () => {
    const source = new StubFrameSource();
    const seen: DecodedFrame[] = [];
    source.subscribe((f) => seen.push(f));

    const frame = emptyFrame(80, 24);
    source.push(frame);

    expect(seen).toEqual([frame]);
  });
});
