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
// #802: the containment assertion in section 3 needs both the widget and the surface it holds.
import type { JustermRenderer as JustermRendererWidget } from "../src/justerm-renderer";
import { JustermRenderer as JustermRendererClass } from "../src/justerm-renderer";
import type {
  SurfaceBackend as SurfaceBackendType,
  TerminalSurface as TerminalSurfaceType,
} from "../src/terminal-surface";

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
// 1b. Roster — the decoder's MODULE-scope exports, which §1 structurally cannot see (#831)
// ---------------------------------------------------------------------------------------------

/**
 * `keyof DecodedFrame` covers what hangs off the *frame*. The decoder also exports standalone
 * functions and classes — `flags()`, and since #831 `underlineStyle()` / `UnderlineStyle` — and
 * those are outside §1 by construction, not by oversight. That blind spot had already cost
 * something before anyone looked: `FlagBits`, this package's hand-kept copy of `flags()`, named
 * **nine of the decoder's eleven** bits, missing `wide_char` and `wrapline`, and nothing anywhere
 * could say so. (`blink` had gone the same way at #576 and was noticed only because a feature
 * needed it.)
 *
 * The mirror itself is no longer hand-kept — `src/types.ts` derives `FlagBits` from the published
 * `Flags` with `keyof`, so that particular roster is gone rather than guarded. What remains, and
 * what this asserts, is the level above it: **a decoder export nobody here has looked at yet.**
 *
 * This one is a named list, unlike §1, and the difference is worth stating because the file's own
 * thesis is derive-don't-restate. There is nothing to derive it *from*: "has a human considered
 * whether this export needs mirroring" is not a fact any type carries. So the list is the
 * assertion, and its value is the direction it fails in — a new export appears in
 * `UnreviewedDecoderExports` on its own, at the moment `package.json`'s version range moves, which
 * is exactly when it becomes reachable.
 *
 * **It has fired once, and the prediction is left here as a record rather than rewritten.**
 * This section was written in #831 saying `underlineStyle` would land here at the next pin
 * bump; at #862's bump it did, before a line of that ticket was written, naming both it and
 * `UnderlineStyle`. They are in the list below because they are now handled — carried by
 * `JustermRenderer` — not because the entry was added to silence the red.
 */
type ReviewedDecoderExports =
  // Called directly through `typeof import("justerm-wasm-decode")` — no mirror, none needed.
  | "decodeFrame"
  | "buildPalette"
  | "isValidRegex"
  | "wireVersion"
  // Mirrored: `DecodedFrame` by hand and deliberately (§1 guards it), `Flags` by derivation.
  | "flags"
  | "DecodedFrame"
  | "Flags"
  // Carried rather than mirrored (#862): `JustermRenderer` holds both and hands them out as
  // `underlineStyle()` / `underlineStyles`, so a consumer never imports the decoder for them.
  // They arrived here exactly as this section predicted — unreviewed, at the pin bump.
  | "underlineStyle"
  | "UnderlineStyle";

type UnreviewedDecoderExports = Exclude<
  Extract<keyof typeof import("justerm-wasm-decode"), string>,
  ReviewedDecoderExports
>;
holds<Equal<UnreviewedDecoderExports, never>>(true);

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

// ---------------------------------------------------------------------------------------------
// 3. Containment — the surface `JustermRenderer.create` composes never escapes it (#802)
// ---------------------------------------------------------------------------------------------

