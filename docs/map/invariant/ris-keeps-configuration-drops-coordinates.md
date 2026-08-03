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
no compiler diagnostic, says which side of the line a field is on. The failure is quiet — no panic,
no wrong pixel, just an embedder's setting quietly back to the built-in one, hours after startup.

**The line itself is not a judgement call.** Ask what the field *is*:

| Kind | RIS | Why | Instances |
|---|---|---|---|
| **Configuration** — chosen by the embedder, meaningful with no buffer | **survives** | RIS resets the *terminal*; the embedder's configuration is not the terminal | `scrollback_limit`, `word_separators` (#545), and the `cols`/`rows` geometry |
| **A coordinate into the buffer** — or anything derived from cell contents | **dies** | RIS wipes every cell, so the coordinate now names nothing. Carrying it would point live state at a buffer that no longer exists | `selection`, `search_highlights`, `active_search_highlight`, the marker sets |
| **A pending obligation to the consumer** | **survives** | it describes bytes the consumer still has to write, not screen state — and RIS *adds* to it (every marker's disposal is announced) | `replies`, `events` |

## Why it is cross-cutting

Three territories, no shared code, one shared mechanism — the fields sit in unrelated features and
are decided by the same question:

- [selection](../territory/selection.md) — `word_separators` is **consumer policy** under
  ADR-0017, so RIS must not revert it, while the `selection` anchors in the same territory must die
  with the buffer they index. **Both halves of the rule appear inside one feature**, which is what
  makes "just remember the field" unworkable as a rule
- [grid & scrollback](../territory/grid-and-scrollback.md) — `scrollback_limit` survives by riding
  the constructor argument, so it is carried *without appearing in the copy-back list*: a reader
  auditing that list undercounts what survives
- [events & replies](../territory/events-and-replies.md) — `replies` / `events` survive **and** are
  appended to during the reset

## Why the references never face it

None of the three has a test for this, because none of them can have the bug: alacritty's
`reset_state` enumerates the fields it clears and never touches `self.config`; xterm.js holds the
equivalent in `OptionsService`, outside anything `fullReset` reaches; ghostty passes its
`selection-word-chars` in per call from `Surface.config`, so the terminal object never stores it.

The shape justerm chose — one struct holding both the buffer and the embedder's knobs, reset by
replacement — is what creates the question, so **the reference cannot supply the answer and no
amount of reading it will**. That is the whole reason this is written down here rather than
re-derived per field.

## What to do when adding a field to `Term`

Answer the table's question in the field's own doc-comment, and if the answer is "configuration",
add the copy-back line **and a test that survives RIS behaviourally** — asserting the field's value
after `feed(b"\x1bc")` is not enough on its own, because a field can be restored and still not be
read by the path that matters. `a_full_reset_keeps_the_consumer_supplied_separator_set`
(`justerm-core/tests/selection.rs`) is the shape: it feeds RIS, then feeds text and checks the
*behaviour* the setting governs.

## History

Discovered writing #545 (the word-boundary set becoming consumer-injected policy), which is the
first field added to `Term` that is unambiguously configuration rather than state. `scrollback_limit`
had been on the safe side of the line since the beginning by accident of being a constructor
argument, so the rule had never had to be stated. Written at the first site rather than the third,
deliberately: the two earlier surviving fields were never *decided*, so there was nothing for a later
reader to find.
