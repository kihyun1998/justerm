// DOM input events → intent objects the backend feeds to justerm-core's pure
// encoders (`encode_key` / `encode_mouse` / …). In frame mode the web only
// normalises events; the protocol/encoding bytes are the backend's job. These
// intent types mirror `justerm-core/src/input.rs` (the backend contract).

/**
 * Mouse event categories the active tracking mode reports — the frame's
 * {@link import("./types").DecodedFrame.mouseWantedEvents} mask (#129, mirroring
 * core `MouseEvents`). A set bit routes that category to the app; else it stays
 * local (selection / scrollback). `Wheel` is the bit S16's wheel routing reads.
 */
export const MouseEvents = {
  /** Button press (every protocol except Off). */
  Down: 1 << 0,
  /** Button release (`?1000`+). */
  Up: 1 << 1,
  /** Wheel turn (`?1000`+ — X10 excludes it). */
  Wheel: 1 << 2,
  /** Motion while a button is held — drag (`?1002`+). */
  Drag: 1 << 3,
  /** Bare motion, no button held (`?1003`). */
  Move: 1 << 4,
} as const;

/** Modifier bitmask — the kitty scheme core uses (Shift=1, Alt=2, Ctrl=4, …). */
export const Mod = {
  None: 0,
  Shift: 1,
  Alt: 2,
  Ctrl: 4,
  Super: 8,
  Hyper: 16,
  Meta: 32,
  CapsLock: 64,
  NumLock: 128,
} as const;

/** A named key (no payload) — mirrors core `Key`'s unit variants. */
export type NamedKey =
  | "up"
  | "down"
  | "left"
  | "right"
  | "home"
  | "end"
  | "pageup"
  | "pagedown"
  | "insert"
  | "delete"
  | "enter"
  | "tab"
  | "backspace"
  | "escape";

/** A logical key (mirrors core `Key`). */
export type Key =
  | { type: "char"; char: string }
  | { type: "f"; n: number }
  | { type: NamedKey };

/** DOM `KeyboardEvent.key` → named {@link Key} variant. */
const NAMED: Readonly<Record<string, NamedKey>> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  Home: "home",
  End: "end",
  PageUp: "pageup",
  PageDown: "pagedown",
  Insert: "insert",
  Delete: "delete",
  Enter: "enter",
  Tab: "tab",
  Backspace: "backspace",
  Escape: "escape",
};

export type KeyAction = "press" | "repeat" | "release";

/** A key event intent (mirrors core `KeyEvent`). */
export interface KeyEvent {
  key: Key;
  /** Modifier bitmask ({@link Mod}). */
  mods: number;
  action: KeyAction;
}

/** The subset of a DOM `KeyboardEvent` the normaliser reads. */
export interface KeyboardEventLike {
  key: string;
  shiftKey: boolean;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
}

/**
 * A normalised input intent — the outbound unit. The backend feeds it to
 * justerm-core's pure encoders (`encode_key` / `encode_mouse` / `encode_paste` /
 * `encode_focus`) and writes the bytes to the PTY. The web never encodes.
 */
export type Intent =
  | { kind: "key"; event: KeyEvent }
  | { kind: "mouse"; event: MouseEvent }
  | { kind: "paste"; text: string }
  | { kind: "focus"; focused: boolean }
  /** Committed text from an IME composition (#116) — RAW, unbracketed, unlike a
   * clipboard `paste` (?2004). The backend encodes it with `encode_paste(text,
   * false)` (core's raw path). Distinct kind so the backend picks the right
   * `bracketed` — IME insertion behaves like typing, not a paste. */
  | { kind: "text"; text: string };

/** The subset of a DOM `<textarea>` the IME composition controller reads (#116).
 * A real `HTMLTextAreaElement` satisfies it; tests pass a mutable plain object so
 * the controller is exercised without a DOM. */
export interface TextareaLike {
  value: string;
  selectionStart: number | null;
  selectionEnd: number | null;
}

/**
 * The outbound seam — where normalised intents go. Frame mode wires this to the
 * consumer's IPC channel (→ backend → core encoders → PTY); the future in-wasm
 * mode wires it to the in-browser engine. The inbound analog is `FrameSource`.
 */
