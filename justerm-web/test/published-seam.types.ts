/**
 * The drift gate on the seam `justerm-web` consumes but does not control (#646).
 *
 * This package sits between two **published** packages and hand-writes each one's shape. The
 * renderer-facing mirror already has a gate — `JustermRenderer.create` binds the real class to
 * {@link import("../src/justerm-renderer").RendererBackend} by a typed *declaration*, so a
 * signature drift there is a compile error (it fired on its first real test, #645). The decoder
 * side had none, which is the gap #627 lived in: the decoder widened `extra` to `Uint32Array`
 * while this package went on narrowing it back to 16 bits — truncating silently above
 * `u16::MAX` and copying the whole column every frame.
 *
 * **What runs this file:** `pnpm typecheck` (its second project, `tsconfig.test.json`), which CI
 * runs in the `web` job. Vitest does **not** — its `include` covers `*.test.ts` only, and there is
 * nothing here to execute at runtime. A red here is a *type* error, not a failing test.
 *
 * **What this file asserts, and why it is not the obvious thing.** It does not compare the
 * decoder against `src/types.ts`'s column *widths*, because those are deliberately
 * `ArrayLike<number>`: a frame reaches this package through `FrameSource.push` from a consumer
 * that may be on any decoder version, so being width-agnostic is the contract (see
 * `src/types.ts:1-15` and the note at `src/justerm-renderer.ts:781-789`). The fact worth gating
 * is one layer out, and it is a statement about the **family** rather than about this package:
 *
 *   > the renderer's parameters must be able to take what the decoder produces.
 *
 * `justerm-web` is simply the only place where both published types are in scope. Neither
 * assertion below routes through this package's own declarations, so nothing in `src/` gains a
 * dependency on the decoder's types.
 *
 * **When it fires.** At the moment a version range in `package.json` moves — and because the two
 * ranges are separate (`justerm-renderer` and `justerm-wasm-decode` are independent pins, bumped
 * as separate steps in #633), a half-bumped state where one has widened and the other has not is
 * unmergeable rather than merely unnoticed.
 */
import type { DecodedFrame as WasmFrame } from "justerm-wasm-decode";
import type { JustermRenderer } from "justerm-renderer";
import type { DecodedFrame as WebFrame } from "../src/types";

/** `A` is usable where `B` is expected. Deliberately one-directional: the question at this seam
 * is whether the decoder's output *feeds* the renderer's input, which is assignability, not
 * identity. A strict `Equal` would also redden on a wasm-bindgen codegen change that moves the
 * `ArrayBufferLike`/`ArrayBuffer` type argument without moving any width — the same TS
 * TypedArray-generic friction `src/justerm-renderer.ts:465-466` already documents — and the
 * cheap fix under pressure is to delete the assertion. */
type Feeds<A, B> = [A] extends [B] ? true : false;
type Equal<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;
const holds = <T extends true>(_: T) => undefined;

// ---------------------------------------------------------------------------------------------
// 1. Roster — every decoder getter is mirrored by `src/types.ts`
// ---------------------------------------------------------------------------------------------

/** wasm-bindgen's resource plumbing, not part of the frame's shape. `[Symbol.dispose]` drops out
 * by taking string keys only; `free()` has to be named. */
type FrameFields<T> = Exclude<Extract<keyof T, string>, "free">;

/**
 * A decoder getter this package's mirror does not declare — the #129/#135 class, where
 * `mouseWantedEvents` existed in core and wasm and reached `types.ts` only at S16.
 *
 * This is **derived**, not a checked-in list of names: `keyof` enumerates whatever the published
 * decoder actually declares, so a getter added upstream lands here without anyone having
 * predicted it, and there is no roster to go stale. It is currently `never` with no exceptions —
 * all 31 getters are mirrored.
 *
 * The opposite direction is deliberately not asserted. A field on {@link WebFrame} with no
 * decoder getter behind it is legitimate: the frame shape is source-agnostic by contract, and a
 * future in-wasm engine may produce one.
 */
type NotMirrored = Exclude<FrameFields<WasmFrame>, keyof WebFrame>;
holds<Equal<NotMirrored, never>>(true);

// ---------------------------------------------------------------------------------------------
// 2. Widths — every column the renderer takes can carry what the decoder produces
// ---------------------------------------------------------------------------------------------

