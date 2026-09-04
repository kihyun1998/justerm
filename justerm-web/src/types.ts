// The one import this file takes from the decoder, and it is `import type` — erased at emit, so
// nothing here gains a runtime dependency. `Palette` already crosses the same way in
// `justerm-renderer.ts` and `accessibility-dom.ts`. See {@link FlagBits} for why this type is
// derived while {@link DecodedFrame} below is deliberately not.
import type { Flags as WasmFlags } from "justerm-wasm-decode";

/**
 * A decoded terminal frame — the unit the renderer consumes.
 *
 * Structure-of-arrays: one column per cell field (`codepoints`/`fg`/`bg`/…) plus
 * a `spans` directory, exactly as `justerm-wasm-decode`'s `DecodedFrame` exposes
 * it. This is a *structural* view — the wasm `DecodedFrame` class satisfies it
 * (its getters return these typed arrays, and it carries extra cursor/scroll
 * getters later slices read), and tests/demos pass plain objects. The shape is
 * source-agnostic: a frame may arrive decoded from a backend wire (frame mode)
 * or be produced by an in-wasm engine (future).
 *
 * Cells are addressed through the span directory, not row-major: walk `spans` in
 * stride-5 chunks and index the columns at each span's `cell_offset`. See
 * {@link import("./render-core").frameToDrawOps}.
 *
 * **Read each column once, into a local, before any loop.** On a plain object these are properties
 * and it makes no difference; on the wasm `DecodedFrame` they are *getters*, and every read builds
 * a fresh typed-array view — or, for `sideTable` / `linkTable`, rebuilds the whole array. The
 * declaration below cannot express the difference, and no fixture in this package exhibits it, so
 * the cost only appears in production (#657). Taking a column as a function parameter, the way
 * `readMarkers` and `readOverlay` do, sidesteps the question entirely.
 */
