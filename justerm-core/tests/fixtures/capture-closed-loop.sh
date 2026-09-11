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
# WHY THE REPLIES COME FROM THE ENGINE AND NOT FROM A TABLE: three of the seven replies
# justerm queues itself are state-dependent -- DSR 6n is the cursor position, DECRQM is
# whichever of 27 mode flags was asked, and the kitty query is the flag stack. vim's two DSR 6n are
# not incidental: it prints U+25BD at a known cell and asks where the cursor ended up
# (ambiguous width), then throws an unknown DCS and an unknown CSI and asks again (parser
# conformance). A harness answering a fixed `1;1R` does not merely give a wrong number --
# it plays a terminal that drew nothing, which is a terminal that does not exist, and the
# capture is then a recording of vim talking to something that is not justerm.
#
# So the loop runs through `examples/reply_filter`, which IS the engine plus a consumer
# policy. It also has to be a consumer and not a pipe. `drain_replies()` answers seven paths
# on its own -- DA1, DA2, DSR 6n, DSR 5n, DECRQM, the kitty flags query and the VT52 DECID
# `ESC Z` -- while SIX query families reach a consumer as a `TermEvent` and are answered by
# policy (ADR-0017): OSC 10, OSC 11, OSC 4, OSC 12, OSC 52, and the colour-scheme query. They
# are exactly the six `pub fn report_*` on the engine. A harness forwarding only replies would
# leave all six silent, and vim asks two of them in this very capture.
#
# WHAT THE POLICY IS, AND WHY IT IS ON THE COMMAND LINE: the recorded bytes are a function
# of the answers, so the fixture encodes a consumer policy whether or not anyone writes it
# down. Measured: handing vim a white background instead of a black one flips its own
# `&background` from `dark` to `light`. It is passed as arguments below, and named in the
# fixture's doc comment, so the choice is visible rather than compiled in.
#
# THE REPRODUCIBILITY GATE IS NOT OPTIONAL, and it is not decorative either. A fixture that
# cannot be re-recorded byte for byte is not evidence of anything, and a closed loop is where
# that stops being free: the stream now depends on our answers and on when they arrive.
#
# What buys it back is the SYNCHRONOUS FRAMING below -- vim blocks on the answer, so the
# recording is insensitive to how long the reply took. Measured here, by injecting a random
# delay in front of every reply: up to 250 ms of jitter leaves all three runs byte-identical,
# and at up to 3 s they come back 3274 / 3195 / 4961 bytes and this script refuses to emit a
# fixture. That refusal is the gate's positive control -- without it "3/3 identical" would
# only ever have meant "nothing has gone wrong yet".
#
# The earlier throwaway had no framing: it matched queries with regexes over a sliding window
# and answered whenever it noticed, and its light-background arm did NOT reproduce. The design
# here is the fix for that, not a precaution against a hypothetical.
#
# A REFUSAL HERE USUALLY MEANS "RUN IT AGAIN", AND THAT IS NOT A CLIMBDOWN. Measured over eight
# run-sets (24 recordings) on 2026-09-11: seven sets agreed 3/3 at 4951 bytes and one did not,
# and the odd recording out differed from its two siblings ONLY in how many times vim redrew its
# ruler on row 24 (plus 11 `CSI 1;1H`) -- 174 bytes of idle repaint. Every count this fixture
# exists for was identical in all three: DA2 1, DSR 6n 2, XTGETTCAP 10. So vim's repaint cadence
# is the residual nondeterminism, the reply-gated conversation is not, and the gate cannot tell
# the two apart -- which is the right way round for it to be wrong.
#
# Do NOT try to suppress the ruler with `--cmd "set noruler"`: measured over six runs, the
# output is byte-identical with and without it -- a fix-shaped no-op, which is worse than
# leaving it alone. *Why* it is a no-op is NOT established: something later in startup puts
# the option back, and `defaults.vim` is the obvious suspect rather than a verified one. The
# measurement is the part to trust here; the sibling comment about `-u NONE` above is there
# because that one's obvious suspect turned out to be the wrong mechanism.
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

# The fixture's doc-comment names a vim version, and a re-recording on another one silently
# makes that provenance false. Print it, like `capture-clipboard.sh` prints `tmux -V`.
echo "=== environment ==="
# Read it whole and trim, rather than `vim --version | head -1`: under `set -o pipefail` the
# closed pipe kills vim with SIGPIPE and takes the script with it (exit 141). The sibling
# capture scripts do pipe into `head` and are fine only because they do not set pipefail.
vim_version=$(vim --version)
echo "${vim_version%%$'\n'*}"
echo "TERM=$TERM (the capture is taken with TERM=xterm-256color, set by the harness)"

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
import random
import select
import struct
import subprocess
import sys
import termios
import time

