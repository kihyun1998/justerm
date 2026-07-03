// Minimum-contrast-ratio colour adjustment (#115), ported faithfully from xterm's
// common/Color.ts (rgb.relativeLuminance / rgba.ensureContrastRatio). justerm has
// no alpha channel, so these operate on packed 0xRRGGBB rather than 0xRRGGBBAA.

/** WCAG relative luminance of the three sRGB channels (0x00..0xFF each). */
function relativeLuminance2(r: number, g: number, b: number): number {
  const rs = r / 255;
  const gs = g / 255;
  const bs = b / 255;
  const rr = rs <= 0.03928 ? rs / 12.92 : Math.pow((rs + 0.055) / 1.055, 2.4);
  const rg = gs <= 0.03928 ? gs / 12.92 : Math.pow((gs + 0.055) / 1.055, 2.4);
  const rb = bs <= 0.03928 ? bs / 12.92 : Math.pow((bs + 0.055) / 1.055, 2.4);
  return rr * 0.2126 + rg * 0.7152 + rb * 0.0722;
}

/** WCAG relative luminance of a packed `0xRRGGBB` colour. */
export function relativeLuminance(rgb: number): number {
  return relativeLuminance2((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
}

/** WCAG contrast ratio between two relative luminances (order-independent). */
export function contrastRatio(l1: number, l2: number): number {
  return l1 < l2 ? (l2 + 0.05) / (l1 + 0.05) : (l1 + 0.05) / (l2 + 0.05);
}

/**
 * Adjust `fg` (packed `0xRRGGBB`) so it meets `ratio` contrast against `bg`,
 * returning the adjusted colour, or `undefined` if the pair already meets it.
 * `undefined` lets the caller keep the original fg (and its alpha/dim). Faithful
 * to xterm: try the luminance direction away from `bg` first, fall back to the
 * other, and keep whichever reaches a higher ratio if neither fully meets it.
 */
export function ensureContrastRatio(bg: number, fg: number, ratio: number): number | undefined {
  const bgL = relativeLuminance(bg);
  const fgL = relativeLuminance(fg);
  const cr = contrastRatio(bgL, fgL);
  if (cr >= ratio) return undefined;
  // Move away from the background's luminance first; if that can't reach the
  // ratio, try the other direction and keep whichever got closer.
  const [first, second] = fgL < bgL ? [reduceLuminance, increaseLuminance] : [increaseLuminance, reduceLuminance];
  const a = first(bg, fg, ratio);
  const aR = contrastRatio(bgL, relativeLuminance(a));
  if (aR < ratio) {
    const b = second(bg, fg, ratio);
    const bR = contrastRatio(bgL, relativeLuminance(b));
    return aR > bR ? a : b;
  }
  return a;
}

/** Darken `fg` in 10% steps until it meets `ratio` against `bg` (or hits black). */
function reduceLuminance(bg: number, fg: number, ratio: number): number {
  const bgL = relativeLuminance(bg);
  let r = (fg >> 16) & 0xff;
  let g = (fg >> 8) & 0xff;
  let b = fg & 0xff;
  let cr = contrastRatio(relativeLuminance2(r, g, b), bgL);
  while (cr < ratio && (r > 0 || g > 0 || b > 0)) {
    r -= Math.max(0, Math.ceil(r * 0.1));
    g -= Math.max(0, Math.ceil(g * 0.1));
    b -= Math.max(0, Math.ceil(b * 0.1));
    cr = contrastRatio(relativeLuminance2(r, g, b), bgL);
  }
  return (r << 16) | (g << 8) | b;
}

/** Lighten `fg` in 10% steps until it meets `ratio` against `bg` (or hits white). */
function increaseLuminance(bg: number, fg: number, ratio: number): number {
  const bgL = relativeLuminance(bg);
  let r = (fg >> 16) & 0xff;
  let g = (fg >> 8) & 0xff;
  let b = fg & 0xff;
  let cr = contrastRatio(relativeLuminance2(r, g, b), bgL);
  while (cr < ratio && (r < 0xff || g < 0xff || b < 0xff)) {
    r = Math.min(0xff, r + Math.ceil((255 - r) * 0.1));
    g = Math.min(0xff, g + Math.ceil((255 - g) * 0.1));
    b = Math.min(0xff, b + Math.ceil((255 - b) * 0.1));
    cr = contrastRatio(relativeLuminance2(r, g, b), bgL);
  }
  return (r << 16) | (g << 8) | b;
}