export interface DecodedFrame {
  readonly cols: number;
  readonly rows: number;
  /** `0` = Full (whole viewport); `1` = Partial (only the listed spans). */
  readonly kind: number;
  /** Per-cell base codepoint, span order (`0` = blank). */
  readonly codepoints: ArrayLike<number>;
  /** Per-cell fg/bg colour refs (tagged u32; resolve with `resolveRgb`). */
  readonly fg: ArrayLike<number>;
  readonly bg: ArrayLike<number>;
  /**
   * Per-cell underline colour ref (SGR 58, #520): tagged u32 like {@link fg}/{@link bg}
   * (`0` = Default → the underline follows the fg). Only cells drawing a coloured underline
   * carry a non-zero value — `justerm-wasm-decode`'s `underlineColor` getter. **Optional**: the
   * wasm class always provides it, but a hand-built frame (a test fixture, or a consumer that
   * predates it) may omit it — the renderer treats an absent column as all-Default.
   */
  readonly underlineColor?: ArrayLike<number>;
  /** Per-cell `CellFlags` bits. */
  readonly flags: ArrayLike<number>;
  /**
   * Per-cell 1-based grapheme-cluster index (`0` = none → `sideTable[extra-1]`).
   *
   * `ArrayLike<number>` like every column here, so a plain-object fixture satisfies it — which
   * means **this declaration cannot pin the column's width**, and a coercion narrowing it on the
   * way to the renderer is invisible to the type system. The decoder returns a `Uint32Array`
   * (widened at #621: a `u16` cannot number one cluster per cell of a viewport the header's own
   * `cols`/`rows` permit); the width that has to agree is the one on `RendererBackend`'s
   * `apply_damage`, and #627 is what happens when it does not.
   */
  readonly extra: ArrayLike<number>;
  /** Span directory, stride 5: `[line, left, right, cell_offset, count]`. */
  readonly spans: ArrayLike<number>;
  /**
   * Live-selection overlay (#108, wire v6): viewport `(row, left, right)`
   * triples, both columns inclusive — `justerm-wasm-decode`'s `selectionSpans`
   * getter. Positions only (the blend colour is web policy, #115). Optional —
   * a frame with no selection omits it (treated as empty).
   */
  readonly selectionSpans?: ArrayLike<number>;
  /**
   * Search-match overlay (#108): same viewport `(row, left, right)` stride-3
   * layout as {@link selectionSpans}, a separate wire group —
   * `justerm-wasm-decode`'s `matchSpans` getter. Set on the backend via
   * `Engine::set_search_highlights`; consumed by search (#110). Optional.
   */
  readonly matchSpans?: ArrayLike<number>;
  /**
   * The *active* (current) search match's spans (#428, wire v12): same viewport
   * `(row, left, right)` stride-3 layout as {@link matchSpans}, a separate wire
   * group — `justerm-wasm-decode`'s `activeMatchSpans` getter. Designated on the
   * backend via `Engine::set_active_search_highlight` (which match is active is
   * consumer policy — next/prev navigation); the member is *also* present in
   * {@link matchSpans}, and the renderer's highlight ranking resolves the
   * overlap (#424). Optional — omitted when nothing is designated.
   */
  readonly activeMatchSpans?: ArrayLike<number>;
  /**
   * Decoration/command markers visible in this viewport (#118/#159, wire v10):
   * stride-5 `(id, row, kind, exitPresent, exitBits)` — `justerm-wasm-decode`'s
   * `markerPositions` getter. `kind`: 0 = Plain (#118 decoration), 1 = PromptStart,
   * 2 = CommandStart, 3 = OutputStart, 4 = CommandFinished (OSC 133). For a finished
   * command, `exitPresent` is 1 and `exitBits` is the exit code as a raw u32 —
   * reinterpret as signed with `exitBits | 0`. Off-screen markers are absent (still
   * alive; disposal comes via a `MarkerDisposed` event). Optional — a frame with no
   * markers omits it. Consumed by decorations (#120) + prompt-nav a11y (#160).
   */
  readonly markerPositions?: ArrayLike<number>;
  /** Grapheme clusters referenced by cells' `extra` index (frame-local). */
  readonly sideTable: readonly string[];
  /**
   * Per-cell OSC 8 hyperlink index (wire v2), span order: `0` = none, else
   * `linkTable[link - 1]` is the URI. Both halves of a wide glyph carry it.
   * `justerm-wasm-decode`'s `link` getter. Optional — a frame with no links omits it.
   */
  readonly link?: ArrayLike<number>;
  /** OSC 8 URIs referenced by cells' `link` index (frame-local) — the decoder's
   * `linkTable` getter. */
  readonly linkTable?: readonly string[];
  /**
   * Cursor state (screen coords, 0-based). `cursorShape`: 0 = Block, 1 =
   * Underline, 2 = Bar. `cursorBlink` is the *mode* — the blink timing is a
   * web-side policy. Optional — a frame may omit them (treated as no cursor).
   */
  readonly cursorRow?: number;
  readonly cursorCol?: number;
  readonly cursorVisible?: boolean;
  readonly cursorShape?: number;
  readonly cursorBlink?: boolean;
  /**
   * Viewport scroll position (#112, wire v5): `displayOffset` lines scrolled up
   * from the bottom (0 = following), `scrollbackLen` history lines. The scrollbar
   * sizes its thumb from these. Optional — a frame may omit them (no scrollback).
   */
  readonly displayOffset?: number;
  readonly scrollbackLen?: number;
  /**
   * Marker-index basis (#490, wire v15) — `justerm-wasm-decode`'s `evictedTotal` /
   * `markerEpoch` getters. Together they let a consumer ask the backend **once** for
   * every live marker's absolute line and keep that answer, instead of the frame
   * carrying all of them every time (measured at 37–70% of an 80×24 frame at ordinary
   * OSC-133 densities).
   *
   * `evictedTotal` counts lines evicted from the front of scrollback, so a held line
   * rebases as `line - (frame.evictedTotal - basisWhenPulled)`. `markerEpoch` moves when
   * that arithmetic cannot repair the index (a reflow, a region scroll that moved a
   * marker, an alt switch, RIS) — re-ask then, and **at most once per frame**: a marker
   * below a bottom margin bumps every output line, and the once-per-frame cap is what
   * keeps that case no worse than the payload this replaces.
   *
   * `evictedTotal` is a `number` carrying a u64: JS is exact to 2^53, four orders of
   * magnitude past any reachable eviction count.
   */
  readonly evictedTotal?: number;
  readonly markerEpoch?: number;
  /**
   * How many markers are live in the active buffer (#490, wire v16) —
   * `justerm-wasm-decode`'s `markerCount` getter.
   *
   * The drift check on a pulled index: compare it against the index's size, and a
   * mismatch means re-pull. It exists for the consumer that wired the pull but not the
   * create/dispose events, which would otherwise drift silently. It cannot see a create
   * and a dispose inside one frame, so it is a net under the events rather than a
   * replacement for them.
   */
  readonly markerCount?: number;
  /**
   * Whether the alternate screen (`?1049`/`?47`) is active (#149, wire v9) —
   * `justerm-wasm-decode`'s `altScreen` getter. The a11y announce policy (#119)
   * suppresses output reads when set (a TUI repaint isn't new output). Optional —
   * a frame may omit it (treated as the primary screen).
   */
  readonly altScreen?: boolean;
  /**
   * Mouse wanted-events mask (#129, wire from #135) — `justerm-wasm-decode`'s
   * `mouseWantedEvents` getter. Which event categories the active tracking mode
   * reports (bit 0 DOWN, 1 UP, 2 WHEEL, 3 DRAG, 4 MOVE; `0` = no reporting), the
   * {@link import("./input").MouseEvents} bitflags. The widget routes a mouse/wheel
   * event to the app when its bit is set, else keeps it local (selection /
   * scrollback) — S16 (#133) reads the WHEEL bit for wheel routing. Encoding the
   * report bytes stays the backend's (`encode_mouse`); only this routing mask
   * crosses. Optional — a frame may omit it (treated as `0`, no reporting).
   */
  readonly mouseWantedEvents?: number;
  /**
   * Scroll op (applied before spans): rows `[scrollTop, scrollBottom]` shifted by
   * `scrollCount` (positive = up). Optional — absent/`hasScroll: false` means no
   * shift. The cell mirror applies it; a span-only frame omits it.
   */
  readonly hasScroll?: boolean;
  readonly scrollTop?: number;
  readonly scrollBottom?: number;
  readonly scrollCount?: number;
}

