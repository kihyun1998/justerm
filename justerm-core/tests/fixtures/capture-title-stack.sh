#!/usr/bin/env bash
#
# Recorder for `fixtures/vim_title_stack.raw` — the XTWINOPS title-stack round
# trip a real `vim` emits (#823).
#
# WHERE TO RUN: on the RHEL 9 VM, under a real PTY (`ssh -tt justerm-vm`). A
# non-interactive shell produces degenerate output and is useless.
#
# WHY THE `--cmd 'set title'`: this is the one non-obvious part of the recipe,
# and leaving it out silently records the wrong thing. RHEL 9's
# `xterm-256color` terminfo has **no `tsl`/`fsl`** capability, so vim comes up
# with `title` off and `t_ts`/`t_fs` empty (`vim -e -s --cmd 'set title?
# t_ts?'` confirms it). It still pushes and pops the title stack — those are
# unconditional — but it never SETS a title, so the pop has nothing to restore
# and the round trip is invisible in the recording. Forcing the option gives
# the stream the shape a consumer with a title-capable TERM actually sees.
#
# Do NOT use `-u NONE`: that is why the repo's older `vim_redraw.raw` carries
# zero XTWINOPS.
#
# LC_ALL pins a UTF-8 locale and must stay one — plain C makes applications drop
# the Unicode output this engine exists to get right.
set -e
cd "$(mktemp -d)"
stty rows 24 cols 80 || true   # the winsize the TUI reads via ioctl

printf 'a\nb\n' > note.txt
{
  printf 'iabc\033'   # \033 = ESC, leave insert mode
  printf ':wq\015'    # \015 = CR
} > keys.txt

LC_ALL=C.UTF-8 TERM=xterm-256color \
  script -q -c "vim --cmd 'set title titlestring=PROBE' -s keys.txt note.txt" \
  vim_title_stack.raw </dev/null

echo "=== size ==="; wc -c vim_title_stack.raw
echo "=== the sequences this fixture exists for ==="
LC_ALL=C.UTF-8 grep -aobP '\x1b\][012];[^\x07\x1b]{0,40}|\x1b\[2[23][0-9;]*t' \
  vim_title_stack.raw | tr -d '\033'
echo "=== BEGIN vim_title_stack.raw.b64 ==="; base64 -w0 vim_title_stack.raw; echo
