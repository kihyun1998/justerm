#!/usr/bin/env bash
#
# Dogfood capture with the REPLY LOOP CLOSED (#891) — the class of sequence every
# other capture in this directory structurally cannot contain.
#
# WHY THIS EXISTS: every other `.raw` here was recorded under `script(1)` or a bare
# `expect`, which copy bytes and answer NOTHING. So a sequence an application sends
# only *after* the terminal replies to it can never appear, however common it is in
# real use. That is not a sampling gap to be fixed with more recordings — the corpus
# excludes the whole class by construction, and a count read out of it is a FLOOR.
#
# The phenomenon is known-detectable rather than merely suspected, which is what makes
# the absence evidence: #824 ran a control pair on the VM and measured that once DA2 is
# answered, vim follows up with ten XTGETTCAP questions --
#
#   DCS + q 2332 ST "#2"  DCS + q 2334 ST "#4"  DCS + q 2569 ST "%i"  DCS + q 2a37 ST "*7"
#   DCS + q 436f ST "Co"  DCS + q 6b31 ST "k1"  DCS + q 6b64 ST "kd"  DCS + q 6b6c ST "kl"
#   DCS + q 6b72 ST "kr"  DCS + q 6b75 ST "ku"
#
# -- and `DCS + q` occurs ZERO times across every open-loop fixture, four of which ask DA2.
#
# WHY THE REPLIES COME FROM THE ENGINE AND NOT FROM A TABLE: three of the six replies
# justerm queues are state-dependent -- DSR 6n is the cursor position, DECRQM is whichever
# of 27 mode flags was asked, and the kitty query is the flag stack. vim's two DSR 6n are
# not incidental: it prints U+25BD at a known cell and asks where the cursor ended up
# (ambiguous width), then throws an unknown DCS and an unknown CSI and asks again (parser
# conformance). A harness answering a fixed `1;1R` does not merely give a wrong number --
# it plays a terminal that drew nothing, which is a terminal that does not exist, and the
# capture is then a recording of vim talking to something that is not justerm.
#
# So the loop runs through `examples/reply_filter`, which IS the engine plus a consumer
# policy. It also has to be a consumer and not a pipe: `drain_replies()` alone answers
# DA1, DA2, DSR, DECRQM and the kitty query, but the four colour/clipboard query families
# reach a consumer as a `TermEvent` and are answered by policy (ADR-0017). A harness that
# forwarded only `drain_replies()` would leave those four silent, and vim asks two of them.
#
# WHAT THE POLICY IS, AND WHY IT IS ON THE COMMAND LINE: the recorded bytes are a function
# of the answers, so the fixture encodes a consumer policy whether or not anyone writes it
# down. Measured: handing vim a white background instead of a black one flips its own
# `&background` from `dark` to `light`. It is passed as arguments below, and named in the
# fixture's doc comment, so the choice is visible rather than compiled in.
#
# THE REPRODUCIBILITY GATE IS NOT OPTIONAL. A fixture that cannot be re-recorded byte for
# byte is not evidence of anything, and a closed loop does NOT guarantee it: measured on
# this VM, the black-background arm came back byte-identical 3/3 while the white-background
# arm did not (2837 vs 2691 bytes, CPR 5 vs 3). Nothing in a single recording tells you
# which of the two you are in, so this script records three times and refuses to emit a
# capture unless all three agree.
#
# WHERE TO RUN: the Linux VM, for consistency with every other capture here. It needs
# python3 (stdlib `pty` only) and vim; it does NOT need Rust -- see the build line below.
#
# BUILDING `reply_filter` WITHOUT INSTALLING ANYTHING ON THE VM (run on the dev machine):
#
#   rustup target add x86_64-unknown-linux-musl
#   CC_x86_64_unknown_linux_musl=clang \
#   CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
#     cargo build --release -p justerm-core --example reply_filter \
#                 --target x86_64-unknown-linux-musl
#   scp target/x86_64-unknown-linux-musl/release/examples/reply_filter <vm>:/tmp/
#
# The `CC_` line is needed because cargo builds justerm-core's dev-dependency graph for an
# example target and `alloca` (via criterion) has a C build script; the responder itself
# uses nothing but `std`. Any cross-capable C compiler will do, clang is merely the one
# already present. The result is a static-pie binary -- the VM gets a file, not a toolchain.
#
# USAGE: capture-closed-loop.sh <path-to-reply_filter> [output-dir]
set -euo pipefail

FILTER="${1:?usage: capture-closed-loop.sh <path-to-reply_filter> [output-dir]}"
OUTDIR="${2:-$(mktemp -d)}"
[ -x "$FILTER" ] || { echo "not executable: $FILTER" >&2; exit 1; }
command -v vim >/dev/null || { echo "vim missing" >&2; exit 1; }
command -v python3 >/dev/null || { echo "python3 missing" >&2; exit 1; }

cd "$OUTDIR"

