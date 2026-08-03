#!/usr/bin/env bash
#
# Dogfood capture for a WRITTEN Unicode space at a line's end (#685) — the axis
# the rest of the corpus does not contain.
#
# WHY THIS EXISTS: justerm trimmed extracted text with `str::trim_end()`, i.e. the
# Unicode `White_Space` **property**, so a non-breaking space (U+00A0) or an
# ideographic space (U+3000) that an application actually printed was deleted from
# `selection_text`, `viewport_logical_lines`, `accessible_text` and `search`'s
# haystack. Measured over all 14 checked-in captures before writing this: NBSP
# (`c2 a0`) appears **0** times and EM SPACE (`e2 80 83`) **0** times, so the
# corpus was structurally blind to it — the same shape #554/#534 record, where an
# axis is present and its combination is not.
#
# WHY A REAL APPLICATION AND NOT A printf: the premise under test is *"a real
# program prints a space that is not U+0020 and puts it at the end of a line"*. A
# deterministic printf of the bytes I already believe in cannot test that premise,
# it re-encodes it. Measured on the VM before recording (real PTY, `script -qec`,
# `LC_ALL=C.UTF-8`): coreutils `ls -1` emits `c2 a0` and `e3 80 80` **unquoted and
# unescaped** for filenames ending in those characters, and `less` reproduces them
# in its own redraw. Both halves below are the applications' own bytes.
#
# WHAT THE GOLDEN CAN AND CANNOT SEE — read this before adding a case:
#   * The **char grid** golden cannot observe this fix at all. `dump()` prints the
#     cell's codepoint, and the cell held the NBSP either way; the trim is on
#     *extracted text*, not on the grid.
#   * Only the **logical-lines** golden can, and only because `logical_lines()` in
#     vttest.rs now normalises with `trim_end_matches(' ')` rather than
#     `trim_end()`. With the harness's original Unicode trim the golden deleted the
#     very character under test and the capture was decoration.
#
# WHERE TO RUN: needs a real PTY — see docs/agents/theflow.md §"Recording a
# capture on the VM". `LC_ALL` must be a UTF-8 locale (plain `C` makes the tools
# escape the very bytes this capture is about).
#
# USAGE: capture-written-space.sh [output-dir]   (defaults to a fresh temp dir)
set -e
cd "${1:-$(mktemp -d)}"

export LC_ALL=C.UTF-8
export TERM=xterm-256color

work=written_space_work
rm -rf "$work" && mkdir "$work"
(
  cd "$work"
  # Filenames whose LAST character is a written Unicode space. `ls -1` puts the
  # name last on the row, so the space lands at the row's end — which is the trim
  # site. A name ending in a plain U+0020 is deliberately absent: it is
  # indistinguishable from the row's padding and is *supposed* to be trimmed.
  printf 'x' > "$(printf 'nbsp-name\xc2\xa0')"
  printf 'x' > "$(printf 'ideo-name\xe3\x80\x80')"
  printf 'x' > "$(printf 'emsp-name\xe2\x80\x83')"
  printf 'x' > plain-name
  # The witness: a name with a written space *between* two visible runs. It is
  # never at a trim site, so it must survive in every state — if it ever differs,
  # the change under test reached further than the row's end.
  printf 'x' > "$(printf 'mid\xc2\xa0witness')"

  printf 'alpha\xc2\xa0\nbeta\xe3\x80\x80\ngamma\xe2\x80\x83\ndelta mid\xc2\xa0run\nomega\n' > lines.txt
)

# Both halves run inside one real PTY session:
#   1. real coreutils `ls -1` — one entry per row, the name last on the row;
#   2. real `less -X -F` paging a file whose lines end in written spaces.
script -qec "
  printf '\033[2J\033[H';
  ls -1 $work;
  less -X -F $work/lines.txt < /dev/null;
" /dev/null > written_space.raw

# --- verify what was actually captured before trusting it --------------------
python3 - <<'PY'
import re
d = open('written_space.raw', 'rb').read()
counts = {name: len(re.findall(pat, d)) for name, pat in (
    ('U+00A0', rb'\xc2\xa0'), ('U+3000', rb'\xe3\x80\x80'), ('U+2003', rb'\xe2\x80\x83'))}
print('written_space.raw %5d bytes  %s' % (len(d), counts))
# A capture that contains none of them proves nothing; fail loudly rather than
# checking in a green fixture that cannot fail.
assert all(v > 0 for v in counts.values()), 'no written Unicode space captured — check LC_ALL'
PY

echo "=== BEGIN written_space.raw.b64 ==="; base64 -w0 written_space.raw 2>/dev/null || base64 written_space.raw; echo
echo "captured in: $PWD"
