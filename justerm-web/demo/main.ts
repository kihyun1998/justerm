// Manual S5 harness — a blinking cursor. Pushes one full frame with a prompt and
// a block cursor after it. Expected result: a solid cursor-coloured block that
// blinks every 600ms; click outside the page (blur) and it stops blinking (stays
// solid); focus again and it resumes. This exercises the cell-invert cursor +
// CursorBlink policy through beamterm (which has no cursor primitive).
// Run: `pnpm demo` (NOT `vite demo` — that skips vite.config.ts + its wasm plugins).
import { BeamtermRenderer, StubFrameSource, Terminal } from "../src/index";
import type { DecodedFrame } from "../src/types";

const COLS = 80;
const ROWS = 24;

const renderer = await BeamtermRenderer.create({
  canvasSelector: "#term",
  fontFamily: "monospace",
  fontSize: 16,
  theme: {
    ansi: [
      0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f,
      0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
    ],
    defaultFg: 0xcdd6f4,
    defaultBg: 0x1e1e2e,
    cursorColor: 0xf5e0dc,
  },
});

const source = new StubFrameSource();
new Terminal(source, renderer).mount();

function frame(
  rows: { line: number; text: string }[],
  cursor: { row: number; col: number; shape: number },
): DecodedFrame {
  const codepoints: number[] = [];
  const spans: number[] = [];
  let offset = 0;
  for (const r of rows) {
    const chars = [...r.text];
    spans.push(r.line, 0, chars.length - 1, offset, chars.length);
    for (const c of chars) codepoints.push(c.codePointAt(0)!);
    offset += chars.length;
  }
  const n = codepoints.length;
  return {
    cols: COLS,
    rows: ROWS,
    kind: 0,
    codepoints,
    fg: new Array(n).fill(0),
    bg: new Array(n).fill(0),
    flags: new Array(n).fill(0),
    extra: new Array(n).fill(0),
    spans,
    sideTable: [],
    cursorRow: cursor.row,
    cursorCol: cursor.col,
    cursorVisible: true,
    cursorShape: cursor.shape,
    cursorBlink: true,
  } as DecodedFrame;
}

// A prompt with a block cursor right after "$ " (cols 0,1 → cursor at col 2).
source.push(
  frame([{ line: 0, text: "justerm-web — cursor demo (click away to unfocus)" }, { line: 2, text: "$ " }], {
    row: 2,
    col: 2,
    shape: 0, // Block
  }),
);

// Focus gates the blink (solid while unfocused).
window.addEventListener("focus", () => renderer.setFocused(true));
window.addEventListener("blur", () => renderer.setFocused(false));
