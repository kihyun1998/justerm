#!/usr/bin/env bash
#
# Capture for a shell's SIGWINCH prompt redraw across a one-call resize (#1082).
#
# WHAT IT RECORDS: interactive bash (`--norc --noprofile -i`) and zsh (`-f -i`) under a real PTY,
# each case one resize from a start width to an end width, then an Enter. A prompt one row long
# and one already wrapped at the start width, narrowed and widened, and once with a command typed
# but not run. Every byte is the shell's own.
#
# FORMAT (`resize_shell_redraw.raw`): a sequence of records, each a header line
# `<case> <cols> <len>\n` followed by exactly `<len>` bytes. Every case has three records in
# order: the start-up output (and the typed text's echo) at the start width, the bytes after a
# resize to the end width, and the bytes after an Enter at the end width. Rows stay 8 throughout.
#
# WHERE TO RUN: any Linux with bash 5.x, zsh and python3 — WSL Ubuntu-24.04 is enough
# (docs/agents/local-os-checks.md). Git Bash has no PTY.
#
# USAGE: capture-resize-shell-redraw.sh [output-dir]   (defaults to a fresh temp dir)
set -e
cd "${1:-$(mktemp -d)}"

python3 - <<'PY'
import fcntl, os, pty, select, struct, termios, time

ROWS = 8
P13 = "user@host:~$ "                      # 13 cells: one row at 30
P32 = "user@host:~/a/very/long/path/x$ "   # 32 cells: two rows at 30
P26 = "user@host:~/projects/app$ "         # 26 cells: the issue's 57 -> 21 shape
CASES = [
    # (case, shell, prompt, start cols, end cols, typed)
    ("bash13-30-10", "bash", P13, 30, 10, b""),
    ("bash32-30-10", "bash", P32, 30, 10, b""),
    ("bash32-30-40", "bash", P32, 30, 40, b""),
    ("bash13typed-30-10", "bash", P13, 30, 10, b"echo hello world"),
    ("bash26-57-21", "bash", P26, 57, 21, b""),
    ("zsh13-30-10", "zsh", P13, 30, 10, b""),
    ("zsh32-30-10", "zsh", P32, 30, 10, b""),
]

def winsz(fd, cols):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, cols, 0, 0))

def drain(fd, quiet=0.5):
    out = b""
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
for case, shell, prompt, c0, c1, typed in CASES:
    pid, fd = pty.fork()
    if pid == 0:
        env = {"PATH": "/usr/bin:/bin", "TERM": "xterm-256color", "PS1": prompt,
               "HOME": "/tmp", "LC_ALL": "C.UTF-8"}
        argv = ["bash", "--norc", "--noprofile", "-i"] if shell == "bash" else ["zsh", "-f", "-i"]
        os.execve("/usr/bin/" + shell, argv, env)
    winsz(fd, c0)
    time.sleep(0.3)
    start = drain(fd, 1.0)
    if typed:
        os.write(fd, typed)
        start += drain(fd)
    records.append((case, c0, start))
    winsz(fd, c1)  # the kernel sends SIGWINCH to the foreground process group
    records.append((case, c1, drain(fd)))
    os.write(fd, b"\r")
    records.append((case, c1, drain(fd)))
    os.write(fd, b"exit\n")
    drain(fd)

with open("resize_shell_redraw.raw", "wb") as f:
    for case, cols, data in records:
        f.write(b"%s %d %d\n" % (case.encode(), cols, len(data)))
        f.write(data)
print("wrote", os.path.abspath("resize_shell_redraw.raw"))
for case, cols, data in records:
    print(case, cols, repr(data))
PY
