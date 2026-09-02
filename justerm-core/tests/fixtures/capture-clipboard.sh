#!/bin/bash
# #828 — record a real `tmux` session that puts text on the outer terminal's
# clipboard with OSC 52.
#
# Run on the RHEL 9.2 VM under a real pty:
#
#   ssh -tt justerm-vm 'stty rows 24 cols 80; stty size; bash capture-clipboard.sh /tmp/out'
#
# `stty size` must print `24 80` inside the SAME invocation — without a tty the
# pty is unsized and tmux lays out to a winsize nobody chose, silently.
#
# WHY A REAL EMITTER AND NOT A SYNTHETIC `52;c;…`: the target field tmux sends is
# **empty** — `ESC ] 52 ; ; <base64>` — not `c`. The spec says an empty field
# means `s0`, the configurable primary/clipboard selection plus cut-buffer 0
# (`ctlseqs.txt:2161`), and the issue's own first draft said an unrecognised
# target should be ignored. Either rule, implemented literally, drops every copy
# a real multiplexer makes, and a fixture written by hand as `52;c;…` would have
# been green throughout. This capture is the thing that says otherwise.
#
# WHY `set-buffer -w` RATHER THAN COPY-MODE: both were measured emitting the same
# form (#828, second comment), and this one needs no key injection, so it is the
# half that reproduces without a human. `-w` is what asks tmux to forward the
# buffer to the outer terminal; `set-clipboard on` is what permits it. Both are
# required — with either missing, tmux writes its own buffer and emits nothing.
#
# The payload is ASCII on purpose: what this fixture exists to pin is the
# **envelope** tmux chooses, and the non-ASCII round trip is proven by a unit
# case in `tests/clipboard.rs` where the bytes can be stated exactly.
set -u
out="${1:-/tmp/clipboard}"
rm -rf "$out"
mkdir -p "$out"

export LC_ALL=C.UTF-8   # not C: it strips the Unicode material this engine exists for
export TERM=xterm-256color

stty size
tmux -V

sock=justerm828
tmux -L "$sock" kill-server >/dev/null 2>&1

script -q -c "tmux -L $sock -f /dev/null new-session \
    'tmux -L $sock set -g set-clipboard on; \
     tmux -L $sock set-buffer -w HELLOJUSTERM; \
     sleep 1'" \
    "$out/tmux_clipboard.raw" >/dev/null 2>&1

tmux -L "$sock" kill-server >/dev/null 2>&1

python3 - "$out/tmux_clipboard.raw" <<'PY'
import re, sys
b = open(sys.argv[1], "rb").read()
print(f"{sys.argv[1]}  {len(b)} bytes")
print("  OSC 52 :", b.count(b"\x1b]52"))
for m in re.finditer(rb"\x1b\]52;([^;]*);([A-Za-z0-9+/=]*)(\x07|\x1b\\)", b):
    print("   target=%r payload=%r term=%r" % (m.group(1), m.group(2), m.group(3)))
PY
