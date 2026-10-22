# Invariant — RIS resets the terminal, so a field survives it iff it is configuration (or a handle the consumer still holds) rather than a coordinate

## The fact

`justerm-core/src/term.rs — Term::full_reset` (RIS, `ESC c`) does not clear fields; it rebuilds the
whole struct with `*self = Term::with_scrollback(cols, rows, self.scrollback_limit)` and copies a
short list back. At HEAD 7bb31f7 that list is `replies`, `events` (after appending a
`MarkerDisposed` for every marker), `word_separators`, `selection_carries_line_end`, `next_tracked_id`,
`next_marker_id` and `marker_epoch` (then bumped); `cols` / `rows` and `scrollback_limit` ride the
constructor arguments. (The old note quoted a five-line copy-back list; it has grown.)

So the default for a new field is "silently reverted the first time an application prints `reset`",
and nothing at the definition site says which side a field is on. The line is not a judgement call —
ask what the field is:

| Kind | RIS | Why | Instances |
|---|---|---|---|
| **Configuration** — chosen by the embedder, meaningful with no buffer | survives | RIS resets the terminal; the embedder's configuration is not the terminal | `scrollback_limit` and `cols` / `rows` (constructor arguments — so auditing the copy-back list undercounts survivors), `word_separators` (#545), `selection_carries_line_end` (#1031) |
| **A coordinate into the buffer**, or anything derived from cells | dies | RIS wipes every cell; carrying it points live state at a buffer that no longer exists | `selection`, `search_highlights`, `active_search_highlight`, the marker sets, the tracked-point sets (#691) |
| **A pending obligation to the consumer** | survives | it describes bytes the consumer still has to write, and RIS adds to it | `replies`, `events` |
| **The id counter behind a handle the consumer still holds** | survives | the coordinate dies, but the id is out in the consumer's hands; a rebuilt counter would reissue it and a stale ask would be answered with a different object's state | `next_tracked_id` (#691 — no disposal event; the holder learns by `None`); `next_marker_id`, because a pulled handle outlives the announcement that killed it, so a reissued id would let a stale `MarkerDisposed(7)` drop the live post-RIS marker 7 |
| **Terminal state the application wrote through the VT stream** | dies | this is the terminal's state, and the party that set it is the party `ESC c` resets; the configuration row's exemption is for the embedder, and both look like "a configured string" at the definition site | `window_title`, `icon_name`, `window_title_stack`, `icon_name_stack` (#823); both screens' kitty keyboard flags and stacks incl. `kitty_flags_inactive` / `kitty_stack_inactive` (#941 — alacritty, ghostty and xterm.js clear both); `modify_other_keys_2` (#890) |

Corrected at HEAD 7bb31f7: the old table said `next_marker_id` does **not** survive ("an announced
death makes the question moot"), contradicting its own paragraph below it; the code carries it across
(`full_reset`'s comment: "so a stale `MarkerDisposed` cannot drop a reissued id"). The id-counter row's
"iff the death is not announced" is therefore dropped: an announcement does not reach a handle pulled
before it.

**The marker epoch rides across and then moves** (#490): resetting it to 0 would leave it equal to what
a quiet session already holds, so the one signal a consumer watches would not fire. `evicted_total`
legitimately restarts; the epoch change stops the consumer rebasing on the old basis.

**State the reset invalidates that is not a field (#835).** The ANSI palette: an application redefines
it with `OSC 4`, the engine relays and keeps nothing (theme-agnostic), so the consumer's copy is the only
copy. The question is not "does it survive" but "does the rebuild owe an announcement". For markers yes
(`MarkerDisposed`); for the palette **no**, decided on #835 — ADR-0004's tie-breaker does not reach a
table DEC never defined, terminfo appends the palette reset after RIS (`xterm-256color`'s
`rs1=\Ec\E]104\007`), and ghostty, which announces every other palette change across its consumer
boundary, sends nothing from `fullReset`. Pinned by `justerm-core/tests/reset.rs —
ris_announces_nothing_about_the_palette` / `decstr_announces_nothing_about_the_palette`.

Title row tally (corrected #835): alacritty clears by hand (`title_stack = Vec::new()`, `title = None` in
`reset_state`), ghostty clears `self.title` and `self.pwd` in `fullReset` (`Terminal.zig:4468-4469`);
the retained strings are a 2–2 tie (ghostty has no title stack in `Terminal` to count), and neither
announces the clearing (2–0).

### A counter carried across RIS still reissues on overflow

Carrying an id counter across RIS keeps it from reissuing an id a consumer still holds — but only
across RIS. Both counters are a `u32` advanced with `+= 1`: `justerm-core/src/term/markers.rs —
Term::push_marker` (behind `add_marker` and the OSC 133 marks) for `next_marker_id`, and
`justerm-core/src/term/tracked.rs — Term::track_point` for `next_tracked_id` (re-verified at HEAD
7bb31f7). Past `u32::MAX` allocations a debug build panics and a release build wraps to `0`, which is
the reissue the copy-back exists to prevent. Neither path calls the other, and the workspace sets no
`overflow-checks` profile, so the two builds disagree.

The two differ in who spends the ids. Markers are allocated by the **stream** (every OSC 133 mark), so
an application can drive the count; the live population is capped at `MAX_MARKERS` (#721), so a wrap
reissues an id long since retired — dangerous only to a consumer still holding that old handle.
Tracked points are allocated by the **consumer** (PenTerm spends one per designation) with no cap on
live points, so a wrap can reissue an id whose point is still registered, and `tracked_point` answers
for whichever comes first (`find`) while `untrack_point` releases both (`retain`). Unmeasured how far either is from reach; nothing bounds a session's
allocation count. (`marker_epoch` uses `wrapping_add` on purpose, compared for equality; it is not a
handle.)

## Why it is cross-cutting

The fields sit in unrelated features, are copied back by one function, and are decided by the same
question; the features' code does not call each other. Selection holds both halves at once —
`word_separators` must survive while the `selection` anchors beside it die — so "just remember the
field" is unworkable as a rule.

No reference can supply the answer, because none faces the question: alacritty's `reset_state` never
touches `self.config`, xterm.js holds configuration in `OptionsService` outside `fullReset`, ghostty
passes `selection-word-chars` per call from `Surface.config`. justerm's shape — one struct holding the
buffer and the embedder's knobs, reset by replacement — creates it, so reading upstream settles no new
field.

## Territories it holds in

- [selection](../territory/selection.md) — `word_separators` (consumer policy under ADR-0017,
  `Term::set_word_separators`, `DEFAULT_WORD_SEPARATORS`) and `selection_carries_line_end` survive; the
  `selection` anchors die.
- [grid & scrollback](../territory/grid-and-scrollback.md) — `scrollback_limit` survives by riding the
  constructor argument, absent from the copy-back list.
- [events & replies](../territory/events-and-replies.md) — both queues survive, and the reset appends
  to `events` (every marker's disposal); the palette's silence beside it is a tested decision.
- [vt-interpretation](../territory/vt-interpretation.md) — RIS (`full_reset`) and DECSTR
  (`Term::soft_reset`) are dispatched here; the application-written terminal state (titles, kitty
  flags, `modify_other_keys_2`) is this territory's. Added here; this territory already linked back.
- [tracked points](../territory/tracked-points.md) — the point sets die, `next_tracked_id` survives
  (#691), and still wraps at `u32::MAX` (§ A counter carried across RIS). Added here (new territory).
- [marker](../territory/marker.md) — the marker sets die after being announced; `next_marker_id` and
  `marker_epoch` survive, the epoch then bumped (#490); `next_marker_id` still wraps at `u32::MAX`
  (§ A counter carried across RIS still reissues on overflow).

## What a violation looks like

Quiet: an embedder's setting is back to the built-in one hours after startup, the first time something
printed `reset` or `tput reset` (several TUIs do on exit). The consumer reports "my configuration
randomly stops applying", and nothing correlates it with the reset.

The mirror: carry a coordinate across and it indexes a wiped buffer. Rebuild an id counter and a stale
ask is answered with another object — #691's RIS test was green while `TrackedId(0)` was silently
reissued, because the rebuild dropped the points for free.

## Discovery history

- **#545** — the word-boundary set became consumer-injected policy, the first field unambiguously
  configuration; `scrollback_limit` had been safe by accident of being a constructor argument. Written
  at the first site deliberately — the alt-screen floor was rediscovered three times before it was.
- **#490** — the epoch rides across and moves.
- **#691** — the id-counter row arrived as a gap in the table, found by asking why the RIS test passed.
- **#823** — four application-written fields on the dying side: the table is not only for survivors;
  a field that dies still needs its answer at the definition site.
- **#835** — the not-a-field axis (palette), and the title tally corrected.
- **#890**, **#941** — `modify_other_keys_2` (also cleared by `soft_reset`) and the kitty flags.
- **#1031** — `selection_carries_line_end`, a second consumer-policy survivor.

## Where it will recur

Every field added to `Term`. Answer the table's question in the field's doc-comment. If the answer is
configuration or a consumer-held id counter, add the copy-back line in `full_reset` **and a behavioural
test** — asserting the value after `feed(b"\x1bc")` is not enough, because a field can be restored and
still not be read by the path that matters. `justerm-core/tests/selection.rs —
a_full_reset_keeps_the_consumer_supplied_separator_set` is the shape: feed RIS, feed text, check the
behaviour the setting governs.

Near-term candidates are configuration — any further policy injected under ADR-0017. A cap for the
unbounded buffer walks (#206, closed with reach measured at zero) would be a field beside
`set_word_separators` and face exactly this question.

A new handle-id counter, beside the two above: carrying it across RIS is half the answer; say also
what happens at `u32::MAX` (widen, wrap with a retirement check, or refuse), because `+= 1` answers
differently in debug and release.

Not transferable: a resize resets fields on a different axis — the reflow territory
records that every row here reasons from "RIS wipes every cell", which a resize does not.
