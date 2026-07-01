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
 * text (prompt/output excluded); `exit` is the code, if the command finished. */
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
  reveal(line: number): void;
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

  /** (Re)query the command list and reset the reading cursor to the end. Call
   * when the accessible view is summoned. */
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
    if (!this.loaded) await this.load();
    const candidates =
      dir === "previous"
        ? this.commands.filter((c) => c.line < this.cursor).sort((a, b) => b.line - a.line)
        : this.commands.filter((c) => c.line > this.cursor).sort((a, b) => a.line - b.line);
    const target = candidates[0];
    if (!target) return; // boundary — clamp to a no-op (VSCode returns on empty)

    this.cursor = target.line;
    this.view.reveal(target.line);
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