COLS=80
ROWS=24
FG="rgb:c7c7/c7c7/c7c7"
BG="rgb:0000/0000/0000"

cat > harness.py <<'PY'
#!/usr/bin/env python3
"""Own the pty; let the engine do every bit of the VT thinking.

Each read from the pty master is handed to `reply_filter` as one length-prefixed
frame and the reply frame is written straight back. The exchange is synchronous, so
the harness never guesses whether a reply has finished arriving -- and it never has
to recognise a sequence, which is the part the previous throwaway got wrong (it
matched with regexes over a 64-byte sliding window and could answer twice).
"""
import fcntl
import os
import pty
import select
import struct
import subprocess
import sys
import termios
import time

FILTER, COLS, ROWS, FG, BG, OUT = sys.argv[1:7]
COLS, ROWS = int(COLS), int(ROWS)

PROBE = r"""
" vim sends t_RV (the DA2 query) from its MAIN LOOP, not during startup: a script that
" probes and quits inline never lets it happen, and the run then contains none of the
" material this capture exists for. Arm a timer, return, let the loop run.
function! s:Act(timer)
  normal! G
  normal! gg
  redraw!
  qa!
endfunction
call timer_start(1500, function('s:Act'))
"""
open("probe.vim", "w").write(PROBE)
open("sample.txt", "w").write("".join("line %02d\n" % i for i in range(1, 9)))

filt = subprocess.Popen(
    [FILTER, str(COLS), str(ROWS), FG, BG],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
)

pid, fd = pty.fork()
if pid == 0:
    os.environ["TERM"] = "xterm-256color"
    os.environ["LC_ALL"] = "C.UTF-8"
    # COLORTERM would hand vim truecolor for free and hide whatever it would otherwise
    # have had to ask for, which is the half this capture is about.
    os.environ.pop("COLORTERM", None)
    # `-i NONE` keeps this machine's viminfo out of a checked-in fixture. `-u NONE` is
    # deliberately NOT here: measured, it takes the capture from ten XTGETTCAP questions
    # to zero -- vim's terminal probing lives in the defaults it skips. It produced a
    # 3027-byte capture that reproduced 3/3 and contained none of the material this file
    # exists for, which is exactly what the inventory print below is for.
    os.execvp("vim", ["vim", "-X", "-i", "NONE", "-S", "probe.vim", "sample.txt"])

fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

raw = b""
deadline = time.time() + 30
while time.time() < deadline:
    r, _, _ = select.select([fd], [], [], 0.3)
    if not r:
        if os.waitpid(pid, os.WNOHANG)[0]:
            break
        continue
    try:
        chunk = os.read(fd, 65536)
    except OSError:
        break
    if not chunk:
        break
    raw += chunk

    filt.stdin.write(struct.pack("<I", len(chunk)) + chunk)
    filt.stdin.flush()
    n = struct.unpack("<I", filt.stdout.read(4))[0]
    if n:
        os.write(fd, filt.stdout.read(n))

try:
    os.waitpid(pid, 0)
except OSError:
    pass
os.close(fd)
filt.stdin.close()
filt.wait()

open(OUT, "wb").write(raw)
print("recorded %s: %d bytes" % (OUT, len(raw)))
PY

# --- record three times; agreement is the admission gate ---------------------
for i in 1 2 3; do
  python3 harness.py "$FILTER" "$COLS" "$ROWS" "$FG" "$BG" "run$i.raw"
done

if cmp -s run1.raw run2.raw && cmp -s run1.raw run3.raw; then
  cp run1.raw vim_closed_loop.raw
  echo "REPRODUCIBLE 3/3 -> $OUTDIR/vim_closed_loop.raw ($(wc -c < vim_closed_loop.raw) bytes)"
else
  echo "NOT REPRODUCIBLE — refusing to emit a fixture. Sizes:" >&2
  wc -c run1.raw run2.raw run3.raw >&2
  exit 1
fi

# --- say what actually landed, so a silent miss is not read as a result ------
python3 - <<'PY'
import re
b = open("vim_closed_loop.raw", "rb").read()
E = b"\x1b"
pats = {
    "DA2 CSI > c": E + rb"\[>c",
    "DSR 6n": E + rb"\[6n",
    "OSC 10/11 query": E + rb"\][0-9]+;\?",
    "XTGETTCAP DCS + q": E + rb"P\+q",
    "XTMODKEYS CSI > m": E + rb"\[>[0-9;]*m",
}
print("--- what this capture contains ---")
for name, p in pats.items():
    print("  %-20s %d" % (name, len(re.findall(p, b))))
caps = [m.group(1) for m in re.finditer(E + rb"P\+q([0-9a-f]+)", b)]
if caps:
    print("  XTGETTCAP names:",
          " ".join(bytes.fromhex(c.decode()).decode("latin1") for c in caps))
else:
    print("  NOTE: no XTGETTCAP — the loop did not reach the state this capture is for.")
PY
