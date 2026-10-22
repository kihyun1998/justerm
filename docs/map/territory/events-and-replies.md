# Territory — consumer events & query replies

## What it is

The engine's two outbound channels that are **not** the frame, kept in one note because one loop joins
them. **Events** (`TermEvent`) are point-in-time notifications queued while parsing: bell, title,
cwd, notifications, colour and palette changes, a column-mode request, marker births and deaths,
and the `OSC 52` clipboard pair. **Replies** are bytes an application asked for and is blocked
waiting on (DA, DSR, DECRQM, the kitty flags query, colour and clipboard answers). Both are
**drained**, never pushed. The loop: a query the engine cannot answer alone leaves as a `Query…`
event, and the consumer answers it by calling a `report_*` method, which queues reply bytes. The
territory also holds `OSC 52` end to end — core's decode (`base64.rs`) and relay, and the web
widget's consumer half (`clipboard.ts`) and event mirror (`events.ts`).

## Governing decisions

- [**ADR-0029 — a coordinate carries its instant, or is re-asked**](../../adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md)
  — its **D4** is this channel's clause: an event can only *carry*, because the queue detaches its
  payload from its instant before any consumer sees it, and what it carries is checked against its
  pull-side sibling rather than a list of axes. It settles what a coordinate-bearing variant owes,
  not what earns a place on the channel (see Known holes).
- [ADR-0020 — what qualifies for the frame snapshot](../../adr/0020-what-qualifies-for-the-frame-snapshot.md)
  — explains why these are *not* in the frame: its R1 is state versus occurrence, and an event fails
  it by construction. It decides the frame's membership, not this channel's shape.
- [ADR-0017 — mechanism in core, policy in the consumer](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md)
  — names the clipboard in the consumer's list, and puts every answer the engine cannot know (a
  colour, a clipboard) behind a `report_*` call.
- `docs/architecture.md` §Hidden VT state carries both contracts — *"Consumer events are
  pull-drained, and OSC 8 is not one of them"* and *"Query replies are an outbound channel, drained
  pull-style and kept apart from events"*.

## Design model

From the source and the records above; dated calls are attributed.

**The two channels**
- **Pull, not push — and the alternative is named.** The engine queues during `feed`; the consumer
  takes with `drain_events` / `drain_replies`, mirroring `damage` / `frame` / `reset_damage`. No
  callback crosses the boundary, so the engine stays decoupled from the consumer's event loop.
  alacritty's `EventListener` is the push model this deliberately does not copy (`event.rs` module
  doc; unpinned).
- **Two channels, kept apart on purpose.** Events say *something happened*; replies are bytes the
  application is waiting for and must reach the byte source, in order. Merging them would make a
  transport obligation look like a notification a consumer may ignore. Replies are raw bytes, not
  typed: the engine encodes, the consumer writes them back uninterpreted.
- **Draining is destructive and the consumer owns the cadence.** Nothing bounds either queue if a
  consumer never drains; the engine has no timer and no back-pressure. A
  [Session](session.md) drains both on every pump; its drivers decide when that is.
- **OSC 8 hyperlinks are deliberately absent.** A hyperlink is per-cell state, not an occurrence —
  the clearest worked example of ADR-0020's rule, stated in `event.rs`'s module doc
  (`osc8_hyperlink_emits_no_event`).
- **`TermEvent` is `#[non_exhaustive]`, by the maintainer on 2026-09-02**, while a slice was adding two
  variants. What decided it was CLAUDE.md's identity statement — `justerm-core` is a reusable,
  independent crate — so there are consumers this repo cannot edit. Measured then: crates.io reverse
  dependencies zero (248 downloads over 11 versions); cost in this workspace zero (`events.ts` mirrors
  the union by hand and never had a compiler relationship to break); and the window closes at 1.0.0,
  while the VT tail adds variants (three, then two, in the two slices before). The argument that lost
  is a real cost: an exhaustive match tells a consumer a new event exists — penterm's then
  `route_event` carried a comment on each dropped arm — and that signal now has to come from release
  notes. PenTerm today maps through `native_event` with a `_ => None` arm (penterm 7dadc3c95
  `src-tauri/src/engine/native.rs`).

