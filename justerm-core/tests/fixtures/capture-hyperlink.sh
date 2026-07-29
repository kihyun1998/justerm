#!/usr/bin/env bash
#
# Dogfood capture for OSC 8 HYPERLINKS and COMBINING CLUSTERS (#621) — the two
# axes the rest of the corpus structurally cannot contain.
#
# WHY THIS EXISTS: measured across every checked-in capture before writing this
# (vim_redraw, top, htop, neovim_kitty, softwrap_shifts, softwrap_wide,
# less_softwrap, undercurl_matrix, alt_resize_vim.post, alt_resize_htop.post),
# replaying each through the engine at 80x24 yields:
#
#   combining cells 0    linked cells 0    link_table 0        — all ten, every time
#
# So `engine_frame_round_trips_real_captures` was green for #621's entire wire
# change while asserting nothing whatsoever about it. That is the #554 pattern
# repeating on a different axis, and the reason is the same and it is structural:
# the corpus is TUI applications, and a full-screen TUI emits neither. OSC 8 is a
# shell/tool phenomenon (`ls --hyperlink`, gcc, rg), and combining marks need real
# Unicode text rather than box-drawing and status bars. Recording more TUIs would
# never have produced either.
#
# WHY TWO CAPTURES (same split as capture-softwrap.sh):
#   1. hyperlink_combining.raw — a DETERMINISTIC printf. The bytes ARE what a
#      terminal receives, so this is the source of truth and it reproduces
#      anywhere, VM or not.
#   2. ls_hyperlink.raw — best-effort REAL `ls --hyperlink=auto`, so a real
#      application's own bytes are on record. Needs a real PTY and coreutils >= 8.29.
#
# WHAT THE DETERMINISTIC HALF MUST CONTAIN, and why each part is not optional:
#
#   - several DISTINCT URIs, so the frame-local renumber has something to number;
#   - ONE URI repeated across many cells AND across rows, so `link_table.len()` is
#     strictly less than the linked-cell count — that gap IS the interning this
#     change decided to keep, and without it the capture cannot tell the two
#     candidate wire shapes apart;
#   - clusters of more than one mark, so a cluster is not merely "one char";
#   - and the combination neither axis reaches alone: a cell carrying a link AND a
#     combining cluster at the same column. Both are sparse groups keyed by the
#     same span-relative column, so this is the one state where their keying can
#     disagree, and no single-feature capture produces it.
#
# THE WITNESS: a capture that cannot fail is decoration (capture-softwrap.sh's
# lesson). The char grid is NOT the witness here and must not be mistaken for one —
# a combining mark is not in the char grid at all (the grid holds the base
# codepoint; the marks live in the row's side map), so a char-grid golden is blind
# to this capture's whole point. The witness is `Frame` equality across the wire:
# `decode(encode(frame)) == frame` compares the combining and link maps and the
# re-armed presence bits, which is exactly what #621 moved. Wire this capture into
# `engine_frame_round_trips_real_captures`, and assert the counts are non-zero
# there, so the capture cannot silently stop testing anything.
#
# WHERE TO RUN: (1) anywhere with bash. (2) needs a real PTY — the Linux VM.
#
# USAGE: capture-hyperlink.sh [output-dir]   (defaults to a fresh temp dir)
set -e
cd "${1:-$(mktemp -d)}"

# OSC 8 is `ESC ] 8 ; params ; URI BEL  text  ESC ] 8 ; ; BEL`.
link() { printf '\033]8;;%s\007%s\033]8;;\007' "$1" "$2"; }