FILTER, COLS, ROWS, FG, BG, OUT = sys.argv[1:7]
COLS, ROWS = int(COLS), int(ROWS)
JITTER = float(os.environ.get("CLOSED_LOOP_JITTER", "0"))

PROBE = r"""
" vim sends t_RV (the DA2 query) from its MAIN LOOP, not during startup: a script that
" probes and quits inline never lets it happen, and the run then contains none of the
" material this capture exists for. Arm a timer, return, let the loop run.
"
" 1500ms is a BUDGET for the whole probe exchange, not a pause -- vim waits on six replies
" here (DA2, DSR 6n x2, DECRQM, OSC 10, OSC 11), so it is what the jitter experiment is
" really testing: 6 x 250ms lands exactly on it, which is why 250ms still reproduces and 3s
" does not. Raising it would buy margin and cost determinism, because the extra idle time is
" spent redrawing the ruler, which is the one thing measured to vary between runs.
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
    # `-i NONE` keeps this machine's viminfo out of a checked-in fixture.
    #
    # `-u NONE` is deliberately NOT here, and the reason is not the one it looks like.
    # Measured on this VM, same harness, only the flags moving:
    #
    #   -X -i NONE            4951 B   DA2 1   DSR 6n 2   OSC? 2   XTGETTCAP 10
    #   -X -u NONE -i NONE    3027 B   DA2 0   DSR 6n 0   OSC? 0   XTGETTCAP  0
    #   -X -u NONE -N -i NONE 4711 B   DA2 1   DSR 6n 2   OSC? 2   XTGETTCAP 10
    #
    # So it is not that `-u NONE` skips `defaults.vim`: it is that `-u NONE` implies
    # 'compatible', under which vim does not probe the terminal AT ALL -- it asks nothing,
    # not merely the follow-ups -- and `-N` puts that back. Worth having straight, because
    # `vim_redraw.raw` is recorded `vim -u NONE -N` (`capture-dogfood.sh`), so its zero
    # XTGETTCAP is attributable to the open loop and not to its flags. It is a clean control
    # and the row above is the measurement that says so.
    #
    # The middle row is also why the inventory below EXITS rather than prints: it reproduced
    # 3/3 and contained none of the material this file exists for.
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

    # JITTER is the gate's positive control and is checked in for that reason: a gate that
    # has never refused anything is indistinguishable from one that cannot. Set
    # CLOSED_LOOP_JITTER=3 and this script refuses (measured: 3274 / 3195 / 4961 bytes).
    if JITTER:
        time.sleep(random.uniform(0, JITTER))
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

# --- the second gate: a REPRODUCIBLE capture can still be an empty one -------
# These are different failures and only one of them is about determinism. The `-u NONE`
# run above reproduced 3/3 and held none of the reply-gated material, so agreement alone
# would have admitted it. This exits non-zero and removes the file rather than printing a
# note next to a fixture that is already on disk.
python3 - <<'PY' || { echo "removing the capture that did not earn its place" >&2; rm -f vim_closed_loop.raw; exit 1; }
import re
import sys

b = open("vim_closed_loop.raw", "rb").read()
E = b"\x1b"
counts = {
    "DA2 CSI > c": len(re.findall(E + rb"\[>c", b)),
    "DSR 6n": len(re.findall(E + rb"\[6n", b)),
    "OSC 10/11 query": len(re.findall(E + rb"\][0-9]+;\?", b)),
    "XTGETTCAP DCS + q": len(re.findall(E + rb"P\+q", b)),
    "XTMODKEYS CSI > m": len(re.findall(E + rb"\[>[0-9;]*m", b)),
}
print("--- what this capture contains ---")
for name, n in counts.items():
    print("  %-20s %d" % (name, n))
caps = [bytes.fromhex(m.group(1).decode()).decode("latin1")
        for m in re.finditer(E + rb"P\+q([0-9a-f]+)", b)]
print("  XTGETTCAP names:", " ".join(caps) if caps else "(none)")

# The whole point of a closed loop is the reply-gated class. Without it this is an
# ordinary open-loop capture that happened to cost more to record.
if not caps:
    sys.exit("REFUSING: no XTGETTCAP — the loop never reached the state this capture is for")
if counts["DA2 CSI > c"] < 1 or counts["DSR 6n"] < 1 or counts["OSC 10/11 query"] < 1:
    sys.exit("REFUSING: a query family this capture is supposed to exercise is missing")
PY