**Who answers a query: the split is who holds the answer**
- **The engine answers what it holds, at once.** DA1 and DA2 inline in `csi_dispatch`, VT52 `DECID` in
  the VT52 escape arm, and DSR (`5n`, `6n`), the kitty flags query and DECRQM through
  `device_status_report` / `kitty_dispatch` / `decrqm` — seven paths that queue a reply with no
  consumer involved. Three are state-dependent (DSR `6n`, DECRQM, kitty flags).
- **What the consumer holds leaves as a `Query…` event**: the colour scheme (`ColorSchemeQuery`), the
  palette, foreground, background and cursor colours, and the clipboard — six families answered by the
  six `report_*` methods. The clipboard is the sharpest case: the engine holds none *by design*.
- **DECRQM answers from the model it has, not a flag kept for the query** (#27). DECCOLM (`?3`) is
  derived from the actual width — a flag would lie if the consumer ignored the resize request (#82).
  Mouse tracking is one enum because the levels are mutually exclusive, so `?1000` queried while
  `?1002` is active reports *reset*. DECCOLM itself reaches the consumer as a request,
  `TermEvent::ColumnMode`, which it may honour by resizing.
- **A `report_*` takes back what it needs rather than the engine remembering it.**
  `report_clipboard(target, text, terminator)` follows `report_palette_color(index, spec,
  terminator)`: the consumer names what it is answering about. alacritty is the alternative and shows
  the cost — its query captures target and terminator in a closure (`alacritty_terminal/src/term/mod.rs:1740`),
  hidden state plus a question about interleaved replies.
- **The terminator travels the same way, and #836 settled that it must (2026-09-02).** The engine used
  to discard `bell_terminated` at the parser boundary, so no `report_*` caller could supply it. Now
  every `Query…` event carries a `Terminator` and the matching `report_*` hands it back, on all five
  OSC reply paths (4, 10, 11, 12, 52). **The deciding property is this channel's**: `drain_events`
  returns a batch, so two queries can be outstanding and answered in either order, and one remembered
  scalar cannot say which exchange it belongs to (`two_outstanding_queries_each_get_their_own_terminator_back`).
  That is the shape [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  states for coordinates, reached independently by a fact that is not one. xterm *does* store the
  terminator, but only for `OSC 52` and in a single scalar with the collision this batch would
  provoke. **The spec settles the direction**: `ctlseqs.txt:2020` — *"when returning information, uses
  the same terminator used in a query"* — which ADR-0004 ranks above every implementation.
  `Terminator` is exhaustive at two members (#843's rule), and ghostty converges on both the partition
  and the closure (`src/terminal/osc.zig:252`). The closure rests on the input space: the 8-bit C1
  `ST` (`0x9C`) is absent because `feed` does not treat a lone `0x80..=0x9F` byte as a control. An OSC
  ended by a bare `ESC` answers `ST`, as xterm hardcodes (`charproc.c:8964`) and ghostty's
  `Terminator.init` returns for a missing byte (`src/terminal/osc.zig:263`); one cancelled by
  `CAN`/`SUB` is not relayed at all (#970, `a_query_cancelled_by_can_or_sub_is_not_answered`).

**Event payloads**
- **A marker's birth and death are both events, and the pair is load-bearing since #490.**
  `MarkerDisposed` was enough while every live marker rode every frame. Once the index is *pulled*, a
  population that only ever shrinks is silently wrong, so `MarkerCreated { id, line, col, kind,
  evicted_total, epoch }` is the mirror. ADR-0020 R1 is why neither is a frame field.
- **An event that carries a coordinate carries the instant it is true at, or nothing (#737, #741).**
  A single `feed` can create a marker and then evict, so the frame closing that batch reports an
  origin the event's line predates; two batches with the same mark and the same three evictions in
  opposite orders were identical on every channel a consumer could see and three lines apart in truth
  (#737). And the instant is not one scalar (#741): markers also move non-uniformly, which only the
  epoch dates. The rule stated against the sibling rather than the axis: **an occurrence carries
  every scalar the pull answering the same question carries**, here `MarkerIndex`'s
  `(line, evicted_total, epoch)` (`MarkerCreated`'s doc).
- **One event, two producers — `Title` names a state, not an act (#823).** XTWINOPS `CSI 23 t` pops a
  title and emits the same `TermEvent::Title` an `OSC 0`/`OSC 2` does (`set_window_title` is the one
  door). That is what both references with a title stack do (xterm.js's `setTitle` fires
  `_onTitleChange`; alacritty's `pop_title` routes through `set_title`) — a second event would ask
  every consumer to learn a distinction it has no use for. The cost is a tense change: the event means
  *"the title is now this"*. Measured on real ptys: applications push at startup before setting a
  title, so a pop routinely restores the empty string — "go back to your default"
  (`xtwinops_a_push_before_any_title_restores_the_empty_one`); and a session that never sets a title
  still produces one `Title("")` per pop, which is why the `cursor_color_nvim.raw` capture recorded for
  #832 gained two events. The stack is `TITLE_STACK_DEPTH` (10) deep and drops the oldest.
- **A notification is relayed, not read (#964).** `OSC 9` and `OSC 777` become one
  `TermEvent::Notification { sequence, payload, maybe_truncated }`, the payload `params[1..]`
  rejoined, under the same no-field guard as the title and cwd arms. The engine does not tell iTerm2's
  free text from ConEmu's numbered subcommands, nor split rxvt-unicode's `notify;title;body`: reading
  is the consumer's (ADR-0017). **The name overstates the content, and the docs say so**: ConEmu's
  `9;9;path` is a working-directory report a shell may send every prompt and does *not* arrive as
  `Cwd`; `777` is urxvt's extension dispatch, where only `notify` is a notification.
  **`maybe_truncated` is the maintainer's call (2026-09-23), and theirs to reverse.** Shown three
  options — flag, lift the 16-field bound for these codes (a shadow capture beside `vte`, or a `vte`
  change), or relay unmarked as `Title` does — they chose the flag. The bound cannot be *detected*,
  only *reached* ([VT interpretation](vt-interpretation.md), #840), so the flag is set at
  `VTE_OSC_FIELD_CAP` fields and is a false positive on a payload with exactly 14 `;`. It does not
  reopen #840, which declined a guard that *drops* the value; this drops nothing. Whether
  `Title`/`Cwd`/`OSC 8` should carry the same flag was not covered; they stay unmarked.
- **A request the engine cannot perform is still an event, and `OSC 52` is the clearest case (#828).**
  What rides the channel is not a clipboard operation but the fact that an application asked for one.
  The engine's half: recognise the sequence, decode the base64, relay `ClipboardStore { target, text }`
  or `QueryClipboard { target, terminator }`, and answer a query only when the consumer calls
  `report_clipboard`. **Dropping the event is how a consumer refuses**, so there is no allow/deny knob.
  The security property falls out of the split: the engine holds no clipboard, so a query it is never
  asked to answer discloses nothing, and a read is refusable independently of a write. PenTerm's
  Native path drops every `QueryClipboard` (and every colour query) in `native_event` — "a clipboard
  read because the app never answers one" (penterm 7dadc3c95 `src-tauri/src/engine/native.rs`).
  **The reason once attached to that was wrong (#841, measured 2026-09-10).** It read *"alacritty's
  four-state `osc52` config has no counterpart — alacritty is the consumer"*. At the pin, alacritty
  gates the sequence inside its engine crate (`alacritty_terminal/src/term/mod.rs:1706`, `:1727`) with
  the policy injected across the crate boundary (`Osc52` on `alacritty_terminal`'s `Config`, `:353`,
  written at `alacritty/src/config/ui_config.rs:125`) — ADR-0017's own shape. The conclusion rests on
  the ADR alone: an engine that touches no clipboard buys nothing by holding a gate in front of a relay.

**`OSC 52` in core**
- **`ClipboardTarget` models three selectors and keeps `p` apart from `s`** (#828). The field admits
  `c`, `p`, `q`, `s` and eight cut buffers; the three a consumer can act on are modelled and the rest
  ignored, as alacritty ignores them, rather than folded onto a neighbour. ghostty folds every
  unrecognised kind onto the clipboard (`src/termio/stream_handler.zig:1013` — read its `switch`, not
  the comment above it). A first draft collapsed `p` and `s` as alacritty does; that is wrong here
  though not there, because alacritty replies with the byte the application sent while this engine
  hands the consumer a value and takes it back at `report_clipboard`, so a collapse would answer
  `ESC ] 52 ; s ; ?` naming `p` (`the_reply_names_the_selector_the_application_wrote`). The type is
  `#[non_exhaustive]` (#843) because `q` and the cut buffers remain unmodelled. ghostty converges on the
  same three open members (`src/terminal/clipboard.zig:2`, `Location { standard, selection, primary, _ }`)
  — which #843 had recorded as impossible, on the premise that Zig has no non-exhaustive enum; a
  trailing `_` is one.
- **It diverges from the spec twice, both argued (#828).** An **empty target is the clipboard**, not
  the spec's `s0`. Neither half of `s0` is representable: the cut buffer is unmodelled, and `s` is
  resolved by xterm through a user resource (policy ADR-0017 puts in the consumer; DECSET 1041 sets it
  from the stream, which justerm declines to track). xterm-as-shipped reads the empty field as
  PRIMARY. What decides it is independent lineages — alacritty never sees an empty field (`vte`
  substitutes `c` first), so it and `vte` are one — ghostty and xterm.js's clipboard addon the other
  two: three lineages, one answer; and `tmux` 3.2a was measured sending the empty form.
  **Malformed in, nothing out**: the spec clears on a payload that is neither base64 nor `?`; clearing
  is destructive, and inferring one from bytes the engine could not parse lets line noise wipe what the
  user copied (`a_malformed_payload_relays_nothing`). xterm *filters* instead; this engine is stricter
  on purpose, since a filter hands the consumer text assembled from bytes the application did not send.
  Non-UTF-8 is refused on the same principle (`a_payload_that_is_not_utf8_relays_nothing`).
- **An empty payload clears** — the spec's `Pd` *"becomes the new selection"* (`ctlseqs.txt:2166`),
  xterm clears before appending (`misc.c:3410`), ghostty pins the same input as *"clear clipboard"*
  (`src/terminal/osc/parsers/clipboard_operation.zig:93`). It is not the spec's *"neither a base64
  string nor `?`"* clause (`:2174`): an empty payload is a well-formed encoding of no bytes
  (`an_empty_payload_is_a_store_of_the_empty_string`). Two fields (`OSC 52 ; c`) is no payload at all
  and relays nothing (`a_missing_payload_field_is_not_an_empty_payload`).
- **The payload is `params[2..]` rejoined, #650's rule**: reading `params[2]` alone would decode a
  well-formed first piece of a payload containing a `;` — a truncated clipboard. Rejoined, the stray
  `;` reaches the decoder and is refused (`a_payload_split_by_a_semicolon_is_not_silently_truncated`).
  A multi-target list like `pc`, which the spec permits (`ctlseqs.txt:2156`), is dropped rather than
  approximated (`an_unmodelled_or_multi_character_target_relays_nothing`): honouring one target of two
  is the same defect one axis over, and `vte`/alacritty's first-byte-wins would do exactly that.
- **The size bound is checked on the fields, before the join** (#828) — `join` is an unconditional full
  copy, so checking after it would let a hostile payload buy a second allocation the size of the first.
  `MAX_CLIPBOARD_BASE64` (16 MiB) **drops an over-long payload whole, where `MAX_COMMAND_TEXT`
  truncates**: a prefix of a command is a usable answer, a prefix of a clipboard is text the user
  pastes believing it whole (`an_oversized_payload_is_dropped_whole`). It refuses the second allocation
  (the rejoin) and everything downstream; the decoded `Vec` is a third, and the `String` is not a
  fourth (`String::from_utf8` reuses its buffer). It does not bound the parser's own allocation
  ([VT interpretation](vt-interpretation.md)) nor the queue. Sized so no real copy reaches it (16 MiB
  of base64 is ~12 MiB of text). Neither reference has a cap to import — alacritty has none, ghostty's
  `MAX_BUF = 2048` is an inline-buffer threshold with an allocator path past it
  (`src/terminal/osc.zig:298`) — so the number is justerm's own.
- **The base64 is decoded in core, by a local implementation** (`base64.rs`, #828). Decoding is
  mechanism, and pushing it out would make every consumer carry base64; alacritty decodes in its core
  too, ghostty one layer out. Not a dependency because `base64` is not in the workspace lockfile at
  all, so it would be a new supply-chain entry for `justerm-core` and everything downstream, for a
  64-entry alphabet proven against RFC 4648 §10's vectors (`encodes_the_rfc4648_vectors`,
  `decodes_the_rfc4648_vectors`). The decoder refuses what makes the bytes ambiguous — a byte outside
  the standard alphabet (`-`/`_` refused, not mapped), an impossible length, interior padding, non-zero
  bits in a final partial group, partial padding (`Zg=`) — and **accepts missing padding**, diverging
  from both references (alacritty requires canonical padding, ghostty rejects unpadded as
  `InvalidPadding`). The reach of unpadded emitters is unmeasured; the asymmetry decides it, since
  rejecting costs a silently dropped clipboard and accepting costs nothing. The encoder always pads.

**`OSC 52` in the web widget**
- **The widget declines the policy too, one layer out** (`clipboard.ts`, #841). It holds no clipboard
  and no permission model: `ClipboardController` routes to a `ClipboardProvider` the embedder supplies,
  and with none it does nothing (`TerminalOptions.clipboard`). The injected-provider shape is xterm.js's
  `addon-clipboard` (`ClipboardAddon.ts:13-16` @ `699f553`, no policy enum, handler registered
  unconditionally at `:20`) — the shape only. Two differences: *layer* (the addon is a separate
  package; xterm.js's `Terminal` implements no `OSC 52`) and *default posture* (the addon defaults to
  `BrowserClipboardProvider`, permissive both ways, `:15`, `:71-79`; this widget's no-provider default
  refuses both, as xterm(C) does — `DEF_ALLOW_WINDOW` `False`, `main.h:119`, `DISALLOWED_PASTE64`
  `",SetSelection,GetSelection"`, `:159`, `:170-172` @ `xterm-410`).
- **Read and write are refused independently.** Defaulting writes open and reads shut is **2 of 4**:
  alacritty's default is `Osc52::OnlyCopy` (`term/mod.rs:377-380` @ `852e971`) and ghostty ships
  `clipboard-write: allow` beside `clipboard-read: ask` (`src/config/Config.zig:2379-2380` @
  `e6e26e1`), both native applications with a config file; the two embeddable references are symmetric
  (xterm.js allows both, xterm(C) denies both). The query path checks for a `ClipboardPort` before it
  touches `readText`, so no permission prompt is raised for an answer nobody can receive.
- **Focus is not a gate — the maintainer's call, 2026-09-10, theirs to reverse.** alacritty checks
  focus both ways (`alacritty/src/event.rs:1903`, `:1908` @ `852e971`); ghostty and xterm.js do not.
  Knowing focus and deciding refusals from it are different acts; an embedder can refuse on focus
  inside its provider.
- **A refusal is silence at all four references** — alacritty `debug!` and return
  (`term/mod.rs:1727-1730`), ghostty `log.info` and return (`src/Surface.zig:5837-5844`), xterm(C)'s
  reply block sits inside `AllowWindowOps` (`misc.c:3378`), xterm.js leaves `OSC 52` unhandled without
  the addon. **A provider's rejection is a refusal, swallowed**; xterm.js's addon instead returns the
  promise to the parser, whose `WriteBuffer.ts:283-286` lets it throw uncaught while the parse resumes.
- **A browser read hangs rather than rejecting** — measured 2026-09-10, Chromium over `localhost`:
  `readText()` with `userActivation.isActive` `true` stayed pending past 2000 ms, and with it `false`
  (the 5 s window waited out, `hasBeenActive` `true`) past 3000 ms, while `writeText()` resolved;
  `permissions.query` read `clipboard-read: "prompt"`, `clipboard-write: "granted"` throughout. The
  second row is the one that matters: an `OSC 52` query arrives from the stream, never with a gesture.
  So a controller promise may never settle (hence `ClipboardController.dispose`), and the widget
  imposes no deadline — a bounded read is the provider's policy. Not measured: a never-activated page,
  a denied prompt, other browsers. **`""` and `null` are different answers**: an empty clipboard still
  replies, because the application is blocked waiting — ghostty says so (`src/Surface.zig:5945-5946`
  @ `e6e26e1`).
- **A query cannot overtake the store before it.** A store and a query arrive back to back in one
  drained batch and both are floated, so the controller keeps the outstanding write per target and a
  query awaits it first; keyed by target so a hung read on one cannot stall another.
- **One subscription serves both surfaces** (`Terminal.mount`). A backend has one channel to push the
  event stream down, so the clipboard pair arrives with the notifications; `dispatchTermEvent` ignores
  the pair and the controller ignores the notifications, so wiring either alone still works. The
  controller is held on the widget because an in-flight read is already past the subscription, so
  `dispose()` ends the controller ([a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md)).
  `handle` is floated rather than awaited: it never rejects, and the channel is fire-and-forget.
- **`events.ts` has two surfaces, and the split is the point.** The `TermEvent` union is the channel
  and carries the `OSC 52` pair, because a backend has one stream. `EventHandlers` is the
  *notification* surface — title, bell, cwd, and since #964 the `OSC 9`/`OSC 777` notification — so a
  new event does not automatically owe a callback; it owes a decision about which surface it belongs
  on. `NotificationEvent` took a callback because the consumer is only told. Core's four colour
  `Query…` events, the set/reset colour events, `ColorSchemeQuery` and `ColumnMode` are not on the
  union: the clipboard pair deliberately did not generalise a channel for them (#841, with the
  maintainer), because the colour queries carry a palette-ownership question that slice did not
  measure.
- **`events.ts`'s `ClipboardTarget` and `Terminator` are not on published-surface's hand-copied
  roster — the maintainer's call, 2026-09-10, theirs to reverse.** That roster lists value spaces that
  reach the package through the decoder's wire lane, where `justerm-wasm-decode` is their home;
  `TermEvent` crosses no decoder lane — it arrives on a side channel the embedder implements, whose
  only type declaration is this package's.

**Inputs and their bounds**
- One `feed` can queue any number of events and replies; nothing bounds either queue but the drain.
- Per item: a title is cut to `MAX_TITLE` chars; a clipboard payload over `MAX_CLIPBOARD_BASE64` is
  dropped; every OSC payload is cut at 16 fields by `vte`. `OSC 7` cwd values, notification payloads
  and colour specs have no length bound of their own (see Known holes).

## Code

- `justerm-core/src/event.rs` — `TermEvent`, `Terminator` (`Terminator::bytes`), `ClipboardTarget`,
  `NotificationSequence`
- `justerm-core/src/term/replies.rs` — `Term::drain_events`, `Term::drain_replies`,
  `Term::report_color_scheme`, `Term::report_palette_color`, `Term::report_foreground`,
  `Term::report_background`, `Term::report_cursor_color`, `Term::report_clipboard`; the producers
  `Term::special_color`, `Term::clipboard` (queues `OSC 52` events only; its reply comes from
  `report_clipboard`), `Term::device_status_report`, `Term::kitty_dispatch`, `Term::decrqm`,
  `Term::set_window_title`, `Term::window_ops`, `push_title`
- `justerm-core/src/term/dispatch.rs` — `osc_dispatch` (title, cwd, notification, palette, clipboard
  arms; the terminator), `csi_dispatch` (DA1, DA2, private DSR `?996`), `VTE_OSC_FIELD_CAP`
- `justerm-core/src/term.rs` — `MAX_CLIPBOARD_BASE64`, `MAX_TITLE`, `TITLE_STACK_DEPTH`,
  `Term::full_reset` (appends the marker disposals)
- `justerm-core/src/lib.rs` — `Engine::drain_events`, `Engine::drain_replies`, the `report_*` forwards
- `justerm-core/src/base64.rs` — `decode`, `encode`, `sextet`, `ALPHABET`
- `justerm-core/tests/events.rs` · `clipboard.rs` · `osc_reply_terminator.rs` · `reply.rs`
- `justerm-web/src/events.ts` — `TermEvent`, `EventHandlers`, `NotificationEvent`,
  `ClipboardStoreEvent`, `ClipboardQueryEvent`, `ClipboardTarget`, `Terminator`, `dispatchTermEvent`
- `justerm-web/src/clipboard.ts` — `ClipboardProvider`, `ClipboardPort`, `ClipboardOptions`,
  `ClipboardController` (`handle`, `dispose`)
- `justerm-web/src/terminal.ts` — `Terminal.mount` (the one subscription), `Terminal.dispose`

## Reference behaviour

In `docs/agents/reference-facts.md` — linked, never restated.

- [OSC 52 — where the references converge, and the two places the spec is not followed](../../agents/reference-facts.md#osc-52--where-the-references-converge-and-the-two-places-the-spec-is-not-followed-828-2026-09-02)
  — the target, empty-field, malformed-payload and padding rows behind the `OSC 52` bullets (#828)
- [The VT-gap sweep of 2026-08-26](../../agents/reference-facts.md#the-vt-gap-sweep-of-2026-08-26--five-decisions-the-references-settled-823832-verified-2026-08-26)
  — the terminator closure and the title stack (#823–#832)
- [Secondary device attributes — report yourself, do not impersonate](../../agents/reference-facts.md#secondary-device-attributes--report-yourself-do-not-impersonate)
  — what each reference puts in a DA2 reply's three fields (#824)
- [Notification sequences — `OSC 9` and `OSC 777`](../../agents/reference-facts.md#notification-sequences--osc-9-and-osc-777-964-verified-2026-09-23)
  — which trees read the two codes in the engine and which leave them to the consumer (#964)

The web clipboard comparisons (#841) are pinned inline above, not in the store. alacritty's
`EventListener`, the rejected push model, is still unpinned.

## Cross-cutting invariants

- [RIS keeps configuration and drops coordinates](../invariant/ris-keeps-configuration-drops-coordinates.md)
  — both queues survive `ESC c`, and this is the only territory where the reset *adds* to one: every
  marker's disposal is announced before the rebuild (`Term::full_reset`). That is the only append,
  tested since #835: the palette is the other consumer-held thing a reset invalidates, and it is
  deliberately announced by neither strength.
- [a coordinate carries the instant it is true at](../invariant/a-coordinate-carries-the-instant-it-is-true-at.md)
  — the channel where an occurrence's payload outlives its instant structurally; `MarkerCreated` is the
  worked case (#737, #741).
- [a layer ends what it exclusively holds](../invariant/a-layer-ends-what-it-exclusively-holds.md)
  — `Terminal` composes the `ClipboardController` and is its only holder, so `dispose()` ends it: an
  in-flight read outlives the event subscription.
- [whoever drains a reply owns writing it back](../invariant/a-drained-reply-is-the-drainers-to-write.md)
  — the reply and event queues are emptied by their drain, so the drainer is the only party left to answer; `ClipboardController.query` answers on `ClipboardPort.report`

## Blast radius

- [frame](frame.md) — anything that fails ADR-0020's state test lands here instead, so a new
  occurrence-shaped fact is decided against both at once.
- [hyperlinks](hyperlinks.md) — the worked example of the split: per-cell state that must not become
  an event.
- [marker](marker.md) — `MarkerCreated`/`MarkerDisposed` carry the marker index's coordinate triple;
  a change to what dates a marker line changes what the event must carry.
- [session](session.md) and [PTY loop](pty.md) — both hand the queues out unfiltered and write every
  reply back unexamined; a reply a child must not receive, or an unbounded event, meets no filter there.
- [input encoding](input-encoding.md) — replies and encoded input are interleaved into one byte
  stream to the child with no framing, so their hand-over order is the only thing separating them.
- [capture corpus](capture-corpus.md) — the closed-loop recorder names every variant and answers the
  colour queries with a fixed policy; a new query variant goes unanswered there except for its loud
  report.
- [widget lifecycle](widget-lifecycle.md) — the consumer drains on a cadence it chooses, and a dropped
  reply is an application hang rather than a missing notification.
- [published surface](published-surface.md) — `TermEvent` is non-exhaustive in core and mirrored by
  hand in `events.ts`; a new variant reaches the web package only when someone copies it.
- [VT interpretation](vt-interpretation.md) — owns the 16-field `vte` bound the rejoin rule and
  `maybe_truncated` sit on, and the unbounded OSC buffer.

## Known holes / open

- **No record for either channel's membership.** ADR-0029 reached this territory only for what a
  coordinate-bearing payload owes. What earns a place as an event — as opposed to frame state or a
  reply — is answered by ADR-0020 for the frame and by nothing here, and one channel carries a response
  obligation: a dropped reply hangs the application.
- **No bound on either queue.** A consumer that never drains grows memory with no signal, and no
  document states whose problem that is. `MAX_CLIPBOARD_BASE64` enlarges it: stores accumulate at up to
  16 MiB each, roughly three orders of magnitude over the next-largest payload.
- **Draining does not bound what one `feed` queues.** `CSI 22 t CSI 23 t` is 10 bytes and queues one
  `Title` of up to `MAX_TITLE` chars, so 120 KB fed at once queued 10 001 titles, 156 MiB (measured by
  the #1038 lens; before #1038's title cap each could be the full title). The bytes live until the
  drain that follows the `feed`.
- **`OSC 7` cwd values, notification payloads and colour specs are bounded by nothing of their own** —
  only by `vte`'s unbounded OSC buffer and its 16-field cut ([VT interpretation](vt-interpretation.md)).
- **The web `ClipboardTarget` union is closed while core's is `#[non_exhaustive]`**, and
  `ClipboardController` passes the target to the provider unexamined — there is no decline arm for a
  target it does not know. No reachable consequence until core names `q` or a cut buffer, and nothing
  gates that day. PenTerm's `clipboard_target` does decline an unknown member (penterm 7dadc3c95).
- **The web widget answers no colour query.** The four colour `Query…` events and `ColorSchemeQuery`
  have no web surface, so a web-hosted application asking `OSC 11 ; ?` waits on the consumer's own
  wiring (#841 left it out on purpose).
- **The rejected push model is unpinned**, and it is the whole argument for the pull design.
