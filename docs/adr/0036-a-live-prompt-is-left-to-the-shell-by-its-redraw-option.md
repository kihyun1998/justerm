# ADR-0036: A live prompt is left to the shell's redraw as far as its `133;A` says

Status: **accepted** (2026-10-26), #1083. **D1–D4 are judgements** the maintainer made during #1083,
each from options whose consequences were measured and shown; D2 and D3 were revised by the
maintainer the same day when a measurement the first answer had not seen arrived (recorded under
each). A fact those measurements could not have shown reopens them; a better argument alone does
not. **D5–D8 are derivations** and fall to a better derivation.

## Context

ADR-0035 keeps the cursor's logical line un-reflowed on a resize, which lands a shell's SIGWINCH
redraw on a one-line prompt. A prompt of two logical lines still leaves a piece when the shell
redraws all of it (zsh climbs over the top line's old rows, which reflow re-split), and the cursor
line's cut costs a program that does not redraw its line (`read -p`). OSC 133 marks say where a
prompt starts; kitty and Ghostty use them to hand the prompt rows to the shell.

## Decision

**D1 — kitty's way: a live prompt's rows are left un-reflowed, never cleared.** Ghostty's way
(reflow, then blank from `A` down) was rejected on measurement (WSL Ubuntu 24.04, real pty, bash 5.2
and zsh with PenTerm's snippet marks, replayed through a prototype): it leaves blank rows above the
prompt, it erases bash's top prompt line that bash never redraws, and with a shell that sends no
`C` (PenTerm's sh/dash snippet, ADR-0070 there) it blanked a running command's whole output. A rule
that is wrong about the marks must cost a re-split, not content.

**D2 — The extent is the shell's statement on its last `133;A`, and no statement means today's
behaviour.**

| on the last `133;A` | kept un-reflowed |
|---|---|
| nothing | the cursor's line (ADR-0035) |
| `redraw=1` | the live prompt, from its `A` down to the cursor |
| `redraw=last`, or `k=s` on a line | the cursor's line |
| `redraw=0` | nothing — the prompt reflows like output |

*Decided on:* first "on by default, `redraw=0` off" as both references do; revised because a
server whose integration predates this would then lose the top line of a two-line bash prompt
(bash redraws only the last line, and PenTerm's bash snippet states nothing) — a regression on
every un-updated server. With no statement as today, nothing gets worse; the two-line zsh prompt is
fixed where the shell sends `redraw=1`. Values follow Ghostty's parser (`0`, `1`, `last`); kitty
reads `redraw=0` and `k=s`.

**D3 — After `C` (command output), the cursor's line is kept as today.** *Decided on:* first
"reflow normally after `C`" to keep `read -p` input; revised when a readline REPL inside the command
(`python3 -q`, recorded) was measured to redraw exactly like bash — reflowing its line duplicated the
input on a narrowing and erased the line above on a widening. A REPL sends no OSC 133, so the
engine cannot tell it from `read -p`; the lesser loss is the program's own line.

**D4 — No consumer switch for the fallback now.** Nobody asked; D2 already changes nothing for an
un-updated shell.

**D5 — A prompt is live only when all three hold**, each from a measured failure:
- *Pairing:* walking back by recency over the marks at or above the cursor, past any `B`, an `A` is
  reached before any `C` or `D`. Without it, bash's Ctrl-L (readline sends `ESC[H ESC[2J` + PS1,
  re-sending only `B`; PenTerm's bash `A` comes from `PROMPT_COMMAND`) paired the redraw with the
  previous command's `A` in scrollback and cut that command and its output for good.
- *On screen:* the `A` is not in scrollback.
- *Input line:* every row from the last mark down to the cursor soft-wraps. Without it, a shell with
  no `C` had its running command's output kept as "prompt" and cut.

**D6 — `A;k=s` records no mark.** It is kitty's secondary prompt line (its bash integration puts it
before the last line of a multi-line PS1); as a `PromptStart` it would add a prompt to command
navigation. It only narrows the redraw to the last line.

**D7 — The same rule runs for the primary while the alt screen is up**, with the primary's cursor
`primary_cursor_on_alt` (ADR-0035 D4).

**D8 — Each `133;A` states its own extent**: an `A` without an option resets to "nothing", as kitty
resets its prompt settings on every `A`.

**Consequences.**
- A two-line zsh prompt still leaves a piece until the shell sends `redraw=1`.
- Ghostty's `133;P;k=s` is not read, so only the `A` form is.
- bash re-sends `B` on every SIGWINCH redraw, so command marks accumulate during a drag, bounded
  by `MAX_MARKERS`. This predates the change.
- PenTerm's side, which is what its snippets send, is penterm#2581.