{
  printf '\033[2J\033[H'
  printf 'HYPERLINK + COMBINING capture\r\n'

  # --- distinct URIs, one per row -----------------------------------------
  for i in 1 2 3 4; do
    printf 'file %d: ' "$i"
    link "file:///home/user/project/src/module_${i}.rs" "module_${i}.rs"
    printf '\r\n'
  done

  # --- ONE URI repeated, across cells and across rows ----------------------
  # This is the interning witness: many linked cells, one table entry. If the
  # engine ever stopped interning, link_table would grow with the rows and the
  # round-trip assertion would still pass — so the TEST checks the ratio, not
  # this file. What this file guarantees is that the ratio is observable at all.
  same='https://ci.example.com/org/repo/actions/runs/1234567890'
  for i in 1 2 3; do
    printf 'build %d: ' "$i"
    link "$same" "the same run, linked again"
    printf '\r\n'
  done

  # --- combining clusters, one and many marks ------------------------------
  # e + acute; a + diaeresis; o + THREE stacked marks. The last one matters:
  # a cluster of one char cannot distinguish a length prefix from a flag.
  printf 'combining: e\xcc\x81 a\xcc\x88 o\xcc\x81\xcc\x88\xcc\xa7 done\r\n'

  # --- the combination: a link whose text also carries combining marks ------
  # A cell keyed in BOTH sparse groups at the same span-relative column. Neither
  # single-feature capture above reaches this state.
  printf 'both: '
  link 'https://example.com/caf%C3%A9' "$(printf 'cafe\xcc\x81 re\xcc\x81sume\xcc\x81')"
  printf '\r\n'

  # --- a wide glyph next to a link, so the pair and the groups interact -----
  printf 'wide: '
  link 'https://example.com/wide' "$(printf '\355\225\234\352\270\200-link')"
  printf '\r\n'

  printf '\033[24;1H'
} > hyperlink_combining.raw

# --- 2. best-effort real `ls --hyperlink` (needs a real PTY) -----------------
# coreutils >= 8.29 supports --hyperlink; 8.32 is what the VM carries.
if command -v expect >/dev/null 2>&1 && ls --hyperlink=auto --version >/dev/null 2>&1; then
  rm -rf lsdir && mkdir lsdir
  for n in alpha.rs beta.rs gamma.md delta.toml epsilon.json; do : > "lsdir/$n"; done
  mkdir -p lsdir/nested
  cat > drive.exp <<'EXP'
set timeout 20
# LC_ALL pins a UTF-8 locale and must stay one -- plain C is NOT a valid
# simplification: it makes the app drop Unicode output, which is the material this
# engine exists to get right (measured on htop: its U+25BD sort glyph vanishes).
spawn -noecho env LC_ALL=C.UTF-8 TERM=xterm-256color LINES=24 COLUMNS=80 \
    ls --hyperlink=always -la --color=never lsdir
expect eof
EXP
  expect drive.exp > ls_hyperlink.raw 2>/dev/null || true
else
  echo "note: expect or ls --hyperlink missing — skipping capture 2" >&2
  : > ls_hyperlink.raw
fi

# --- verify what was actually captured before trusting it --------------------
python3 - <<'PY'
import re, os
for name in ("hyperlink_combining.raw", "ls_hyperlink.raw"):
    if not os.path.exists(name):
        continue
    d = open(name, 'rb').read()
    opens = re.findall(rb'\x1b\]8;[^;]*;([^\x07\x1b]*)\x07', d)
    uris = [u for u in opens if u]
    # Combining marks: U+0300..U+036F encode as 0xCC 0x80..0xCD 0xAF in UTF-8.
    marks = len(re.findall(rb'[\xcc\xcd][\x80-\xbf]', d))
    print("%-26s %5d bytes  osc8-open %-3d distinct-uri %-3d combining-marks %d"
          % (name, len(d), len(uris), len(set(uris)), marks))
PY

echo "=== BEGIN hyperlink_combining.raw.b64 ==="
base64 -w0 hyperlink_combining.raw 2>/dev/null || base64 hyperlink_combining.raw; echo
echo "=== BEGIN ls_hyperlink.raw.b64 ==="
base64 -w0 ls_hyperlink.raw 2>/dev/null || base64 ls_hyperlink.raw; echo
echo "captured in: $PWD"