export interface InputSink {
  send(intent: Intent): void;
}

/** An in-memory {@link InputSink} that records intents — for tests and demos. */
export class StubInputSink implements InputSink {
  readonly sent: Intent[] = [];
  send(intent: Intent): void {
    this.sent.push(intent);
  }
}

/** A mouse button (mirrors core `MouseButton`). */
export type MouseButton =
  | "left"
  | "middle"
  | "right"
  | "wheelUp"
  | "wheelDown"
  | "wheelLeft"
  | "wheelRight"
  | "back"
  | "forward";

export type MouseAction = "press" | "release" | "motion";

/** A mouse event intent in viewport cell coords (mirrors core `MouseEvent`). */
export interface MouseEvent {
  /** The button, or `null` for bare motion with no button held. */
  button: MouseButton | null;
  action: MouseAction;
  /** 0-based cell coordinates (the encoding shifts to 1-based on the wire). */
  col: number;
  row: number;
  /** 0-based pixel coordinates (used only by the `?1016` SGR-pixels encoding). */
  px: number;
  py: number;
  mods: number;
}

/** The subset of a DOM `MouseEvent` the normaliser reads. */
export interface MouseEventLike {
  clientX: number;
  clientY: number;
  /** DOM button: 0 = left, 1 = middle, 2 = right (the press/release trigger). */
  button: number;
  /** DOM buttons bitmask of held buttons: 1 = left, 2 = right, 4 = middle. */
  buttons: number;
  shiftKey: boolean;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  /** DOM event timestamp (ms). Used to tell a quick alt-click (cursor move) from
   * a slow one. Optional — only the selection controller's alt-click path reads
   * it; events that never take that path may omit it. */
  timeStamp?: number;
}

/** Canvas origin + cell size, to map pixels to cells. */
export interface CellGeometry {
  originX: number;
  originY: number;
  cellWidth: number;
  cellHeight: number;
}

const DOM_BUTTON: Readonly<Record<number, MouseButton>> = { 0: "left", 1: "middle", 2: "right" };

/** Which button an event concerns: the trigger on press/release, the *held*
 * button (from the `buttons` bitmask) on motion — `null` for bare motion. */
function buttonOf(ev: MouseEventLike, action: MouseAction): MouseButton | null {
  if (action === "motion") {
    if (ev.buttons & 1) return "left";
    if (ev.buttons & 2) return "right";
    if (ev.buttons & 4) return "middle";
    return null;
  }
  return DOM_BUTTON[ev.button] ?? null;
}

/** Normalise a DOM mouse event into a {@link MouseEvent} intent. */
export function mouseFromDom(ev: MouseEventLike, action: MouseAction, geom: CellGeometry): MouseEvent {
  return cellEvent(ev, buttonOf(ev, action), action, geom);
}

/** Normalise a wheel notch into a wheel-button press {@link MouseEvent} (the
 * app-mouse-reporting case). `deltaY < 0` is up. */
export function wheelMouseFromDom(ev: MouseEventLike, deltaY: number, geom: CellGeometry): MouseEvent {
  return cellEvent(ev, deltaY < 0 ? "wheelUp" : "wheelDown", "press", geom);
}

function cellEvent(
  ev: MouseEventLike,
  button: MouseButton | null,
  action: MouseAction,
  geom: CellGeometry,
): MouseEvent {
  const px = ev.clientX - geom.originX;
  const py = ev.clientY - geom.originY;
  return {
    button,
    action,
    col: Math.floor(px / geom.cellWidth),
    row: Math.floor(py / geom.cellHeight),
    px,
    py,
    mods: modsOf(ev),
  };
}

/** Normalise a DOM keyboard event into a {@link KeyEvent} intent. */
export function keyFromDom(ev: KeyboardEventLike): KeyEvent {
  return { key: keyOf(ev.key), mods: modsOf(ev), action: "press" };
}

