# Territory — VT interpretation

## What it is

The write path's front door: `vte` splits the byte stream into actions and this territory executes
them — print a glyph, run a C0 control, dispatch a CSI / ESC / OSC, set a mode, answer a query — and
decides what each sequence *means*, including which ones it deliberately ignores. It owns the
`vte::Perform` implementation, the DEC mode table, the VT52 dialect, charsets, tab stops, the `REP`
retention, the title stack and the per-OSC field rules. The verbs it calls (erase, scroll, wrap,
cursor moves) are written in `term.rs` and belong to the territories named under Blast radius; this
note owns the dispatch onto them and the meaning of each sequence. Its contract is as much what is
*not* implemented as what is: conformance is accumulated, never declared complete (#47).

## Governing decisions

- [ADR-0001 — build on `vte`, not `alacritty_terminal`](../../adr/0001-build-on-vte-not-alacritty-terminal.md)
  — delegate the hard parsing to a stable crate, own everything above it. Every limit below that
  starts "`vte` gives no way to…" is a cost of this record.
- [ADR-0004 — spec-faithful where Alacritty merely omits](../../adr/0004-spec-faithful-when-alacritty-omits.md)
  — the tie-breaker when the reference is silent rather than deliberate. A reference's silence is not
  permission; where it omits a spec'd behaviour the spec wins. Whether a given verb followed the spec
  or the reference is not recorded per verb (Known holes).
- [ADR-0017 — mechanism in core, policy in the consumer](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  — why several modes are tracked and not acted on, why OSC payloads are relayed rather than applied,
  and the ground for the `OSC 4` blank-spec divergence (#834).
- [ADR-0025 — row and wide-pair cell state ownership](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  — D2's per-verb table is a rule *about* the verbs this territory dispatches to, and #584 left the
  write path unsplit so that conformance stays readable in one pass (`term.rs`'s module list: every
  read surface moved to `term/`, the write path did not).

## Design model

From reading the source, ADRs and the issues cited; the rules are mostly the source's own, with the
decision records above as their frame.

**The entry surface.**

- **`Engine::feed` is the only byte entry, and it is not a bare `advance`.** It splits its input at
  each `CAN` (0x18) / `SUB` (0x1a) with `memchr2` and advances that one byte alone with
  `Term::cancel_byte_in_flight` raised, so an `osc_dispatch` inside that advance is the OSC the byte
  cancels and is dropped (#970). `vte` ends an OSC on either byte by calling `osc_dispatch` and then
  `execute`, with `bell_terminated = false` exactly as for `ST`, so inside the handler a cancelled
  and a finished OSC are the same call — the decision has to be taken before dispatch. The rejected
  alternative was holding every non-`BEL` OSC until the next callback: holding means copying the
  fields, which buys a hostile `OSC 52` exactly the second allocation `MAX_CLIPBOARD_BASE64` refuses.
  Measured on #970: the scan costs ~1 ms per 32 MB on the five bench inputs against a `feed` of
  ~500–700 ms; an all-cancel-byte stream is ~11x slower (≈60 vs ≈700 MB/s) and still faster per byte
  than recorded output. An OSC ended by `ESC` and *then* a cancel stays applied (xterm would cancel
  it). Not covered: xterm's visible error character for `SUB`/`CAN`, and a DCS cancelled the same way
  (it reaches `unhook`). `Parser` and `Term` are separate `Engine` fields because `advance` borrows
  both mutably.
- **`Perform` is the whole state-changing surface**: `print`, `execute`, `csi_dispatch`,
  `esc_dispatch`, `osc_dispatch`, `unhook` (`term/dispatch.rs`). `hook` and `put` are not
  implemented, so a DCS payload is ignored. `unhook` exists only to disarm `REP` (below); it is
  reachable in ordinary use because, with DA2 answered, vim follows up with XTGETTCAP `DCS + q`
  queries this engine does not answer (pinned on recorded bytes in
  `justerm-core/tests/closed_loop_capture.rs`). The disarm sits at the *end* of the DCS because no CSI
  can arrive between `hook` and `unhook`, so a disarm in `hook` was a guard no mutation could redden
  (it was added, then removed, #825). Which terminator makes it load-bearing is measured in
  [`reference-facts.md`](../../agents/reference-facts.md#unhook-is-the-only-disarm-on-two-of-three-dcs-terminators-825-measured-2026-09-07)
  — a test feeding only `ESC \` proves nothing, which is what the first version of
  `rep_after_a_dcs_repeats_nothing` did. An **unterminated** DCS is a deliberate divergence from xterm
  in the safe direction: xterm's parser never returns to ground so it would still repeat, while `vte`
  calls `unhook` on the abort and this engine does not repeat.
- **The input space is UTF-8, which puts the 8-bit C1 controls outside it — deliberately (#847).** A
  lone `0x80..=0x9F` byte is ill-formed input, not a control: `0x9B` opens no CSI, `0x9D` no OSC,
  `0x90` no DCS, and `0x9C` terminates no string — nor does `C2 9C`, because the OSC state consumes
  bytes and cannot ask which codepoint they belong to. An OSC payload legitimately carries 8-bit
  text: `0x9C` is the last byte of `한` (`ED 95 9C`), and all six occurrences in the captures were
  that (#847, 2026-09-03). `vte` routes such a byte to `execute`, which handles C0 only, so the byte
  is inert and what follows prints as text — and an OSC "closed" with `0x9C` stays open until `BEL`,
  `ESC`, `CAN` or `SUB` (`Engine::feed`'s doc-comment). Both `vte` and ghostty accept `0x9C` for DCS
  and APC and refuse it for OSC — a payload-range split, not 7-bit vs 8-bit. This is the rare entry
  that is *deliberately absent* rather than not-reached-yet. Rows:
  [`reference-facts.md`](../../agents/reference-facts.md#the-8-bit-c1-st-as-an-osc-terminator-and-why-the-same-parsers-take-it-for-dcs-847-verified-2026-09-03).
- **A private prefix is an *intermediate*, so an unrouted `(prefix, final)` pair is unreachable
  rather than unhandled (#824).** `vte` collects `< = > ?` into the same `intermediates` slice as the
  0x20..0x2F bytes, and `csi_dispatch` returns early on any intermediate it does not name — adding a
  `match` arm for the final does nothing. Routed pairs at HEAD: the kitty `u` family, DA2
  (`CSI > c`), XTMODKEYS (`CSI > m`, #890), DECRQM / private DSR / DECSET under `?`, DECSTR (`!`),
  DECSCUSR (` `). Each guard is keyed on the whole slice (`intermediates == [b'>']`), not `.first()` —
  a `.first()` match makes `CSI > $ c` DA2, which is alacritty's behaviour and not the other three
  references'. The remaining members are silent by construction, so their count is a measurement:
  the routed `>` pairs are three of the ten xterm routes, chosen by reach. Recount at HEAD 7bb31f7
  over `justerm-core/tests/fixtures/*.raw` (20 files): `CSI > 4 ; Pv m` 10 times, `CSI > c` 5,
  `CSI > q` (XTVERSION) once — so `CSI > q` now leads the unrouted tail (#47). Two cautions for anyone
  quoting a corpus count: it is true of one revision only (the `> q` figure read zero here from
  2026-09-02, when #842's `tmux_clipboard.raw` added one, until 2026-09-11), and it is a **floor**,
  because every capture but `vim_closed_loop.raw` is open-loop (`script(1)` or a bare `expect`, which
  answer nothing). Recount at HEAD: `DCS + q` occurs only in `vim_closed_loop.raw`, while four
  open-loop fixtures ask DA2. How that capture was recorded is [capture corpus](capture-corpus.md)'s.

**`REP` and the lifecycle `vte` cannot express (#825, #866).**

- **A rule xterm derives from its parser, this engine has to enumerate.** xterm assigns the retained
  character only when its parser is back in ground, so a completed escape sequence disarms `REP` as
  a consequence of the parser's shape (`charproc.c:6478`, #825). `vte` 0.15's `Parser` publishes
  `new` / `new_with_size` / `advance` / `advance_until_terminated` and nothing about its state, so
  the rule becomes a list: every `Perform` callback but `print` clears the anchor. The member that
  makes it a rule rather than a list of callbacks is `Term::resize`, which is not a parser callback
  and still invalidates the retention. Anything new that moves the cursor or reflows owes a clear,
  and nothing enforces it. `justerm-core/Cargo.toml`'s comment on the `vte` pin asks for this entry
  to be re-read on a minor bump: if parser state becomes reachable, the enumeration can be replaced.
- **The census** (the anchor is `Term::repeat_anchor`, the `(row, col)` of the last printed cluster's
  lead cell). *Set* by the three arms of `Term::place_grapheme` that give a cell content — the
  mode-2027 join (`try_grapheme_join`), the combining attach (`push_combining`) and `write_glyph` —
  each returning where it wrote. *Cleared* by `execute`, `esc_dispatch`, `osc_dispatch`, `unhook`,
  by `csi_dispatch` (which takes it up front because the function has six early returns, and lets
  `REP` put it back, repeat, and clear what the repeats re-armed — pinned by
  `rep_does_not_rearm_itself`), by `resize`, and by the two print outcomes that write no cell: `DEL`
  (the only width-`None` scalar that reaches `place_grapheme`) and a width-2 glyph dropped at the last
  column under `?7l` (`write_glyph` returns `None`). *Translated* by `Term::clear`, which moves the
  anchor up with the cursor's row and drops it otherwise — a site the census on #825 did not list.
  ghostty is the outlier and re-arms (`printRepeat` calls `print`).
- **The anchor names a cell that holds the cluster.** A promotion at the last column relocates the
  cluster to the next row (#303), so the join anchors where it landed (pinned by
  `rep_after_a_promotion_relocated_the_cluster_repeats_the_cluster`). The grapheme is read back from
  the cell rather than stored; the *position* is recorded because it cannot be re-derived — with
  autowrap off the cursor reaches the last column both by filling it and by moving onto it, and an
  earlier guess (`pending_wrap || (!autowrap && col + 1 == cols)`) repeated an unrelated `Z` on
  `?7l` + `ZZZZZ` + `CUP` + `abcd` + `CSI 1 b`. `Term::cursor_cluster_col` read the anchor too for a
  while (#865) and went back to the deferred-wrap flag once #869 fixed it; the constraint is `REP`'s
  own. `REP` re-enters at `place_grapheme`, below the VT52 `ESC Y` intercept and the GL charset
  translation, because a cell holds the *translated* glyph — insurance that becomes load-bearing the
  moment a charset whose output overlaps its input is added. xterm arms on every printed scalar and
  rejects zero-width ones at `REP` (`charproc.c:6154`); equivalent here. `Term::repeat_last` bounds
  the count twice (a progress guard that stops when an iteration places no new cell, and a cap of a
  buffer's worth of cells); why each is `docs/architecture.md` § Hidden VT state. What is repeated —
  the cluster read off the cell — is the maintainer's product call where the references give three
  answers (2026-09-07, `repeat_last`'s doc-comment).
- **The enumeration cannot be completed against `vte` 0.15 at all (#866, measured).** `CsiIgnore`
  returns to ground without dispatching and `DcsIgnore` never calls `hook`/`unhook`, so a malformed
  `CSI 1 ? b`, `CSI SP 1 p`, `CSI ? ? m` or `DCS 1 ? q … ST` ends with no callback to hang a clear on;
  each gives 4 dashes after `-` where xterm gives 1. What bounds the consequence is the *shape* of
  what is retained: a recorded **position** degrades to repeating the genuinely last-printed
  grapheme. **The general form (#866):** where a lifecycle must rest on an enumeration that cannot be
  completed, prefer state whose stale value is still a true statement about the buffer over state
  whose stale value is a guess.

**Modes, charsets, tabs.**

- **Modes are hidden state the engine owns** — `set_dec_private_mode` folds DECSET/DECRST over every
  parameter (#56: htop batches `?1006;1000h`); non-private SM/RM handles IRM (4) and LNM (20). Each
  changes what a later byte means. **Several are tracked and not acted on** — synchronized output
  (`?2026`), colour-scheme updates (`?2031`), win32-input-mode (`?9001`), application keypad
  (`?66`, tracked for DECRQM; xterm.js likewise never reads it, #74): the engine records the flag
  and the consumer owns the behaviour (ADR-0017). DECCOLM (`?3`) changes nothing here and emits
  `TermEvent::ColumnMode` for the consumer to honour by resizing (#82). The alt-screen modes are
  [alt screen](alt-screen.md)'s.
- **DEC Special Graphics maps `` ` ``..`~` and leaves `_` (0x5F) a literal underscore** (#62,
  `dec_special_graphics`), matching xterm.js and alacritty rather than the strict-DEC reading of 0x5F
  as blank. Only G0/G1 with ASCII, DEC Special Graphics and UK are designated; G2/G3 and GR are not.
- **VT52 (`?2l`) is a second dialect, mode-gated inside `esc_dispatch`, not a second parser** (#84).
  `ESC Y`'s two coordinate bytes arrive as `print` calls (vte returned to ground after `Y`), so
  `print` intercepts them first. `ESC c` is honoured even in VT52 because `full_reset` rebuilds `Term`
  with the mode cleared. Graphics mode (`ESC F`/`G`) is a documented no-op.
- **Tab stops are explicit per-column state** (`tabs`), not a modulo: HTS sets, TBC clears, default
  every eighth column (`is_default_tab_stop`). `CBT` (`put_back_tab`, #826) walks the same table
  backwards and is written as `HT`'s mirror so the directions cannot disagree; `CHT`
  (`put_forward_tabs`, #898) is `HT` counted and adds no third walk. What each does to the deferred
  wrap is [cursor position](cursor-position.md)'s; how the table survives a resize is
  [reflow](reflow.md)'s.
- **The scroll region redefines what "scroll" means.** DECSTBM changes which rows `IND` / `RI` / `LF`
  move and which leave the screen, so nearly every vertical-motion verb reads it — since #898 the
  relative moves too ([cursor position](cursor-position.md)). An invalid region (`top >= bottom`) is
  ignored and a valid one homes the cursor (`set_scroll_region`).

**What each OSC field means — an empty field has no general meaning.**

- **Across families**, every answer is different and each is deliberate (#832, #828, #834): for a
  dynamic colour (`OSC 10/11/12`, `special_color`) an empty field addresses its slot, changes nothing
  and still advances, so `OSC 10 ; ; <bg>` reaches the background alone, and the stack ends after the
  cursor slot because xterm's next slots are the pointer colours justerm does not model (dropping a
  fourth spec beats mis-addressing it); for a hyperlink (`OSC 8`) an empty URI closes the link; for a
  title (`OSC 0/2`) an empty string is the new title, while a *fieldless* OSC is ignored; for
  `OSC 104` an empty payload resets the whole table; for `OSC 52` an empty target names the clipboard
  — the only form real applications emit (tmux 3.2a, captured, #828) — and an absent payload field
  (`OSC 52 ; c`) differs from an empty one (`OSC 52 ; c ;`, a store of the empty string, which is how
  the sequence clears a selection); for `OSC 7` an empty payload is relayed as itself (`Cwd("")`),
  because there the blank carries information — ghostty reads it consumer-side as "reset the pwd"
  (`osc/parsers/report_pwd.zig`), a policy justerm leaves to its consumer. The list is open: an arm's
  absence means nobody asked, never that it agrees with a neighbour.
- **`OSC 4`'s empty spec drops its own pair and the sequence continues (#834).** `OSC 4 ; 1 ; ; 2 ;
  #fff` relays index 2 alone, and pairs before the blank survive. That is xterm.js's and
  alacritty-via-`vte`'s answer, **not** xterm's (an empty or unparseable name aborts the remaining
  pairs, `misc.c:3013-3016`; do not cite `:3003`, which guards the index range), and a **deliberate
  divergence** rather than a gap ADR-0004 fails to reach: xterm's trigger, `strlen(spec) == 0` checked
  before any parser (`misc.c:3105-3107`), *is* observable to a theme-agnostic engine — that reading
  was believed false here until #834 read the tree. What decided it: inferring "the rest is corrupt"
  from a blank is a judgement about application intent, which ADR-0017 puts on the consumer's side.
  The references are 2–2. The guard tests emptiness only (#834 Out of Scope): a space-only spec is
  relayed verbatim, while TAB and NUL are C0 bytes `vte` drops inside an OSC string, so those fields
  arrive genuinely empty (pinned by
  `osc4_a_space_spec_is_relayed_but_a_c0_only_spec_arrives_empty`). One event per pair; the walk
  advances two fields whether or not a pair emits, so a dropped pair cannot misalign the rest.
  ghostty relays nothing on the blank-spec input by a third mechanism (`tokenizeScalar` drops the
  empty token and re-aligns), visible only where the re-aligned pair parses.
- **`OSC 104`'s empty payload is both `OSC 104` and `OSC 104 ;` (#832).** `vte` hands them over as
  `["104"]` and `["104", ""]`; testing only the first let the second fall into the index loop, where
  the reset evaporated. xterm tests the payload string (`misc.c:3057`), as xterm.js does, so
  `OSC 104 ; ;` takes the index path. `OSC 110/111/112` reset one slot each and never stack.
- **Within the dynamic-colour family the references split 3–1 on the advance.** xterm, xterm.js and
  `vte` consume the empty slot; ghostty's `tokenizeScalar` drops empty fields, so `OSC 10 ; ; <spec>`
  sets its *foreground* there. justerm follows the three, which ADR-0004 settles independently
  (`ctlseqs.txt` indexes by parameter). Rows:
  [`reference-facts.md` § Cursor colour](../../agents/reference-facts.md#cursor-colour) and
  [§ empty parameters](../../agents/reference-facts.md#empty-parameters-are-not-empty-values).
- **The payload is `params[1..]` (or `[2..]` for `OSC 8`) rejoined on `;`** (#650, #880), because
  `vte` splits at every `;`. Rows:
  [§ where an OSC payload ends](../../agents/reference-facts.md#where-an-osc-payload-ends--neither-reference-splits-past-ps-880-verified-2026-09-08).

**Bounds on outside input.**

- **The bytes are unbounded; a handler that builds anything from a payload bounds it itself (#828).**
  `vte` is built with default features, so its OSC accumulator is a `Vec<u8>`, not the
  `ArrayVec<_, 1024>` of its `no_std` path — a 4 MB `OSC 52` reaches `osc_dispatch` complete
  (measured with a probe, #828). A handler bound cannot stop the parser allocating; it stops the
  *second* allocation. At HEAD: `MAX_CLIPBOARD_BASE64` (16 MiB, `OSC 52`), `MAX_TITLE` (4096 `char`s,
  `OSC 0/2`, truncated), `MAX_LINK_URI` (10 000 000 bytes, `OSC 8` ignored whole) and `MAX_LINK_ID`
  (250, id dropped) — the last three from #1038. Truncation for a title because a title is shown,
  not followed; a URI is dropped whole. `OSC 7` and `OSC 9`/`777` are bounded by nothing here and
  retain nothing: they are relayed as events.
- **The fields are bounded: `vte` records at most 16 field boundaries per OSC** (`VTE_OSC_FIELD_CAP`,
  its private `MAX_OSC_PARAMS` in 0.15.0), so what lies past the 16th `;` never reaches
  `osc_dispatch`, and a payload cut there looks exactly like a complete one. Measured on the engine
  (#840): `OSC 8` URIs complete up to 13 `;`, cut from 14; `OSC 0/2` titles and `OSC 7` cwd values
  complete up to 14, cut from 15; `OSC 4` keeps its first 7 pairs, a correct prefix; `OSC 52` cannot be
  cut into a valid value because a rejoined `;` fails base64. Alacritty's parser path cuts at the same
  boundaries, so the cause is `vte`; xterm.js and ghostty read the raw payload and bound an OSC in
  bytes by dropping it. **Not guarded, by the maintainer's call (#840):** refusing at exactly 16
  fields would drop the complete value on the boundary, and reach measured low (`ls --hyperlink` and
  penterm's bash/zsh/fish/pwsh OSC 7 integrations percent-encode `;`; only its cmd.exe integration
  sends a raw path). **One payload is marked instead (#964):** `TermEvent::Notification` carries
  `maybe_truncated`, set when all 16 fields arrived; see [events and replies](events-and-replies.md).
- **A retained title is bounded in length as well as stack depth (#1038).** Depth alone
  (`TITLE_STACK_DEPTH`, 10, oldest dropped) let a 10 MiB title and ten `CSI 22 t` keep 236 MiB on
  727865d — eleven copies on each of two axes, plus the capacity `String::clone_from` keeps. OSC 0/2
  now cut the title to `MAX_TITLE` before it reaches state, stack or `TermEvent::Title`, and
  `set_window_title` assigns rather than `clone_from`s. What remains of that measurement (16 MiB) is
  the parser's OSC buffer (Known holes).

**Sequences that turn a relay into state.**

- **XTWINOPS is two operations here (#823).** The engine owns no window, so most of the family is
  meaningless; `window_ops` handles 22 (push) and 23 (pop) and ignores every other first parameter.
  22/23 were the single most-emitted unimplemented sequence in the #823 capture sweep. Answering a
  pop requires holding the title, so parsing OSC 0/2 and forwarding the string — enough since #12 —
  stopped being enough: **a later sequence can turn a pass-through into state**, and nothing about
  the original relay says so. Two axes (window title, icon name), each its own bounded stack, because
  the second parameter selects one and vim uses all three values — a single stack restores the wrong
  string, which is alacritty's behaviour. xterm keeps one stack of `{icon, window}` pairs and walks
  back through older slots when the popped member is empty — a third model. The optional third
  parameter (direct slot access) is ignored, a deliberate divergence from the spec decided on a
  five-way reach measurement on #823 and pinned by `xtwinops_the_third_parameter_is_ignored`. Rows:
  [§ the title stack](../../agents/reference-facts.md#the-title-stack).
- **DA2 (`CSI > c`) answers `ESC [ > 1 ; Pv ; 0 c` (#824).** What answering buys, measured as a
  control pair on a real pty (RHEL 9.2, vim 8.2, `TERM=xterm-256color`, 24x80, #824): no reply gives
  `ttymouse=xterm`, a reply gives `sgr` — legacy encoding cannot report a column past 223 or a
  release. The larger effect: answering makes vim ask ten XTGETTCAP questions (`Co`, `ku`, `kd`,
  `kl`, `kr`, `k1`, `#2`, `#4`, `%i`, `*7`), which justerm does not answer — they fall to the
  intermediate catch-all, so the capability is unlocked and then unanswered (#47). The requests
  appear at `Pv` 276 and 1500 and not at 1, 94 or 95, bracketing vim's gate to (95, 276]. Whether
  they appear is stable across runs, how many is not. modifyOtherKeys is not gated on any of it: vim
  emits `CSI > 4 ; 2 m` before it asks. **`Pv` is the crate version padded base-100**
  (`version_number`, alacritty's scheme; `ctlseqs.txt` fixes no encoding), and the base is
  load-bearing — vim picks its mouse protocol off the field (two runs of 11 arms each, #824):

  | `Pv` | vim's `ttymouse` |
  |---|---|
  | < 95 | `xterm` (no upgrade) |
  | = 95 | `sgr` — vim special-cases macOS Terminal's exact `>1;95;0c` |
  | 95..276 | `xterm2` |
  | ≥ 277 | `sgr` |

  The measurement ran at 1500 (0.15.0); the workspace is 0.30.0 at HEAD, so `DA2_VERSION` is 3000,
  still past all three thresholds. A `0.2.x` would map to 200 and silently cost every consumer the
  SGR mouse encoding. Edges, all deliberate: a component of 100 or more carries into the next place;
  the pre-release suffix is cut at the **first** `-` (or `+`) — alacritty cuts at the last and
  mis-parses `-rc.1-dev`; and monotonicity holds only below `u16::MAX`, since `vte` saturates a CSI
  parameter and ghostty types the field `u16` (a `Pv` past 65535 — major version 7 — arrives
  saturated). Why `Pp = 1` and `Pc = 0` is `docs/architecture.md` § Hidden VT state; rows:
  [§ secondary device attributes](../../agents/reference-facts.md#secondary-device-attributes--report-yourself-do-not-impersonate).

**Resets.**

- **RIS (`full_reset`) and DECSTR (`soft_reset`) are two reset strengths**, and what each does *not*
  clear is the part that matters; which fields RIS carries across is decided by
  [RIS keeps configuration, drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md).
- **Neither announces anything about the palette, and the silence is a decision (#835).** An
  application can redefine the ANSI table with `OSC 4` or the dynamic colours with `OSC 10/11/12`, and
  a reset emits no `Reset*` event, so a consumer that honoured the redefinition keeps it. xterm is the
  one reference that resets its own table, on both strengths. Three grounds decided against following
  it (not the head-count): ADR-0004 does not reach the question, because `OSC 4`/`104` are xterm's
  invention over a table DEC never defined (the "inventor owns the semantics" move that settled
  `XTREVWRAP` does not transfer — this is what a DEC sequence does to state an invented one left);
  terminfo settles the reach from outside every implementation (`xterm-256color`'s
  `rs1=\Ec\E]104\007`, `linux`'s `rs1=\Ec\E]R` — both append a palette reset after `RIS`, which
  therefore already arrives as `OSC 104` and is relayed); and ghostty, the one reference built in this
  shape, holds the palette and announces changes yet sends nothing from `fullReset`. The pen half of
  xterm's block is mirrored on both strengths here, and the dynamic colours are restored by no
  reference at all. Rows and the reversal criterion:
  [§ what a reset does to the colours](../../agents/reference-facts.md#what-a-reset-does-to-the-colours--the-palette-splits-13-the-dynamic-colours-are-04-835-verified-2026-09-08),
  #835.

## Code

- `justerm-core/src/lib.rs` — `Engine::feed` (the cancel-byte split), the `Engine` struct holding
  `Parser` and `Term` apart
- `justerm-core/src/term/dispatch.rs` — `impl Perform for Term`: `print`, `execute`, `csi_dispatch`,
  `esc_dispatch`, `osc_dispatch`, `unhook`; `Term::set_dec_private_mode`, `Term::vt52_dispatch`,
  `Term::vt52_take_coord`, `Term::link_for_id`, `osc8_link_id`, `VTE_OSC_FIELD_CAP`, `DA2_VERSION`
- `justerm-core/src/term/replies.rs` — `Term::window_ops`, `push_title`, `Term::set_window_title`,
  `Term::special_color`, `Term::clipboard`, `Term::decrqm`, `Term::device_status_report`,
  `Term::kitty_dispatch` (the arms dispatch hands queries and relays to)
- `justerm-core/src/term.rs` — `Term::place_grapheme`, `Term::repeat_last`, `Term::cluster_text`,
  the fields `repeat_anchor` and `cancel_byte_in_flight`; `Charset`, `dec_special_graphics`;
  `Term::put_tab`, `Term::put_forward_tabs`, `Term::put_back_tab`, `Term::set_tab_stop`,
  `Term::clear_tab_stop`, `is_default_tab_stop`; `Term::set_scroll_region`; `Term::full_reset`,
  `Term::soft_reset`; `version_number`; the bounds `MAX_TITLE`, `TITLE_STACK_DEPTH`, `MAX_LINK_URI`,
  `MAX_LINK_ID`, `MAX_CLIPBOARD_BASE64`
- `justerm-core/Cargo.toml` — the `vte` pin and its comment pointing back here

Hand-written: `docs/architecture.md` § Hidden VT state is the catalogue of modelled and unmodelled
behaviour (52 bulleted entries at HEAD 7bb31f7, most now describing modelled state).

## Reference behaviour

- [Soft wrap is a row property](../../agents/reference-facts.md#soft-wrap-is-a-row-property) — which
  erase verbs end a wrap, per verb
- [What a blanked / freed cell is made of](../../agents/reference-facts.md#what-a-blanked--freed-cell-is-made-of)
  — what an erase fills with
- [The VT-gap sweep of 2026-08-26](../../agents/reference-facts.md#the-vt-gap-sweep-of-2026-08-26--five-decisions-the-references-settled-823832-verified-2026-08-26)
  — `REP`, DA2, the title stack, `OSC 52`, empty OSC fields (the subsections linked above)
- [REP's retained character](../../agents/reference-facts.md#reps-retained-character--the-two-obvious-references-disagree-and-xterm-breaks-the-tie)
  · [REP's repeated unit](../../agents/reference-facts.md#reps-repeated-unit--a-third-axis-and-here-the-trees-split-30)
  · [two `vte` routings](../../agents/reference-facts.md#two-vte-routings-that-a-single-grep-gets-backwards-825-verified-2026-09-07)
- [OSC 52](../../agents/reference-facts.md#osc-52--where-the-references-converge-and-the-two-places-the-spec-is-not-followed-828-2026-09-02)
  — including the `vte` dispatch row on `CAN`/`SUB` that #970 rests on
- [`OSC 4`'s empty spec](../../agents/reference-facts.md#osc-4s-empty-spec-splits-22-and-xterms-trigger-is-available-to-justerm)
  · [the empty-field advance](../../agents/reference-facts.md#the-empty-field-advance-splits-31-and-the-outlier-believes-it-does-not)

## Cross-cutting invariants

- [RIS keeps configuration, drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — `RIS` and `DECSTR` are dispatched here, and `full_reset` rebuilds `Term` from the constructor, so
  every field the rebuild must carry across is decided by that note's table

## Blast radius

Every stateful territory downstream, because this is where state is written.

- [soft wrap](soft-wrap.md) · [wide glyph](wide-glyph.md) — the verbs dispatched here are the subjects
  of ADR-0025's per-verb table; a new erase or shift verb inherits the obligation to say whether it
  ends a wrap and repairs a pair
- [cursor position](cursor-position.md) — every positioning verb owes a deferred-wrap decision, and
  `REP`'s anchor assumes the print path's arm sites
- [pen](pen.md) — SGR is parsed here; the pen assumes every attribute it carries arrived through
  `sgr`
- [grapheme clusters](grapheme-clusters.md) — `REP` copies a cluster cell by cell, so the per-cell
  cluster cap is what bounds a repeated cluster's memory
- [alt screen](alt-screen.md) — the `?47/1047/1048/1049` arms are dispatched here; the swap assumes
  it is reached only through them and RIS
- [damage](damage.md) — every mutation records a span; a verb that forgets is invisible to a
  frame-mode consumer
- [marker](marker.md) · [selection](selection.md) · [tracked points](tracked-points.md) — the
  anchor-maintenance calls sit inside these verbs; a new row-moving verb that skips them leaves
  anchors on content that moved
- [reflow](reflow.md) — `resize` is a member of the `REP` clear list and owns the tab table's extension
- [input encoding](input-encoding.md) — the modes it reads (DECCKM, kitty flags, modifyOtherKeys,
  mouse, focus, bracketed paste) are set by *these* arms, which is why the encoder cannot live in the
  consumer
- [events and replies](events-and-replies.md) — the relay arms decide what reaches an event and with
  which payload bound; that note's consumers assume the bound
- [capture corpus](capture-corpus.md) — a verb whose sequence kind moves a surface on a recorded
  stream fails that capture's `*.ignored.golden` when it stops (#895)
- [robustness testing](robustness-testing.md) — `feed` is the untrusted entry the property and fuzz
  targets drive; a handler that allocates from a payload without a bound is what they cannot see as
  a panic

## Known holes / open

- **The parser keeps its largest OSC's capacity for the `Engine`'s life (#1039), kept on purpose.**
  `vte` with `std` buffers an OSC in a `Vec` and ends it with `clear()`, so a 100 MiB OSC leaves
  128 MiB (the largest single OSC, rounded up to a power of two); later ones reuse it. Intended in
  `vte`, and alacritty holds its parser the same way. The maintainer left it on 2026-10-08: a
  high-water mark, not growth (~16–32 MiB after an `OSC 52` copy at `MAX_CLIPBOARD_BASE64`), and the
  fix — replacing the parser after a large OSC via `advance_until_terminated`, re-feeding `ESC` for an
  ST-terminated one — is a stateful path in `feed`. Revisit when per-terminal memory matters to a
  consumer. Nothing else bounds an OSC's byte size.
- **`OSC 7` and `OSC 9`/`777` payloads are relayed with no size bound** (up to the parser's buffer).
  They retain nothing in the engine; the consumer receives the whole string.
- **Conformance is accumulated, never declared complete (#47).** Nothing distinguishes *deliberately
  absent* (the C1 entry above) from *not reached yet* except prose. Known members of the tail named
  above: XTVERSION and the other unrouted `>` finals, XTGETTCAP, DCS payloads in general, G2/G3, the
  `REP` clear on `CsiIgnore` / `DcsIgnore`.
- **The `REP` clear list is enforced by nothing.** A new verb that moves the cursor or reflows owes a
  clear (or, like `Term::clear`, a translation) and no test fails if it forgets.
- **Modes tracked but not acted on have no single list.** Each is documented at its field; a consumer
  discovering which flags it must honour has to read the `Term` struct.
- **ADR-0004's tie-breaker is stated once and applied everywhere.** Whether a given verb followed the
  spec or the reference is not recorded per verb.
- **The hidden-state catalogue has no home in the graph.** `architecture.md` § Hidden VT state is the
  only artifact that lists what is *not* built; its entries are distributed across this map by
  subject.
