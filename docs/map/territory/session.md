# Territory — session

## What it is

The **Session** (GLOSSARY "Session", "Pump"; #1042, #1043): one `Engine` bound to one frame sink, the
unit a consumer feeds, pumps and subscribes to. It has no clock and does no I/O. It owns three
decisions every frame-mode consumer used to write for itself: when a frame is worth encoding
(`Engine::frame_pending`), that sending a frame is the ack (damage is reset as it is sent), and that a
subscriber arriving late starts from a `Full` frame. Everything a **Pump** hands back (reply bytes,
events) is the caller's to deliver. Two drivers exist: `justerm-pty` (its reader and pump threads,
[PTY loop](pty.md)) and PenTerm, which holds a Session per Native pane and pumps an SSH pane's on its
own 16 ms frame clock.

## Governing decisions

- [ADR-0034 — the family owns the PTY loop, and the Session lives in core](../../adr/0034-the-family-owns-the-pty-loop-and-the-session-lives-in-core.md)
  — **D2** (a core module, a pure state machine: feed, pump, subscribe, resize; events returned
  unfiltered), **D3** (an overlay change raises a frame-pending signal separate from damage), **D4**
  (no clock in the Session; pacing is the driver's), and the #1057 amendment's **R2** (the tick sees
  the Session under its lock, before the pump) and **R6** (`Session::send_pending`). Its out-of-scope
  list keeps the Session off wasm (the reserved `justerm-wasm-engine`).
- [ADR-0014 — carry interaction overlays in the frame](../../adr/0014-carry-interaction-overlays-in-the-frame.md)
  — why an overlay change is no damage, and so why the Session cannot ask `damage()` whether to send.
- [ADR-0003 — damage model: incremental bounds](../../adr/0003-damage-model-incremental-bounds.md)
  — the ack-gated reset the Session performs on every send.
- `docs/architecture.md` §Cadence ("The Session packages the engine half of this loop, and still has
  no clock") states the send-is-the-ack contract and who it suits.

## Design model

From ADR-0034 and the source; every rule below is pinned by a test in `justerm-core/tests/session.rs`
unless it says otherwise.

- **A frame is encoded only when `Engine::frame_pending` is raised, and only with a sink.** The private
  `flush` is the one place a frame leaves: no pending frame, nothing is encoded; no sink, nothing is
  sent **and the frame stays pending**
  (`send_pending_without_a_sink_sends_nothing_and_keeps_the_frame_pending`). `frame_pending` is wider
  than damage (ADR-0034 D3, #1043/#1044): output while scrolled up, every viewport scroll, every
  selection/search/active-match/marker change and the two selection-resolution setters raise it with
  no damage. `output_while_scrolled_up_sends_a_frame_though_it_leaves_no_damage` reddens if the
  predicate is swapped for `damage()` being non-empty ([damage](damage.md)). Draining and
  `report_*` do not raise it.
- **The send is the ack.** `flush` calls `Engine::reset_damage` right after `send_frame`. That is
  right for a transport that does not drop frames (a Tauri channel, a local socket); a consumer that
  needs a real round-trip ack drives `Engine` directly (architecture.md §Cadence).
- **A pump empties both queues every time, sink or not** (`a_pump_without_a_sink_still_hands_back_replies`,
  `events_come_back_from_pump_unfiltered_and_the_queue_is_then_empty`). Events are not filtered:
  choosing among them is consumer policy (ADR-0034 D2).
- **Whoever pumps owns the replies.** A pump drains bytes an application is blocked on, so the caller
  of `pump` is the only party that can write them back. This is why `Pty::session` forbids a consumer
  to pump (the pty's pump thread writes back; a consumer's pump would take replies nobody writes), and
  why `send_pending` exists (#1057 R6): it is `flush` without the drain, for a consumer that wants an
  edit on screen now while someone else owns the pump
  (`send_pending_sends_the_pending_frame_and_leaves_replies_and_events_for_the_pump`).
- **Replies are not paced by the Session, but nothing separates them either.** `pump` drains replies
  together with sending the frame, so a driver that pumps on a timer paces replies with frames unless
  it drains them itself. `justerm-pty` does: its reader drains replies right after each feed, through
  `Session::edit` (see [PTY loop](pty.md) for why a ConPTY needs it). PenTerm's SSH loop does not: it
  writes `Tick::replies` only on its 16 ms frame tick (penterm 7dadc3c95
  `src-tauri/src/ssh/manager/read_loop.rs`, the `frame_due` branch calling `NativeEngine::tick`). Read
  2026-10-22, not measured.
- **A late subscriber starts from a whole screen.** `subscribe` marks the screen fully damaged, sends
  a `Full` frame at once, resets damage, then replaces any previous sink
  (`a_late_subscriber_starts_from_a_full_frame`, `subscribing_again_replaces_the_sink`). PenTerm's
  wrapper notes why: a widget drops frames pushed before it mounts (penterm 7dadc3c95
  `src-tauri/src/engine/native.rs` — `NativeEngine::subscribe`).
- **`resize` returns the adopted size.** The engine widens a too-narrow width to its minimum, and the
  return value is what the PTY or SSH channel must be told (`resize_returns_the_size_the_engine_adopted`).
  It runs through `edit`, so an ended session sends the resize frame itself.
- **Ending inverts who sends.** `end` sets the flag and flushes; from then on every `edit` and
  `resize` sends its own frame, because no pump will come (`an_ended_session_sends_a_view_change_without_a_pump`,
  `ending_a_session_sends_the_output_that_arrived_after_the_last_pump`). The engine stays readable
  and scrollable until the Session is dropped. `end` is idempotent: PenTerm's `EndedEngines::keep`
  ends a local pane's Session again after `justerm-pty` already ended it (penterm 7dadc3c95
  `src-tauri/src/engine/ended.rs`).
- **`edit` is the one door for a view change.** It takes a closure over `&mut Engine` and flushes
  only when ended. `feed` is a separate door and does **not** flush on an ended Session (see Known
  holes).
- **What bounds the input:** nothing in the Session. `feed` takes any slice; both queues grow until
  the next pump, and the `pump` doc says the engine keeps them unbounded. The bound is the driver's
  cadence: `justerm-pty` reads at most 64 KiB per feed, drains replies after each one and pumps every
  `pump_every`.
- **Neither public type is `#[non_exhaustive]`** (#844): `Session`'s fields are private and it is
  built only by `new`; `Pumped` is built by the Session and accepted by nothing, and has a `Default`.
- **Two driver shapes over one type.** `justerm-pty` holds the Session in an `Arc<Mutex<_>>` shared by
  its reader and pump threads and hands the same lock to the consumer (`Pty::session`). PenTerm wraps
  that same `Arc` in `NativeEngine::over` for a local pane, and builds its own for an SSH pane, which
  its read loop feeds through `feed_if_native` and pumps through `NativeEngine::tick` (penterm
  7dadc3c95 `src-tauri/src/engine/session.rs`; `each_transport_drives_the_engine_through_its_one_route`
  pins both routes by reading the source). PenTerm's own `FrameSink` takes `&self` and is adapted to
  core's `&mut self` one by a private `Sink`.
- **The tick reads `frame_pending` before the pump resets it** (ADR-0034 R2). PenTerm's
  `PromptTracker::observe` asks it under the Session's lock just before the pump, from `on_tick` for a
  local pane and from `NativeEngine::tick` for an SSH one, so a prompt change fed between a check and
  a pump cannot be lost.

## Code

- `justerm-core/src/session.rs` — `FrameSink::send_frame`, `Pumped`, `Session::new`,
  `Session::engine`, `Session::edit`, `Session::subscribe`, `Session::feed`, `Session::pump`,
  `Session::resize`, `Session::end`, `Session::is_ended`, `Session::send_pending`, `Session::flush`
- `justerm-core/src/lib.rs` — `Engine::frame_pending` (its doc lists every raise),
  `Engine::reset_damage`, `Engine::mark_fully_damaged`, `Engine::drain_replies`, `Engine::drain_events`
- `justerm-core/tests/session.rs` — `Recorder`, `subscribed`, `quiet`, `one_frame_after`, and one
  test per raise of the frame-pending signal
- `justerm-pty/src/lib.rs` — `feed_session`, `Pump::pump`, `Pump::run`, `Pty::session` (the first
  driver; detail in [PTY loop](pty.md))
- Outside this tree (PenTerm, read at 7dadc3c95): src-tauri/src/engine/native.rs — NativeEngine::over,
  NativeEngine::tick, NativeEngine::subscribe, NativeEngine::feed, NativeEngine::paint_lines,
  PromptTracker::observe; src-tauri/src/engine/session.rs — feed_if_native; src-tauri/src/engine/ended.rs
  — EndedEngines::keep; src-tauri/src/pty/manager.rs — the Pty::spawn call and its on_tick

## Reference behaviour

**None.** The Session was never compared against a reference. The pacing split it packages was
cross-checked against Mosh, alacritty and xterm.js in `docs/architecture.md` §Cadence (#13), and
ADR-0034 D1 names the PTY layout peers from memory; neither compares a session object.

## Cross-cutting invariants

- [whoever drains a reply owns writing it back](../invariant/a-drained-reply-is-the-drainers-to-write.md)
  — `Session::pump` drains both queues and hands them back; `send_pending` presents without draining, for a party that does not own the pump

## Blast radius

- [PTY loop](pty.md) — its threads assume `pump` drains both queues and that `flush` fires only on
  `frame_pending`; its rule that a consumer must not pump exists only because `pump` drains replies.
- [damage](damage.md) — the Session resets damage on send, so a change to what `reset_damage` clears
  or to what raises `frame_pending` changes what the next pump sends.
- [events & replies](events-and-replies.md) — the Session hands both queues out unfiltered and bounds
  neither; a new event variant or reply path reaches every driver unchanged.
- [frame](frame.md) and [wire format](wire-format.md) — every sink receives `encode`d wire bytes, so a
  frame-content or wire change reaches every subscriber, and a late subscriber assumes a `Full` frame
  carries the whole screen.
- [viewport](viewport.md), [selection](selection.md), [search](search.md),
  [active match](active-match.md), [marker](marker.md) — each mutator there must raise
  `frame_pending`, or its change never leaves a Session (overlay changes leave no damage).

## Known holes / open

- **Both queues are unbounded between pumps.** A driver that feeds and never pumps grows memory with no
  signal; the Session documents this and leaves the bound to the driver's cadence.
- **`feed` on an ended Session does not send a frame.** `end`'s doc promises a frame only for `edit` and
  `resize`; `feed` bypasses `edit`, so output fed after `end` stays pending until the next `edit`.
  Unreachable under `justerm-pty` (its reader checks `is_ended` and feeds nothing after it), and
  PenTerm routes its feed through `edit`. Found by reading (2026-10-22); no test covers it.
- **No reply-only drain.** A driver wanting replies unpaced must reach `Engine::drain_replies` through
  `edit`, as `justerm-pty` does; nothing in the Session's own surface says so.
- **Not on wasm.** ADR-0034 leaves exposing the Session to wasm out of scope; the in-browser engine
  mode `justerm-web`'s `InputSink` doc anticipates has no Session.
