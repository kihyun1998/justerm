# ADR-0023: A spacing setting is CSS pixels, because the font description it belongs to is

Status: **accepted** (2026-07-22; proposed 2026-07-21). Records an implemented decision (#338) that
diverges from both references on a public setter's **unit**. The authority it establishes — our own API's
internal coherence for a consumer-facing setting's unit — is carried in the bindings' tie-breaker table,
so a future metric setter routes by it without reopening this. Scoped to consumer-facing metric settings on
`justerm-renderer`; the cell they modify is ADR-0022, the tier they live in is ADR-0021.

## Context

`setLetterSpacing` widens the grid cell without changing the glyph. The only question this ADR settles
is **what unit its argument is in** — CSS pixels (which the renderer then scales by the device pixel
ratio) or device pixels (which the consumer must scale itself).

justerm takes **CSS pixels** and applies `round(letter_spacing * dpr)` device px to the cell
(`webgl.rs::set_letter_spacing`). Both references take **device pixels**.

### Both references split the units of one font description

- **xterm.js** — `fontSize` is CSS pixels (the canvas font is set at `fontSize * dpr`), but
  `letterSpacing` is added to an already-device-px char width:
  `dimensions.device.cell.width = dimensions.device.char.width + Math.round(letterSpacing)`
  (`addons/addon-webgl/src/WebglRenderer.ts:671`; `DomRenderer.ts:140` agrees). So one font description
  speaks two units.
- **alacritty** — the font size is scaled first (`alacritty/src/display/mod.rs:411-414`:
  `let scale_factor = window.scale_factor as f32; let font_size = config.font.size().scale(scale_factor);`),
  so `crossfont`'s metrics come back in device px. `font.offset` is then added to them **raw**
  (`:1608-1615`, `compute_cell_size`: `(metrics.average_advance + offset_x).floor()`), never scaled.
- **And alacritty is inconsistent with itself about it.** `window.padding` *is* scaled —
  `config/window.rs:123-127`: `(f32::from(self.padding.x) * scale_factor).floor()`. Two user-facing
  pixel settings in the same config file, one scaled and one not, with nothing saying so.

### What that costs, concretely

Under the reference behaviour, `letterSpacing: 2` is a **2-CSS-px** gap on a dpr-1 display and a
**1-CSS-px** gap on a Retina one. The same setting renders differently depending on the monitor, and
*moving a window between displays changes the text layout* — while the font size, being logical, does
not move. The user set one thing; half of it is density-dependent.

## Decision

**A consumer-facing spacing setting is in CSS pixels, and the renderer scales it.** `letter_spacing` is
CSS px; the cell adds `round(letter_spacing * dpr)` device px.

The rule generalises past this one setter: **a setting expressed in the same space as an existing
logical setting must use that space.** `font_size` is CSS px, so everything that modifies the box that
font draws into is CSS px. A future metric setter picks its unit by this rule, not by what the
references do.

`line_height` is exempt by construction — it is a **multiplier**, so it has no unit and the question
does not arise. That is also why the divergence is confined to `letter_spacing` today.

### Why the divergence is worth the maintenance

Reference parity is not free, and this ADR spends it because the alternative makes the API incoherent
rather than merely different: the consumer would set `font_size` in one space and `letter_spacing` in
another, on the same object, with the mismatch invisible until it moved a window. ADR-0019 already
demoted xterm from validator to design input for cell composition; this is the same posture one layer
down — the references are consulted for *what problems exist*, and here both exhibit the problem rather
than solving it.

### The payoff is at the DPR change

Because spacing is stored logically, a density change re-derives the cell correctly for free:
`rebake_atlas` re-measures the glyph box at the new device size and re-applies the same CSS-px spacing
(#322 + #338 + #359). A gap keeps its apparent size when the window moves to another monitor. A
device-px setting would need the consumer to notice the DPR change and re-scale, which is exactly the
kind of derived work ADR-0017 keeps out of consumers.

## Consequences

- **A consumer porting from xterm must convert.** `letterSpacing: 2` in xterm is 2 *device* px; passing
  `2` here is 2 *CSS* px, i.e. 4 device px at dpr 2. This is the one migration note the divergence
  creates, and it belongs in the setter's doc-comment — where it already is.
- **The renderer owns the scaling, so the consumer cannot get it wrong.** There is no path where a
  consumer scales spacing itself; the setter takes logical px, full stop.
- **The rule is what carries forward, not the instance.** If a future setter takes a distance (padding,
  a cursor inset, a decoration border width), it is CSS px because `font_size` is — decided here, not
  re-argued there.
- **Clamping is unaffected.** `MAX_CELL_PX` (#339) bounds the *device* cell after scaling, so it keeps
  working regardless of which unit the input was in.

## Alternatives considered

- **(A) Take device pixels, as both references do.** Rejected. It buys parity on a value no test
  compares and no consumer copies verbatim, and it costs coherence within a single font description —
  the exact defect both references exhibit, and which alacritty's own scaled `window.padding` shows is
  not a considered position upstream but an inconsistency.
- **(B) Take CSS px but expose the device value too, so a consumer can choose.** Rejected: two ways to
  say one thing, and the second only exists to let a consumer reproduce the bug.
- **(C) Make it a multiplier, like `line_height`.** Rejected: spacing is an absolute gap — expressing it
  relative to the cell width means it changes when the font changes, which is not what the setting means.

## Out of scope

- **The cell the spacing modifies** — ADR-0022.
- **Whether the setting is per-config or per-grid** — ADR-0021's tier rule answers it (`letter_spacing`
  keys the per-config tier).
- **`line_height`** — a multiplier; no unit question.
