#!/usr/bin/env bash
#
# Dogfood capture for issue #20 — record real terminal byte streams (a vim edit
# session + live-monitor TUIs: top and htop) as frozen golden fixtures.
#
# WHERE TO RUN: on a Linux box, inside a REAL terminal session.
#   - SSH in from Windows via PuTTY / Windows Terminal / ssh  (recommended:
#     pasting the base64 output back is easiest), OR
#   - run directly in the VM console window.
#   What matters: the TUIs must run on Linux under a real PTY. A non-interactive
#   shell (e.g. an agent's tool shell) produces degenerate output and is useless.
#
# WHAT IT DOES: drives vim with a SCRIPTED keystroke file (no human typing) and
# runs each live monitor in the FOREGROUND for a few seconds, recording every
# byte written to the terminal into *.raw via script(1), then prints each as
# base64 to paste back.
#
# WHY base64: CI never re-runs these apps. We freeze ONE capture's raw bytes and
# feed() them; frozen input -> deterministic snapshot, so a monitor's live
# system state cannot cause flakiness.
#
# NOTES:
#   - htop needs EPEL on RHEL (dnf install epel-release htop). If it is missing
#     its capture is skipped (empty *.raw) and the rest still runs.
#   - A full-screen TUI must own the controlling tty: run it in the FOREGROUND
#     via timeout(1). Backgrounding it (`top &`) yields "failed tty get".
#   - Never `top -b` -- batch mode is plain text, not a redraw.
#
set -e
cd "$(mktemp -d)"
stty rows 24 cols 80 || true   # fix PTY winsize (the value the TUIs read via ioctl)

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

# Capture a foreground TUI for ~4s, or leave an empty *.raw if it is not installed.
#   $1 = output basename (-> $1.raw)   $2 = command line (first word = binary)
capture_tui() {
  local out="$1.raw" cmd="$2"
  : > "$out"
  if command -v "${cmd%% *}" >/dev/null; then
    TERM=xterm-256color script -q -c "timeout -s INT 4 $cmd" "$out" </dev/null || true
  else
    echo "NOTE: ${cmd%% *} not installed -> $out left empty" >&2
  fi
}

capture_tui top  'top -d 1'
capture_tui htop 'htop'

echo "=== sizes ==="; wc -c vim.raw top.raw htop.raw
for f in vim top htop; do
  echo "=== BEGIN $f.raw.b64 ==="; base64 -w0 "$f.raw"; echo
done
