#!/usr/bin/env bash
#
# Dogfood capture for SOFT WRAP under row-shift verbs (#554) — the combination
# the other four dogfood captures structurally cannot contain.
#
# WHY THIS EXISTS: htop / top / vim / neovim all place every row with CUP, so the
# terminal is never asked to continue a line onto the next row. Replaying all four
# through the engine yields ZERO soft-wrapped rows, which means the whole
# soft-wrap / wide-pair cluster (ADR-0025, spine #552) has no dogfood coverage at
# all — #540's defects were green against every capture in the corpus while
# actively merging two unrelated logical lines.
#
# The pattern behind that is structural, not accidental, and it decides the shape
# of this script: a program that emits IL/DL is a FULL-SCREEN application, and a
# full-screen application positions with CUP and never soft-wraps. Soft wrap is a
# shell/pager phenomenon. Measured here before writing this (real PTY, 80x24):
#
#   nano (long line, ^K/^U)   DL 0  IL 0  SU 0  SD 0  RI  0   wrapped rows 0
#   vim / htop / top / neovim DL 0-2 IL 0-2 SU 0 SD 0 RI 0    wrapped rows 0
#   less -X (page backward)   DL 0  IL 0  SU 0  SD 0  RI 24   wrapped rows YES
#   shell + tput il1/dl1      DL 2  IL 3  SU 0  SD 0  RI  0   wrapped rows YES
#
# So no single real application produces both halves, and recording more TUIs —
# on this machine or on the VM — would never have produced them either.
#
# WHY TWO CAPTURES (same split as capture-undercurl.sh):
#   1. softwrap_shifts.raw — a DETERMINISTIC printf of soft-wrapped content
#      followed by every row-shift verb (IL / DL / SU / SD / RI), including one
#      inside a DECSTBM region. The bytes ARE what a terminal receives, so this is
#      the source of truth and it is reproducible anywhere, VM or not.
#   2. less_softwrap.raw — best-effort REAL `less`, whose backward scrolling emits
#      RI over genuinely soft-wrapped long lines. This is the half that proves a
#      real application reaches this state; it needs a real PTY.
#
# The honest distinction, which must not be smoothed over when reading the
# fixtures: in (1) the soft wrap is real terminal behaviour (nothing positions the
# cursor; the line is simply longer than the screen) but the shift verbs are
# emitted deliberately rather than by an application deciding to redraw. In (2)
# every byte is the application's own.
#
# WHERE TO RUN: (1) anywhere with bash. (2) needs a real PTY — the Linux VM is
# preferred for consistency with the other captures, but `expect` on any machine
# produces a valid stream (record where it was taken in the fixture's doc comment).
#
# USAGE: capture-softwrap.sh [output-dir]   (defaults to a fresh temp dir)
set -e
cd "${1:-$(mktemp -d)}"

cols=80

