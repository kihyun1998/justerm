# Territory — alt screen

## What it is

The alternate screen buffer: a second grid that a full-screen application (vim, htop, less) swaps in
with DECSET `?1049` (save the cursor, switch, clear), `?47` / `?1047` (switch and clear, no cursor
save) or `?1048` (cursor save/restore, no switch), and swaps out again on reset. It owns the `on_alt`
flag, the swap itself, the alt-screen cursor slot, the per-screen kitty keyboard state, and what every
other piece of engine state does at the swap and while the alt screen is up: no scrollback, no reflow
on resize, and a rule per holder of a buffer coordinate (markers, tracked points, selection, search,
paint, text readers).

## Governing decisions

- [ADR-0015 — marker primitive and overlay marker group](../../adr/0015-marker-primitive-and-overlay-marker-group.md)
  § "Why markers survive an alt-screen swap" — the per-kind table for a swap: the selection clears,
  search highlights invalidate, markers survive (primary markers dormant, re-appearing on return).
  Per-buffer alt markers (#186/#187) were added later and are disposed on leave.
- [ADR-0009 — O(1) scroll in a grid row ring](../../adr/0009-o1-scroll-in-grid-row-ring.md) — records
  the alt screen having no scrollback as an established invariant and keeps it out of the ring: an
  alt-screen scroll is a region scroll (swap), never a history push.
- [ADR-0029 — a published coordinate carries its instant or is re-asked](../../adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md)
  — scoped *out* of "which buffer a coordinate names", but its D3.2 amendment (#743) closes one
  which-buffer answer: `Engine::command_lines` stays primary-scoped on the alt screen, because
  answering nothing there would make absence ambiguous between *disposed* and *wrong screen*.
- Not governed: the swap mechanics, the no-reflow rule (#567) and the alt-screen resize of markers are
  decided in issues and tests, not records.

## Design model

From reading the source and tests at HEAD 7bb31f7.

- **The swap is a `std::mem::swap` of `grid` and `alt_grid`; there is no third grid.** `alt_grid`
  holds whichever screen is *not* shown. `Term::switch_to_alt` swaps, clears the newly active grid,
  swaps the kitty state, sets `on_alt`, zeroes `display_offset`, drops the selection, invalidates search
  highlights and marks full damage; `Term::switch_to_primary` mirrors it (no clear — the primary comes
  back as it was). Both are guarded, so a double enter or leave is a no-op. `enter_alt_screen`
  (`?1049h`) is `save_alt_cursor` then `switch_to_alt`; `leave_alt_screen` (`?1049l`) is
  `switch_to_primary` then `restore_alt_cursor`. `?47` and `?1047` are treated identically, switch
  only and clearing on enter, which is xterm.js's model (`justerm-core/tests/alt_screen.rs` header,
  #72); `?1049` = `?1048` + `?47`.
- **The cursor slot is `saved_cursor`, separate from DECSC's `decsc`.** `?1048h` and `?1049h` copy the
  whole `Cursor` (position, pen, deferred-wrap flag); the restore keeps the current DECTCEM visibility
  (#38/#72) and then settles a restored park that is no longer at the last column
  (`Term::settle_restored_wrap`, #848 — [cursor position](cursor-position.md)). A bare `?1048` on the
  primary uses the same slot.
- **The alt screen has no scrollback.** `Term::linefeed_inner` pushes history only on a top-anchored
  *primary* scroll; on the alt screen every scroll is a region scroll and the evicted row is gone.
  The concatenated `[scrollback ++ grid]` space therefore holds the **primary's** history under the
  alt grid, and every absolute walk floors at `scrollback.len()` while `on_alt` —
  [the alt floor](../invariant/alt-screen-buffer-floor.md). `Term::set_display_offset` returns early
  on alt, so the viewport cannot scroll into the primary's history.
- **The alt screen resizes but does not reflow (#567).** Both grids take the new dimensions on every
  resize (leaving alt used to restore an old-sized grid, a mismatch that could panic a damage-driven
  render), but only the primary re-splits content. The alt pane runs through `reflow_pane` with
  `ReflowDims { reflow: false, limit: 0 }`: rows are re-fit and `Row::resize` truncates through pairs,
  which is why a lead without its spacer is a legal alt state ([wide glyph](wide-glyph.md)). All three
  references agree with one flag on the same resize function (ghostty `alt.resize(.{ .reflow = false
  })`, alacritty `grid.resize(!is_alt, …)`, xterm.js `_hasScrollback`; cited in
  `tests/alt_no_reflow.rs`). Re-splitting was not decided: `8f09d58` reached for a helper that
  re-splits when the column count changes, and on a real htop recording across a live `SIGWINCH` it
  left debris in cells htop does not overwrite (htop repaints without erasing; vim hides it by
  clearing). Pinned by `a_real_htop_repaint_is_not_polluted_by_a_re_split`.
- **While on alt, a resize still reflows the inactive primary** with `saved_cursor` as its cursor,
  carrying the primary markers, primary tracked points and the primary change record. The alt half
  converts alt markers and alt tracked points to alt-local rows, re-fits, and re-bases them on the
  primary scrollback's new length; one whose row the shrink pushed off is disposed (markers announce
  `MarkerDisposed`, tracked points are released silently). Before that repair, the alt half's extras
  counted from the top of a discarded history and were re-based onto the primary scrollback —
  two regions added together (`tests/alt_resize_markers.rs`).
- **Holders of buffer coordinates, at the swap and while alt is up:**

  | holder | at enter / leave | while on alt |
  |---|---|---|
  | selection | cleared both ways | ordinary; rotates with region scrolls; **dropped by any geometry change** (#660 — a selection made on alt outlived a shrink and `selection_range` indexed past the grid) |
  | search highlights | invalidated both ways | invalidated by any shift, as on primary |
  | markers | primary list dormant; alt list (`alt_markers`, plain anchors only) disposed and announced on leave; epoch bumped when any marker exists | `add_marker` anchors an alt-scoped marker that rides the alt frame and rotates with alt scrolls (#187) |
  | OSC 133 command marks | — | ignored (`add_command_mark` returns early, #188); `command_lines` keeps answering from `[scrollback ++ primary]` |
  | tracked points | `alt_tracked` cleared on leave, unannounced; a point of the inactive buffer answers `None` (#691) | scoped to the active buffer |
  | paint change record | per buffer (`normal_watch` / `alt_watch`, #967) | a coordinate carries its buffer (`LineRef::alt`) |
  | kitty keyboard flags + stack | swapped per screen (`swap_kitty_keyboard`, #941) | independent of the primary's |

- **What does not swap.** Tab stops, scroll margins, DEC modes (mouse, bracketed paste, DECAWM…),
  charsets, the pen (a `?47` switch keeps the cursor and its pen), `decsc` and the title stacks are
  one copy shared by both screens (`architecture.md` § Hidden VT state: "tab stops + scroll margins
  are *not* per-screen").
- **Out-of-band and in-band clears on alt.** `Engine::clear` returns `false` and changes nothing on the
  alt screen (ghostty's answer, #936); `ED 3` drops the **primary's** history underneath, as xterm
  does, and every alt line only shifts (`Term::erase_history`). RIS rebuilds `Term`, which leaves the
  alt screen.
- **What a consumer sees.** The frame header carries `alt_screen` (wire v9, #149) and DECRQM reports
  `?47`/`?1047`/`?1049` from the one `on_alt` flag. `Term::accessible_text` shows only the alt buffer,
  and a wrapped alt line joins without primary bleed (`tests/accessible.rs`). PenTerm reads the flag
  to gate typed-command injection: `penterm 7dadc3c95 src-tauri/src/engine/native.rs — gate_of`,
  which builds a whole `engine.frame()` to read `alt_screen` because the engine has no getter for it.
- **Outside input** is only the four DECSET/DECRST numbers and RIS; nothing here takes a size.

## Code

- `justerm-core/src/term.rs` — `Term::switch_to_alt`, `Term::switch_to_primary`,
  `Term::enter_alt_screen`, `Term::leave_alt_screen`, `Term::save_alt_cursor`,
  `Term::restore_alt_cursor`, `Term::swap_kitty_keyboard`, `Term::settle_restored_wrap`; the fields
  `on_alt`, `alt_grid`, `saved_cursor`, `alt_markers`, `alt_tracked`, `alt_watch`,
  `kitty_flags_inactive`, `kitty_stack_inactive`; `Term::resize`'s alt branch with `reflow_pane` and
  `ReflowDims`; `Term::linefeed_inner` (no history on alt); `Term::erase_history`, `Term::clear`
- `justerm-core/src/term/dispatch.rs` — `Term::set_dec_private_mode`, the `47` / `1047` / `1048` /
  `1049` arms
- `justerm-core/src/term/replies.rs` — `Term::decrqm` (the alt-buffer modes)
- `justerm-core/src/term/walk.rs` — `Term::abs_floor`
- `justerm-core/src/term/viewport.rs` — `Term::set_display_offset` (no-op on alt)
- `justerm-core/src/term/markers.rs` — `Term::add_command_mark` (ignored on alt)
- `justerm-core/src/serialize.rs` — `Frame` (`alt_screen`)

Tests that pin it: `justerm-core/tests/alt_screen.rs`, `alt_no_reflow.rs`, `alt_markers.rs`,
`alt_resize_markers.rs`, `alt_selection_resize.rs`, and the alt cases in `accessible.rs`,
`kitty.rs`, `ed3.rs`, `clear.rs`, `tracked_points.rs`.

## Reference behaviour

- [A selection when the screen changes under it](../../agents/reference-facts.md#a-selection-when-the-screen-changes-under-it-660-verified-2026-07-31)
  — ghostty clears a selection on a screen swap exactly as justerm does; the three answers to a
  resize under a selection (#660)
- [The kitty flag stack per screen](../../agents/reference-facts.md#the-kitty-flag-stack-per-screen-and-legacy-alt-on-the-c0-named-keys-941-verified-2026-09-18)
  — alacritty keeps separate stacks swapped only when the screen actually changes (#941)
- [Dropping history](../../agents/reference-facts.md#dropping-history--ed-3-in-band-and-a-clear-command-out-of-band-936-verified-2026-09-18)
  — `ED 3` on alt drops the primary's history in xterm; a Clear command does nothing on alt in ghostty
- [Relating a coordinate to the document it names](../../agents/reference-facts.md#relating-a-coordinate-to-the-document-it-names--how-the-references-make-which-buffer-unaskable-743-verified-2026-08-07)
  — how the references make "which buffer" unaskable, where justerm's single concatenated space has
  to answer it
- The no-reflow rule's reference sites are cited in `justerm-core/tests/alt_no_reflow.rs`'s header,
  not in the reference-facts store.

## Cross-cutting invariants

- [alt-screen absolute-index floor](../invariant/alt-screen-buffer-floor.md) — this territory owns
  `on_alt` and the swap whose single `[scrollback ++ grid]` layout makes the floor necessary; every
  walk that floors reads the flag set here

## Blast radius

- [selection](selection.md) · [search](search.md) · [logical lines](logical-lines.md) ·
  [accessibility](accessibility.md) · [settled-cell paint](settled-cell-paint.md) — each walks the
  concatenated buffer and assumes `on_alt` means "the scrollback under the grid is not this screen's"
- [marker](marker.md) · [tracked points](tracked-points.md) — per-buffer storage selected by
  `on_alt`; they assume the swap is the only place the active list changes, and that leave disposes the
  alt population
- [reflow](reflow.md) — assumes the alt pane is re-fit, never re-split, and that the primary is
  reflowed through `saved_cursor` while alt is up
- [wide glyph](wide-glyph.md) — the alt resize leaves leads without spacers; the relocation bound
  `2 < cols` exists for that state
- [soft wrap](soft-wrap.md) — the write path's two reaches into scrollback are guarded by `on_alt`
- [input encoding](input-encoding.md) — the kitty flags it reads are per screen; it assumes the
  swap exchanges them
- [cursor position](cursor-position.md) — the alt slot restores a deferred wrap through the same
  settle as DECRC
- [viewport](viewport.md) — `display_offset` is pinned to 0 on alt; `viewport_link_at` stays above
  the floor only because of that
- [frame](frame.md) — the `alt_screen` header field is the consumer's only view of this state

## Known holes / open

- **No engine getter for the alt flag.** It reaches a consumer only in the frame header, so PenTerm
  builds a full frame per keypress to read one bool (`gate_of` above).
- **`decsc` is one slot shared by both screens.** A DECSC on the primary followed by a DECRC inside an
  alt application restores the primary's position; whether the references keep a saved cursor per
  screen was not compared.
- **`?47` clears the alt grid on enter, following xterm.js**; xterm's own `?47` / `?1047` semantics
  (which of enter or leave clears) were not compared.
- **Viewing alt scrollback is impossible by construction.** If it ever becomes a feature,
  `display_offset` stops being 0 on alt and `viewport_link_at` needs the floor like any other walk.
