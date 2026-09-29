import type { TextareaAnchor } from "./composition";

/** The colour a suggestion draws in. `"default"` and `indexed` follow a theme change. */
export type SuggestionColor = "default" | { readonly indexed: number } | { readonly rgb: number };

/** How {@link Terminal.setSuggestion} draws its text. */
export interface SuggestionOptions {
  /** Default `"default"`: the theme's foreground. */
  readonly color?: SuggestionColor;
  /** Draw with the SGR 2 (dim) treatment. Default `true`. */
  readonly dim?: boolean;
}

/** `color` as a frame's tagged colour reference: high byte `0` Default, `1` Indexed, `2` Rgb. */
export function suggestionColorRef(color: SuggestionColor | undefined): number {
  if (color === undefined || color === "default") return 0;
  if ("indexed" in color) return (1 << 24) | (color.indexed & 0xff);
  return ((2 << 24) | (color.rgb & 0xffffff)) >>> 0;
}

/**
 * The viewport cell a suggestion is drawn from — the engine cursor's cell mapped through the
 * display offset — or `undefined` when nothing should be drawn: no text, no cursor yet, the cursor's
 * row off screen, or an IME composition open ([justerm#972](https://github.com/kihyun1998/justerm/issues/972)).
 */
export function suggestionCell(
  cursor: TextareaAnchor | undefined,
  displayOffset: number,
  rows: number,
  text: string,
  composing: boolean,
): TextareaAnchor | undefined {
  if (!cursor || text === "" || composing) return undefined;
  const row = cursor.row + displayOffset;
  return row < rows ? { col: cursor.col, row } : undefined;
}
