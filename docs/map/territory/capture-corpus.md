# Territory — the capture corpus

## What it is

The recorded VT streams under `justerm-core/tests/fixtures/` (`*.raw`), the goldens taken from them
(`*.golden`), and the `capture-*.sh` scripts that record them. It is **material, not test source** —
and it is also an **instrument**: this repo decides VT priorities by counts read out of it, so what
the corpus *cannot* contain is as load-bearing as what it does.

Before #891 the instrument had almost no design of its own, because every recorder was a byte copier.
Closing the reply loop gave it one, and that design had nowhere to live except
[VT interpretation](vt-interpretation.md), which is about how the engine *reads* a stream rather than
how this repo *records* one.

## Governing decisions

- [ADR-0030 — which directory owns what](../../adr/0030-which-directory-owns-what.md) — puts recorded
  material and its `capture-*.sh` in `tests/fixtures/`, and chooses **one topic file per behaviour**
  over a ref harness. That second row is why a capture here is consumed by named tests rather than
  graded as a whole: a ref harness records that a capture matched, never *which behaviour* it was
- [ADR-0017 — mechanism in core, policy in the consumer](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  — not written about captures, and it decides the closed-loop one anyway. Six query families are
  answered by the consumer, so a recording made against a consumer **encodes that consumer's policy**

## Design model

- **Three kinds of recorder, and they are not interchangeable.** A *deterministic printf* is the
  bytes a terminal receives, reproducible anywhere (`softwrap_shifts`, the undercurl matrix). A *real
  application under a byte copier* — `script(1)` or a bare `expect` — is every other capture; the
  application's bytes are its own, and nothing answers it. A *closed loop* is one capture
  (`vim_closed_loop.raw`, #891), recorded through a consumer that answers with this engine. Read a
  capture's doc comment for which it is before reading a claim out of it.
- **A count read out of the corpus is a floor, and only true of one revision.** Floor, because the
  byte copiers answer nothing, so a sequence an application sends only *after* a reply is excluded
  by construction rather than under-sampled. Revision-specific, because captures get added: the
  worked case is in [VT interpretation](vt-interpretation.md), where a `CSI > q` count changed sign
  between two days. Re-measure; do not cite.
- **The absence was known-detectable, which is what made it evidence.** #824 measured that once DA2
  is answered, vim follows with ten XTGETTCAP questions; across every open-loop capture that asks DA2
  the count is zero. `justerm-core/tests/closed_loop_capture.rs` holds both halves side by side,
  because ten means nothing without the zero next to it.
- **The open-loop control is clean, and the reason is not the obvious one.** `vim_redraw.raw` is
  recorded `vim -u NONE -N`, and `-u NONE` alone looks like a confound. Measured through the closed
  loop, flags as the only variable: `-X -i NONE` → 10 XTGETTCAP; `-X -u NONE -i NONE` → **0, and no
  DA2 either**; `-X -u NONE -N -i NONE` → 10. `-u NONE` implies *compatible*, a compatible vim probes
  nothing at all, and `-N` restores it. So a capture that asks DA2 was taken from a nocompatible vim,
  which is exactly the property the control needs — `alt_resize_vim.pre.raw` qualifies on its bytes
  even though its recipe was never written down.
- **A closed loop answers with the engine, never with a table.** Three of the seven replies the
  engine queues itself are state-dependent (DSR 6n, DECRQM, the kitty flags query), and vim's two DSR
  6n are *probes*: it prints U+25BD at a known cell and reads the column back for the ambiguous width,
  then throws an unknown DCS and CSI and reads it again. The engine answers `2;2R` and `3;1R`; a
  fixed `1;1R` would be a terminal that drew nothing, and the recording would be of a conversation
  with something else. The #824 throwaway answered from a table and a sliding regex window.
- **It is a consumer, not a pipe.** `drain_replies()` answers seven paths; six query families reach
  a consumer as a `TermEvent` and are answered by policy — exactly the six `report_*` methods. A
  harness that forwarded replies only would leave all six silent, and vim asks two of them.
- **So the bytes are a function of the policy, and the policy is recorded.** Handing vim a white
  background instead of a black one flips its own `&background` to `light`. The policy is passed on
  the command line in `capture-closed-loop.sh` and named in the test's doc comment, rather than
  compiled in where nobody would see which one a fixture was taken under.
- **Two admission gates, and they catch disjoint failures.** *Reproducibility*: record three times,
  refuse unless byte-identical — its positive control is checked in (`CLOSED_LOOP_JITTER=3` comes
  back 3274 / 3195 / 4961 bytes and is refused). *Content*: refuse a capture that holds none of the
  reply-gated material — its positive control is the `-u NONE` run, which reproduced 3/3 and held
  nothing. Either gate alone admits what the other refuses.
- **Synchronous framing is what makes a closed loop reproducible at all.** Each pty read is one
  length-prefixed frame and its reply one frame back, so vim blocks on the answer and the timing of
  the reply drops out. The 1500 ms timer in the probe is a budget for that whole exchange: six
  replies at 250 ms of injected jitter lands exactly on it, which is why 250 ms still reproduces and
  3 s does not. **Raising it buys margin and costs determinism** — the extra idle time is spent
  redrawing the ruler, which is the one thing measured to vary between runs (one run-set in eight,
  174 bytes, every reply-gated count identical). A refusal from the reproducibility gate therefore
  usually means *run it again*.
- **A capture test states what it can and cannot observe.** The capture tests carry the same table
  and open with the same sentence — *a capture that cannot fail reads as coverage while proving
  nothing*. The sharpest instance: a test that constructs no `Engine` guards the corpus, not the code,
  and no change under `src/` can redden it. The fixture itself goes through `redden` like an
  assertion — pointing the constant at an open-loop capture must turn the test red.
- **The DA2 version is re-derived in each capture test, on purpose.** A third independent copy of
  the arithmetic rather than a shared helper, because what those files are for is disagreeing with
  the engine, and a derivation imported from it could not.
- **Adding a capture moves every test that globs the directory.** `span_bounds.rs` replays every
  `.raw` and holds a floor rather than a count for exactly that reason — it read 255 frames before
  #891 and 321 after, and a pinned count would have broken on the addition instead of on a defect.
- **An inventory golden pins what each capture does nothing with (#895).** Every other capture test
  pins what a stream does, so a handler whose effect reaches none of their goldens could stop firing
  with the suite green. `ignored_inventory.rs` replays each capture once whole and once per sequence
  kind with that kind removed, and `<capture>.ignored.golden` records which consumer surfaces moved.
  Three things make its `-` a measurement rather than a shrug. **A positive control per surface**:
  every surface must move for some kind somewhere in the corpus, or it is a broken instrument. The
  mutation that proves it drops the mouse surface and regenerates the goldens; they go green, and
  the control goes red. **A scanner guard**: a token that runs long swallows the next sequence and
  manufactures a false effect, measured when the DCS branch was made to ignore ST. **A kind keeps a
  parameter where the parameter names a function** (`22t` pushes a title and `23t` pops one), and
  drops it where it is a quantity.
- **The inventory's verdict is end-state only, and that was the maintainer's scope call.** `-` means
  the state at the end of the stream is identical without the kind, not that the engine ignores it:
  a mode set and reset before the cut reads `-` (htop's `?1006;1000h`, vim's `>4m`), as does state
  with no getter that nothing later exercises (tab stops, the saved cursor, charsets). Comparing the
  trajectory would see those; it was shown with its cost and left out of #895. The list of captures is
  hand-kept rather than globbed, so adding a capture is a deliberate entry there and not a silent
  change of what is pinned.
- **`.raw` is binary to git.** A line-ending conversion rewrites the CR/LF bytes inside a stream and
  every replay built on it breaks (#20); `.gitattributes` pins it by extension so the rule survives
  crate moves.

## Code

- `justerm-core/tests/fixtures/` — the corpus. `capture-closed-loop.sh` is the closed-loop recorder
  and embeds its pty harness; `capture-dogfood.sh`, `capture-softwrap.sh`, `capture-title-stack.sh`,
  `capture-clipboard.sh`, `capture-cursor-color.sh`, `capture-kitty.sh`, `capture-undercurl.sh`,
  `capture-hyperlink.sh`, `capture-osc133.sh` and `capture-written-space.sh` are the rest — open
  loop, and two of them (`capture-softwrap.sh`, `capture-undercurl.sh`) also emit a deterministic
  printf alongside the real application
- `justerm-core/examples/reply_filter.rs` — the consumer the closed loop runs through: `Engine`
  plus the policy answers for every `TermEvent` query, and a loud report for a variant added after
  it was written (`TermEvent` is non-exhaustive, so the compiler cannot announce one)
- `justerm-core/tests/closed_loop_capture.rs` — the two sides of the open-loop gap, and the replay of
  the reply half
- `justerm-core/tests/title_stack_capture.rs` · `justerm-core/tests/clipboard_capture.rs` ·
  `justerm-core/tests/cursor_color_capture.rs` — sibling capture tests, and the source of the
  can-and-cannot-observe convention
- `justerm-core/tests/span_bounds.rs` — the directory-globbing replay whose frame floor moves with
  the corpus
- `justerm-core/tests/ignored_inventory.rs` and `tests/fixtures/*.ignored.golden` — the per-kind
  differential replay, its surface controls and its scanner guard
- `justerm-core/src/term.rs` — `drain_replies` and the `report_*` methods are the two halves a closed
  loop has to drive

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated**.

- [How a recorded stream is taken, and what its replay can see of the reply half](../../agents/reference-facts.md#how-a-recorded-stream-is-taken-and-what-its-replay-can-see-of-the-reply-half-891-verified-2026-09-14)
  — which reference recordings are closed-loop by construction, and that neither replay records or
  asserts a reply

## Cross-cutting invariants

**None.** The reproducibility discipline looks like one and is not yet: the deterministic-printf
versus real-application split already lived in `capture-softwrap.sh` before #891, so the closed loop
sharpened a known rule for one more recorder rather than revealing a fact that holds across
territories.

## Blast radius

- [VT interpretation](vt-interpretation.md) — the consumer of the counts. Every priority argued from
  reach reads this corpus, so a recorder that cannot hold a class makes that class look unimportant.
  And the reverse since #895: a stream-driven handler that starts or stops having an effect moves an
  `*.ignored.golden` in the same change
- [events & replies](events-and-replies.md) — a closed loop is the only thing in this repo that
  drives both channels against a real application; `reply_filter.rs` is a consumer of that territory
- [input encoding](input-encoding.md) — `modify_other_keys.rs` drives the encoder from
  `vim_redraw.raw`, so the mode a capture leaves set is an input-side fact as well
- [soft wrap](soft-wrap.md) — the territory the deterministic-printf recorder was invented for,
  because no real full-screen application soft-wraps

## Known holes / open

- **Only one policy arm is recorded.** The white-background run is measured and described, not
  checked in, so the claim that policy changes the bytes is documented rather than pinned.
- **vim's ten XTGETTCAP questions go unanswered.** The closed-loop capture proves they are reached
  in ordinary use, and a test in it inverts the day they are answered; the engine does not answer
  them.
- **The inventory cannot see a handler whose effect is undone before the end of its stream.** A
  mode set and later reset before the cut, and state with no getter that nothing exercises
  afterwards, read `-` while handled. So a regression in `?1006;1000h` is invisible on `htop.raw`
  and caught only on `alt_resize_htop`, where the stream ends before the reset.
- **No capture closes an OSC with ST**; every OSC in the corpus ends in BEL. The scanner's ST branch
  is therefore proven only on synthetic bytes, and a capture that uses it is the first real exercise.
- **`alt_resize_htop` and `alt_resize_vim` have no recorder script.** What is known is in their
  consuming test's doc comment; the vim flags and the exact dwell are not, so neither can be
  re-recorded to the byte.
- **Why `--cmd "set noruler"` is a no-op is not established** — measured byte-identical with and
  without, and the obvious suspect is unverified, next to a sibling case where the obvious suspect
  turned out to be the wrong mechanism.
