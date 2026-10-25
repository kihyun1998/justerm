#!/usr/bin/env bash
#
# Capture for a shell's SIGWINCH prompt redraw across a narrowing drag (#1076).
#
# WHAT IT RECORDS: an interactive `bash --norc --noprofile -i` under a real PTY,
# with a 27-cell prompt (26 visible characters and the trailing space), resized
# one column at a time from 30 down to 20. Width 27 is the one where the prompt
# fills the row exactly, so the cursor parks in the deferred wrap; the step to 26
# is the one #1076 is about. Every byte is bash's own redraw.
#
# FORMAT (`resize_prompt.raw`): a sequence of records, each a header line
# `<cols> <len>\n` followed by exactly `<len>` bytes. The first record is the
# start-up output at its width; every later record means "resize to <cols>, then
# feed these bytes". Rows stay 6 throughout.
#
# WHERE TO RUN: any Linux with bash 5.x and python3 — WSL Ubuntu-24.04 is enough
# (docs/agents/local-os-checks.md). Git Bash has no PTY.
#
# USAGE: capture-resize-prompt.sh [output-dir]   (defaults to a fresh temp dir)
set -e
cd "${1:-$(mktemp -d)}"

python3 - <<'PY'
import fcntl, os, pty, select, struct, termios, time

PS1 = "abcdefghijklmnopqrstuvwxyz "  # 26 visible characters + the trailing space
ROWS = 6

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

pid, fd = pty.fork()
if pid == 0:
    env = {"PATH": "/usr/bin:/bin", "TERM": "xterm-256color", "PS1": PS1,
           "HOME": "/tmp", "LC_ALL": "C.UTF-8"}
    os.execve("/bin/bash", ["bash", "--norc", "--noprofile", "-i"], env)

winsz(fd, 30)
time.sleep(0.3)
records = [(30, drain(fd))]
for cols in range(29, 19, -1):
    winsz(fd, cols)  # the kernel sends SIGWINCH to the foreground process group
    records.append((cols, drain(fd)))
os.write(fd, b"exit\n")
drain(fd)

with open("resize_prompt.raw", "wb") as f:
    for cols, data in records:
        f.write(b"%d %d\n" % (cols, len(data)))
        f.write(data)
print("wrote", os.path.abspath("resize_prompt.raw"))
for cols, data in records:
    print(cols, repr(data))
PY
