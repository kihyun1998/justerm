import { mouseFromDom, MouseEvents, type CellGeometry, type MouseEvent, type MouseEventLike } from "./input";

/**
 * Whether a press reports to the application rather than acting locally: the application tracks
 * presses (the DOWN bit of the frame's `mouseWantedEvents` mask, #129) and Shift is not held.
 * Shift is the force-selection modifier on every platform (#902).
 */
export function pressGoesToApp(mouseWantedEvents: number | undefined, ev: Pick<MouseEventLike, "shiftKey">): boolean {
  return ((mouseWantedEvents ?? 0) & MouseEvents.Down) !== 0 && !ev.shiftKey;
}

/** The DOM `buttons` bit of the DOM `button` number (left 1, right 2, middle 4, back 8, forward 16). */
function buttonBit(button: number): number {
  return button === 1 ? 4 : button === 2 ? 2 : 1 << button;
}

/** A DOM pointer event as the router reads it: {@link MouseEventLike} plus the click count. */
export interface PointerEventLike extends MouseEventLike {
  /** DOM click count (1 = single, 2 = double, …). */
  detail: number;
}

/** The local half of a press — the shape {@link import("./selection").SelectionController} has. */
export interface LocalPointer {
  /** `forced`: the press is local only because Shift overrode an application that tracks presses. */
  mouseDown(ev: MouseEventLike, detail: number, forced?: boolean): void;
  mouseMove(ev: MouseEventLike): void;
  mouseUp(ev: MouseEventLike): void;
  tick(): void;
  /** Drop the selection because the user typed (#913). Optional: a consumer on the older shape
   * keeps working and simply does not drop its selection. */
  clear?(): void;
}

/** The link half of the pointer — the shape {@link import("./link-tracker").LinkTracker} has (#934).
 * A cell is a viewport `[row, col]`, or `undefined` for a pointer outside the grid. */
export interface LinkPointer {
  /** A buttonless pointer is over `cell`; `undefined` also when a press there would not stay local.
   * `ev` is the motion, for its modifiers. */
  pointer(cell: readonly [number, number] | undefined, ev?: MouseEventLike): void;
  /** A single primary press that stays local; `undefined` for one that can press no link. */
  press(cell: readonly [number, number] | undefined, ev: MouseEventLike): void;
  /** That press's pointer moved to `cell`. */
  drag(cell: readonly [number, number] | undefined): void;
  /** The release of a local primary press. */
  release(cell: readonly [number, number] | undefined, ev: MouseEventLike): void;
}

/** The viewport cell under the pointer, or `undefined` when the pointer is outside the grid or the
 * box cannot be measured. */
function cellAt(ev: MouseEventLike, geom: CellGeometry | undefined): readonly [number, number] | undefined {
  if (!geom) return undefined;
  const col = Math.floor((ev.clientX - geom.originX) / geom.cellWidth);
  const row = Math.floor((ev.clientY - geom.originY) / geom.cellHeight);
  if (!(col >= 0 && col < geom.cols && row >= 0 && row < geom.rows)) return undefined;
  return [row, col];
}

/** What a {@link PointerRouter} reads and drives. */
export interface PointerRouterDeps {
  /** The latest frame's `mouseWantedEvents` mask. */
  mask(): number;
  /** The cell geometry, or `undefined` when the box cannot be measured (#819). */
  getGeometry(): CellGeometry | undefined;
  /** Where a pointer report for the application goes. */
  send(event: MouseEvent): void;
  /** Where a press that stays local goes; absent, such a press does nothing. */
  local?: LocalPointer;
  /** Where hover and a local primary click go for links (#934); absent, links are inert. */
  links?: LinkPointer;
  /** Start or stop calling the local handler's `tick()` on a timer. */
  setTicking(on: boolean): void;
}

/**
 * Routes pointer events, one press at a time, to the application or to a {@link LocalPointer}.
 *
 * The route is decided at the press and holds until the gesture ends. A gesture reported to the
 * application reports its drag (DRAG bit) and release (UP bit) and ends when no button is held — at
 * its release, or at a buttonless move or a lone press when that release never arrived; an X10
 * application (DOWN only) gets the press alone. A primary-button press that stays local hands its
 * motion and release to the local handler and ticks it meanwhile. Bare motion with no gesture and no
 * button held is reported under the MOVE bit. Nothing is reported without a measured box, nor for a
 * press or release of a button the intent cannot name.
 *
 * Pure: the widget binds the DOM listeners — {@link down} and {@link hover} on its element,
 * {@link move} and {@link up} on `window` while {@link active} — and owns the timer.
 */
