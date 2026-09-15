import { mouseFromDom, MouseEvents, type CellGeometry, type MouseEvent, type MouseEventLike } from "./input";

/**
 * Whether a press reports to the application rather than acting locally: the application tracks
 * presses (the DOWN bit of the frame's `mouseWantedEvents` mask, #129) and Shift is not held.
 * Shift is the force-selection modifier on every platform (#902).
 */
export function pressGoesToApp(mouseWantedEvents: number | undefined, ev: Pick<MouseEventLike, "shiftKey">): boolean {
  return ((mouseWantedEvents ?? 0) & MouseEvents.Down) !== 0 && !ev.shiftKey;
}

/** A DOM pointer event as the router reads it: {@link MouseEventLike} plus the click count. */
export interface PointerEventLike extends MouseEventLike {
  /** DOM click count (1 = single, 2 = double, …). */
  detail: number;
}

/** The local half of a press — the shape {@link import("./selection").SelectionController} has. */
export interface LocalPointer {
  mouseDown(ev: MouseEventLike, detail: number, forced?: boolean): void;
  mouseMove(ev: MouseEventLike): void;
  mouseUp(ev: MouseEventLike): void;
  tick(): void;
}

export interface PointerRouterDeps {
  mask(): number;
  getGeometry(): CellGeometry | undefined;
  send(event: MouseEvent): void;
  local?: LocalPointer;
  setTicking(on: boolean): void;
}

export class PointerRouter {
  private gesture: "none" | "app" | "local" = "none";

  constructor(private readonly deps: PointerRouterDeps) {}

  /** Whether a press is still being followed to its release. */
  get active(): boolean {
    return this.gesture !== "none";
  }

  down(ev: PointerEventLike): boolean {
    if (this.gesture === "app" || (this.gesture === "none" && pressGoesToApp(this.deps.mask(), ev))) {
      if (!this.report(ev, "press")) return false;
      this.gesture = "app";
      return true;
    }
    const local = this.deps.local;
    if (!local) return false;
    local.mouseDown(ev, ev.detail, (this.deps.mask() & MouseEvents.Down) !== 0);
    if (ev.button === 0 && this.gesture === "none") {
      this.gesture = "local";
      this.deps.setTicking(true);
    }
    return true;
  }

  move(ev: MouseEventLike): void {
    if (this.gesture === "local") {
      this.deps.local?.mouseMove(ev);
      return;
    }
    if (this.gesture !== "app") return;
    if (ev.buttons !== 0 && (this.deps.mask() & MouseEvents.Drag) !== 0) this.report(ev, "motion");
  }

  hover(ev: MouseEventLike): void {
    if (this.gesture !== "none" || ev.buttons !== 0) return;
    if ((this.deps.mask() & MouseEvents.Move) !== 0) this.report(ev, "motion");
  }

  up(ev: MouseEventLike): void {
    if (this.gesture === "local") {
      this.gesture = "none";
      this.deps.setTicking(false);
      this.deps.local?.mouseUp(ev);
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
