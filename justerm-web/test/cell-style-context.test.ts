import { describe, expect, it } from "vitest";
import { cellStyleContext } from "../src/justerm-renderer";

/**
 * #862 — the widget carries the decoder's underline-style accessor and its named values so a
 * consumer never imports `justerm-wasm-decode` for them (#827 story 15).
 *
 * **This asserts reference identity, not shape, and that is the whole point.** The wiring it covers
 * is the one thing on this seam `pnpm typecheck` cannot see: `decoder.wireVersion` in the accessor's
 * place typechecks clean and ships a widget that answers `16` for every cell. TypeScript allows it
 * twice over — a function taking fewer parameters satisfies one taking more, and a numeric enum
 * accepts a plain `number` — so no signature written here would redden on it. Comparing the
 * *identity* of what comes out against what went in does.
 *
 * Measured before this file existed: that mutation was green through `typecheck`, `test` and
 * `build`, and only a hand-driven browser probe caught it.
 */
describe("cellStyleContext — the decoder members the widget carries (#862)", () => {
  /** A decoder stand-in whose members are distinguishable by identity, which is what is asserted. */
  const underlineStyle = (flags: number) => ((flags >> 11) & 7) as never;
  const UnderlineStyle = { None: 0, Single: 1, Double: 2, Curly: 3, Dotted: 4, Dashed: 5 };
  /** A plausible wrong neighbour: same module, different member, and it typechecks in that slot. */
  const wireVersion = () => 16;

  it("hands back the accessor it was given, not a neighbour", () => {
    const ctx = cellStyleContext({ underlineStyle, UnderlineStyle });
    expect(ctx.underlineStyleOf).toBe(underlineStyle);
    expect(ctx.underlineStyleOf).not.toBe(wireVersion);
  });

  it("hands back the value map it was given", () => {
    const ctx = cellStyleContext({ underlineStyle, UnderlineStyle });
    expect(ctx.styleValues).toBe(UnderlineStyle);
  });

  it("does not wrap, copy or normalise either one", () => {
    // A wrapper would satisfy the identity check above only if it were the same object, so this
    // states the consequence rather than re-testing it: what the widget hands a consumer is the
    // decoder's own function, so its totality (an unknown 3-bit value reads as `Single`) is the
    // decoder's, not a second normalisation invented on this side.
    const ctx = cellStyleContext({ underlineStyle, UnderlineStyle });
    expect(ctx.underlineStyleOf(3 << 11)).toBe(3);
    expect(Object.is(ctx.styleValues, UnderlineStyle)).toBe(true);
  });
});
