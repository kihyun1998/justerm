# Cross-cutting invariant — the alt-screen absolute-index floor

## The fact

**Every reader that walks the single `[scrollback ++ grid]` buffer by absolute line index must floor
at `scrollback.len()` on the alt screen**, and must not descend below it or join across it.

Centralised form — `justerm-core/src/term/walk.rs`, `Term::abs_floor` (the buffer-walk primitives got
their own module in #585; `walk.rs`'s module doc is the site-local statement of this rule and is worth
reading at the site):

```rust
fn abs_floor(&self) -> usize {
    if self.on_alt { self.scrollback.len() } else { 0 }
}
```

## Why it is cross-cutting

**One storage layout, two logical spaces.** justerm addresses scrollback and screen through a single
concatenated coordinate space (`[scrollback ++ grid]`). When the alt screen is active, that
`scrollback` holds the *primary* buffer's history — a separate logical space with no relation to what
is on screen. xterm gets this isolation for free because its `Buffer` objects are physically separate;
justerm must **reproduce it at every walk site**.

So the fact is owned by no single territory. Note that the sites below **do not call each other** —
they share only a *storage assumption*. This is precisely the class of dependency a territory-to-
territory edge cannot express, and the reason this node kind exists.

## Territories it holds in

**Do not maintain the site list by hand — three artifacts already drifted apart doing that** (this
note, `theflow.md`'s Step 5 entry and `walk.rs`'s module doc each named a *different* set, and none
matched the code). Ask instead:

```sh
rg 'abs_floor\(\)' justerm-core/src/ | rg -v 'fn abs_floor'
```

Five call sites, as of #585/#586 — the territory links are the graph edge, the grep is the authority:

- [selection](../territory/selection.md) — `Term::prev_pos` / `Term::next_pos`, the logical-line step
  (`term/walk.rs`; #207)
- [search & active match](../territory/search.md) — `Term::search_with` (`term/search.rs`; #144)
- [logical lines](../territory/logical-lines.md) — `Term::viewport_logical_lines` (`term.rs`; #113)
- **a11y / whole-buffer text** *(no note yet)* — `Term::accessible_text` (`term.rs`). It walks the
  same way and was in none of the three hand-written lists, including this one

**Two more sites satisfy the floor without calling it**, and they are the ones a grep for the
function's name will never surface: `Term::end_wrap` and `Term::shift_region` reason it out in
comments instead (*"on the alt screen `abs_floor()` is the screen top, so no join crosses the
boundary at all"*). They belong to
[wide glyph & soft wrap](../territory/wide-glyph-and-soft-wrap.md). A structural argument is a
legitimate way to hold the invariant — but it is invisible to the check above, so it is written down
here rather than left to be rediscovered as a "missing" floor.

## What a violation looks like

Inside an alt-screen application (vim · htop · less), **text the user cannot see gets pulled in**.
Concretely: row 0 of the alt grid soft-wrap-joins with the last row of the primary scrollback beneath
it, so copy, search and word-selection return content that is not on screen.

It only appears when the primary's last row happens to carry `WRAPLINE`, which makes reproduction
**conditional** — and that is why it was found three separate times instead of once.

## Discovery history

This section is the argument for the node's existence.

| Occurrence | Site | Issue |
|---|---|---|
| 1st | `viewport_logical_lines` | #113 |
| 2nd | `search()` | #144 |
| 3rd | `prev_pos` (word selection) | #207 |

The same fact, found three times independently. The three sites have no call relationship and share
only the storage assumption, so fixing any one of them offered no path to the others.

**`abs_floor()` was extracted after the third discovery, not before it** — and a helper prevents
nothing, because whoever writes the *next* site has no reason to look for it. What prevents the fourth
occurrence is reading this note when adding a walk.

## Where it will recur

Any *new* reader that walks the buffer by absolute index. Test: if a function uses
`scrollback.len() + grid.rows()` as its total, or reaches cells through `abs_line` / `abs_row`, it is
subject to this invariant.

**Do not search for `abs_floor` and conclude you are done.** #585 folded the last open-coded copies
into calls, so `rg abs_floor` now finds every walk that *has* a floor — and that is the opposite of
the defect. All three historical misses **predate** the helper: each was a fresh absolute walk with
no floor at all, mentioning `abs_floor` nowhere. The grep that finds the next one is for raw
`scrollback.len()` arithmetic, not for the function's name. Centralising an expression makes the
correct sites findable; it does not make the incorrect ones findable, and only the second matters
here.