import type { TermEvent } from "./events";

/** Unsubscribe handle returned by {@link FrameSource.subscribe}. */
export type Unsubscribe = () => void;

/**
 * A source of decoded frames, abstract over where they come from.
 *
 * Frame mode wires this to the consumer's IPC channel (decoding the backend's
 * wire frames); the future in-wasm mode wires it to an in-browser engine. The
 * renderer never knows which — it just subscribes.
 */
export interface FrameSource {
  subscribe(listener: (frame: DecodedFrame) => void): Unsubscribe;
  /**
   * Subscribe to fire-and-forget consumer events (#117) — title/bell/cwd from
   * core's `drain_events`, delivered OUT-OF-BAND (not on the frame wire). Frame
   * mode wires this to the backend's event side channel; the in-wasm mode drains
   * the engine. Optional — a source with no event channel omits it, and the widget
   * simply never fires the consumer's {@link import("./events").EventHandlers}.
   */
  subscribeEvents?(listener: (event: TermEvent) => void): Unsubscribe;
}

/**
 * Flag bit positions, from the decoder's `flags()`. Structural for testability —
 * a caller passes the wasm module's own constants, and tests pass literals.
 *
 * Lived in `render-core.ts` until #504 removed that module: the widget composites
 * nothing (the renderer does it in wasm since #273), so the only survivor of the
 * old per-cell decode is this bit map, which the a11y mirror and the renderer
 * adapter both read.
 *
 * **Derived from the published `Flags`, not written out (#831).** It was a hand-kept list of
 * names until it was measured against its source and found to be nine of eleven: `wide_char` and
 * `wrapline` had never been added, and nothing could say so — the seam gate one directory over
 * derives over `keyof DecodedFrame`, and these are module-scope exports, structurally outside it.
 * The list had already failed once the same way (`blink`, #576, see below).
 *
 * `keyof` is what removes the roster rather than checking it: a bit added upstream lands here at
 * the next pin bump with nobody having predicted it, exactly as {@link DecodedFrame}'s own gate
 * works one level down. **Testability is unaffected** — this is still a structural object of
 * numbers, so a test passes `{ bold: 1, … }` as before; what it can no longer do is pass a
 * *subset*.
 *
 * The width is deliberately widened to `number` rather than inherited. `Flags` declares each bit
 * as a `number` already, but restating it here keeps this type independent of how wasm-bindgen
 * chooses to render an integer field — the same reason {@link DecodedFrame} types every column
 * `ArrayLike<number>`.
 *
 * `free` is wasm-bindgen's resource plumbing, not a flag; `Symbol.dispose` drops out by taking
 * string keys only.
 *
 * Prior art for the shape: Ruffle's web wrapper imports its types straight from the generated
 * `.d.ts` (`import type { RuffleInstanceBuilder } from "../dist/ruffle_web"`) and hand-writes
 * none, and Automerge's JS package vendors the wasm output into its own `dist/` so no version
 * range exists to drift across. Neither carries a mirror of a published binding. What justerm has
 * that they do not is a *second producer* — a frame reaches this widget through
 * {@link FrameSource} from any consumer on any decoder version, which is why
 * {@link DecodedFrame} stays hand-written and width-agnostic. That reason does not extend here:
 * nothing but the decoder produces these constants.
 *
 * The instance that made the class visible: `blink` (SGR 5, #576) was the last cell flag the wasm
 * getter exposed and this mirror did not, so nothing on this side could name a blinking cell —
 * the renderer conceals such a cell on the off phase of the consumer's text-blink clock exactly
 * as it conceals `hidden` (`ESC[8m`), the two sharing `is_concealed`. It was added by hand. The
 * two after it were not, which is the whole argument for deriving.
 */
