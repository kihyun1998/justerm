// Manual S1 harness — wires the real beamterm renderer to a stub source and
// pushes one empty frame. Expected result: an empty cleared grid on the canvas.
// Run: `pnpm demo` (Vite + vite-plugin-wasm handle the TS + beamterm WASM).
import { BeamtermRenderer, StubFrameSource, Terminal } from "../src/index";

const source = new StubFrameSource();
const renderer = await BeamtermRenderer.create({
  canvasSelector: "#term",
  fontFamily: "monospace",
  fontSize: 16,
  defaultBg: 0x1e1e2eff,
});
const term = new Terminal(source, renderer);
term.mount();

// An empty full frame → the grid clears to defaultBg (S2 will fill cells).
source.push({ cols: 80, rows: 24, kind: "full" });
