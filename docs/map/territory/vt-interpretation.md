# Territory — VT interpretation

## What it is

The write path: bytes in, screen state out. `vte` splits the stream into actions and this territory
executes them — print a glyph, move the cursor, erase a region, scroll a margin, set a mode. It is
what remains in `term.rs` after #584 moved the read surfaces out, and it is still the largest single
file in the engine.

**Its contract is as much about what is *not* implemented as what is.** `architecture.md`'s
§"Hidden VT state" is a 30-entry catalogue of behaviour that has to be modelled and largely is not —
for a terminal engine, that list is half the specification.

## Governing decisions

- [**ADR-0001 — build on `vte`, not `alacritty_terminal`**](../../adr/0001-build-on-vte-not-alacritty-terminal.md)
  — delegate the genuinely hard parsing to a stable crate; own everything above it
- [**ADR-0004 — follow the DEC spec where Alacritty merely omits a spec'd behaviour**](../../adr/0004-spec-faithful-when-alacritty-omits.md)
  — the tie-breaker when the reference is silent rather than deliberate
- [ADR-0025 — row and wide-pair cell state ownership](../../adr/0025-row-and-wide-pair-cell-state-ownership.md)
  — D2's per-verb table is a rule *about* these verbs, and #584 deliberately left the write path
  unsplit so that conformance stays readable in one pass

## Design model

- **`Perform` is the whole entry surface.** `print` · `execute` · `csi_dispatch` · `esc_dispatch` ·
  `osc_dispatch` · `unhook` — six methods since #825, and everything the engine does to state hangs
  off them. `unhook` is the odd one: it handles no DCS and exists only to clear the `REP` retention
  bit, which is the shape the next entry is about. It is reachable in ordinary use — with DA2
  answered, `vim` follows up with XTGETTCAP `DCS + q` queries this engine does not answer, pinned on
  recorded bytes in `justerm-core/tests/closed_loop_capture.rs`. It disarms at the *end* of the DCS
  because no CSI can arrive between `hook` and `unhook`, so a disarm in `hook` is a guard no mutation
  can redden. Which terminator decides whether it is load-bearing is measured in
  [`reference-facts.md`](../../agents/reference-facts.md#unhook-is-the-only-disarm-on-two-of-three-dcs-terminators-825-measured-2026-09-07)
  — a test feeding only `ESC \` proves nothing here, which is what the first version of
  `rep_after_a_dcs_repeats_nothing` did. The **unterminated** row is a deliberate divergence from
  xterm in the safe direction: an unterminated DCS never returns xterm's parser to the ground state,
  so xterm would still repeat, while `vte` calls `unhook` on the abort and this engine does not.
  Disarming too eagerly can only turn `REP` into a no-op.
- **A rule that xterm derives from its parser, this engine has to enumerate — and `vte` gives it no
  way not to (#825).** `REP` needs *"what was last printed"*, and xterm gets its lifecycle for free:
  it assigns the retained character only when the parser is back in the ground state, so a completed
  escape sequence disarms the repeat as a consequence of the parser's shape rather than by an
  explicit clear (`charproc.c:6478`). ghostty is the outlier and re-arms (`printRepeat` calls
  `print`); `REP`'s own arm puts back what the dispatch took, repeats, then clears what the repeats
  re-armed, pinned by `rep_does_not_rearm_itself`. `vte`'s `Parser` publishes `new` / `new_with_size` / `advance`
  / `advance_until_terminated` and **nothing about its state**, so that formulation is unavailable
  and the rule becomes a list: every `Perform` callback but `print` clears the bit. The list is what
  xterm's shape exists to avoid, and it has already cost once — `unhook` was added, then a mutation
  showed the `hook` half of it could never fail and it was removed. **The member that makes this a
  rule rather than a list of dispatch methods is `resize`**, which is not a parser callback at all
  and still invalidates the retention. Anything new that moves the cursor or reflows owes a clear,
  and nothing enforces it. **The census** (the anchor is `Term::repeat_anchor`, the `(row, col)`
  of the last printed cluster's lead cell): *set* by the three sites that give a cell content, all
  reached through `place_grapheme`, each returning where it wrote; *cleared* by `execute`,
  `esc_dispatch`, `osc_dispatch`, `unhook`, by `csi_dispatch` (which takes it up front and lets
  `REP` disarm itself) and by `resize`, whose reflow moves the cell out from under it. `print` clears
  only on its zero-cell path; `hook` and `put` do not, because nothing that could read it arrives
  before `unhook` does.
  **The anchor must name a cell that holds the cluster.** A promotion at the last column relocates
  the cluster to the next row (#303), so the join anchors on where it landed, not where it was
  joined — anchored on the vacated column, `REP` repeats the blanks `vacate_for_wrap` left (pinned by
  `rep_after_a_promotion_relocated_the_cluster_repeats_the_cluster`). For a while
  `Term::cursor_cluster_col` read the anchor too, because the deferred-wrap flag could not express a
  pin under `?7l` (#865); #869 fixed the flag and that reader went back to it. The constraint is
  `REP`'s own, so the second reader leaving is no permission to undo it. The grapheme is read back
  from the cell rather than stored, which keeps the repeated unit in step with the cell model; the
  *position* is recorded because it cannot be re-derived — with autowrap off the cursor reaches the
  last column both by filling it and by advancing onto it, and an earlier guess
  (`pending_wrap || (!autowrap && col + 1 == cols)`) repeated an unrelated `Z` on
  `?7l` + `ZZZZZ` + `CUP` + `abcd` + `CSI 1 b`.
  **And the enumeration cannot be completed against `vte` 0.15 at all**: `State::CsiIgnore`
  returns to ground at `src/lib.rs:222` without dispatching, and `State::DcsIgnore` never
  calls `hook`/`unhook`, so a malformed `CSI 1 ? b` or `DCS 1 ? q … ST` ends with no
  callback for the engine to hang a clear on (`CSI 1 ? b`, `CSI SP 1 p` and `CSI ? ? m` all reach
  `CsiIgnore`; each gives 4 dashes after `-` where xterm gives 1). Measured, not derived. What bounds the
  consequence is the *shape* of what is retained: a recorded **position** degrades to
  repeating the genuinely last-printed grapheme, where the cursor-derived guess this
  started with repeated whatever happened to sit in the last column.
  **The general form is the part that outlives `REP` (#866): where a lifecycle must rest on an
  enumeration that cannot be completed, prefer state whose stale value is still a true statement
  about the buffer over state whose stale value is a guess.** A retained *position* degrades to a
  fact about a cell that is still true; a retained character behind a flag degrades to a
  fabrication, and the two cost the same to write.
- **The input space is UTF-8, which puts the 8-bit C1 controls outside it — deliberately, and this
  entry exists because nothing else said so (#847).** A lone `0x80..=0x9F` byte is ill-formed input,
  not a control: `0x9B` opens no CSI, `0x9D` no OSC, `0x90` no DCS, and `0x9C` terminates no string
  — nor does `C2 9C`, which is unambiguous and still declined, because the OSC state consumes
  *bytes* and cannot ask what codepoint they belong to. The reason is that an OSC payload
  legitimately carries 8-bit text: `0x9C` is the last byte of `한` (`ED 95 9C`), and all six
  occurrences in this repo's captures are that. Mechanically, `vte` routes such a byte to
  `execute()` (`lib.rs:633`) and this territory implements `execute` for C0 only, so the byte is
  inert and whatever followed it prints as text — a shape worth knowing before reading it as a
  dropped sequence. The same split is visible *inside* `vte` and inside ghostty, and it is not
  about 7-bit versus 8-bit: both accept `0x9C` for **DCS** and **APC**, whose payload ranges stop at
  `0x7E`/`0x7F`, and refuse it for OSC, whose range runs to `0xFF`. Rows in
  [`reference-facts.md`](../../agents/reference-facts.md). **This is the rare entry that is
  *deliberately absent* rather than not-reached-yet** — the distinction "Known holes" below says
  nothing preserves except prose, which is why it is prose here.
- **DEC Special Graphics maps `` ` ``..`~` and leaves `_` (0x5F) a literal underscore** (#62),
  matching xterm.js and alacritty rather than the strict-DEC reading of 0x5F as blank.
- **Modes are hidden state the engine owns and reports nowhere.** Origin (DECOM), autowrap (DECAWM),
  insert (IRM), newline (LNM), reverse wraparound, bracketed paste, synchronized output,
  colour-scheme updates, grapheme clustering — each changes what a later byte *means*.
- **Several modes are tracked but not acted on**, deliberately: the engine records the flag and the
  consumer owns the behaviour (synchronized output's paint-hold, colour-scheme notification). That
  pattern repeats often enough to be the territory's signature — see ADR-0017.
- **An empty OSC field means something different in every family — and, inside one family, in every
  reference.** Two independent traps, measured while adding the cursor slot (#832), and the second
  is the one that bites.

  *Across families*, the obvious generalisation is wrong in both directions: for a **dynamic colour**
  (OSC 10/11/12) an empty field *addresses its slot and changes nothing* while the stack still
  advances past it, so `OSC 10 ; ; <bg>` is how xterm reaches the background alone (`ChangeColorsRequest`'s offset loop at `misc.c:3679`,
  walking `OSC_TEXT_FG` → `OSC_TEXT_BG` → `OSC_TEXT_CURSOR`, `ptyx.h:1018-1020` — the stack ends after the cursor here, because xterm's next slots are the
  pointer colours `OSC_MOUSE_FG` = 13 and `OSC_MOUSE_BG` = 14, which justerm does not model, and
  dropping a fourth spec is better than mis-addressing it; the skip at `misc.c:3684`,
  `:3687` — its *implementation*; `ctlseqs.txt:2082` documents only the stack); for a **hyperlink**
  (OSC 8) an empty URI *closes* the current link; for a **title** (OSC 0/2) an empty string *is* the
  new title. The neighbour that looks identical is not: xterm's OSC 4 path has no skip at all — an
  empty or unparseable name **aborts the remaining pairs** (`misc.c:3013-3016`, *"stop on any
  error"*, in the loop opening at `:2993`). **Do not cite `:3003` for this**, as this entry did until
  #834 read the tree: that line is another `break` in the same loop, carrying the near-identical
  comment *"quit on any error"*, and it guards the **index range** rather than the colour. And the
  abort's trigger for a blank field is `strlen(spec) == 0` — checked with no parser at `:3105-3107`,
  with `XParseColor` in the `else if` at `:3111` and never reached — which is exactly the observation
  a theme-agnostic engine *can* make. Rows in
  [`reference-facts.md`](../../agents/reference-facts.md).

  *And `OSC 52` is a fifth answer, added by #828*: an empty **target** field is neither "skip" nor
  "unrecognised" — it *names the clipboard*, and it is the only form real applications emit (tmux
  3.2a, captured). So far: an empty field means skip for a colour slot, close for a hyperlink, the
  new value for a title, reset-everything for `OSC 104`, and **a default target** for a clipboard
  request. Nothing generalises across them, which is the entry's point; what generalises is that each
  one has a deliberate rule somewhere and none of them is the obvious one.

  **`OSC 4` is the sixth (#834): an empty *spec* names no colour, so its own pair is dropped and the
  sequence continues** — `OSC 4 ; 1 ; ; 2 ; #fff` relays index 2 alone, and the pairs *before* the
  blank survive too. That is xterm.js's and alacritty-via-`vte`'s answer, **not** the tie-breaker's,
  and it is a **deliberate divergence rather than a gap ADR-0004 fails to reach** — the reading that
  xterm's trigger is unavailable to a theme-agnostic engine is false, and was believed here until the
  tree was read (`misc.c:3105-3107`). What decided it is on #834: inferring "the rest is corrupt"
  from a blank field is a judgement about *application intent*, which ADR-0017 puts on the consumer's
  side. The references are **2–2**, cached in
  [`reference-facts.md`](../../agents/reference-facts.md).

  **`OSC 4`'s guard tests emptiness only**, deliberately (#834 Out of Scope): a space-only spec is
  relayed verbatim, while TAB and NUL are C0 bytes `vte` drops inside an OSC string, so those fields
  arrive genuinely empty and are dropped — both pinned by tests, so a later widening to whitespace
  reddens. One event per pair, xterm's `while slots > 1`; the walk advances two fields whether or not
  a pair produces an event, so a dropped pair
  cannot misalign the ones after it. ghostty relays nothing on the blank-spec input by a third
  mechanism: `tokenizeScalar` drops the empty token (`color.zig:130`), the pairing re-aligns, and
  `RGB.parse("2")` fails into `catch return result` (`:210`) — a re-alignment visible only where the
  re-aligned pair parses: `OSC 4 ; 1 ; ; #fff` sets index 1 there and nothing anywhere else.

  **`OSC 104`'s empty payload is both `OSC 104` and `OSC 104 ;` (#832).** `vte` hands them over as
  `["104"]` and `["104", ""]`, and testing only the first let the second fall into the index loop,
  where `"".parse::<u8>()` fails and the reset evaporated silently. xterm tests the payload string,
  not the field count (`if (*buf != '\0')`, `misc.c:3057`, whose else-branch is *"resetting all
  colors"* at `:3077`), and xterm.js gates on the same emptiness (`InputHandler.ts:3223-3224`, a
  slot-less RESTORE). So `OSC 104 ; ;` — xterm's buf is `";"` — takes the index path; ghostty's
  `tokenizeScalar` drops both separators and resets everything, and ADR-0004 puts the spec proxy on
  top. `OSC 110` / `111` / `112` reset one slot each and never stack: xterm's reset path resolves a
  single index from the code itself and walks nothing (`misc.c:3729`).

  **And `OSC 7` is a seventh, in the opposite direction**: an empty payload is relayed *as itself*,
  `OSC 7 ;` → `Cwd("")` (`term.rs`), because there the blank carries information the consumer can
  act on. One reference pins exactly that shape under a named test (ghostty
  `osc/parsers/report_pwd.zig:38-48`) and reads it consumer-side as "reset the pwd as if we never saw
  one" (`termio/stream_handler.zig:1074-1081`) — a *policy* justerm deliberately leaves to its own
  consumer. **This list is open, not closed**: it grows whenever an arm is settled, and an arm's
  absence here means nobody has asked, never that it agrees with a neighbour. Note the shape #828
  added on the *payload* side too, since it looks like the same question
  and is not: a payload **field that is absent** (`OSC 52 ; c`) and an **empty payload**
  (`OSC 52 ; c ;`) are different sequences — the second is a store of the empty string, which is how
  the sequence clears a selection.

  *Within* the dynamic-colour family the references then split **3–1 on the advance**, which no
  amount of reading one of them reveals. xterm, xterm.js and vte all consume the empty slot and move
  to the next; ghostty tokenizes the payload with `tokenizeScalar`, which **drops empty fields
  entirely**, so `OSC 10 ; ; <spec>` sets its *foreground* where the other three set the background
  — under a comment claiming *"This matches the xterm behavior"*. The divergence is a consequence of
  choosing `tokenize` over `split`, and ghostty has no test that would catch it. Rows in
  [`reference-facts.md`](../../agents/reference-facts.md#cursor-colour). justerm follows the three,
  which ADR-0004 settles independently: `ctlseqs.txt:2082` indexes by *parameter*, not by value.
- **XTWINOPS is two operations here, and `CSI t` is not a handled final (#823).** The engine owns no
  window, so most of the family is meaningless: 14/16 ask about pixels the engine has no concept of,
  and resize/move/iconify are requests about a window the consumer owns. 22 and 23 are pure VT
  state, and were the single most-emitted unimplemented sequence in the capture sweep that produced
  #823; every other first parameter falls through and is ignored.
- **A sequence can make the engine *retain* something it previously only relayed (#823).** XTWINOPS
  `CSI 22 t` / `CSI 23 t` push and pop the window title, and answering a pop requires holding the
  title — so parsing OSC 0/2 and forwarding the string, which had been enough since #12, stopped
  being enough. The general shape is worth naming because it will recur: *a later sequence can turn
  a pass-through into state*, and nothing about the original relay says so. Two axes are involved
  (window title and icon name), each with its own bounded stack, because the sequence's second
  parameter selects one and `vim` uses all three values — a single stack restores the wrong string,
  which is what alacritty does, its dispatch never reading past the first parameter. That is a
  choice among **three** models rather than two, and the spec makes none of them: xterm keeps one
  stack of `{icon, window}` *pairs* and walks back through older slots when the popped member is
  empty, which handles the axis correctly by a different mechanism and does not share a depth
  budget with two stacks. Rows in [`reference-facts.md`](../../agents/reference-facts.md). The optional
  third parameter (direct stack-slot access) is a **deliberate divergence from the spec**, decided
  on a five-way reach measurement recorded in #823 and pinned by a test whose name says so.
- **A private prefix is an *intermediate* here, so an unrouted `(prefix, final)` pair is
  unreachable rather than unhandled — and the difference is invisible (#824).** `vte` collects
  `0x3C..=0x3F` (`< = > ?`) into the same `intermediates` slice as the true 0x20..0x2F bytes, and
  `csi_dispatch` returns early on any intermediate it does not name. So a sequence in that family is
  not "missing from the `match`" — the `match` is never reached, and adding an arm for its final
  does nothing. DA2 (`CSI > c`) sat there from the beginning; the kitty `u` path had already opened
  one such pair, DA2 is the second, and **XTMODKEYS (`CSI > m`) is the third since #890**. Two consequences worth carrying: the fix is always a guard
  *above* the catch-all, keyed on the pair rather than on the prefix alone (a `.first()` match makes
  `CSI > $ c` DA2, which is what alacritty does and the other three references do not); and the
  remaining members are silent by construction, so their count is a measurement rather than a
  reading — the routed pairs are three of the ten `>` finals xterm routes, chosen by reach rather
  than completeness. Across this repo's captures `CSI > m` (XTMODKEYS) occurs 10 times against
  DA2's 5 — all `Pp = 4`, vim setting it at startup and clearing it on exit, the clear the more
  frequent (re-measured 2026-09-11 after #891's twentieth fixture; 4 and 7 when #890 chose on them,
  same order) — and `CSI > q`
  (XTVERSION) **once**, which still inverts the order the reference trees suggest. Rows in
  [`reference-facts.md`](../../agents/reference-facts.md). That count is what decided the order
  they were taken in: XTMODKEYS was the next one routed (#890) precisely because it was the
  highest, so **it is no longer in the tail** and `CSI > q` now leads what is. The unrouted rest
  is #47 tail. A count here is a claim about the corpus at one revision of it, and the
  paragraph below is what that costs when it is not re-measured.
  **The `> q` figure was `zero` here until 2026-09-11 and the corpus had contained one since
  2026-09-02** (#842's `tmux_clipboard.raw`); a fresh tmux attach recorded on the 11th emits it too,
  so tmux asks unconditionally. Two things follow for anyone quoting a count out of this corpus.
  It is only true of one revision of the corpus — re-measure rather than cite. And it is a **floor**,
  because all but one capture is *open-loop*: recorded under `script(1)` or a bare `expect`
  (`less_softwrap` and both `alt_resize_*` are the second kind), both of which copy bytes and
  answer nothing, so nothing an application sends only *after* a reply can appear in it. That is
  visible in the corpus rather than assumed — answering DA2 is measured (the entry below) to
  make vim ask ten `DCS + q` XTGETTCAP questions, and `DCS + q` occurs **zero** times across every
  open-loop fixture, four of which ask DA2.
- **What answering DA2 buys, measured as a control pair on a real pty (#824).** DA2 is the query vim
  uses to fill `v:termresponse` and identify what it is talking to. RHEL 9.2, vim 8.2,
  `TERM=xterm-256color`, 24x80, every other query answered identically in both arms, controls run
  before and after: no reply gives `ttymouse=xterm`, `ESC[>1;1500;0c` gives `ttymouse=sgr`. That
  is what the version number buys *directly*: legacy `xterm` mouse encoding cannot report a column
  past 223 or a release, and `sgr` has neither limit. **The larger effect is the second one**:
  answering makes vim ask ten more questions — XTGETTCAP (`DCS + q <hex> ST`) for `Co`, `ku`, `kd`,
  `kl`, `kr`, `k1`, `#2`, `#4`, `%i`, `*7`, the colour count and the arrow / function / shifted key
  codes, because a terminal produces different key codes in different modes. vim's `term.txt` gates
  that on "patchlevel 141 or higher"; measured rather than taken from the doc, the requests appear at
  `Pv` 276 and 1500 and not at 1, 94 or 95, which brackets the gate to (95, 276] — consistent with
  141 without pinning it. Stable across runs is *whether* they appear, not how many: one arm sent
  each capability once where every other sent it twice. **justerm answers none of them** — they fall
  to the same intermediate catch-all — so the capability is unlocked and then unanswered, which is
  #47 tail. modifyOtherKeys is *not* gated on any of it: vim emits `CSI > 4 ; 2 m` about 180 bytes
  before it asks. Why `Pp = 1` and `Pc = 0` is `architecture.md` § Hidden VT state; the reference
  rows are [`reference-facts.md` § Secondary device attributes](../../agents/reference-facts.md#secondary-device-attributes--report-yourself-do-not-impersonate).
- **One capture is closed-loop** (#891) — `vim_closed_loop.raw` holds all ten. How it was recorded,
  why the replies had to be the engine's, and why its bytes encode a consumer policy are
  [the capture corpus](capture-corpus.md)'s, not this territory's: this one only needs to know that
  a count from the other captures is still a floor.
- **An OSC payload arrives unbounded, and a handler that builds anything from one bounds it
  itself (#828).** Measured with a throwaway probe rather than read off the crate: `vte` is built
  with its default features, so its OSC accumulator is a `Vec<u8>` and **not** the
  `ArrayVec<_, MAX_OSC_RAW = 1024>` of its `no_std` path — a 4 MB `OSC 52` reaches `osc_dispatch`
  complete, as three params totalling 4 000 003 bytes. The consequence is easy to state backwards: a
  bound on a handler cannot stop the engine allocating, because the parser already did. What it
  stops is the *second* allocation — the decoded value and whatever the handler then hands a
  consumer. `MAX_CLIPBOARD_BASE64` is the only one today; the payloads `OSC 0/2`, `OSC 7` and
  `OSC 8` retain are bounded by nothing, which is a fact about this territory and not a claim that
  it is wrong.
- **The bytes are unbounded, and the fields are not.** `vte` records at most 16 field boundaries
  per OSC (`src/lib.rs:531-532` in 0.15.0 and on master), so what lies past the 16th `;` never
  reaches `osc_dispatch`. A payload cut there arrives looking exactly like a complete 16-field one.
  So the rejoin rule (#650, #880) recovers a payload only up to that bound. Measured on the engine
  (#840):
  - `OSC 8` URIs are complete up to 13 `;` and cut from 14.
  - `OSC 0`/`2` titles and `OSC 7` cwd values are complete up to 14 `;` and cut from 15.
  - `OSC 4` keeps its first 7 pairs, a correct prefix.
  - `OSC 52` cannot be cut into a valid value, because a rejoined `;` fails base64.

  The parser path Alacritty pins cuts at the same boundaries to the same byte lengths, so the
  cause is vte and not the handlers. Handing a handler a pre-split field array is where this
  differs from xterm.js and ghostty, whose handlers read the raw payload and which bound an OSC in
  bytes by dropping it.
  **Not guarded, by the maintainer's call.** Refusing at exactly 16 fields would drop the complete
  value on the boundary. And reach measured low: `ls --hyperlink` and penterm's bash, zsh, fish and
  pwsh OSC 7 integrations percent-encode `;`, and only its cmd.exe integration sends a raw path.
  **One payload is marked instead (#964):** `TermEvent::Notification` (`OSC 9`/`777`) carries
  `maybe_truncated`, set when all 16 fields arrived — complete at 14 `;`, a prefix from 15. A
  mark drops nothing, so it sits beside that call rather than reversing it; see
  [events and replies](events-and-replies.md).
- **`CAN` and `SUB` cancel an OSC; they do not end it (#970).** `vte` ends the OSC string on either
  byte by calling `osc_dispatch` and *then* `execute`, with `bell_terminated = false` exactly as for
  `ST`, so inside the handler a cancelled OSC and a finished one are the same call. xterm cancels
  (see [reference facts](../../agents/reference-facts.md), the `OSC 52` table's `vte` dispatch row
  and the two under it). The engine decides it **before** dispatch: `Engine::feed` splits its input at
  each cancel byte (`memchr2`) and advances that byte alone with `Term::cancel_byte_in_flight` raised,
  so an `osc_dispatch` inside that one-byte advance is by construction the one it cancels.
  **The alternative that was written first and dropped:** hold every non-`BEL` OSC and apply it at the
  next callback. It works, but holding means copying the fields, which buys a hostile `OSC 52` exactly
  the second allocation `MAX_CLIPBOARD_BASE64` refuses. The split copies nothing and leaves `vte`'s run
  between cancel bytes untouched; the scan measured ~1 ms per 32 MB on the five bench inputs, against a
  `feed` of ~500-700 ms for the same bytes. The pathological stream is all cancel bytes, one `advance`
  each: measured ~11x slower than before (≈60 vs ≈700 MB/s) and still faster per byte than ordinary
  recorded output, so it buys an attacker nothing. An OSC ended by `ESC` and *then* a cancel stays
  applied — xterm would cancel it, but a bare-`ESC` ending is relayed by the rule above. Not covered:
  the visible error character (xterm draws one for `SUB` at every id from 100 and for `CAN` at
  VT100-class ids), and a DCS cancelled the same way, which reaches `unhook` rather than this.
- **Tab stops are explicit per-column state**, not a modulo: HTS sets, TBC clears, default every
  eighth column. A modulo would be wrong the moment an application moves one — and since #826 that
  is two verbs' problem rather than one, because `CBT` walks the same table backwards. The two walks
  are written as mirrors for exactly that reason: a count repeats the *walk*, so the directions
  cannot disagree about where a stop is, and `HT` followed by the same number of `CBT` returns to
  where it started. `CHT` (#898) is `HT` counted, so it adds no third walk. One thing they
  deliberately do **not** mirror is the deferred wrap — see
  [cursor position](cursor-position.md), where justerm turns out to be the outlier against all four
  references.
- **The scroll region redefines what "scroll" means.** DECSTBM changes which rows `IND` / `RI` /
  `LF` move and which leave the screen, so nearly every vertical-motion verb reads it — since #898
  the relative moves too, by the rule in [cursor position](cursor-position.md).
- **RIS and DECSTR are two reset strengths** and the split is itself hidden state — what each does
  *not* clear is the part that matters. **Neither says anything about the palette, and the silence is
  a decision (#835).** An application can redefine the ANSI table with `OSC 4`, or the
  foreground/background/cursor with `OSC 10`/`11`/`12`, and a reset announces no `Reset*` event — so a
  consumer that honoured the redefinition keeps it, including after the application that set it has
  exited. The engine holds no palette, which is *why* the silence is the consumer's problem rather
  than a no-op, and equally why an announcement could only be unconditional. xterm is the one
  reference that resets its own table, on **both** strengths (`charproc.c:14366`, in the
  `if_OPT_ISO_COLORS` block above the `if (full)` split). Three grounds decided against following it,
  and the head-count is not one of them. **ADR-0004's tie-breaker does not reach the question**: it
  defers to xterm where the *spec* mandates what alacritty omits, and `OSC 4`/`104` are xterm's own
  invention over a table DEC never defined, so no DEC text says what `RIS` does to one — a genuine
  ambiguity, which that ADR routes the other way. (The "the inventor owns the semantics" move that
  settled `XTREVWRAP` does not transfer: that was an invented sequence's own meaning, this is what a
  **DEC** sequence does to state the invented one left behind.) **terminfo then settles the reach
  from outside every implementation**: xterm's own `xterm-256color` spells `rs1=\Ec\E]104\007` and
  `linux` spells `rs1=\Ec\E]R` — both append an explicit palette reset *after* `RIS`, which neither
  would need if `\Ec` implied one — so the reset an application actually performs already arrives as
  `OSC 104` and is already relayed. **And the one reference built in this shape declines**: ghostty
  holds the palette *and* announces every change across a consumer boundary, and its override mask
  would make a selective announcement free — its `fullReset` still sends none. Two neighbouring facts
  that are easy to merge and must not be: the **pen** half of xterm's block (`reset_SGR_Colors`) is
  already mirrored on both strengths, and the **dynamic** colours are restored by no reference at
  all, xterm included. Rows in [`reference-facts.md`](../../agents/reference-facts.md); the reversal
  criterion is on #835.
- **The write path was deliberately not extracted** by #584: splitting by VT verb would scatter
  ADR-0025's row and wide-pair invariants across files, which is the failure that record exists to
  name.

## Code

- `justerm-core/src/term/dispatch.rs` — the `Perform` implementation: `print`, `execute`,
  `csi_dispatch`, `esc_dispatch`, `osc_dispatch`, `unhook`; the DEC private modes
  (`set_dec_private_mode`), the VT52 sub-parser (`vt52_dispatch`) and the OSC 8 id lookup
  (`osc8_link_id`, `link_for_id`). A child of `term`, so it drives the write path's private methods
  directly
- `justerm-core/src/term.rs` — the verbs dispatch calls and the mode flags they read: `put_tab` /
  `put_back_tab` / `put_forward_tabs`, and the write path, which #584 keeps in one file for
  ADR-0025. `place_grapheme` is the print path below the charset translation, which `repeat_last`
  re-enters; `repeat_anchor` is the anchor the census above keeps in step
- `justerm-core/src/lib.rs` — `Engine::feed`, which is only `parser.advance(&mut term, bytes)`; the
  `Parser` and `Term` are separate fields because `advance` borrows both mutably
- `docs/architecture.md` §"Hidden VT state" — the catalogue of what is modelled, partly modelled and
  not modelled

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated** (each row carries a `file:line` at a
recorded SHA; a paraphrase drops the pin).

- [Soft wrap is a row property](../../agents/reference-facts.md#soft-wrap-is-a-row-property) — which
  verbs end a wrap, per verb, in two references
- [What a blanked / freed cell is made of](../../agents/reference-facts.md#what-a-blanked--freed-cell-is-made-of)
  — what an erase fills with

ADR-0004 is the rule that governs how these are read: a reference's **silence** is not permission,
and where it merely omits a spec'd behaviour the spec wins. That is a different stance from "match
the reference", and it is the one this territory operates under.

## Cross-cutting invariants

- [RIS keeps configuration, drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — `RIS` and `DECSTR` are verbs here, and `full_reset` rebuilds `Term` from the constructor, so
  every field the rebuild must carry across is decided by that note's table

## Blast radius

Every stateful territory downstream, because this is where state is written.

- [soft wrap](soft-wrap.md) · [wide glyph](wide-glyph.md) — the verbs are the subjects of ADR-0025's
  per-verb table
- [cursor position](cursor-position.md) · [pen](pen.md) — moved and stamped here
- [damage](damage.md) — every mutation records a span; a verb that forgets is invisible
- [marker](marker.md) · [selection](selection.md) — the anchor-maintenance calls sit inside these
  verbs, line for line beside each other
- [reflow](reflow.md) — `resize` lives here too, and was deliberately not extracted
- [input encoding](input-encoding.md) — the modes it reads are set by *these* verbs, which is why the
  encoder cannot live in the consumer
- [the capture corpus](capture-corpus.md) — a verb whose sequence kind moves a surface on a recorded
  stream fails that capture's `*.ignored.golden` when it stops (#895); which verbs that does not
  cover is recorded there

## Known holes / open

- **The hidden-state catalogue is 30 entries and no territory owns most of them.** They are
  distributed across this map by subject, but the catalogue itself has no home in the graph — it is a
  section of a spec file, and the only artifact that knows what is *not* built.
- **Conformance is accumulated, never declared complete.** Coverage grows dogfood-first, so
  "not implemented" is a normal state here rather than a defect — and nothing distinguishes
  *deliberately absent* from *not reached yet* except prose. The perpetual tail is tracked in #47.
- **Modes tracked but not acted on have no single list.** Each is documented at its field; a consumer
  discovering which flags it must honour has to read the struct.
- **ADR-0004's tie-breaker is stated once and applied everywhere.** Whether a given verb followed the
  spec or the reference is not recorded per verb.
