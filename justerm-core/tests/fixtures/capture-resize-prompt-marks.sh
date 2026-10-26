#!/usr/bin/env bash
#
# Capture for a shell's SIGWINCH redraw of an OSC 133-marked prompt (#1083).
#
# WHAT IT RECORDS: interactive bash (`--noprofile --rcfile`) and zsh (`ZDOTDIR`) under a real PTY,
# each with PenTerm's shell-integration marks (bash: `PROMPT_COMMAND` prints `133;D` + `133;A`, PS1
# ends in `133;B`, PS0 prints `133;C`; zsh: precmd `D`, preexec `C`, PS1 wrapped in `A … B`). The
# `A` may carry an option (`;redraw=1`). Each case is a start-up, then actions: a resize, typed
# bytes, or Ctrl-L. Every byte is the shell's own.
#
# FORMAT (`resize_prompt_marks.raw`): a sequence of records, each a header line
# `<case> <op> <cols> <len>\n` followed by exactly `<len>` bytes, `op` being `start`, `resize` or
# `type`. `start` is the start-up output at its width; `resize` means "resize to <cols>, then feed
# these bytes"; `type` means "feed these bytes" (what the shell echoed). Rows stay 12 throughout.
#
# WHERE TO RUN: any Linux with bash 5.x, zsh, python3 — WSL Ubuntu-24.04 is enough
# (docs/agents/local-os-checks.md). Git Bash has no PTY.
#
# USAGE: capture-resize-prompt-marks.sh [output-dir]   (defaults to a fresh temp dir)
set -e
cd "${1:-$(mktemp -d)}"

python3 - <<'PY'
import fcntl, os, pty, select, struct, termios, time, tempfile

ROWS = 12
WORK = tempfile.mkdtemp()
BASHRC = r'''
PS1="$REC_PS1"
__penterm_prompt() { local ec=$?; printf '\e]133;D;%s\a\e]133;A%s\a' "$ec" "$REC_A_OPT"; return $ec; }
PS1="$PS1"'\[\e]133;B\a\]'
PS0='\[\e]133;C\a\]'"${PS0:-}"
PROMPT_COMMAND='__penterm_prompt'
'''
ZSHRC = r'''
PS1="$REC_PS1"
autoload -Uz add-zsh-hook
__penterm_preexec() { print -rn -- $'\e]133;C\a'; }
__penterm_precmd() { local ec=$?; print -rn -- $'\e]133;D;'"$ec"$'\a'; return $ec; }
add-zsh-hook preexec __penterm_preexec
add-zsh-hook precmd __penterm_precmd
PS1="%{"$'\e]133;A'"$REC_A_OPT"$'\a'"%}$PS1%{"$'\e]133;B\a'"%}"
'''
open(os.path.join(WORK, 'bashrc'), 'w').write(BASHRC)
os.makedirs(os.path.join(WORK, 'zdot'))
open(os.path.join(WORK, 'zdot', '.zshrc'), 'w').write(ZSHRC)

P13 = 'user@host:~$ '
P32 = 'user@host:~/a/very/long/path/x$ '
TWO = 'top line here\nuser@host:~$ '
CR = b'\r'
READ_BASH = b"read -p 'Name of the thing please: ' x" + CR
PY = b'python3 -q' + CR
PY_LINE = b'print("hello world from the repl")'
CASES = [
    # (case, shell, A option, PS1, start cols, actions)
    ('bash-13-narrow', 'bash', '', P13, 30, [('resize', 10)]),
    ('bash-two-narrow', 'bash', '', TWO, 30, [('resize', 10)]),
    ('bash-typed-narrow', 'bash', '', P13, 30, [('type', b'echo hello world'), ('resize', 10)]),
    ('bash-32-widen', 'bash', '', P32, 30, [('resize', 40)]),
    ('bash-steps', 'bash', '', P13, 15, [('resize', 14), ('resize', 13), ('resize', 12), ('resize', 11), ('resize', 10)]),
    ('bash-readp', 'bash', '', P13, 30, [('type', READ_BASH), ('type', b'abc'), ('resize', 10), ('type', b'def')]),
    ('bash-ctrll', 'bash', ';redraw=1', P13, 30,
     [('type', b'echo ' + b'W' * 28 + b'; seq 3' + CR), ('type', b'\x0c'), ('resize', 10)]),
    ('zsh-two-narrow', 'zsh', '', TWO, 30, [('resize', 10)]),
    ('zsh1-13-narrow', 'zsh', ';redraw=1', P13, 30, [('resize', 10)]),
    ('zsh1-two-narrow', 'zsh', ';redraw=1', TWO, 30, [('resize', 10)]),
    ('zsh1-32-widen', 'zsh', ';redraw=1', P32, 30, [('resize', 40)]),
    ('zsh1-python-narrow', 'zsh', ';redraw=1', P13, 40, [('type', PY), ('type', PY_LINE), ('resize', 12)]),
    ('zsh1-python-widen', 'zsh', ';redraw=1', P13, 20, [('type', PY), ('type', PY_LINE), ('resize', 40)]),
]

def winsz(fd, cols):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', ROWS, cols, 0, 0))

def drain(fd, quiet=0.5):
    out = b''
    while True:
        r, _, _ = select.select([fd], [], [], quiet)
        if not r:
            return out
        try:
            chunk = os.read(fd, 4096)
        except OSError:
            return out
        if not chunk:
            return out
        out += chunk

records = []
for case, shell, opt, ps1, cols, actions in CASES:
    pid, fd = pty.fork()
    if pid == 0:
        env = {'TERM': 'xterm-256color', 'PATH': '/usr/bin:/bin', 'HOME': WORK, 'LC_ALL': 'C.UTF-8',
               'REC_PS1': ps1, 'REC_A_OPT': opt}
        if shell == 'bash':
            os.execve('/usr/bin/bash', ['bash', '--noprofile', '--rcfile', os.path.join(WORK, 'bashrc'), '-i'], env)
        env['ZDOTDIR'] = os.path.join(WORK, 'zdot')
        os.execve('/usr/bin/zsh', ['zsh', '-i'], env)
    winsz(fd, cols)
    time.sleep(0.3)
    records.append((case, 'start', cols, drain(fd, 1.5)))
    for op, arg in actions:
        if op == 'resize':
            cols = arg
            winsz(fd, cols)  # the kernel sends SIGWINCH to the foreground process group
        else:
            os.write(fd, arg)
        records.append((case, op, cols, drain(fd)))
    os.kill(pid, 9)

with open('resize_prompt_marks.raw', 'wb') as f:
    for case, op, cols, data in records:
        f.write(b'%s %s %d %d\n' % (case.encode(), op.encode(), cols, len(data)))
        f.write(data)
print('wrote', os.path.abspath('resize_prompt_marks.raw'))
for case, op, cols, data in records:
    print(case, op, cols, repr(data)[:140])
PY
