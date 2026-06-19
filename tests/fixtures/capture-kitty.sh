#!/usr/bin/env bash
#
# Dogfood capture for #23 — record a real neovim session whose output stream
# carries the kitty keyboard-protocol negotiation, so justerm can be fed a
# genuine stream that enables/disables the protocol and verified to a stable
# grid + flag stack.
#
# WHERE TO RUN: on the Linux VM, inside a real terminal (SSH/PuTTY is fine —
# see the note below on why we force-enable). A non-interactive shell produces
# degenerate output and is useless.
#
# WHY FORCE-ENABLE: neovim only emits the kitty enable sequence (CSI > flags u)
# when it detects the terminal supports the protocol — via a query/response
# handshake the recording PTY (and PuTTY) does not answer. So this init makes
# neovim emit the push on entry and the pop on exit itself. The bytes are the
# real protocol format; only the *trigger* is forced. neovim's redraw output is
# unchanged (it enables kitty to receive richer *input*, not to change output).
#
# WHAT IT DOES: drives neovim with a SCRIPTED keystroke file (no human typing),
# records every byte neovim writes via script(1) into neovim.raw, prints base64
# to paste back here.
#
# NOTES:
#   - neovim on RHEL: `dnf install -y neovim` (EPEL). If missing, this aborts.
#   - 80x24 winsize is fixed so the capture is deterministic.
#
set -e
command -v nvim >/dev/null || { echo "ERROR: nvim not installed (dnf install -y neovim)" >&2; exit 1; }
cd "$(mktemp -d)"
stty rows 24 cols 80 || true

# --- force the kitty keyboard protocol push/pop into neovim's output stream ---
# CSI > 1 u  = push flags=1 (disambiguate) ; CSI < u = pop. Written raw so they
# land in the captured PTY stream regardless of terminal support.
cat > init.lua <<'LUA'
vim.api.nvim_create_autocmd("VimEnter",  { callback = function() io.stdout:write("\27[>1u"); io.stdout:flush() end })
vim.api.nvim_create_autocmd("VimLeave",  { callback = function() io.stdout:write("\27[<u");  io.stdout:flush() end })
LUA

# --- deterministic neovim driver: \033=ESC, \015=Enter(CR) ---
{
  printf 'iHello from justerm kitty dogfood.\015'
  printf 'A second line to edit.\015'
  printf 'Third line.\033'              # ESC: leave insert mode
  printf 'ggOtop insert\033'            # open line at top   -> IL redraw
  printf '2Gdd'                         # delete a line      -> DL redraw
  printf 'yyp'                          # duplicate a line
  printf '$a <end>\033'                 # append at line end -> ICH redraw
  printf '/line\015'                    # search (scroll/highlight)
  printf 'G'                            # jump to bottom
  printf ':q!\015'                      # quit without saving
} > keys.txt

: > out.txt
TERM=xterm-256color script -q -c 'nvim -u init.lua -i NONE -s keys.txt out.txt' neovim.raw </dev/null || true

echo "=== size ==="; wc -c neovim.raw
echo "=== sanity: kitty push/pop present? (expect a >1u and a <u) ==="
grep -ac $'\033\[>1u' neovim.raw && echo "  push found" || echo "  WARN: no push captured"
echo "=== BEGIN neovim.raw.b64 ==="; base64 -w0 neovim.raw; echo
