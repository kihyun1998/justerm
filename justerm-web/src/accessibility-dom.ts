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
import { a11ySelectionToPort } from "./a11y-selection";
import type { TreeSelection } from "./a11y-selection";
import { CellMirror } from "./cell-mirror";
import { ScreenReaderState } from "./screen-reader";
import type { FlagBits } from "./render-core";
import type { SelectionPort } from "./selection";
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
    private readonly onBoundary: (position: "top" | "bottom", cameFromInner: boolean) => void,
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

  /** The viewport-row index of the listitem containing `node`, or `null` if `node`
   * isn't inside the row tree (#152 — resolving a DOM selection endpoint to a row).
   * Walks up from the node to its listitem ancestor. */
  rowIndexOf(node: Node | null): number | null {
    if (!node || !this.container.contains(node)) return null;
    let el: Node | null = node;
    while (el && el !== this.container) {
      const i = this.rows.indexOf(el as HTMLElement);
      if (i !== -1) return i;
      el = el.parentNode;
    }
    return null;
  }

  /** Put the boundary listeners on the current first/last rows (re-bound after
   * each resize, since those elements change). */
  private bindBoundaries(): void {
    for (const [i, el] of this.rows.entries()) {
      const position = i === 0 ? "top" : i === this.rows.length - 1 ? "bottom" : null;
      if (!position) {
        el.onfocus = null;
        continue;
      }
      // The inner neighbour whose focus, if it preceded this one, means the user
      // walked outward (xterm's `relatedTarget` guard, passed to the controller).
      const inner = this.rows[position === "top" ? 1 : this.rows.length - 2];
      el.onfocus = (e) => this.onBoundary(position, e.relatedTarget === inner);
    }
  }
}

/** A hidden `aria-live` region that announces new output. */
class DomLiveRegion implements LiveRegionSink {
  readonly el: HTMLElement;

  constructor(doc: Document) {
    this.el = doc.createElement("div");
    // `assertive`: primary-screen output is the headline announce and should
    // interrupt (#119 spec). Alt-screen repaints never reach here — the
    // controller suppresses them upstream — so this only ever carries output.
    this.el.setAttribute("aria-live", "assertive");
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

  private readonly srState: ScreenReaderState;
  /** #152 selection bridge (optional): the `selectionchange` listener + the port it
   * drives. Absent → no bridge installed (SR announce/tree still work). */
  private readonly selectionPort: SelectionPort | undefined;
  private readonly onSelectionChange: (() => void) | undefined;

  constructor(
    private readonly doc: Document,
    private readonly palette: Palette,
    private readonly flagBits: FlagBits,
    opts: {
      onScroll?: (lines: number) => void;
      /** Shared SR-active gate (#161). Pass the same instance used to gate the
       * command announce (#160) so one host toggle governs both. Defaults to a
       * fresh, active gate (announce on). */
      screenReaderState?: ScreenReaderState;
      /** The selection write seam (S8/#109). When provided, an AT text selection in
       * the row tree bridges to the engine selection (#152) — the same port the
       * mouse selection drives. Absent → no a11y selection bridge. */
      selectionPort?: SelectionPort;
    } = {},
  ) {
    this.srState = opts.screenReaderState ?? new ScreenReaderState();
    this.tree = new DomA11yTree(doc, (pos, fromInner) =>
      this.controller.onBoundaryFocus(pos, fromInner),
    );
    this.live = new DomLiveRegion(doc);
    this.controller = new AccessibilityController({
      tree: this.tree,
      // Gate the live announce on SR-active (#161).
      live: this.srState.gateLive(this.live),
      // Skip the per-frame row-tree churn while inactive (#169) — bookkeeping is
      // kept, so reactivation re-syncs instantly (see setScreenReaderActive).
      isActive: () => this.srState.isActive(),
      onScroll: opts.onScroll,
    });
    this.root = doc.createElement("div");
    srOnly(this.root);
    this.root.appendChild(this.tree.container);
    this.root.appendChild(this.live.el);

    // #152: bridge an AT text selection in the row tree to the engine selection. The
    // browser fires `selectionchange` on the document; resolve it to tree coordinates
    // and drive the same SelectionPort the mouse uses. Only when a port is wired.
    this.selectionPort = opts.selectionPort;
    if (this.selectionPort) {
      this.onSelectionChange = () => this.bridgeSelection();
      doc.addEventListener("selectionchange", this.onSelectionChange);
    }
  }

  /** Resolve the document's current selection to {@link TreeSelection} and drive the
   * port (#152). Runs on every `selectionchange`; a selection outside the row tree is
   * a no-op (the bridge's `anchor === null` guard). DOM glue — proven live, not in the
   * DOM-less test env; the resolution + mapping logic is unit-tested in `a11ySelectionToPort`. */
  private bridgeSelection(): void {
    if (!this.selectionPort || !this.mirror) return;
    const s = this.doc.getSelection();
    if (!s) return;
    const anchorRow = this.tree.rowIndexOf(s.anchorNode);
    const focusRow = this.tree.rowIndexOf(s.focusNode);
    const sel: TreeSelection = {
      anchor: anchorRow === null ? null : { row: anchorRow, offset: s.anchorOffset },
      focus: focusRow === null ? null : { row: focusRow, offset: s.focusOffset },
      collapsed: s.isCollapsed,
    };
    a11ySelectionToPort(sel, (r) => this.mirror!.rowCells(r).columns, this.selectionPort);
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
    // A `DecodedFrame` structurally satisfies `A11yFrame` (rows / displayOffset /
    // scrollbackLen / scroll op / altScreen #149) — the controller suppresses
    // announce on the alt screen.
    this.controller.onFrame(frame, rows);
  }

  /** Set whether a screen reader is active (#161) — the host injects its own SR
   * detection (a browser can't detect one). While inactive, output announces are
   * suppressed (#161) and the row-tree DOM churn is skipped (#169); reactivating
   * re-syncs the tree from the cached frame at once (no cold rebuild). Share the
   * gate with the command announce (#160) via the `screenReaderState` option. */
  setScreenReaderActive(active: boolean): void {
    const was = this.srState.isActive();
    this.srState.setActive(active);
    // Reactivated → refresh the stale tree from the cached frame at once (no cold
    // rebuild) AND drop any announce backlog so it isn't replayed (#169).
    if (active && !was) this.controller.reactivate();
  }

  /** A key was typed (for echo dedup) — the consumer forwards its input here. */
  onKey(char: string): void {
    this.controller.onKey(char);
  }

  /** The widget lost focus. */
  onBlur(): void {
    this.controller.onBlur();
  }

  /** Tear down: cancel the controller's pending announce and detach the hidden
   * root from the DOM. */
  dispose(): void {
    if (this.onSelectionChange) {
      this.doc.removeEventListener("selectionchange", this.onSelectionChange);
    }
    this.controller.dispose();
    this.root.remove();
  }
}
