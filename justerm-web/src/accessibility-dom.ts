/**
 * DOM glue for the screen-reader mirror (#119): concrete {@link A11yTreeSink}
 * and {@link LiveRegionSink} backed by hidden DOM, plus {@link Accessibility}
 * that owns a {@link CellMirror} (to read viewport row text) and drives the pure
 * {@link AccessibilityController}.
 *
 * The logic lives in the controller (unit-tested); these wrappers are thin DOM
 * and are exercised by the demo + a real screen reader, not vitest (no DOM in
 * the node test env) — the project's established split.
 */
import { AccessibilityController } from "./accessibility";
import type { A11yTreeSink, LiveRegionSink } from "./accessibility";
import { CellMirror } from "./cell-mirror";
import type { FlagBits } from "./render-core";
import type { DecodedFrame } from "./types";
import type { Palette } from "justerm-wasm-decode/colors.js";

/** Off-screen-but-readable styling (the SR-only pattern): present to assistive
 * tech, invisible on screen. `display:none`/`visibility:hidden` would hide it
 * from the screen reader too, so we clip instead. */
function srOnly(el: HTMLElement): void {
  Object.assign(el.style, {
    position: "absolute",
    width: "1px",
    height: "1px",
    overflow: "hidden",
    clip: "rect(0 0 0 0)",
    clipPath: "inset(50%)",
    whiteSpace: "nowrap",
    margin: "-1px",
    padding: "0",
    border: "0",
  });
}

/** A hidden `role="list"` of `role="listitem"` rows mirroring the viewport. */
class DomA11yTree implements A11yTreeSink {
  readonly container: HTMLElement;
  private readonly rows: HTMLElement[] = [];

  constructor(
    private readonly doc: Document,
    private readonly onBoundary: (position: "top" | "bottom") => void,
  ) {
    this.container = doc.createElement("div");
    this.container.setAttribute("role", "list");
  }

  resize(rows: number): void {
    while (this.rows.length < rows) {
      const el = this.doc.createElement("div");
      el.setAttribute("role", "listitem");
      el.tabIndex = -1;
      this.container.appendChild(el);
      this.rows.push(el);
    }
    while (this.rows.length > rows) {
      this.container.removeChild(this.rows.pop()!);
    }
    this.bindBoundaries();
  }

  setRow(i: number, text: string, posInSet: number, setSize: number): void {
    const el = this.rows[i];
    if (!el) return;
    // A blank row gets a non-breaking space so it's still a focusable stop.
    el.textContent = text.length === 0 ? " " : text;
    el.setAttribute("aria-posinset", String(posInSet));
    el.setAttribute("aria-setsize", String(setSize));
  }

  focusRow(i: number): void {
    this.rows[i]?.focus();
  }

  /** Put the boundary listeners on the current first/last rows (re-bound after
   * each resize, since those elements change). */
  private bindBoundaries(): void {
    for (const [i, el] of this.rows.entries()) {
      const position = i === 0 ? "top" : i === this.rows.length - 1 ? "bottom" : null;
      el.onfocus = position ? () => this.onBoundary(position) : null;
    }
  }
}

/** A hidden `aria-live` region that announces new output. */
class DomLiveRegion implements LiveRegionSink {
  readonly el: HTMLElement;

  constructor(doc: Document) {
    this.el = doc.createElement("div");
    this.el.setAttribute("aria-live", "polite");
    this.el.setAttribute("aria-atomic", "false");
  }

  announce(text: string): void {
    // Replacing (not appending) keeps the node from growing unbounded; the
    // controller already sends only the new delta. Clear first so an identical
    // delta still re-triggers the live announcement.
    this.el.textContent = "";
    this.el.textContent = text;
  }

  clear(): void {
    this.el.textContent = "";
  }
}

/**
 * The screen-reader accessibility adapter: mount {@link root} beside the canvas,
 * feed it every frame, and forward keystrokes/blur. Owns its own
 * {@link CellMirror} so it reads viewport text independently of the renderer.
 */
export class Accessibility {
  /** Mount this beside the terminal canvas (contains the hidden tree + live region). */
  readonly root: HTMLElement;
  private readonly controller: AccessibilityController;
  private readonly tree: DomA11yTree;
  private readonly live: DomLiveRegion;
  private mirror: CellMirror | undefined;
  private cols = 0;
  private rows = 0;

  constructor(
    doc: Document,
    private readonly palette: Palette,
    private readonly flagBits: FlagBits,
    opts: { onScroll?: (lines: number) => void } = {},
  ) {
    this.tree = new DomA11yTree(doc, (pos) => this.controller.onBoundaryFocus(pos));
    this.live = new DomLiveRegion(doc);
    this.controller = new AccessibilityController({
      tree: this.tree,
      live: this.live,
      onScroll: opts.onScroll,
    });
    this.root = doc.createElement("div");
    srOnly(this.root);
    this.root.appendChild(this.tree.container);
    this.root.appendChild(this.live.el);
  }

  /** Mirror a frame: update the cell store, then drive the controller with the
   * frame header + each viewport row's text. */
  onFrame(frame: DecodedFrame): void {
    if (!this.mirror || this.cols !== frame.cols || this.rows !== frame.rows) {
      this.mirror = new CellMirror(frame.cols, frame.rows, this.palette, this.flagBits);
      this.cols = frame.cols;
      this.rows = frame.rows;
    }
    this.mirror.applyFrame(frame);
    const rows = Array.from({ length: frame.rows }, (_, y) => this.mirror!.rowText(y));
    // A `DecodedFrame` structurally satisfies `A11yFrame` (it carries
    // rows/cursorRow/displayOffset/scrollbackLen/scroll op); `altScreen` is
    // absent until #149, so the controller treats it as the primary screen.
    this.controller.onFrame(frame, rows);
  }

  /** A key was typed (for echo dedup) — the consumer forwards its input here. */
  onKey(char: string): void {
    this.controller.onKey(char);
  }

  /** The widget lost focus. */
  onBlur(): void {
    this.controller.onBlur();
  }
}
