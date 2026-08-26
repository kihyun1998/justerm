#!/bin/bash
# #832 — record a real nvim session that exercises the cursor-colour sequences.
#
# Run on the RHEL 9.2 VM under a real pty:
#
#   ssh -tt justerm-vm 'stty rows 24 cols 80; stty size; bash capture-cursor-color.sh /tmp/out'
#
# `stty size` must print `24 80` inside the SAME invocation — without a tty the
# pty is unsized and nvim lays out to a winsize nobody chose, silently.
#
# What this captures that no synthetic fixture would have: nvim's OSC 12 is the
# **degenerate empty-spec form** `ESC ] 12 ; BEL`, with no colour in it at all,
# even when a cursor highlight IS configured. An implementation that relayed the
# raw spec without the empty-spec rule would fire a spurious cursor-colour change
# carrying an empty string, twice, every time a user opens an editor.
#
# The `Cursor` highlight plus `guicursor` is what makes nvim touch the cursor
# colour at all; a default `nvim -u NONE` emits only OSC 112 (measured: 2x reset,
# 1x OSC 11 query, zero OSC 12).
set -u
out="${1:-/tmp/cursor-color}"
rm -rf "$out"
mkdir -p "$out"

export LC_ALL=C.UTF-8   # not C: it strips the Unicode material this engine exists for
export TERM=xterm-256color

stty size
nvim --version | head -1

script -q -c 'nvim -u NONE -c "set termguicolors" -c "hi Cursor guibg=#ff0000" -c "set guicursor=a:block-Cursor" -c "normal ix" -c q!' \
    "$out/cursor_color_nvim.raw" >/dev/null 2>&1

python3 - "$out/cursor_color_nvim.raw" <<'PY'
import sys
b = open(sys.argv[1], "rb").read()
print(f"{sys.argv[1]}  {len(b)} bytes")
print("  OSC 12 :", b.count(b"\x1b]12"))
print("  OSC 112:", b.count(b"\x1b]112"))
PY
