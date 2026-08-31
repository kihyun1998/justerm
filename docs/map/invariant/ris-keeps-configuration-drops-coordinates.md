# Cross-cutting invariant — RIS resets the terminal, so a field survives it iff it is configuration rather than a coordinate

## The fact

`Term::full_reset` (RIS, `ESC c`) does not clear fields — it **rebuilds the whole struct** and then
copies a short list back:

```rust
let replies = std::mem::take(&mut self.replies);
let mut events = std::mem::take(&mut self.events);
let word_separators = std::mem::take(&mut self.word_separators);
let (cols, rows) = (self.grid.cols(), self.grid.rows());
*self = Term::with_scrollback(cols, rows, self.scrollback_limit);
```

So the default for a **new** field is "silently reverted the first time an application prints
`reset`", and that default is invisible at the definition site: nothing in `Term`'s field list, and
no compiler diagnostic, says which side of the line a field is on.

**The line itself is not a judgement call.** Ask what the field *is*:

| Kind | RIS | Why | Instances |
|---|---|---|---|
| **Configuration** — chosen by the embedder, meaningful with no buffer | **survives** | RIS resets the *terminal*; the embedder's configuration is not the terminal | `scrollback_limit`, `word_separators` (#545), and the `cols`/`rows` geometry |
| **A coordinate into the buffer** — or anything derived from cell contents | **dies** | RIS wipes every cell, so the coordinate now names nothing. Carrying it would point live state at a buffer that no longer exists | `selection`, `search_highlights`, `active_search_highlight`, the marker sets, the tracked-point sets (#691) |
| **A pending obligation to the consumer** | **survives** | it describes bytes the consumer still has to write, not screen state — and RIS *adds* to it | `replies`, `events` |
| **The id counter behind a handle the consumer still holds** | **survives iff the handle's death is not announced** | the coordinate dies with the buffer (row above) — but the *id* naming it is out in the consumer's hands, and a rebuilt counter reissues it. Then a stale ask is answered with a **different** object's state, silently. An announced death makes the question moot, because the holder has already been told | `next_tracked_id` survives (#691, no disposal event — the holder learns by being told `None`); `next_marker_id` does **not**, and reissues freely, because every marker's disposal is announced before the rebuild |
| **Terminal state the *application* wrote through the VT stream** — not a coordinate, not derived from any cell | **dies** | RIS resets the terminal, and this *is* the terminal's state; the party that set it is the party `ESC c` is resetting. The first row's exemption is for the **embedder**, and an application is not the embedder — which is the whole distinction, since both look like "a string somebody configured" at the definition site | `window_title`, `icon_name` and the two XTWINOPS title stacks (#823). alacritty is the one reference that faces this question and answers it the same way, by hand: `title_stack = Vec::new()` and `title = None` in `reset_state` |

## Why it is cross-cutting

Three territories, no shared code, one shared mechanism — the fields sit in unrelated features and
are decided by the same question. **Selection holds both halves at once**, which is what makes
"just remember the field" unworkable as a rule: `word_separators` must survive `ESC c` while the
`selection` anchors beside it must die with the buffer they index.

None of the three references can supply the answer, because none of them faces the question:
alacritty's `reset_state` enumerates what it clears and never touches `self.config`; xterm.js holds
the equivalent in `OptionsService`, outside anything `fullReset` reaches; ghostty passes
`selection-word-chars` in per call from `Surface.config`. The shape justerm chose — one struct
holding both the buffer and the embedder's knobs, reset by replacement — is what creates it, so no
amount of reading upstream settles a new field.

## Territories it holds in

- [selection](../territory/selection.md) — `word_separators` is consumer policy under ADR-0017 and
  survives; the `selection` anchors in the same territory do not
- [grid & scrollback](../territory/grid-and-scrollback.md) — `scrollback_limit` survives by riding
  the constructor argument, so it is carried **without appearing in the copy-back list**: auditing
  that list undercounts what survives
- [events & replies](../territory/events-and-replies.md) — both queues survive, and this is the one
  place the reset *appends*: every marker's disposal is announced before the rebuild

## What a violation looks like

Quiet. No panic, no wrong pixel — an embedder's setting is simply back to the built-in one, hours
after startup, the first time something in the session printed `reset` or `tput reset` (several TUIs
do it on exit). A consumer reports "my configuration randomly stops applying"; nothing correlates it
with the reset, because the reset is invisible in the consumer's own code.

The mirror violation is louder but rarer: carry a *coordinate* across, and it now indexes a buffer
that was wiped — a selection or marker pointing at content that no longer exists.

## Discovery history

Discovered writing **#545** (the word-boundary set becoming consumer-injected policy), which is the
first field added to `Term` that is unambiguously configuration rather than state. `scrollback_limit`
had been on the safe side since the beginning by accident of being a constructor argument, so the
rule had never had to be stated.

Written at the **first** site rather than the third, deliberately: the two earlier surviving fields
were never *decided*, so a later reader had nothing to find. The alt-screen floor is the
counter-example this repo already paid for — the same fact was rediscovered three times over months
before anyone wrote it down.

**The fourth row arrived with #691**, and it arrived as a *gap in this table* rather than as a bug
report: the new field was neither configuration, nor a coordinate, nor a pending obligation, and the
table's three kinds all pointed the wrong way. It was found by asking why the RIS test **passed** —
the wholesale rebuild drops the tracked points for free, so the test was green while the id counter
underneath it silently reissued `TrackedId(0)`. A green test over a rebuilt counter is exactly the
shape this note warns about at the top: the default is invisible at the definition site.

## Where it will recur

Every field added to `Term` from here. **Not all of them are configuration** — that sentence stood
here until #823 added four fields on the dying side, and the correction is the useful part: the
table is read as though its job were only to catch survivors, because a field that dies needs no
code. It is not. A field that dies still needs the *question answered at its definition site*, or
the next reader cannot tell a decision from an accident — which is exactly how the fifth row above
arrived, as a gap rather than as a bug.

The concrete near-term candidates are configuration, i.e. on the surviving side and easy to miss:

- a cap for the unbounded buffer walks (#206), if it is ever made settable rather than a constant
- any further policy injected under ADR-0017 the way #545 injected the first one

When adding one, answer the table's question in the field's own doc-comment, and if the answer is
"configuration", add the copy-back line **and a behavioural test** — asserting the field's value
after `feed(b"\x1bc")` is not enough, because a field can be restored and still not be read by the
path that matters. `a_full_reset_keeps_the_consumer_supplied_separator_set` is the shape: feed RIS,
then feed text, then check the behaviour the setting governs.

## Code

- `justerm-core/src/term.rs` — `Term::full_reset` (the copy-back list), `Term::with_scrollback`
  (what a rebuild restores from arguments), `Term::set_word_separators`, `DEFAULT_WORD_SEPARATORS`
- `justerm-core/tests/selection.rs` — the RIS-survival test named under §"Where it will recur"
  (spelled out there rather than here: this section's symbols are resolved against the source tree,
  which does not include `tests/`)

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated**. The relevant fact is an absence:
all three references keep the embedder's configuration *outside* the object their reset replaces
(alacritty's `Term::config`, xterm.js's `OptionsService`, ghostty's per-call
`boundary_codepoints` from `Surface.config`), so none of them has a test for this and none can be
cited as precedent for justerm's copy-back list.