export class PointerRouter {
  private gesture: "none" | "app" | "local" = "none";
  /** The last buttonless motion over the element, until the pointer leaves it. */
  private lastHover: MouseEventLike | undefined;

  constructor(private readonly deps: PointerRouterDeps) {}

  /** Whether a press is still being followed to its release. */
  get active(): boolean {
    return this.gesture !== "none";
  }

  /** A press. Returns whether it was acted on, so the caller can cancel its default. */
  down(ev: PointerEventLike): boolean {
    // A reported gesture with no other button held is one whose release never arrived.
    if (this.gesture === "app" && (ev.buttons & ~buttonBit(ev.button)) === 0) this.gesture = "none";
    if (this.gesture === "app" || (this.gesture === "none" && pressGoesToApp(this.deps.mask(), ev))) {
      if (!this.report(ev, "press")) return false;
      this.gesture = "app";
      return true;
    }
    const { local, links } = this.deps;
    if (!local && !links) return false;
    local?.mouseDown(ev, ev.detail, (this.deps.mask() & MouseEvents.Down) !== 0);
    if (ev.button === 0) {
      // A double click's second press is a word selection, so it presses no link.
      links?.press(ev.detail === 1 ? cellAt(ev, this.deps.getGeometry()) : undefined, ev);
      if (this.gesture === "none") {
        this.gesture = "local";
        if (local) this.deps.setTicking(true);
      }
    }
    return true;
  }

  /** Motion during a gesture. */
  move(ev: MouseEventLike): void {
    if (this.gesture === "local") {
      this.deps.local?.mouseMove(ev);
      this.deps.links?.drag(cellAt(ev, this.deps.getGeometry()));
      return;
    }
    if (this.gesture !== "app") return;
    if (ev.buttons === 0) {
      this.gesture = "none"; // its release never arrived
      return;
    }
    if ((this.deps.mask() & MouseEvents.Drag) !== 0) this.report(ev, "motion");
  }

  /** Motion over the element. */
  hover(ev: MouseEventLike): void {
    if (this.gesture !== "none" || ev.buttons !== 0) return;
    this.lastHover = ev;
    this.hoverLinks(ev);
    if ((this.deps.mask() & MouseEvents.Move) !== 0) this.report(ev, "motion");
  }

  /** Ask the last hover's question again — a frame may have changed the mask under a resting pointer. */
  refresh(): void {
    if (this.gesture === "none" && this.lastHover) this.hoverLinks(this.lastHover);
  }

  /** The pointer left the element, or moved onto something over it that is not the grid. */
  leave(): void {
    this.lastHover = undefined;
    this.deps.links?.pointer(undefined);
  }

  /** Hover says what a press here would do, so it asks the press's own question. */
  private hoverLinks(ev: MouseEventLike): void {
    const links = this.deps.links;
    if (!links) return;
    links.pointer(pressGoesToApp(this.deps.mask(), ev) ? undefined : cellAt(ev, this.deps.getGeometry()), ev);
  }

  /** A release during a gesture. */
  up(ev: MouseEventLike): void {
    if (this.gesture === "local") {
      this.gesture = "none";
      if (this.deps.local) this.deps.setTicking(false);
      this.deps.local?.mouseUp(ev);
      this.deps.links?.release(cellAt(ev, this.deps.getGeometry()), ev);
      return;
    }
    if (this.gesture !== "app") return;
    if ((this.deps.mask() & MouseEvents.Up) !== 0) this.report(ev, "release");
    if (ev.buttons === 0) this.gesture = "none";
  }

  private report(ev: MouseEventLike, action: "press" | "release" | "motion"): boolean {
    const geom = this.deps.getGeometry();
    if (!geom) return false;
    const event = mouseFromDom(ev, action, geom);
    if (event.button === null && action !== "motion") return false;
    this.deps.send(event);
    return true;
  }
}