export type FlagBits = {
  [K in Exclude<Extract<keyof WasmFlags, string>, "free">]: number;
};

/**
 * How a cell's underline is drawn — the value of `SGR 4 : Ps` (#862).
 *
 * **A field, not a flag, which is why it is not in {@link FlagBits}.** That map answers eleven
 * yes-or-no questions with one bit each; this one is "which of six", and no mask can answer it. The
 * decoder made the same split for the same reason (#831), and forcing it into the bit map here
 * would leave a consumer shifting by hand — the thing both surfaces exist to prevent.
 *
 * `None` is a **member** of the style, not the absence of one: a cell that is not underlined reads
 * as `None`. `flags[i] & F.underline` and a non-`None` style are the same question asked twice —
 * the engine derives the flag from the field, so they cannot disagree on a word this decoder
 * produced.
 *
 * Read a cell's style with {@link import("./justerm-renderer").JustermRenderer.underlineStyle} and
 * name the values with its `underlineStyles`; a consumer never imports `justerm-wasm-decode` to do
 * either (#827 story 15).
 */
export type UnderlineStyle = import("justerm-wasm-decode").UnderlineStyle;

/**
 * The named underline-style values, exactly as the decoder freezes them (#862) — `styles.Curly`
 * rather than `3`.
 *
 * Written as a **type-level** module reference on purpose. `import type` cannot carry an enum's
 * value side, and a real `import { UnderlineStyle }` would make the decoder a *static* runtime
 * dependency of `src/` — which the widget deliberately avoids, loading it with `await import(...)`
 * to keep the wasm init off the module graph (see `JustermRenderer`'s class doc). `typeof
 * import(...)` in type position is erased at emit and adds no edge; `test/published-seam.types.ts`
 * uses the same form.
 *
 * Derived rather than restated, for the reason {@link FlagBits} carries: a value added upstream
 * arrives here at the next pin bump with nobody having predicted it, and there is no roster to go
 * stale.
 */
export type UnderlineStyles = (typeof import("justerm-wasm-decode"))["UnderlineStyle"];
