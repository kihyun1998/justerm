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
  kind with that kind removed. `<capture>.ignored.golden` records, per kind, **the names of the
  surfaces that moved**, or `-`. That is stricter than a binary ignored/effect verdict, which is what
  #895 described: a kind that moves one surface more or one fewer also fails. The whole corpus
  replays in about 0.5 s, so it sits in the default suite.
- **What makes its `-` a measurement, each proven by a mutation that reddened it.**
  - **A kind keeps every parameter that selects a function.** Only the quantity finals (positions,
    counts, margins) drop theirs. With SGR folded into one `CSI m` kind, making `7` a no-op left the
    test green. With the parameters kept, `top`'s `CSI 7m` row flips and the test fails. The same
    split separates `J` from `3J` and `>4;2m` from `>4;m`.
  - **Each surface name travels with its value**, and the positive control fails on a listed surface
    no kind moves. Blanking the mouse value and regenerating the goldens turns them green and the
    control red. A golden naming an unknown surface fails too.
  - **The frame is read after a damage ack.** Without it, the wire frame carried the scroll ops
    accumulated over the whole replay, which is history rather than end state. Exactly three rows
    moved for that reason alone.
  - **A scanner guard.** A token that runs long swallows the next sequence and manufactures a false
    effect; making the DCS branch ignore ST was caught. The OSC branch is proven on synthetic bytes,
    because the corpus never closes an OSC with ST.
  - **The engine mutations** were DECCKM set, `?1000h` set and SGR 7 made no-ops, plus key encodings
    probed without modifiers. Each flipped a named row.
- **The inventory's verdict is end-state only, and that was the maintainer's scope call.** `-` means
  the state at the end of the stream is identical without the kind, not that the engine ignores it.
  State with no getter that nothing later exercises reads `-` while handled (tab stops, the saved
  cursor, charsets). Comparing the trajectory would see it; that alternative was shown with its cost
  and left out of #895. The list of captures is hand-kept rather than globbed, so adding a capture is
  a deliberate entry there and not a silent change of what is pinned.
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
- `justerm-core/tests/ignored_inventory.rs` and `justerm-core/tests/fixtures/*.ignored.golden` — the
  per-kind differential replay, its surface controls and its scanner guard
- `justerm-core/src/term/replies.rs` — `drain_replies` and the `report_*` methods are the two halves a closed
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
  And the reverse since #895: a handler whose kind moves a surface in some capture and then stops
  moving it fails an `*.ignored.golden`. Not every handler is so placed — see Known holes
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
- **The inventory guards only handlers whose kind already moves a surface in some capture.** A kind
  that reads `-` everywhere can stop working with nothing failing, and so can one whose only effect
  is later overwritten within the same kind. A set with a separate reset *is* covered: making
  `?1000h` a no-op flips `htop`'s `?1006;1000l` row, because that reset no longer undoes anything.
- **Kinds that occur only after the alt-screen cut are not inventoried at all**: `?1049l` in every
  alt capture, the `23t` title pops in four of them, and `OSC 112` in `cursor_color_nvim`. The
  whole-stream capture tests that consume those files still pin them.
- **Interaction state is never seeded before a replay.** No selection, tracked point, marker or
  search highlight exists, so a stream verb's fixups to that state read `-`. What else covers it on
  real captures is narrower than it looks: `alt_selection_resize.rs` seeds a selection across the
  `alt_resize_*` resizes, and `selection_column_bound.rs` / `match_span_column_bound.rs` pin column
  bounds. Whether scroll, erase and line insert/delete carry a seeded anchor correctly through a real
  stream is pinned nowhere. Seeding was left out of #895 by the maintainer as a separate property
  needing its own placement decisions; whether to take it up was not decided.
- **The scanner diverges from a real parser where the corpus does not reach.** It takes any byte as a
  CSI final (so a C0 inside a CSI would end it), does not abort on CAN or SUB, and does not recognise
  8-bit C1 or an OSC closed by ST in the corpus. Measured over the cut corpus: none of these occur, and
  every OSC ends in BEL, so the OSC ST branch is proven only on synthetic bytes.
- **`alt_resize_htop` and `alt_resize_vim` have no recorder script.** What is known is in their
  consuming test's doc comment; the vim flags and the exact dwell are not, so neither can be
  re-recorded to the byte.
- **Why `--cmd "set noruler"` is a no-op is not established** — measured byte-identical with and
  without, and the obvious suspect is unverified, next to a sibling case where the obvious suspect
  turned out to be the wrong mechanism.
