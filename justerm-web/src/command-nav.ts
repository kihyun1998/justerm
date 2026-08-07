/**
 * Prompt-to-prompt command navigation (#166), the frame-mode analog of VSCode's
 * `navigateToCommand` (`terminal.accessibility.contribution.ts:173`). A
 * screen-reader user in the accessible view (#150) walks the *whole command
 * history* — Previous/Next jump the reading cursor to the adjacent command,
 * reveal it, announce the typed command line, and fire the exit-driven success/
 * fail signal (#160 reuse).
 *
 * Pure logic — the command list comes from core over a query seam
 * ({@link CommandNavPort}, sibling of `AccessiblePort`; core `Engine::command_lines`),
 * and the reveal/announce/signal are injected sinks (ADR-0017: the marks + text +
 * document-line mapping are core's; the navigation *policy* is the consumer's).
 * In frame mode the web side has no scrollback cells, so the command text and its
 * document line *must* come from core — the boundary is physically enforced.
 */

import type { LiveRegionSink } from "./accessibility";
import type { SignalSink } from "./command-announce";

/** One executed command from core's `command_lines` query. `line` is a *document*
 * line (accessible-view coordinates, soft-wrap collapsed); `command` is the typed
 * text (prompt/output excluded); `exit` is the code, if the command finished.
 *
 * `line` is an index into the accessible view's document and is only valid against
 * the document sampled with it (#743). It cannot be rebased by any scalar core
 * publishes: the document space moves on axes the absolute space does not — an
 * eviction that pops a soft-wrap continuation row moves the absolute lines and not
 * this one, and ordinary output that makes a row wrap moves this one while nothing
 * else moves at all. On the alt screen it additionally indexes the *primary*
 * document while the view is showing the alt one. Hold the pair or re-ask for the
 * pair; never carry the line alone. */
export interface CommandInfo {
  readonly line: number;
  readonly command: string;
  readonly exit?: number;
}

/** The read-query seam to core's `Engine::command_lines` (sibling of
 * `AccessiblePort`). Frame mode wires it to the backend over IPC. */
export interface CommandNavPort {
  commands(): Promise<CommandInfo[]>;
}

/** The accessible-view reading cursor the nav drives: reveal (and move focus to)
 * a document line. A thin wrapper over {@link DomAccessibleView} satisfies it —
 * the counterpart to VSCode's `setPosition`. */
export interface NavView {
  /** Move the reading cursor to document line `line`, and **report whether that
   * line exists in the document currently held** (#743).
   *
   * The boolean is the load-bearing part. A document line is only meaningful
   * against the document it indexes, and the view is the only thing that knows
   * which document that is — so this asks the owner of the state rather than
   * tracking an edge beside it (the shape #746/#747 reached for the sibling
   * coordinate cache). It answers `false` for all three ways the pairing breaks,
   * and the caller needs none of them separately: no document is open, the line is
   * past the end of a document sampled at another instant, or the view is showing
   * the alt screen while the line indexes the primary one. */
  reveal(line: number): boolean;
}

/**
 * Drives command navigation over the accessible view. Load the command list when
 * the view is summoned, then {@link previous}/{@link next} jump the reading
 * cursor. Filtering mirrors VSCode exactly: Previous = commands above the cursor
 * (`line < cursor`, nearest); Next = commands below (`line > cursor`, nearest);
 * an empty filtered set clamps (no-op).
 *
 * The reading cursor is controller-owned (not read back from the DOM, which a
 * `<pre>` can't report), reset to the end on {@link load} so a fresh summon starts
 * Previous from the last command.
 */
export class CommandNavController {
  private commands: CommandInfo[] = [];
  private loaded = false;
  /** Reading position as a document line; `+Infinity` = "at the end" so the first
   * Previous lands on the last command and the first Next is a no-op. */
  private cursor = Number.POSITIVE_INFINITY;

  constructor(
    private readonly port: CommandNavPort,
    private readonly live: LiveRegionSink,
    private readonly signal: SignalSink,
    private readonly view: NavView,
  ) {}

  /**
   * (Re)query the command list and reset the reading cursor to the end. **Call this
   * when the accessible view is summoned, and sample the view's document in the same
   * breath** — this is the single point at which the list is taken, and nothing else
   * re-takes it (#743).
   *
   * Both halves matter. The lines are *document* lines into the text the view is
   * showing, so a list sampled at a different instant from that text is off by
   * however many rows moved in between — and core cannot rebase it for us, because
   * the document space moves on axes no published scalar dates (`Engine::command_lines`,
   * ADR-0029 D2/D3). A frame-mode backend should therefore answer `commands()` and
   * the accessible text from **one engine borrow**; over IPC they are two messages,
   * and the pairing is only as tight as the backend makes it.
   */
  async load(): Promise<void> {
    this.commands = await this.port.commands();
    this.loaded = true;
    this.cursor = Number.POSITIVE_INFINITY;
  }

  /** Jump to the command above the reading cursor (VSCode Previous). */
  async previous(): Promise<void> {
    await this.jump("previous");
  }

  /** Jump to the command below the reading cursor (VSCode Next). */
  async next(): Promise<void> {
    await this.jump("next");
  }

  private async jump(dir: "previous" | "next"): Promise<void> {
    // Nav before {@link load} is meaningless, not merely early: `line` is a document
    // line into the accessible view's document, and before a summon there is no
    // document to index. Loading here instead — which this did until #743 — samples
    // the list at an instant no document was taken at, and then holds it for the rest
    // of the session, because nothing clears `loaded`.
    if (!this.loaded) return;
    const candidates =
      dir === "previous"
        ? this.commands.filter((c) => c.line < this.cursor).sort((a, b) => b.line - a.line)
        : this.commands.filter((c) => c.line > this.cursor).sort((a, b) => a.line - b.line);
    const target = candidates[0];
    if (!target) return; // boundary — clamp to a no-op (VSCode returns on empty)

    // The reveal is asked *first*, and nothing else happens unless it lands. `loaded`
    // above is an **edge** — set once, cleared nowhere — so on its own it says the list
    // was sampled, not that the document it indexes is still there. The view owns that
    // state, so the view is asked for it. Without this, closing the view and pressing
    // Previous announced a command and played its earcon while the reading cursor did
    // not move, because `reveal` no-ops silently on a torn-down document.
    //
    // It bounds the damage; it does not make a stale line safe. A line that resolves
    // *in range* against the wrong document is indistinguishable from a right one here
    // — which is why the contract is to re-ask, and why this is a guard rather than the
    // fix (#743).
    if (!this.view.reveal(target.line)) return;

    this.cursor = target.line;
    // VSCode only `alert`s when the command line is non-empty (a bare Enter has
    // no text to read); the position move + signal still happen.
    if (target.command) this.live.announce(target.command);
    const failed = target.exit !== undefined && target.exit !== 0;
    if (failed) this.signal.commandFailed();
    else this.signal.commandSucceeded();
  }
}

/** A preset {@link CommandNavPort} for the demo/tests — the simplest concrete
 * source behind the query seam (mirrors `StubAccessiblePort`). */
export class StubCommandNavPort implements CommandNavPort {
  /** What the next {@link commands} query resolves to (set by the demo/tests). */
  list: CommandInfo[] = [];
  commands(): Promise<CommandInfo[]> {
    return Promise.resolve(this.list);
  }
}
