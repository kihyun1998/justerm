// Manual S2 harness — wires the real beamterm renderer to a stub source and
// pushes one golden frame ("hi"). Expected result: the word "hi" at the top-left
// of an otherwise empty grid, on the theme background.
// Run: `pnpm demo` (Vite + vite-plugin-wasm handle the TS + both WASM modules).
import { BeamtermRenderer, StubFrameSource, Terminal } from "../src/index";
import type { DecodedFrame } from "../src/types";

const renderer = await BeamtermRenderer.create({
  canvasSelector: "#term",
  fontFamily: "monospace",
  fontSize: 16,
  theme: {
    // Standard xterm ANSI 16 (slots 0..15); buildPalette fills 16..255.
    ansi: [
      0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f,
      0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
    ],
    defaultFg: 0xcdd6f4,
    defaultBg: 0x1e1e2e,
  },
});

const source = new StubFrameSource();
const term = new Terminal(source, renderer);
term.mount();

// A one-span Full frame spelling "hi" at (0,0) with default colours. The SoA
// columns + span directory are exactly what justerm-wasm-decode emits.
const hi: DecodedFrame = {
  cols: 80,
  rows: 24,
  kind: 0, // Full
  codepoints: ["h", "i"].map((c) => c.codePointAt(0)!),
  fg: [0, 0], // Default refs
  bg: [0, 0],
  flags: [0, 0],
  extra: [0, 0],
  spans: [0, 0, 1, 0, 2], // [line, left, right, cell_offset, count]
  sideTable: [],
};
source.push(hi);
