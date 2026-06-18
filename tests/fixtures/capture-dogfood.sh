#!/usr/bin/env bash
#
# Dogfood capture for issue #20 — record real terminal byte streams (a vim edit
# session + a live-monitor TUI) as frozen golden fixtures.
#   Live monitor: htop if present (EPEL), else top (procps-ng, always on RHEL).
#   On RHEL without EPEL, top is captured automatically — no install needed.
#
# WHERE TO RUN: on a Linux box, inside a REAL terminal session.
#   - SSH in from Windows via PuTTY / Windows Terminal / ssh  (recommended:
#     pasting the base64 output back is easiest), OR
#   - run directly in the VM console window.
#   What matters: vim/htop must run on Linux under a real PTY. A non-interactive
#   shell (e.g. an agent's tool shell) produces degenerate output and is useless.
#
# WHAT IT DOES: drives vim with a SCRIPTED keystroke file (no human typing),
# records everything the program writes to the terminal into *.raw via script(1),
# then prints each capture as base64 to paste back.
#
# WHY base64: CI never re-runs vim/htop. We freeze ONE capture's raw bytes and
# feed() those bytes; frozen input -> deterministic snapshot, so htop's live
# system state cannot cause flakiness.
#
set -e
cd "$(mktemp -d)"
stty rows 24 cols 80 || true   # fix PTY winsize (the value vim/htop read via ioctl)

# --- deterministic vim driver: \033=ESC, \015=Enter(CR) ---
{
  printf 'iHello from justerm dogfood capture.\015'
  printf 'Second line of text.\015'
  printf 'Third line for editing.\015'
  printf 'Fourth line.\015'
  printf 'Fifth and last line.\033'      # ESC: leave insert mode
  printf 'ggOinserted near the top\033'  # open line at top   -> IL  redraw
  printf '3Gdd'                          # delete line 3      -> DL  redraw
  printf 'yyp'                           # duplicate a line
  printf '$a  <appended>\033'            # append at line end -> ICH redraw
  printf '0x'                            # delete first char  -> DCH redraw
  printf '/edit\015'                     # search (scroll/highlight)
  printf 'G'                             # jump to bottom
  printf ':wq\015'                       # save & quit
} > keys.txt

: > note.txt
TERM=xterm-256color script -q -c 'vim -u NONE -N -s keys.txt note.txt' vim.raw </dev/null

# --- live monitor: a TUI that redraws on a timer. Prefer htop (EPEL); fall back
#     to top (procps-ng, always on RHEL). Run in the FOREGROUND (a full-screen
#     TUI must own the controlling tty -- backgrounding it gives "failed tty
#     get") and auto-quit via timeout(1). NOTE: never `top -b` -- batch mode is
#     plain text, not a redraw. ---
: > monitor.raw
if command -v htop >/dev/null; then MON='htop'; else MON='top -d 1'; fi
TERM=xterm-256color script -q -c "timeout -s INT 4 $MON" monitor.raw </dev/null || true

echo "=== sizes ==="; wc -c vim.raw monitor.raw
echo "monitor program: $MON"
echo "=== BEGIN vim.raw.b64 ===";     base64 -w0 vim.raw;     echo
echo "=== BEGIN monitor.raw.b64 ==="; base64 -w0 monitor.raw; echo
