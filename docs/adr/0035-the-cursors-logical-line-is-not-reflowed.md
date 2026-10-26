# ADR-0035: A column resize does not reflow the logical line holding the cursor

Status: **accepted** (2026-10-26), #1082. **D1 is a judgement** made by the maintainer during #1082's
triage. It was picked from enumerated options whose consequences were shown, and it reverses the
choice #1076 made the same day. The facts it was decided on are listed under it. A fact those facts
could not have shown reopens it; a better argument alone does not. **D2–D5 are derivations** from
D1, the code and the named references, and fall to a better derivation.

## Context

On the primary screen a column resize re-splits every soft-wrapped logical line, history included
(`grid::reflow`), and until now the cursor rode that re-split as a tracked point. #1076 found a shell
prompt duplicated after a one-column narrowing and kept the cursor on its row in that one case (a
parked cursor landing exactly past a full row), choosing that over xterm.js's policy of not reflowing
the cursor's line. #1082 found the same duplicate after a narrowing that skips the exact width in one
call, which is what a fast pane drag does.

## Decision

**D1 — On the primary screen, a column resize does not reflow the logical line that holds the cursor,
narrowing or widening.** Its rows are cut or padded to the new width (`Row::resize`) and keep their
wrap flags; every other line reflows as before. The alternate screen does not reflow at all and is
unchanged.

*Decided on* (all measured 2026-10-26):
- **What the shells send.** Recorded under a real pty on WSL Ubuntu 24.04, one `TIOCSWINSZ` per case:
  bash 5.2.21 answers SIGWINCH with `\r ESC[K \r`, plus `ESC[A ESC[K \r` per row its prompt took **at
  the old width**, then the prompt at the new width; zsh (`-f`) climbs the same `ESC[A` and clears
  with `ESC[J`. Both find the prompt by relative motion counted in old-width rows, so any rule that
  re-splits the cursor's line sends them to the wrong row. The bytes are
  `justerm-core/tests/fixtures/resize_shell_redraw.raw`, recorded by the script beside it.
- **The replay.** Those bytes, after `OUT1\r\nOUT2\r\n` and the prompt on a 30-column screen, left
  exactly one prompt in 6 of 6 cases under xterm.js (`@xterm/headless` 6.1.0-beta.301) and failed 6 of
  6 under justerm `a264e46`: a piece of the prompt (one or two rows) left above it on every narrowing,
  and on the widening `OUT2` erased, because bash's `ESC[A ESC[K` landed on it.
- **The references.** xterm.js skips the cursor's wrapped run in both directions by default
  (`reflowCursorLine: false`, *"shells usually handle this themselves"*). alacritty reflows it and the
  cursor follows. ghostty reflows it and, only when OSC 133 says the cursor is at a prompt, blanks the
  prompt rows; an SSH session without shell integration never says so. Rows in
  `docs/agents/reference-facts.md` § "A parked cursor on a trailing blank across a narrowing".
- **The cost, measured in xterm.js.** A narrowing **cuts** the cursor's line, and widening back does not
  restore it: `progress: 0123456789abcdefghij` with the cursor at its end keeps `progress: ` at 10
  columns and at 30 again. A widening leaves the line's rows padded, so a program that does not redraw
  reads blanks inside its line (`abcdef␠␠ghZ`). A shell redraws the line and shows neither.

*Not covered by this decision*: a consumer switch to reflow the cursor line (xterm.js's
`reflowCursorLine: true`), and OSC 133–driven prompt clearing. Neither was asked for.

**D2 — The cursor keeps its row and its column, clamped onto the last column when the row got
narrower than it, and parked only if it arrived parked.** Its column is not read back from the
re-split, so a cursor after trailing blanks keeps them (`ab␠␠` then a resize then `X` is `ab␠␠X`, as
ghostty's pin and alacritty's whitespace rule keep it). Arming the deferred wrap on a cursor that was
not parked would glue its row to the next line on the next print (#562 constraint 2); xterm.js clamps
without arming. A cursor that arrived parked keeps the park, so #848's carry still steps it onto the
one-past column when a widening makes room.

**D3 — A cursor line with no content is reflowed as before.** It reads the same either way, and the
absorbed-blank path keeps the cursor's distance without spending a row on a pane that has no history.

**D4 — While the alt screen is up, the primary's cursor is where the cursor was when the screen
switched**, carried through each resize, and its line is the one kept. The `?1049` / `?1048` save
slot cannot stand in: `?47` / `?1047` write nothing to it and `?1048h` on the alt screen writes alt
coordinates, and keeping the line under a stale slot cut unrelated primary text. xterm.js keeps a
cursor per buffer for the same reason. The slot rides the reflow as a point.

**D5 — A wide lead the cut strands on a kept row that continues becomes the wrap artefact**, as the
re-split leaves it (#533). Left a lead, a later reflow joined it mid-line, where ADR-0025's
last-column guarantee does not hold, and an overwrite took its neighbour for its spacer. A lead
stranded on a hard-ended row stays, ADR-0025's legal last-column state (#529).

**Consequences.** #1076's special case (`stays_parked` in `reflow_pane`) is gone: under D1 the cursor
never comes back from the re-split past a full row on the primary. Points on the kept line (selection
endpoints, marks, tracked points, the paint change record) keep their row with `col` clamped to the
new width, which each holder already reads. An OSC 133 mark on the prompt line is therefore not moved
to where the shell's redraw puts its text; before this decision the leftover prompt piece put the
redraw on the wrong rows instead.
