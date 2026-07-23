#!/usr/bin/env bash
#
# Dogfood capture for SGR 58/59 — coloured & styled underlines (the "undercurl"
# family). Records a real byte stream that carries SGR 58 (set underline colour)
# in EVERY encoding form, so justerm-core can be fed a genuine stream and
# verified to a stable grid + per-cell underline-colour state.
#
# WHERE TO RUN: on the Linux VM, inside a real terminal (SSH/PuTTY is fine).
#
# WHY TWO CAPTURES:
#   1. undercurl_matrix.raw  — a DETERMINISTIC printf of every SGR 58 form. This
#      is the source of truth: the bytes ARE what a terminal receives, and it
#      covers the sub-parameter matrix a real app would only emit one corner of.
#   2. undercurl_nvim.raw    — BEST EFFORT real-app (neovim spell). neovim only
#      emits SGR 58 when it believes the terminal supports styled underlines
#      (terminfo Su/Smulx), which a recording PTY lacks — so we force it via the
#      &t_ terminal-code overrides. On some builds this still emits nothing; the
#      sanity grep below tells you. The matrix capture stands on its own.
#
# THE FORM MATRIX (why each line exists — this is the parser's hidden-state list):
#   CSI 58:2::R:G:B m   RGB, colon sub-params, EMPTY colour-space field (kitty/nvim)
#   CSI 58:2:C:R:G:B m  RGB, colon, WITH a colour-space id C (some emitters)
#   CSI 58:5:N m        indexed (256-colour), colon
#   CSI 58;2;R;G;B m    RGB, legacy SEMICOLON sub-params
#   CSI 58;5;N m        indexed, legacy semicolon
#   CSI 59 m            reset underline colour to default (follows the fg)
#   CSI 4:3 m           curly (undercurl); 4:1 straight, 4:2 double, 4:4 dotted, 4:5 dashed
#   CSI 4 m / CSI 24 m  legacy underline on / off
# The colour is INDEPENDENT of the underline style and of the fg: SGR 58 sets a
# colour that only the underline uses (renderer #513 `line_fg` is where it lands).
#
set -e
cd "$(mktemp -d)"
stty rows 24 cols 80 2>/dev/null || true
E=$'\033'  # ESC
N=$'\r\n'  # CRLF — a real byte pair, not the literal "\r\n" that a %s argument keeps verbatim

# --- 1. deterministic matrix (guaranteed) ------------------------------------
{
  printf '%s' "${E}[2J${E}[H"                       # clear + home
  # RGB, colon, empty colour-space, curly underline — the nvim/kitty spell form.
  printf '%s' "${E}[4:3m${E}[58:2::255:0:0mmisspeled${E}[59m${E}[4:0m  (58:2:: rgb curly)${N}"
  # RGB, colon, WITH colour-space id 1, double underline.
  printf '%s' "${E}[4:2m${E}[58:2:1:0:200:0mwarnign${E}[59m${E}[4:0m  (58:2:C rgb double)${N}"
  # Indexed (bright red = 9), colon, straight underline.
  printf '%s' "${E}[4:1m${E}[58:5:9meror${E}[59m${E}[24m  (58:5 indexed straight)${N}"
  # Legacy SEMICOLON RGB, plain underline.
  printf '%s' "${E}[4m${E}[58;2;0;128;255mhyperlink${E}[59m${E}[24m  (58;2 rgb semicolon)${N}"
  # Legacy SEMICOLON indexed (green 46), dotted underline.
  printf '%s' "${E}[4:4m${E}[58;5;46mhint${E}[59m${E}[4:0m  (58;5 indexed semicolon)${N}"
  # Colour set but NO underline attribute — must be inert until an underline turns on,
  # then re-used; proves the colour is state, not tied to the 4m that happened to precede it.
  printf '%s' "${E}[58:2::128:64:255mno-underline-here${E}[4:5mthen-dashed${E}[59m${E}[4:0m  (deferred colour)${N}"
  # fg and underline colour DIFFER on one run — the whole point of SGR 58.
  printf '%s' "${E}[38;2;255;255;255m${E}[4:3m${E}[58:2::255:0:0mwhite text, red curl${E}[0m${N}"
  printf '%s' "${E}[0m"                             # full SGR reset
} > undercurl_matrix.raw

# --- 2. best-effort real neovim (spell → SpellBad undercurl) ------------------
if command -v nvim >/dev/null; then
  cat > init.lua <<'LUA'
-- Force neovim to EMIT styled + coloured underlines regardless of terminfo, the
-- same way capture-kitty forces the kitty push. These &t_ overrides make the
-- redraw stream carry the real SGR 4:3 / 58:2 bytes.
vim.o.termguicolors = true
vim.cmd([[let &t_Cs = "\e[4:3m"]])            -- undercurl start
vim.cmd([[let &t_Ce = "\e[4:0m"]])            -- underline end
vim.cmd([[let &t_8u = "\e[58:2::%lu:%lu:%lum"]]) -- set underline colour (RGB)
vim.cmd([[hi SpellBad gui=undercurl guisp=#ff0000]])
vim.cmd([[hi SpellCap gui=undercurl guisp=#ffaa00]])
vim.o.spell = true
vim.o.spelllang = "en"
LUA
  printf 'iThisss lien has mispelled wordz and lowercase paris.\033' > keys.txt  # \033=ESC leaves insert
  printf ':redraw!\015' >> keys.txt                                              # \015=Enter forces a repaint
  printf ':q!\015'      >> keys.txt
  TERM=xterm-256color script -q -c 'nvim -u init.lua -i NONE -s keys.txt' undercurl_nvim.raw </dev/null || true
fi

echo "=== sizes ==="; wc -c undercurl_matrix.raw undercurl_nvim.raw 2>/dev/null
echo "=== sanity: SGR 58 present? (expect >=7 in the matrix; nvim best-effort) ==="
for f in undercurl_matrix.raw undercurl_nvim.raw; do
  [ -f "$f" ] || continue
  # -o one match per line, then count lines — `grep -c` counts matching LINES, not matches.
  c=$(grep -ao $'\033\[58' "$f" 2>/dev/null | wc -l | tr -d ' ')
  echo "  $f: ${c:-0} occurrences of CSI 58"
done
echo "=== BEGIN undercurl_matrix.raw.b64 ==="; base64 -w0 undercurl_matrix.raw; echo
if [ -f undercurl_nvim.raw ]; then
  echo "=== BEGIN undercurl_nvim.raw.b64 (best effort — may be empty of 58) ==="
  base64 -w0 undercurl_nvim.raw; echo
fi