# --- 1. deterministic soft-wrap + row shifts (guaranteed) --------------------
# No CUP for the content: each line is simply longer than `cols`, so the terminal
# itself continues it onto the next row — which is the state under test.
{
  printf '\033[2J\033[H'
  printf 'HEAD line, short.\r\n'
  # 2 rows worth of one logical line, twice, with short lines between so the
  # shifts below land next to a wrap boundary rather than in blank space.
  printf 'WRAP-A %s\r\n' "$(printf 'a%.0s' $(seq 1 $((cols + 20))))"
  printf 'MID one\r\n'
  printf 'WRAP-B %s\r\n' "$(printf 'b%.0s' $(seq 1 $((cols + 20))))"
  printf 'MID two\r\n'
  printf 'WRAP-C %s\r\n' "$(printf 'c%.0s' $(seq 1 $((cols + 20))))"
  printf 'TAIL line, short.\r\n'

  # IL / DL at the top of the screen: the row above the shifted range is the last
  # row of a soft-wrapped pair, which is #540's seam.
  printf '\033[3;1H'; printf '\033[L'      # IL 1
  printf '\033[3;1H'; printf '\033[M'      # DL 1
  printf '\033[5;1H'; printf '\033[2L'     # IL 2
  printf '\033[5;1H'; printf '\033[2M'     # DL 2

  # SU / SD over the whole screen.
  printf '\033[S'                          # SU 1
  printf '\033[T'                          # SD 1
  printf '\033[2S'                         # SU 2
  printf '\033[2T'                         # SD 2

  # RI at the top margin, and the same verbs inside a DECSTBM region so the
  # region boundaries (the other seam) are exercised too.
  printf '\033[1;1H\033M'                  # RI at row 0
  printf '\033[4;12r'                      # DECSTBM rows 4..12
  printf '\033[4;1H'; printf '\033M'       # RI at the region top
  printf '\033[6;1H'; printf '\033[M'      # DL inside the region
  printf '\033[6;1H'; printf '\033[L'      # IL inside the region
  printf '\033[S'                          # SU inside the region
  printf '\033[T'                          # SD inside the region
  printf '\033[r'                          # margins back to full screen

  # --- the witness ---------------------------------------------------------
  # Exercising the verbs is not enough: measured, the block above passes
  # identically with the row-shift wrap repair (#540) turned off, because the
  # later shifts wash the stale flags back out. A capture that cannot fail is
  # decoration, so the stream ends on a state that *pins* the repair.
  #
  # The shape is the defect's own: soft-wrap a line, delete its continuation, then
  # write into the row that slid up. With the repair, the two are separate lines.
  # Without it, the row above still claims to continue and the marker is swallowed
  # into it — a difference the char grid cannot show, because the same characters
  # sit in the same cells either way.
  printf '\033[2J\033[H'
  printf 'WITNESS HEAD\r\n'
  printf 'W %s\r\n' "$(printf 'w%.0s' $(seq 1 $((cols + 10))))"   # wraps onto row 3
  printf 'VICTIM ROW\r\n'
  printf '\033[3;1H\033[M'                 # DL 1 — delete the continuation row
  printf '\033[3;1HMARKER'                 # write into the row that slid up
} > softwrap_shifts.raw

# --- 2. best-effort real less (needs a real PTY) -----------------------------
if command -v expect >/dev/null 2>&1 && command -v less >/dev/null 2>&1; then
  python3 -c "
for i in range(40):
    print(('LINE%02d ' % i) + ('x'*140 if i % 5 == 0 else 'short body'))
" > long.txt
  cat > drive.exp <<'EXP'
set timeout 25
spawn -noecho env TERM=xterm-256color LINES=24 COLUMNS=80 less -X long.txt
sleep 1
send " "      ;# forward a page
sleep 1
send "b"      ;# backward a page -> RI over wrapped content
sleep 1
send "\r"     ;# forward one line
sleep 1
send "y"      ;# backward one line -> RI again
sleep 1
send "q"
expect eof
EXP
  expect drive.exp > less_softwrap.raw 2>/dev/null || true
else
  echo "note: expect/less missing — skipping capture 2" >&2
  : > less_softwrap.raw
fi

# --- verify what was actually captured before trusting it --------------------
python3 - <<'PY'
import re, os
for name in ("softwrap_shifts.raw", "less_softwrap.raw"):
    if not os.path.exists(name):
        continue
    d = open(name, 'rb').read()
    c = lambda p: len(re.findall(p, d))
    # CSI M (DL) and ESC M (RI) are disjoint patterns — the `[` keeps them apart.
    print("%-22s %5d bytes  DL %-3d IL %-3d SU %-3d SD %-3d RI %-3d DECSTBM %d"
          % (name, len(d), c(rb'\x1b\[[0-9;]*M'), c(rb'\x1b\[[0-9;]*L'),
             c(rb'\x1b\[[0-9;]*S'), c(rb'\x1b\[[0-9;]*T'),
             c(rb'\x1bM'), c(rb'\x1b\[[0-9]*;[0-9]*r')))
PY

echo "=== BEGIN softwrap_shifts.raw.b64 ==="; base64 -w0 softwrap_shifts.raw 2>/dev/null || base64 softwrap_shifts.raw; echo
echo "=== BEGIN less_softwrap.raw.b64 ===";   base64 -w0 less_softwrap.raw 2>/dev/null   || base64 less_softwrap.raw;   echo
echo "captured in: $PWD"