/**
 * **This assertion replaces a runtime guard, and that is the whole reason it exists.**
 *
 * `TerminalSurface.addGrid` used to take an `ownsExtent` flag and throw when a second tenant tried
 * to join a surface whose buffer one terminal sizes. #802 deleted it: the state it guarded is
 * unreachable, because the only surface any terminal auto-sizes is the one
 * {@link import("../src/justerm-renderer").JustermRenderer.create} composed, and that surface is a
 * private field with no accessor. Nobody can obtain it, so nobody can attach to it.
 *
 * That guarantee is **structural**, and a structural guarantee with nothing checking it is one
 * refactor away from silently going. The moment a `get surface()`, a `surface()` method or a public
 * field hands it back, `create`'s arrangement stops being exclusive and every consequence the class
 * derives from "I composed this surface" — it sizes the drawing buffer to its own grid (#331's
 * exactness), it presents synchronously, it disposes the surface — becomes wrong for the sibling
 * that just attached.
 *
 * So this reddens *at the moment someone exposes it*, which is when the deletion needs
 * reconsidering, rather than in a browser afterwards. It is a `typecheck` failure, not a test
 * failure — the same as everything else in this file.
 *
 * **Scope, stated because the sentence above is otherwise too strong.** `private` is a TypeScript
 * fact, erased at emit: the parameter property compiles to an ordinary own enumerable
 * `this.surface`, so a JS host, `Object.values`, or a `as any` cast still reaches it. What is
 * defended is that **no typed consumer** can, and that no future edit quietly makes it typed —
 * which is the failure a person actually walks into. A host casting past a documented-private field
 * has left the contract, and the guard this replaced would not have helped it either: that guard
 * lived on one surface's registry, and the same hazard is reachable with entirely public API by
 * opening a *second* surface on the same canvas, which it never covered.
 *
 * Written as a **derivation over `keyof`** rather than as a list of forbidden names, for the reason
 * §1 gives: a name nobody predicted still lands here.
 */
/**
 * Does `T` carry a surface **anywhere** — returned, awaited, contained, or handed to a callback?
 *
 * Every clause here is one the first version let through, and each was verified by probe rather
 * than reasoned about. The naive shape (`T extends TS ? true : T extends () => TS ? true : false`)
 * caught only a plain getter, method or field, which is exactly the set the first mutation pass
 * happened to try — a four-way green that proved the four shapes it tested and nothing else.
 *
 * `[T] extends [...]` throughout, **not** the naked `T`: a naked conditional distributes over a
 * union, so `TerminalSurface | undefined` resolves to `true | false` — `boolean` — and
 * `boolean extends true` is false. A field nulled on dispose is the most plausible way for one to
 * appear, so the distributive form missed the likeliest field shape.
 */
/** Is a surface one of `T`'s constituents? `Extract` rather than `extends`, so a union carrying one
 * — the shape a field nulled on dispose takes — is caught rather than resolving to `boolean`. */
type Mentions<T> = [Extract<T, TerminalSurfaceType<SurfaceBackendType>>] extends [never] ? false : true;

/**
 * Does `T` hand a surface back — directly, as a union member, awaited, or from a function?
 *
 * **What it does NOT catch, stated rather than left for the next person to discover.** A surface
 * reached through a container or a callback parameter — `TerminalSurface[]`, `{ s: TerminalSurface }`,
 * `(cb: (s: TerminalSurface) => void) => void` — passes. Those were measured, not assumed: a probe
 * ran each shape through this exact definition inside `tsconfig.test.json`. They are left uncovered
 * deliberately, because chasing them costs a recursive conditional nobody can read and none of them
 * is a plausible way for *one* surface to escape *this* class. The shapes that are plausible — a
 * getter, a method, a field, a nullable field, a promise, and **any of those as a static** — are
 * covered and each was verified red by mutation.
 */
type YieldsSurface<T> =
  Mentions<T> extends true ? true
  : [T] extends [Promise<infer U>] ? Mentions<U>
  : [T] extends [(...args: never[]) => infer R]
      ? Mentions<R> extends true ? true
        : [R] extends [Promise<infer RU>] ? Mentions<RU> : false
  : false;

/** Every member that hands back the surface. Must be `never`. */
type LeaksFrom<T> = { [K in keyof T]: YieldsSurface<T[K]> extends true ? K : never }[keyof T];

/**
 * **Instance side and static side, because `keyof` on an instance type excludes statics** — and the
 * static side is where the escape is most likely to appear. `create` is itself a static, and the
 * option #802 explicitly weighed and did not take ("let `create` expose the surface it composed")
 * would land as exactly that: a static returning the pair. Asserting only the instance side left
 * this assertion green for the one change it exists to catch.
 */
type SurfaceEscapes = LeaksFrom<JustermRendererWidget> | LeaksFrom<typeof JustermRendererClass>;

holds<Equal<SurfaceEscapes, never>>(true);
