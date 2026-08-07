#!/usr/bin/env bash
#
# Dogfood capture for OSC-133 SHELL INTEGRATION under a real `clear` (#750) — the
# combination none of the other captures can contain.
#
# WHY THIS EXISTS: every other recording in this directory is a full-screen
# application (vim / htop / top / neovim) or a pager (less). None of them emits an
# OSC 133 sequence at all, because shell integration is a property of the *shell's
# prompt*, not of the programs it launches. So the whole command-mark surface —
# `command_marks`, `command_lines`, and the mark lifetime #750 is about — had zero
# dogfood coverage, and the synthetic fixtures were the only evidence that the
# sequences even interleave the way they were written to assume.
#
# Three things this measured that the synthetic fixtures had guessed at, all three
# recorded in `tests/command_lines_capture.rs`:
#
#   1. a real `clear` emits `ESC[H ESC[2J ESC[3J` — ED 2 (handled) plus ED 3
#      (unimplemented), so the fix's dominant path is confirmed real;
#   2. `readline` emits a PARTIAL `ESC[K` at the cursor while the user edits — not
#      the `\r ESC[K` whole-row redraw an earlier draft of the fix's rationale
#      claimed. The claim was corrected against this recording;
#   3. bash's `DEBUG` trap fires `133;C` more than once per command, including
#      before the first prompt, so the B→C pairing walk meets stray `C` marks in
#      ordinary traffic.
#
# DETERMINISM: this recording is NOT byte-reproducible — it contains a real shell's
# timing and a `$?`-driven `PROMPT_COMMAND`. Unlike `capture-softwrap.sh` it cannot
# be used to certify the transfer path; re-record it only to extend the material,
# and re-derive the golden in `command_lines_capture.rs` when you do.
#
# RUN IT: on the RHEL VM, per `docs/agents/theflow.md` § "Recording a capture on
# the VM". `-tt` is mandatory and its absence is silent.
#
#   scp justerm-core/tests/fixtures/capture-osc133.sh justerm-vm:/tmp/
#   ssh -tt justerm-vm 'rm -rf /tmp/capout && mkdir /tmp/capout \
#       && bash /tmp/capture-osc133.sh /tmp/capout'
#   scp justerm-vm:/tmp/capout/'*.raw' justerm-core/tests/fixtures/
#
# `expect` is required (present on the box since 2026-08-03) — without it this
# writes a 0-byte file and says nothing.
set -u
out="${1:?usage: capture-osc133.sh <outdir>}"
mkdir -p "$out"
export TERM=xterm-256color
export LC_ALL=C.UTF-8

# A minimal OSC-133 integration, close to what starship / bash-preexec install:
# A before the prompt, B after it (so B is emitted BEFORE the user types — which is
# what makes readline's edit-time EL land between B and C), C at command start via
# the DEBUG trap, D with the exit code from PROMPT_COMMAND.
cat > /tmp/rc133 <<'RC'
PS1='\[\e]133;A\a\]$ \[\e]133;B\a\]'
PS2=''
trap 'printf "\033]133;C\007"' DEBUG
PROMPT_COMMAND='__e=$?; printf "\033]133;D;%s\007" "$__e"'
RC

cat > /tmp/drive.exp <<'EXP'
set timeout 20
spawn -noecho bash --noprofile --rcfile /tmp/rc133 -i
expect -re {\$ $}
# A command typed WITH a correction, so readline redraws the input line while the
# CommandStart mark is already standing on that row.
send "echoo hi"
sleep 0.3
send "\177\177\177\177"
sleep 0.3
send " one\r"
expect -re {\$ $}
send "echo two\r"
expect -re {\$ $}
# The verb under test.
send "clear\r"
expect -re {\$ $}
# And a command after it, which is what used to be reported twice.
send "echo three\r"
expect -re {\$ $}
send "exit\r"
expect eof
EXP

# 10x40 keeps the replay small enough to read by hand; print the size inside the
# same invocation, because an unsized pty fails silently.
script -q -c "stty rows 10 cols 40; stty size; expect -f /tmp/drive.exp" \
    "$out/osc133_clear.raw"
echo "bytes: $(wc -c < "$out/osc133_clear.raw")"