/**
 * Drop the grid handle every per-grid export has taken since renderer 0.15.0 (#773), so the indices
 * below stay in **wire order** — the order the decoder's columns are named in.
 *
 * This is not cosmetic. Shifting every index by one instead would have re-pointed each assertion at
 * its neighbour, and the neighbours are mostly `Uint32Array` too: when the grid parameter landed,
 * six of the twelve assertions below went red and the other six went on passing **against the wrong
 * column**. A gate that half-fires is worse than one that does not, because the half that fires is
 * taken as the whole signal.
 *
 * `never` is the deliberate failure mode: if a future leading parameter is not a `number`, every
 * assertion below breaks at once rather than sliding one position quietly.
 */
type WithoutGrid<T extends unknown[]> = T extends [grid: number, ...rest: infer R] ? R : never;

type ApplyDamage = WithoutGrid<Parameters<JustermRenderer["apply_damage"]>>;
type SetOverlay = WithoutGrid<Parameters<JustermRenderer["setOverlay"]>>;
type SetActiveMatch = WithoutGrid<Parameters<JustermRenderer["setActiveMatch"]>>;

// `apply_damage(grid, header, spans, codepoints, fg, bg, flags, extra, side_table, underline_colors?)`
// — indices below are AFTER `grid` is dropped, i.e. `header` is 0.
holds<Feeds<WasmFrame["spans"], ApplyDamage[1]>>(true);
holds<Feeds<WasmFrame["codepoints"], ApplyDamage[2]>>(true);
holds<Feeds<WasmFrame["fg"], ApplyDamage[3]>>(true);
holds<Feeds<WasmFrame["bg"], ApplyDamage[4]>>(true);
holds<Feeds<WasmFrame["flags"], ApplyDamage[5]>>(true);
/** #627 itself. This is the pair that disagreed for a release. */
holds<Feeds<WasmFrame["extra"], ApplyDamage[6]>>(true);
holds<Feeds<WasmFrame["sideTable"], ApplyDamage[7]>>(true);
holds<Feeds<WasmFrame["underlineColor"], NonNullable<ApplyDamage[8]>>>(true);

holds<Feeds<WasmFrame["selectionSpans"], SetOverlay[0]>>(true);
holds<Feeds<WasmFrame["matchSpans"], SetOverlay[1]>>(true);
holds<Feeds<WasmFrame["activeMatchSpans"], SetActiveMatch[0]>>(true);

// ---------------------------------------------------------------------------------------------
// 3. What this file cannot see — stated, rather than left to look covered
// ---------------------------------------------------------------------------------------------

/**
 * The decoder members with no renderer parameter to compare against. Section 2 pins a width by
 * finding a *consumer* that declares one; for these there is none — every path in this package
 * takes them as `ArrayLike<number>` (`src/links.ts:35,45`, `src/markers.ts:39`, `src/overlay.ts`),
 * which any integer width satisfies. Pinning them would mean writing an expected width into this
 * file by hand, which is a roster and would go stale exactly as section 1 avoids.
 *
 * This is not hypothetical. `link` widened from `Uint16Array` to `Uint32Array` at decoder 0.12.0
 * — the same release, and the same event, as the `extra` widening that became #627 — and it
 * reached this package at the #633 pin bump with nothing anywhere observing it. It is harmless
 * (no coercion narrows it, so no value is lost), but it is the class this section names.
 *
 * Declared as a type so that it too is derived: if a future renderer method starts taking one of
 * these columns, the member should move from here into section 2, and the compiler will not
 * remind anyone.
 */
export type UnpinnedByThisFile = Extract<
  FrameFields<WasmFrame>,
  "link" | "linkTable" | "markerPositions"
>;

/**
 * Two further classes no type-level check on this seam can reach at all, recorded so the file is
 * not read as proof of more than it is:
 *
 * - **Element semantics at an unchanged width** — 1-based vs 0-based, tagged vs raw, a stride
 *   change, or a viewport-relative index becoming absolute. The standing example is historical
 *   and none the weaker for it: `markerLines` (absolute buffer line) and `markerPositions`
 *   (viewport row) were two identically-typed `Uint32Array` groups whose only difference was
 *   meaning, and nothing on this seam could have told them apart. `markerLines` left the wire in
 *   v16 (#490) — note that its departure is itself the second class below, and this file did not
 *   see that either: it was named in the `Extract` above, where a member that stops existing
 *   silently resolves to nothing.
 * - **A getter whose wire group silently stopped being populated.** The `.d.ts` describes the
 *   shape wasm-bindgen generates, not what the encoder puts in it.
 *
 * Both need a value-level round trip through a real `decodeFrame` to observe, and nothing in this
 * package performs one — `decodeFrame` has zero call sites here, by the same design that makes
 * the frame source-agnostic.
 */
export type NotReachableByTypes = never;
