/**
 * On-demand accessible view (#150): the VSCode "Accessible Buffer" analog. A
 * screen-reader user summons it to read the *whole buffer* (scrollback + screen)
 * as one navigable, copyable document — the escape hatch from the aria-live
 * firehose / 20-line cap of the row-tree mirror (#119).
 *
 * Pure logic — no DOM: the full-buffer text comes from core over a query seam
 * ({@link AccessiblePort}, sibling of `SelectionPort.text`), and the rendered
 * document is an injected {@link AccessibleView} sink. In frame mode the web
 * side has no scrollback cells, so the text *must* come from core — the boundary
 * is physically enforced.
 */

/**
 * The read-query seam to the engine's full-buffer text (core
 * `Engine::accessible_text`). Frame mode wires it to the backend over IPC, like
 * `SelectionPort.text`; the result is soft-wrap-joined logical lines.
 */
export interface AccessiblePort {
  /** The whole buffer as one document string (`\n` between logical lines). */
  text(): Promise<string>;
}

/** The rendered document sink: a hidden, focusable, read-only element a screen
 * reader navigates as a document. A thin DOM wrapper satisfies it. */
export interface AccessibleView {
  /** Render `text` as the document and move focus into it. */
  show(text: string): void;
  /** Tear the document down (the controller then restores focus). */
  hide(): void;
}

/**
 * Drives the accessible view: summon → query core → show the document; close →
 * hide → restore focus to the widget. The document itself (navigation, copy,
 * screen-reader reading) is the host element's job, exactly as VSCode leans on
 * its editor — this only orchestrates.
 */
export class AccessibleViewController {
  private open = false;
  private readonly restoreFocus: () => void;

  constructor(
    private readonly port: AccessiblePort,
    private readonly view: AccessibleView,
    opts: { restoreFocus?: () => void } = {},
  ) {
    this.restoreFocus = opts.restoreFocus ?? (() => {});
  }

  /** Fetch the whole-buffer text and show it as the document. */
  async summon(): Promise<void> {
    const text = await this.port.text();
    this.view.show(text);
    this.open = true;
  }

  /** Tear the document down and return focus to the widget. Inert if not open. */
  close(): void {
    if (!this.open) return;
    this.view.hide();
    this.open = false;
    this.restoreFocus();
  }

  /** Whether the view is currently open. */
  isOpen(): boolean {
    return this.open;
  }
}

/** A recording/preset {@link AccessiblePort} for tests and the demo — the
 * simplest concrete source behind the query seam (mirrors `StubSelectionPort`). */
export class StubAccessiblePort implements AccessiblePort {
  /** What the next {@link text} query resolves to (set by tests/demo). */
  value = "";
  text(): Promise<string> {
    return Promise.resolve(this.value);
  }
}

/**
 * A DOM {@link AccessibleView}: a full-screen, focusable, read-only `role=document`
 * overlay a screen reader navigates as a document (arrow keys, copy). Thin DOM —
 * verified in the demo + a real screen reader, not vitest.
 */
export class DomAccessibleView implements AccessibleView {
  /** Mount this over the terminal; the controller drives show/hide. */
  readonly el: HTMLElement;

  constructor(doc: Document, onEscape?: () => void) {
    this.el = doc.createElement("pre");
    this.el.setAttribute("role", "document");
    this.el.setAttribute("aria-label", "Terminal accessible view");
    this.el.tabIndex = 0;
    Object.assign(this.el.style, {
      position: "fixed",
      inset: "0",
      margin: "0",
      padding: "1rem",
      overflow: "auto",
      display: "none",
      whiteSpace: "pre-wrap",
      background: "#1e1e2e",
      color: "#cdd6f4",
      font: "14px monospace",
      zIndex: "100",
    });
    // Escape is the document's own close affordance; the consumer routes it to
    // the controller's close (which hides this and restores focus).
    this.el.addEventListener("keydown", (e) => {
      if (e.key === "Escape") onEscape?.();
    });
  }

  show(text: string): void {
    this.el.textContent = text;
    this.el.style.display = "block";
    this.el.focus();
  }

  hide(): void {
    this.el.style.display = "none";
    this.el.textContent = "";
  }
}
