# ADR-0033: Subpixel text coverage is one light mask per glyph and a dark-ink curve measured per configuration

Status: **accepted** (2026-09-22), #961. Every choice below is a **derivation** from the measurements
it cites — a better measurement reopens it. The one **judgement** in the change is the maintainer's
and is not recorded here: that #961's slice carries the `justerm-web` option as well as the renderer's.

**The layer this governs**: what a glyph slot *holds* — its coverage channels — and how the composite
reads them. It sits beside ADR-0031, which governs where the bake *puts* a glyph (place, scale, refuse),
and follows ADR-0031 D1/D2: the references below are a catalogue of mechanisms, never a vote.

## Context

A slot held grayscale coverage only: white ink, coverage in alpha, drawn onto a transparent canvas. A
browser applies LCD (ClearType) antialiasing only to text drawn over an opaque background, so nothing
per-channel could ever reach the atlas, and a justerm pane read blurrier than an xterm.js WebGL pane
beside it (#961's crop measurement, Windows 11, Consolas, 125 %).

**The composite is already one fragment that owns both the background and the glyph** (ADR-0019), with
no GL blending. So per-channel coverage needs no dual-source blending here: `mix` takes a `vec3` weight.
What was open was what the slot should hold.

Measured on Windows 11, Chromium 149 (headless) and 153 (headed), with Consolas, Cascadia Mono and
Courier New at 14–26 px. Error is the mean absolute per-channel difference, 0–255, between a
reconstruction `mix(bg, fg, coverage)` and the same text drawn by the browser into an opaque canvas in
the real colours, over the pixels that differ from the background (one string at an integer origin,
white/black and black/white plus a few neutral and dark-on-light pairs, dpr 1 — a spike, not in the
tree; `demo/subpixel.html` is the in-tree measure, of total ink):

| Slot holds | light ink on dark | dark ink on light |
|---|---|---|
| grayscale coverage (before this record) | 14–30 | 21–43 |
| one light mask (white over black), used as-is | 0–4 | 21–27, max 87 |
| one dark mask (black over white), used as-is | — | 0–3 |
| one light mask, raised to a fitted exponent for dark ink | 0–4 | **1.9–3.5** |

The dark mask is not a separate shape: per channel it is the light mask through one monotone curve.
A 256-entry curve fitted on one face reproduced the others at 1.2–3.1, and a single power `l^g` at
1.9–3.5 (`g = 2.65` on this machine; the same spike). The curve is the platform's text gamma — a
ClearType setting — not a property of a face.

## Decision

### D1 — A slot carries the light mask in RGB and the grayscale coverage in A

A configuration that opts in bakes each text glyph twice: the transparent draw it always had (A), and
the same glyph white over opaque black (RGB). Slot count, texture format and eviction are unchanged; no
second texture exists. A colour emoji keeps its own RGB, and a builtin block element keeps its bitmap:
it is background-class ink (`glyph_class` asks `builtin::owns`), whose coverage the composite reads
from alpha only. Pure half: `justerm-renderer/src/lcd.rs`.

Rejected: **baking over the cell's background** (xterm.js with `allowTransparency: false`). It is exact,
but it keys an entry by foreground *and* background, and every background override ADR-0019 applies per
fragment — selection, active match, decoration, the block cursor — would need its own entry or fall
back. **One mask used as-is** (the alacritty shape) is off by up to 87 on light themes.

### D2 — Dark ink raises the mask to an exponent measured per configuration, channel by channel

At bake, a calibration string is drawn at both polarities and `lcd::fit_dark_gamma` fits the exponent.
The shader applies it **per channel**: a channel whose ink value is below 0.75 raises that channel of
the mask to the exponent, and a channel at or above it uses the mask as-is. The browser's own draw
behaves that way — pure green (`#00ff00`) on black takes the light curve on its green channel, which
a rule on the ink's luminance (0.72) would miss. Swept over 60 inks (neutral, primary, secondary and
tinted, six levels) on 7 backgrounds: per channel, mean error 1.45 and worst pair 6.3; by Rec. 709
luminance, 2.91 and 19.2 — worse than grayscale on saturated light inks. The per-channel error is flat
for thresholds between 0.65 and 0.8 and 0.75 sits inside that plateau, so the proof does not tell 0.65
from 0.75, on purpose; it does tell 0.95 and 0.25, and a luminance rule, apart. The exponent is
measured rather than fixed because it is a platform setting; a constant fitted on one machine would be
wrong on the next.

### D3 — Per-channel coverage applies only over an opaque background

A fragment has one alpha. Where the default background is translucent (`setBgAlpha` < 1), three
coverages cannot be expressed, so that cell keeps grayscale and draws exactly the bytes it drew before.
xterm.js falls back to grayscale under `allowTransparency` for the same reason.

### D4 — The setting is a per-grid selector that keys the configuration

It changes what the atlas holds, so it is the seventh `ConfigKey` selector (ADR-0021 D1/D2), set by
`setSubpixelAntialiasing` or `addGrid`'s trailing argument. It moves no cell.

## Consequences

- **Off is bit-identical to before.** With the setting off, `u_lcd_gamma` is 0 and the shader reads the
  alpha on all three channels. Measured against `master`'s build when this landed: 14 scenes (light,
  dark, grey and coloured ink, bold, the default background, a translucent one; 16 and 28 px, with
  glyphs that spill past their cell) at 4 densities, 56 of 56 byte-identical. `demo/subpixel.html`
  keeps the in-tree half — off, on, then off again draws the first bytes.
- **The browser decides where fringes appear, and often declines.** Chromium drew the default
  `monospace` aliased at 24 device px and below (LCD text from 26) and drew no LCD text at 49 device
  px and above. There
  the three channels come back equal — but dark ink still takes the exponent, and was measured closer to
  the browser's own draw than grayscale is (−0.2 % against +15 % total ink at dpr 2).
- **A consumer whose default background is transparent gets nothing from it** until that background is
  opaque. That is D3, not a gap in the renderer.
- **Cost**: two extra canvas draws and one readback per baked glyph, and one calibration per
  configuration bake — drawn at no more than 64 device px and fitted over light levels rather than
  pixels, so it does not grow with the font. Measured (dev build, headless Chromium) for a new
  configuration's whole bake: 16.5 ms against 9.1 ms at 16 CSS px, dpr 1.25; about 1.8–2.5× across
  12–200 px. No texture memory.
- **Ink that leaves its cell keeps the coverage it had inside it.** The bleed band (ADR-0019 R1.2) is
  read by the receiving cell from the owner's slot, so that read takes the owner's mask and the owner's
  ink colour too; a background-class owner stays scalar, since its RGB is not a mask.
- **Falsifier**: a platform where the dark mask is not one curve of the light mask — a fitted exponent
  whose error exceeds grayscale's. That reopens D1 toward a second mask per slot.
