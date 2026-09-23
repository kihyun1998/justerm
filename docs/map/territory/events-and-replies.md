# Territory — consumer events & query replies

## What it is

Two outbound channels that are **not** the frame. Events are point-in-time notifications the engine
accumulates while parsing (bell, title change, clipboard request); replies are bytes the application
asked for and expects back on the PTY (DA, DSR, DECRQM, colour queries). Both are **drained**, never
pushed.

The second territory this map missed entirely on its first pass — nothing about it is cell state, so
nothing about it appears in the frame.

## Governing decisions

**One, and it covers a property of the payload rather than the channel itself.**

- [**ADR-0029 — a coordinate carries its instant, or is re-asked**](../../adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md)
  — its **D4** is this channel's clause: an event can only *carry*, because its payload is detached
  from its instant by the queue before any consumer sees it, and what it carries is checked against
  its pull-side sibling rather than against a list of axes. That settles what a coordinate-bearing
  variant owes. It does **not** settle what earns a place on this channel at all — see the hole below
- [ADR-0020 — what qualifies for the frame snapshot](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  is the record that explains why these are *not* in the frame: its first rule is state versus event,
  and an event fails it by construction. It decides the frame's membership, not this channel's shape
- `docs/architecture.md` §"Hidden VT state" carries both contracts — *"consumer events are
  pull-drained, and OSC 8 is not one of them"* and *"query replies are an outbound channel, drained
  pull-style and kept apart from events"*

## Design model

- **A marker's birth and death are both events, and the pair is load-bearing since #490.**
  `MarkerDisposed` was enough while every live marker rode every frame — absence was observable.
  Once the index is *pulled*, a population that only ever shrinks is silently wrong, so
  `MarkerCreated { id, line, kind, evicted_total, epoch }` is the mirror. ADR-0020 R1 is why neither
  is a frame field: an appearance and a disappearance are occurrences, not state.

- **An event that carries a coordinate carries the instant it is true at, or it carries nothing
  (#737).** This entry read *"`line` is absolute on the same basis the frame header reports"* until it
  was measured false. A single `feed` can create a marker and then evict, so the frame closing that
  batch reports an origin the event's line predates; two batches with the same mark and the same three
  evictions in opposite orders were identical on **every** channel a consumer can see — the event
  line, both frame bases, the epoch, `marker_count` — and three lines apart in truth. The repair is
  the pairing `MarkerIndex` already uses: the line travels with its `evicted_total`. The general
  shape is what makes this a design-model entry rather than a bug note — a frame is a *snapshot* and
  carries its own basis by construction, while an event is a *point in time* whose payload outlives
  the instant that gave it meaning.
  **And "the instant" is not one scalar (#741).** The repair above was written as *carry the basis*,
  which is the axis the reproducing batch happened to move; markers also move **non-uniformly**, and
  a birth queued when `marker_epoch` bumps describes a buffer that no longer exists. The rule that
  needs no forcing case is stated against the sibling rather than the axis: **an occurrence carries
  every scalar the pull answering the same question carries**, here `MarkerIndex`'s full
  `(line, evicted_total, epoch)`. This channel has now had the same fact re-derived at two
  granularities in one day, which is why the invariant note below exists rather than a third bullet.

- **One event, two producers — `Title` names a state, not an act (#823).** XTWINOPS `CSI 22 t` /
  `CSI 23 t` gave this channel its first event with more than one origin: a title *pop* emits the
  same `TermEvent::Title` an `OSC 0`/`OSC 2` does. That is deliberate and it is what both references
  carrying a title stack do (xterm.js's `setTitle` fires `_onTitleChange`; alacritty's `pop_title`
  routes through `set_title`) — a second event would ask every consumer to learn a distinction it
  has no use for. The cost is a **tense change in the contract**: the event means *"the title is now
  this"*, and a consumer reading it as *"the application just chose this"* is wrong on the restore.
  Two consequences that are not obvious from the sequence alone, both measured on real ptys: every
  application pushes at **startup, before setting a title**, so a pop routinely restores the *empty*
  string — which means "go back to your default", not "show a blank title"; and a session that never
  sets a title still produces one `Title("")` per pop, which is why the `cursor_color_nvim.raw`
  capture recorded for #832 gained two events it did not have.
- **A request the engine cannot perform is still an event, and `OSC 52` is the clearest case
  (#828).** The clipboard is the consumer's by definition — ADR-0017 names it in the else-list — so
  what rides this channel is not a clipboard operation but the *fact that an application asked for
  one*. The engine's half is the sequence: recognise it, decode the base64, relay
  `ClipboardStore { target, text }`, and answer a `QueryClipboard` only when the consumer calls
  `report_clipboard`. **Dropping the event is how a consumer refuses**, which is why there is no
  allow/deny knob here. The security property falls out of the same split rather than being added
  to it: the engine holds no clipboard, so a query it is never asked to answer discloses nothing,
  and a *read* is refusable independently of a *write*.
  **The reason attached to that used to be wrong, and the correction is worth keeping (#841,
  measured 2026-09-10).** This entry read *"alacritty's four-state `osc52` config has no
  counterpart — alacritty is the consumer"*. Re-opened at the pin, alacritty gates the sequence
  **inside its engine crate** (`alacritty_terminal/src/term/mod.rs:1706`, `:1727`) with the policy
  *injected across the crate boundary* — `Osc52` is a field on `alacritty_terminal`'s `Config`
  (`:353`), written by the app at `alacritty/src/config/ui_config.rs:125`. That is ADR-0017's own
  shape, not a luxury bought by being the whole terminal, so it was never a reason we *could not*
  do the same. The conclusion is unchanged and rests on the ADR alone: policy is the consumer's,
  and an engine that touches no clipboard buys nothing by holding a gate in front of a relay.
  **An empty payload clears** — the spec's `Pd` *"becomes the new selection"* (`ctlseqs.txt:2166`)
  and xterm clears before appending (`misc.c:3410`); ghostty pins the same input as *"clear
  clipboard"* (`src/terminal/osc/parsers/clipboard_operation.zig:93`). It is **not** the spec's
  *"neither a base64 string nor `?`"* clause (`:2174`), which the engine diverges from: an empty
  payload is a well-formed encoding of no bytes and never reaches that sentence.
- **A notification is relayed, not read (#964).** `OSC 9` and `OSC 777` become one
  `TermEvent::Notification { sequence, payload, maybe_truncated }` with the payload as sent after
  the code — `params[1..]` rejoined, under the same no-field guard as the title and cwd arms. The
  engine does not tell iTerm2's free text from ConEmu's numbered subcommands, nor split
  rxvt-unicode's `notify;title;body`: that is reading, and reading is the consumer's (ADR-0017).
  **The name overstates the content, and the docs say so.** Both codes are families: ConEmu's
  `9;9;path` is a working-directory report a shell may send every prompt, and does *not* arrive as
  `Cwd`; `777` is urxvt's extension dispatch, where only `notify` is a notification. A consumer that
  shows every payload not starting `4;` toasts a path per prompt, which is why the doc-comments name
  the families rather than two forms.
  The references split on exactly that line — ghostty reads both in its engine, xterm.js leaves
  them to an addon, alacritty drops them — in
  [notification sequences](../../agents/reference-facts.md#notification-sequences--osc-9-and-osc-777-964-verified-2026-09-23).
  **`maybe_truncated` is the maintainer's call (2026-09-23), and theirs to reverse.** Shown three
  options — flag, lift the 16-field bound for these two codes (a shadow capture beside `vte`, or a
  `vte` change), or relay unmarked as `Title` does — they chose the flag. What it was decided on: the
  bound cannot be *detected*, only *reached* (see [VT interpretation](vt-interpretation.md), #840),
  so the flag is set at 16 fields and is a false positive on the payload with exactly 14 `;`. It
  does **not** reopen #840's call, which declined a guard that *drops* the value; this drops
  nothing. What it did **not** cover: whether `Title`/`Cwd`/`OSC 8` should carry the same flag —
  those stay unmarked, and #840's reasoning still governs them.
- **`OSC 52`'s base64 is decoded in core, by a local implementation** (`base64.rs`, #828). Decoding is
  mechanism — it depends only on the byte stream — and pushing it outward would make every consumer
  carry base64 to use an engine feature; alacritty decodes in its core too, ghostty one layer out.
  It is not a dependency because `base64` is not in the workspace lockfile at all, so it would be a
  genuinely new supply-chain entry for `justerm-core` and everything downstream, against a short
  dependency list whose entries each do something hard; RFC 4648 is a 64-entry alphabet, proven here
  against the RFC's §10 vectors. The decoder refuses what makes the decoded bytes ambiguous — a byte
  outside the standard alphabet (`-`/`_` are refused, not mapped), an impossible length, interior
  padding, non-zero bits in a final partial group, partial padding (`Zg=`) — and **accepts missing
  padding**, which diverges from **both** references (alacritty requires canonical padding, ghostty
  rejects unpadded as `InvalidPadding`). The reach of unpadded emitters is unmeasured; the asymmetry
  decides it, since rejecting costs a silently dropped clipboard and accepting costs nothing. The
  encoder always pads — the laxity is for what is accepted, never for what is sent.
- **The widget's half of `OSC 52` declines the policy too, one layer out** (`clipboard.ts`, #841).
  It holds no clipboard and no permission model: it routes to a `ClipboardProvider` the embedder
  supplies, and with none it does nothing. The injected-provider shape is xterm.js's
  `addon-clipboard` (`ClipboardAddon.ts:13-16` @ `699f553`, no policy enum anywhere, handler
  registered unconditionally at `:20`) — the shape only. Two differences: *layer* (the addon is a
  separate package; xterm.js's `Terminal` implements no `OSC 52`, where this widget gates it on
  `TerminalOptions.clipboard`), and *default posture* (the addon defaults to
  `BrowserClipboardProvider`, permissive both ways, `:15`, `:71-79`; this widget's no-provider
  default refuses both, as xterm(C) does — `DEF_ALLOW_WINDOW` `False`, `main.h:119`,
  `DISALLOWED_PASTE64` `",SetSelection,GetSelection"`, `:159`, `:170-172` @ `xterm-410`).
  **Read and write are refused independently.** Defaulting writes open and reads shut is **2 of 4**,
  not unanimous (an earlier count said 3-for-3): alacritty's default is `Osc52::OnlyCopy`
  (`term/mod.rs:377-380` @ `852e971`) and ghostty ships `clipboard-write: allow` beside
  `clipboard-read: ask` (`src/config/Config.zig:2379-2380` @ `e6e26e1`) — both native applications
  with a config file; the two embeddable references are symmetric (xterm.js allows both, xterm(C)
  denies both). **Focus is not a gate — the maintainer's call, 2026-09-10, and theirs to reverse.**
  alacritty checks focus both ways (`alacritty/src/event.rs:1903`, `:1908` @ `852e971`); ghostty
  and xterm.js do not — 1 of 3. Knowing focus and deciding refusals from it are different acts, and
  only the first is the widget's; an embedder can refuse on focus inside its provider. **A refusal is
  silence at all four references** — alacritty `debug!` and return (`term/mod.rs:1727-1730`),
  ghostty `log.info` and return (`src/Surface.zig:5837-5844`), xterm(C)'s reply block sits inside
  `AllowWindowOps` (`misc.c:3378`), xterm.js leaves `OSC 52` unhandled without the addon; none sends
  a "denied" reply. **A provider's rejection is a refusal, swallowed**; xterm.js's addon returns the
  promise to the parser, whose `WriteBuffer.ts:283-286` deliberately lets it throw as an uncaught
  error while the parse resumes. **A browser read hangs rather than rejecting** — measured
  2026-09-10, Chromium over `localhost`: `readText()` with `userActivation.isActive` `true` stayed
  pending past 2000 ms, and with it `false` (the 5 s window waited out, `hasBeenActive` `true`)
  past 3000 ms, while `writeText()` resolved; `permissions.query` read `clipboard-read: "prompt"`,
  `clipboard-write: "granted"` throughout. The second row is the one that matters, since an `OSC 52`
  query arrives from the stream and never with a gesture. So a controller promise may never settle
  (hence `ClipboardController.dispose`), and the widget imposes no deadline — a bounded read is the
  provider's policy. Not measured: a never-activated page, a denied prompt, other browsers. And
  `""` and `null` are different answers: an empty clipboard still replies, because the application
  is blocked waiting — ghostty says so outright (`src/Surface.zig:5945-5946` @ `e6e26e1`).
- **`ClipboardTarget` models three of `OSC 52`'s selectors and keeps `p` apart from `s`** (#828). The
  field admits `c`, `p`, `q`, `s` and eight cut buffers; the three a consumer can act on are modelled
  and the rest ignored, as alacritty ignores them, rather than folded onto a neighbour, which would be
  the engine inventing an equivalence. ghostty folds every unrecognised kind onto the clipboard
  (`src/termio/stream_handler.zig:1013`) — read its `switch`, not the comment above it, which claims
  it always uses the standard clipboard. A first draft collapsed `p` and `s` as alacritty does, and
  that is wrong *here* though not there: alacritty replies with the byte the application sent, while
  this engine hands the consumer a value and takes it back at `report_clipboard`, so a collapse would
  answer `ESC ] 52 ; s ; ?` naming `p`. The type is `#[non_exhaustive]` because `q` and the cut
  buffers remain unmodelled. ghostty converges on the same three open members
  (`src/terminal/clipboard.zig:2`, `Location { standard, selection, primary, _ }`) — which #843 had
  recorded as impossible, on the premise that Zig has no non-exhaustive enum; a trailing `_` is one.
  An OSC ended by a bare `ESC` answers `ST`, as xterm hardcodes (`charproc.c:8964`) and ghostty's
  `Terminator.init` returns for a missing byte (`src/terminal/osc.zig:263`).
- **`OSC 52` diverges from the spec twice, and both are argued here (#828).** An **empty target is
  the clipboard**, not the spec's `s0`. Neither half of `s0` is representable: the cut buffer is
  unmodelled outright, and `s` is *merely* unmodelled — xterm resolves it through a user resource,
  which is policy ADR-0017 puts in the consumer, and DECSET 1041 sets that resource from the stream,
  so an engine tracking 1041 could resolve it; justerm declines to. xterm-as-shipped reads the empty
  field as PRIMARY, so this diverges from xterm's default and not merely from its manual. What
  decides it is independent lineages — fewer than a naive count, because alacritty never sees an
  empty field (`vte` substitutes `c` first), so those two are one — ghostty and xterm.js's clipboard
  addon being the other two: three lineages, one answer. And the emitter agrees: `tmux`'s
  `set-clipboard` is documented as setting the terminal clipboard, and `tmux` 3.2a is measured
  sending the empty form. **Malformed in, nothing out**: the spec clears on a payload that is neither
  base64 nor `?`; clearing is destructive, and inferring one from bytes the engine could not parse
  lets line noise wipe what the user copied by hand. xterm has no validator to disagree with — it
  *filters* — so the disagreement is about what to accept, and this engine is the stricter on
  purpose: a filter hands the consumer text assembled from bytes the application did not send.
  Non-UTF-8 is refused on the same principle, since every text surface this crate publishes is UTF-8.
  The payload is `params[2..]` rejoined, #650's rule: reading `params[2]` alone would take a payload
  containing a `;` and *successfully* decode its well-formed first piece — a truncated clipboard, the
  one failure this handler must not have. Rejoined, the stray `;` reaches the decoder and is refused.
  A multi-target list like `pc`, which the spec permits (`ctlseqs.txt:2156`), is dropped rather
  than approximated: honouring one target of two is the same defect as truncating a payload, one
  axis over, and `vte`/alacritty's first-byte-wins would do exactly that.
  Rows in [`reference-facts.md`](../../agents/reference-facts.md#osc-52--where-the-references-converge-and-the-two-places-the-spec-is-not-followed-828-2026-09-02).
- **`OSC 52`'s size bound is checked on the fields, before the join** (#828) — `join` is itself an
  unconditional full copy, so checking after it would let a hostile payload buy a second allocation
  the size of the first. The bound is `MAX_CLIPBOARD_BASE64`; the parser's own allocation is
  unbounded ([VT interpretation](vt-interpretation.md)).
- **DECRQM answers from the model it has, not from a flag kept for the query** (#27). DECCOLM (`?3`)
  is derived from the actual width, never a tracked flag — a flag would lie if the consumer ignored
  the resize request (#82). Mouse tracking is one enum because the levels are mutually exclusive,
  so `?1000` queried while `?1002` is active reports *reset* — faithful to that model.
- **A `report_*` takes back what it needs rather than the engine remembering it.**
  `report_clipboard(target, text, terminator)` follows `report_palette_color(index, spec,
  terminator)`: the consumer names the target it is answering about. alacritty is the alternative and
  shows the cost — its query captures target and terminator in a closure
  (`alacritty_terminal/src/term/mod.rs:1740`), which is hidden state plus a question about
  interleaved replies, bought for something the consumer already holds.
- **The terminator travels the same way, and #836 settled that it must (2026-09-02).** It was the one
  fact of the exchange no `report_*` caller could supply, because the engine discarded
  `bell_terminated` at the parser boundary. Now a `Query…` event carries it and the matching
  `report_*` hands it back, on all five OSC reply paths. **The deciding property is this channel's,
  not the sequence's**: `drain_events` returns a *batch*, so two queries can be outstanding at once
  and answered in either order, and one remembered scalar cannot say which exchange it belongs to.
  That is the same shape [`a coordinate carries the instant it is true at`](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  states for coordinates — an occurrence's payload is detached from its instant by the queue, so it
  can only carry — reached independently by a fact that is not a coordinate. xterm is the
  counterexample worth knowing: it *does* store the terminator, but only for `OSC 52` and only
  because its selection retrieval is asynchronous, in a single scalar with the collision this
  channel's batch would provoke. **The spec settles the direction, not only the tally**:
  `ctlseqs.txt:2020` — *"when returning information, uses the same terminator used in a query"* —
  which ADR-0004 ranks above every implementation. `OscTerminator` is exhaustive at two members, and
  ghostty converges on both the partition and the closure (`src/terminal/osc.zig:252`, a two-member
  enum with no trailing `_`, Zig's open-enum marker). The closure rests on the input space: the 8-bit
  C1 `ST` (`0x9C`) is absent because `feed` does not treat a lone `0x80..=0x9F` byte as a control, not
  because the spec stops at two. Pins: [the VT-gap sweep](../../agents/reference-facts.md#the-vt-gap-sweep-of-2026-08-26--five-decisions-the-references-settled-823832-verified-2026-08-26).
- **`TermEvent` is `#[non_exhaustive]`, by the maintainer on 2026-09-02**, while a slice was adding
  two variants. What decided it was `CLAUDE.md`'s identity statement — `justerm-core` is a reusable,
  independent crate — which says there are consumers this repo cannot edit; a crate that were
  internal to penterm would want the opposite. Three measurements behind it: crates.io reverse
  dependencies **zero** (248 downloads over 11 versions), so there was nothing external to observe;
  cost in this workspace **zero** (`cargo test --workspace`, 87 suites, and `clippy -D warnings`
  green; `justerm-web`'s `events.ts` mirrors the union by hand and never had a compiler relationship
  to break); and the window **closes at 1.0.0**, after which adding it is itself breaking, while the
  VT tail adds variants at a measured rate (three, then two, in the two slices before). **The argument
  that lost is a real cost**: an exhaustive match tells a consumer a new event exists — penterm's
  `route_event` carries a comment on each dropped arm, written by someone the compiler had just
  informed — and that signal now has to come from release notes. It lost because this is a
  notification channel where ignoring an unknown event is documented as safe.
- **Pull, not push — and the alternative is named.** The engine queues during `feed` and the consumer
  takes with `drain_events`, mirroring `damage` / `frame` / `reset_damage`. No callback crosses the
  boundary, so the engine stays decoupled from the consumer's event loop. alacritty's `EventListener`
  is the push model this deliberately does not copy, because it would couple them.
- **Two channels, kept apart on purpose.** Events describe *something happened*; replies are **bytes
  the application is waiting for** and must reach the PTY, in order. Merging them would make a
  transport obligation look like a notification a consumer may ignore.
- **OSC 8 hyperlinks are deliberately absent.** A hyperlink is per-cell state — *which cells are
  links* — not a point-in-time event, so it is modelled like graphemes in its own slice. This is the
  clearest worked example of ADR-0020's state-versus-event rule, and it lives in a module comment.
- **Draining is destructive and the consumer owns the cadence.** Nothing bounds the queue if a
  consumer never drains; the engine has no timer and no back-pressure of its own.
- **Replies are raw bytes, not typed.** The engine encodes; the consumer writes them to the PTY
  without interpretation — the same "mechanism here, transport yours" split the wire format uses.

- **`events.ts`'s `ClipboardTarget` and `Terminator` are not on published-surface's hand-copied
  roster — the maintainer's call, 2026-09-10, and theirs to reverse.** That roster lists value spaces
  this package transcribes from core that reach it through the decoder's wire lane, where
  `justerm-wasm-decode` is their proper home; `TermEvent` crosses no decoder lane — it arrives on a
  side channel the embedder implements, whose only type declaration is this package's — so a string
  union here is the right home.

## Code

- `justerm-core/src/event.rs` — `TermEvent`, the event surface
- `justerm-core/src/term/replies.rs` — `Term::drain_events`, `Term::drain_replies`, and the
  `Term::report_*` methods that queue replies (`report_background`, `report_foreground`,
  `report_cursor_color`, `report_palette_color`, `report_color_scheme`, `report_clipboard`);
  `Term::clipboard` is the `OSC 52` half that queues onto both channels. **`report_*` is not the
  whole producer set**: a query the engine can answer alone queues its reply without calling back
  out — DA1 and DA2 inline in `csi_dispatch` ([VT interpretation](vt-interpretation.md)), DSR, the kitty
  flags query and DECRQM through `device_status_report` / `kitty_dispatch` / `decrqm` beside the
  `report_*` methods. The split is *who holds the
  answer*: a colour is the consumer's, so it must call back in; a device identity is the engine's —
  and `OSC 52` is the sharpest case of the first, since the engine holds no clipboard *by design*
- `justerm-core/src/lib.rs` — `Engine::drain_events`, `Engine::drain_replies`
- `justerm-core/src/base64.rs` — the RFC 4648 transform `OSC 52` needs in both directions, kept in
  the engine because it is mechanism and kept out of the dependency list because it is small
- `justerm-web/src/events.ts` — the widget's mirror of this channel. **The narrowing moved in
  #841, and where it moved to is the point**: the `TermEvent` *union* now carries the `OSC 52`
  pair too, because a backend has one stream to push down. What stays narrower is
  `EventHandlers`, the *notification* surface — title, bell, cwd and, since #964, the
  `OSC 9`/`OSC 777` notification — so a new event here still does not automatically owe a
  callback; it owes a decision about which of the two surfaces it belongs on. `NotificationEvent`
  took the callback because the consumer is only told: no reply is owed
- `justerm-web/src/clipboard.ts` — the `OSC 52` consumer half (#841). `ClipboardController` takes
  the pair off that same subscription, routes it to an embedder-injected `ClipboardProvider`, and
  answers a query on a `ClipboardPort`. The widget holds no clipboard and no policy: with no
  provider it does nothing in either direction. **The first request→answer seam in that package**
  — its six existing ports are one-way commands, and core's other four `Query…` events are still
  unwired. It also **ends what it holds**: an in-flight read is already past the subscription, so
  `Terminal.dispose()` calls `ClipboardController.dispose()` to latch the landing

## Reference behaviour

In `docs/agents/reference-facts.md` — **linked, never restated**.

- [Secondary device attributes — report yourself, do not impersonate](../../agents/reference-facts.md#secondary-device-attributes--report-yourself-do-not-impersonate)
  — what each reference puts in a DA2 reply's three fields, which of them gate on the first
  parameter, and the one field where justerm follows none of the majority's *reasons* even though it
  matches their value (#824)
- [Notification sequences — `OSC 9` and `OSC 777`](../../agents/reference-facts.md#notification-sequences--osc-9-and-osc-777-964-verified-2026-09-23)
  — which trees read the two codes in the engine and which leave them to the consumer (#964)

alacritty's `EventListener` is still named as the rejected alternative in a module comment and is
still **unpinned** — a rejected design is exactly the claim worth pinning, since it is the argument
for the current one.

## Cross-cutting invariants

- [RIS keeps configuration and drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — both queues survive `ESC c`, and this is the only territory where the reset *adds* to one: every
  marker's disposal is announced into `events` before the rebuild, so the consumer drops decorations
  that now name nothing. That disposal is the **only** thing the reset appends, which #835 tested
  rather than assumed: the palette is the other consumer-held thing a reset invalidates, and it is
  deliberately announced by neither strength
- [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  — this is the channel where it bites hardest, because an occurrence's payload outlives the instant
  that gave it meaning and the frame's basis does not reach here. `MarkerCreated` is the worked case
  (#737), and the note lays the three channels side by side, which is the only view that shows two of
  them silent

## Blast radius

- [frame](frame.md) — the boundary partner. Anything that fails ADR-0020's state test lands here
  instead, so the two are decided together
- [hyperlinks](hyperlinks.md) — the worked example of that split: per-cell state, not an event
- [input encoding](input-encoding.md) — replies travel the same direction as encoded input and share
  the consumer's PTY write path, but are generated by parsing rather than by a user action
- [widget lifecycle](widget-lifecycle.md) — the consumer must drain both on a cadence it chooses, and
  a reply that is dropped is an application hang rather than a missing notification

## Known holes / open

- **Still no record for either channel's *membership*.** ADR-0029 (above) reached this territory in
  2026-08-06, but only for what a coordinate-bearing payload owes — the channels themselves are
  unrecorded, and one of them carries a *response obligation*: a dropped reply hangs the application
  waiting for it.
- **No bound on either queue.** A consumer that never drains grows memory with no signal, and no
  document states whose problem that is.
- **The web `ClipboardTarget` union is closed while core's is `#[non_exhaustive]`**, and
  `ClipboardController` passes the target to the provider unexamined — there is no decline arm for a
  target it does not know. No reachable consequence until core names `q` or a cut buffer, and
  nothing here gates that day.
- **The rejected push model is unpinned**, and it is the whole argument for the pull design.
- **`TermEvent`'s membership has no record.** What earns a place as an event — as opposed to frame
  state or a reply — is answered by ADR-0020 for the frame and by nothing for this channel.
