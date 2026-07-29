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

**Derive what a grep can see; hand-write only what it cannot — and label which is which.** Both
halves are load-bearing, and this section is the evidence for both. Hand-maintaining the *derivable*
half is what went wrong: this note, `theflow.md`'s Step 5 entry and `walk.rs`'s module doc each named
a different set of call sites and none matched the code. But the two sites in the second half below
appear in **no** grep for `abs_floor`, and they are the ones most likely to be mistaken for a missing
floor — so a grep-only rule would delete exactly what is hardest to rediscover.

Derivable half — ask, do not store:

```sh
rg 'abs_floor\(\)' justerm-core/src/ | rg -v 'fn abs_floor'
```

Call sites as of #601 — the territory links are the graph edge, the grep is the authority. Both
artifacts that used to keep their own copy now point here instead: `theflow.md`'s Step 5 entry and,
since #602, `walk.rs`'s module doc.

- [selection](../territory/selection.md) — `Term::prev_pos` / `Term::next_pos`, the logical-line step
  (`term/walk.rs`; #207)
- [search & active match](../territory/search.md) — `Term::search_with` (`term/search.rs`; #144)
- [logical lines](../territory/logical-lines.md) — `Term::viewport_logical_lines`
  (`term/logical.rs` since #601; #113)
- **a11y / whole-buffer text** *(no territory note yet)* — `Term::accessible_text`, which moved to
  `term/selection.rs` with #587 because it reuses selection's extraction path, not because it is a
  selection. Its `## Code` entry is carried by **both** [selection](../territory/selection.md) and
  [logical lines](../territory/logical-lines.md)

Non-derivable half — **maintained by hand on purpose, and this is the part no automation replaces.**
Two sites satisfy the floor *without calling it*: `Term::end_wrap` and `Term::shift_region` argue it
out in comments instead (*"on the alt screen `abs_floor()` is the screen top, so no join crosses the
boundary at all"*). They belong to
[soft wrap](../territory/soft-wrap.md). A structural argument is a
legitimate way to hold the invariant, and it is permanently invisible to the grep above — which makes
these the two entries most likely to be read as a *missing* floor and "fixed" into a redundant call.
Update them by hand when the write path moves; never expect a tool to notice.

A third satisfies it by **construction** rather than by argument, found by #601's pass: `Term::viewport_link_at`
reaches cells through `abs_row` — so the recurrence test below flags it — but its index is
`scrollback.len() - display_offset + row`, and `display_offset` is pinned to 0 on the alt screen
(`enter_alt_screen` zeroes it, `set_display_offset` returns early while `on_alt`). So on alt the index
is `scrollback.len() + row`, already at or above the floor. **Validity condition:** this holds only
while `display_offset` cannot be non-zero on the alt screen. If viewing alt scrollback ever becomes a
thing, this site needs the floor like any other.

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

**A third category exists, and the test above does not catch it — deliberately unfloored walks.**
`Term::doc_line_of` and `Term::command_start` (`term/markers.rs` since #588) walk absolutely and have
no floor *and must not get one*. They do not choose the grid themselves — `Term::command_lines` threads
`primary_grid()` into them, and into `extract_lines` on the same call, so all three run in
`[scrollback ++ primary]`: one coherent buffer even while the alt screen is showing, because OSC 133
command marks are primary-only by definition (`#192`). They also slip the test's phrasing, reaching cells
through `row_in` / `line_in(grid, …)` rather than `abs_line` / `abs_row`. The failure mode here is the
mirror of the other three: not a missing floor, but someone adding one and silently breaking command
navigation on the alt screen. **Validity condition:** this holds only while command marks stay
primary-scoped. If an alt-scoped command mark is ever introduced, these two walks need the floor after
all.

**Do not search for `abs_floor` and conclude you are done.** #585 folded the last open-coded copies
into calls, so `rg abs_floor` now finds every walk that *has* a floor — and that is the opposite of
the defect. All three historical misses **predate** the helper: each was a fresh absolute walk with
no floor at all, mentioning `abs_floor` nowhere. The grep that finds the next one is for raw
`scrollback.len()` arithmetic, not for the function's name. Centralising an expression makes the
correct sites findable; it does not make the incorrect ones findable, and only the second matters
here.