export interface CaptureOptions {
  /** Current canvas origin + cell size (read per event — it changes on resize). */
  getGeometry(): CellGeometry;
  /**
   * Whether mouse/wheel events should be *reported to the app* (the app has a
   * mouse-tracking mode on). When false they stay local (selection / scrollback).
   * Until the frame exposes the mode (a core-surface gap — see #111 notes), the
   * consumer supplies it; default `false` (no app reporting).
   */
  mouseReporting?(): boolean;
  /**
   * A gate consulted on each keydown before it becomes a key intent (#116). When
   * it returns `false` the key is left alone — no intent, no `preventDefault` — so
   * an IME can own it (a `keyCode` 229 composition key, or a key that finalizes a
   * composition). Return `true` (the default when absent) to send it as a key.
   */
  beforeKey?(ev: KeyboardEvent): boolean;
}

/**
 * Attach DOM listeners to `target` that normalise events and push intents to
 * `sink`. Keys always report (unless {@link CaptureOptions.beforeKey} vetoes them
 * for an IME); mouse/wheel report only when `mouseReporting()` (else the consumer
 * handles them locally). Returns a disposer.
 *
 * Browser-only glue — not unit-tested; the per-event normalisation it calls
 * ({@link keyFromDom} / {@link mouseFromDom} / {@link wheelMouseFromDom}) is.
 */
export function captureInput(target: HTMLElement, sink: InputSink, opts: CaptureOptions): () => void {
  const reporting = (): boolean => opts.mouseReporting?.() ?? false;
  const onKey = (e: KeyboardEvent): void => {
    if (opts.beforeKey && !opts.beforeKey(e)) return; // an IME owns this key
    e.preventDefault();
    sink.send({ kind: "key", event: keyFromDom(e) });
  };
  const onMouse =
    (action: MouseAction) =>
    (e: globalThis.MouseEvent): void => {
      if (!reporting()) return;
      sink.send({ kind: "mouse", event: mouseFromDom(e, action, opts.getGeometry()) });
    };
  const onWheel = (e: WheelEvent): void => {
    if (!reporting()) return;
    sink.send({ kind: "mouse", event: wheelMouseFromDom(e, e.deltaY, opts.getGeometry()) });
  };
  const onPaste = (e: ClipboardEvent): void => {
    // preventDefault so the pasted text doesn't also land in the (hidden IME) textarea
    // and accumulate there (#116); we forward it as an intent instead. xterm does the same.
    e.preventDefault();
    sink.send({ kind: "paste", text: e.clipboardData?.getData("text") ?? "" });
  };
  const onFocus = (): void => sink.send({ kind: "focus", focused: true });
  const onBlur = (): void => sink.send({ kind: "focus", focused: false });

  const mousedown = onMouse("press");
  const mouseup = onMouse("release");
  const mousemove = onMouse("motion");
  target.addEventListener("keydown", onKey);
  target.addEventListener("mousedown", mousedown);
  target.addEventListener("mouseup", mouseup);
  target.addEventListener("mousemove", mousemove);
  target.addEventListener("wheel", onWheel);
  target.addEventListener("paste", onPaste);
  target.addEventListener("focus", onFocus);
  target.addEventListener("blur", onBlur);

  return () => {
    target.removeEventListener("keydown", onKey);
    target.removeEventListener("mousedown", mousedown);
    target.removeEventListener("mouseup", mouseup);
    target.removeEventListener("mousemove", mousemove);
    target.removeEventListener("wheel", onWheel);
    target.removeEventListener("paste", onPaste);
    target.removeEventListener("focus", onFocus);
    target.removeEventListener("blur", onBlur);
  };
}

/** DOM modifier booleans → the kitty modifier bitmask (core `Modifiers`). */
function modsOf(ev: {
  shiftKey: boolean;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
}): number {
  return (
    (ev.shiftKey ? Mod.Shift : 0) |
    (ev.altKey ? Mod.Alt : 0) |
    (ev.ctrlKey ? Mod.Ctrl : 0) |
    (ev.metaKey ? Mod.Super : 0)
  );
}

function keyOf(domKey: string): Key {
  const named = NAMED[domKey];
  if (named) return { type: named };
  const fn = /^F(\d{1,2})$/.exec(domKey);
  if (fn) return { type: "f", n: Number(fn[1]) };
  return { type: "char", char: domKey };
}
