# Invariant — an absolute walk of `[scrollback ++ grid]` floors at `scrollback.len()` on the alt screen

## The fact

Every reader that walks the single `[scrollback ++ grid]` buffer by absolute line index must, on the
alt screen, floor at `scrollback.len()` — never descend below it and never soft-wrap-join across it —
and must also bound itself above at `scrollback.len() + grid.rows()`. The one spelling of the floor is
`justerm-core/src/term/walk.rs — Term::abs_floor` (`if on_alt { scrollback.len() } else { 0 }`;
re-verified at HEAD 7bb31f7). `walk.rs`'s module doc is the site-local statement and points here for
the site list.

The obligation is two-sided. The floor is the alt-screen half; the ceiling holds on both screens
(#660: `selection_range` read `abs_line` before its own visibility filter and indexed past the grid).
The shared root of both is that `[scrollback ++ grid]` is addressed by a bare `usize` carrying no range,
so every walk re-derives its own bounds and each can forget a different end.

The floor is a **bound, not an identity**. It protects a walk that starts from a row it already knows
is on this screen. It cannot rescue a stored coordinate: the primary grid and the alt grid occupy the
same absolute indices `[scrollback.len(), scrollback.len() + rows)`, so a stored point needs the
buffer it names carried with it (#691, #967).

## Why it is cross-cutting

One storage layout, two logical spaces. On the alt screen `scrollback` holds the *primary* buffer's
history, unrelated to what is on screen. xterm gets the isolation free because its `Buffer` objects are
physically separate; justerm must reproduce it at every walk site (old note, reading of xterm, 2026-07).

The sites below do not call each other — search, logical-line assembly, selection stepping, paint and
the write path share only the storage assumption. Most of them now call the shared `abs_floor`, but
that is a spelling, not a funnel: a new walk whose author does not know the rule will not call it
either (all three historical misses predate the helper), and four sites hold the rule without calling
it. So the invariant is not folded into one primitive.

## Territories it holds in

Derivable half — ask, do not store:

```sh
rg 'abs_floor\(\)' justerm-core/src/ | rg -v 'fn abs_floor'
```

At HEAD 7bb31f7 that names seven call sites, in these territories:

- [selection](../territory/selection.md) — `term/walk.rs — Term::prev_pos` / `Term::next_pos`, the
  logical-line step (#207); `term/selection.rs — Term::select_all`, which selects from the floor.
- [search](../territory/search.md) — `term/search.rs — Term::search_with` (#144).
- [logical lines](../territory/logical-lines.md) — `term/logical.rs — Term::viewport_logical_lines`
  (#113).
- [settled-cell paint](../territory/settled-cell-paint.md) — `term/paint.rs — Term::changed_logical_lines`
  (#967).
- [accessibility](../territory/accessibility.md) — `term/selection.rs — Term::accessible_text`, which
  lives beside selection because it reuses `extract_lines`, not because it is a selection.
- [tracked points](../territory/tracked-points.md) — `term/tracked.rs — Term::tracked_point`. The floor
  alone was wrong here twice (#691): first one floor for every point, then a per-buffer floor, neither
  of which separates a primary grid row from an alt row at the same index. The fix scopes the read to
  the active buffer (`Term::tracked`, `alt_tracked` / `normal_tracked`), answers `None` for a point of
  the inactive one, and only then is `abs_floor` correct.

Non-derivable half — hand-maintained on purpose; no grep for `abs_floor` finds these, and each is the
kind most likely to be "fixed" into a redundant call:

- [soft wrap](../territory/soft-wrap.md) — the write path holds the floor by an explicit
  `!self.on_alt` guard rather than `abs_floor`: `term.rs — Term::shift_region` clears the last
  scrollback row's wrap at `top == 0` only on the primary, and `Term::void_wrap_artefact_above` reaches
  into `scrollback.back_mut()` only on the primary.
- [settled-cell paint](../territory/settled-cell-paint.md) — `term/paint.rs — Term::paint_logical_line`
  receives a coordinate the consumer held across IPC, so it carries the identity (`LineRef::alt`) and
  floors by the buffer that names (`scrollback.len()` for alt, `0` for primary), not by `abs_floor`,
  since a primary paint may land while the alt screen is up (#967).
- [viewport](../territory/viewport.md) — `term/viewport.rs — Term::viewport_link_at` indexes
  `scrollback.len() - display_offset + row`, at or above the floor by construction because
  `display_offset` is pinned to 0 on alt (`term.rs — Term::switch_to_alt` zeroes it,
  `term/viewport.rs — Term::set_display_offset` returns early while `on_alt`). Valid only while alt
  scrollback cannot be viewed.
- [alt screen](../territory/alt-screen.md) — owns `on_alt` and the swap (`switch_to_alt` /
  `switch_to_primary`) whose layout makes the floor necessary.
- [grid & scrollback](../territory/grid-and-scrollback.md) — owns the concatenated
  `[scrollback ++ grid]` addressing (`term/walk.rs — Term::abs_line` / `Term::abs_row`) the walks use.

Deliberately unfloored — must **not** get a floor:

- [marker](../territory/marker.md) — `term/markers.rs — Term::doc_line_of` and `Term::command_start`
  walk absolutely with no floor. `Term::command_lines` threads `primary_grid()` into them and into
  `extract_lines`, so all three run in `[scrollback ++ primary]` even while alt is up, because OSC 133
  command marks are primary-only (#192). They reach cells through `row_in` / `line_in(grid, …)`, so
  the recurrence test below does not catch them. Valid only while command marks stay primary-scoped,
  and since #743 that condition guards a published contract: `Engine::command_lines` is discharged by
  re-ask under [ADR-0029](../../adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md)
  D3.2 precisely because this population's scope never flips. Pinned by
  `justerm-core/tests/command_lines_document.rs`.

## What a violation looks like

Inside an alt-screen application (vim, htop, less), text the user cannot see is pulled in: row 0 of
the alt grid soft-wrap-joins with the last primary scrollback row beneath it, so copy, search and word
selection return content that is not on screen. It reproduces only when the primary's last row carries
`WRAPLINE`, which is why it was found three times instead of once. The ceiling half (#660) is a panic
on either screen when a resolved range runs past the last row. The stored-coordinate half (#691) is a
point registered on the primary that reads back as an alt row during an excursion.

## Discovery history

| Occurrence | Site | Evidence |
|---|---|---|
| 1st | `viewport_logical_lines` | #113 |
| 2nd | `search` | #144 |
| 3rd | `prev_pos` (word selection) | #207 |
| helper | `abs_floor` extracted after the third; #585 folded the last open-coded copies into calls | #585 |
| construction | `viewport_link_at` found to hold by construction | #601 |
| ceiling | `selection_range` past the last row | #660 |
| identity | `tracked_point` — floor is a bound, not an identity | #691 |
| identity, first time | `paint_logical_line` carries `LineRef::alt` from the start | #967 |
| contract | `command_lines` re-ask discharge depends on primary-only scope | #743, ADR-0029 D3.2 |

The three sites of #113 / #144 / #207 have no call relationship, so fixing one offered no path to the
others. A helper prevents nothing for a site whose author has never heard of it; reading this note
when adding a walk does.

Corrected at HEAD 7bb31f7: the old note said `end_wrap` and `shift_region` hold the floor "by argument
in comments"; that comment is gone, `end_wrap` no longer touches scrollback, and the write-path sites
are `shift_region` and `void_wrap_artefact_above`, by an explicit `!self.on_alt` guard.
`tracked_point` now lives in `term/tracked.rs`, not search/marker. `select_all` is a seventh call site
the old list did not have.

## Where it will recur

Any new reader that walks the buffer by absolute index. Test: if a function uses
`scrollback.len() + grid.rows()` as its total, or reaches cells through `abs_line` / `abs_row`, it
needs `abs_floor` **and** a bound against `scrollback.len() + grid.rows()`.
`term/search.rs — Term::match_spans` is the site to copy: it bounds before it reads.

If a function stores an absolute line and reads it later, a floor is not enough: carry which buffer it
names, as `LineRef::alt` does.

If a function walks a grid it was handed (`row_in` / `line_in`), ask which buffer that grid is; the
`command_lines` walks are correct without a floor and break if one is added.

Do not grep for `abs_floor` and conclude you are done: it finds the walks that have a floor. The next
miss is found by grepping raw `scrollback.len()` arithmetic.
