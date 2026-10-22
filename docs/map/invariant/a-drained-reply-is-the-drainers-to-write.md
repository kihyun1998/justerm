# Invariant — whoever drains a reply from the engine owns writing it back

## The fact

The engine queues the bytes an application asked for (DA, DSR, DECRQM, the kitty flags query, and the
colour and clipboard answers a consumer hands it through `report_*`). A drain **empties** the queue:
`justerm-core/src/term/replies.rs — Term::drain_replies`, reached by `Engine::drain_replies` and
`Session::pump`. Nothing re-queues. So the party that drains is the only party that can still write
the reply to the application, and a drain that does not write it leaves the application blocked on an
answer that no longer exists anywhere.

The same holds one step earlier for a query the engine cannot answer itself: it arrives as a
`TermEvent` (`QueryForeground`, `QueryBackground`, `QueryCursorColor`, `QueryPaletteColor`,
`ColorSchemeQuery`, `QueryClipboard`), drained by `Engine::drain_events`, and only the drainer can answer it through
`report_*` — or refuse it deliberately, which for the clipboard is silence (#841).

Sites at HEAD 7bb31f7, none calling another:

1. `justerm-pty/src/lib.rs — feed_session` drains after each read and calls `write_back` under the
   session lock; `Pump::pump` takes `Session::pump`'s replies and calls `write_back` under the same lock,
   so drain order is hand-over order.
2. `justerm-core/examples/reply_filter.rs — main` answers the query events (`answer_events`), then
   `drain_replies` and writes the result to its stdout — it exists because a harness forwarding
   `drain_replies` alone would leave the colour and clipboard queries silent (#891).
3. `justerm-web/src/clipboard.ts — ClipboardController.query` takes a drained clipboard query event and
   answers it on `ClipboardPort.report`, including `""` for an empty clipboard, because the application
   is blocked waiting.
4. `penterm 7dadc3c95 src-tauri/src/ssh/manager/read_loop.rs` — in the `frame_due` branch, `native.tick()`
   (`src-tauri/src/engine/native.rs — tick`, which calls `Session::pump`) returns `tick.replies`, and the
   loop writes them to the SSH channel.

The converse is what keeps a second party from draining: `justerm-core/src/session.rs —
Session::send_pending` sends the pending frame and drains nothing, so a consumer that changed the view
can show it while another party owns the pump; and `justerm-pty/src/lib.rs — Pty::session`'s doc-comment
forbids the consumer to `pump` the session it hands out, because only the pty writes back to the child.
PenTerm follows that split: `native.rs — paint_lines` calls `send_pending`.

## Why it is cross-cutting

The engine does no I/O (CLAUDE.md), so the write-back is always in another crate or another program,
and every one of them reaches it independently: a pty crate, a capture harness, a web widget, a
Tauri SSH loop. They share only the assumption that "drain" is a hand-over, not a read. A drain looks
like a getter at the call site; nothing at it says that the bytes it returns are the only copy.

## Territories it holds in

- [events & replies](../territory/events-and-replies.md) — owns the two queues and their drains, and the
  `report_*` answers; the clipboard controller is the web widget's drainer-side answer.
- [session](../territory/session.md) — `Session::pump` drains both queues and hands them back;
  `send_pending` is the non-draining present for a party that does not own the pump.
- [pty](../territory/pty.md) — `feed_session` and `Pump::pump` are the drainers that write back, and
  `Pty::session` keeps a consumer from draining underneath them.
- [capture corpus](../territory/capture-corpus.md) — `reply_filter.rs` is the drainer that closes the
  loop for a recording; a capture whose replies are dropped records an application waiting.

## What a violation looks like

The application hangs or times out on a query: vim waits on `OSC 11` before deciding its background, a
shell's prompt waits on DSR. It reproduces whenever a second party drains — a consumer calling
`pump()` on `Pty::session()`'s handle instead of `send_pending()`, a harness that forwards
`drain_replies` but drops the query events — and nothing errors: the queue is simply empty the next
time the owner looks.

## Discovery history

- **#891** (2026-09-14, `74d61d7`) — the capture harness needed a consumer, not a pipe, because the
  query events are answered by policy.
- **#841** (2026-09-10, `ac91d0e`) — the clipboard query is the web widget's one request → answer seam;
  refusing is silence, an empty clipboard still answers.
- **#1057** (2026-10-13, `f10dcb6`) — `Session::send_pending` and the "must not pump" rule on
  `Pty::session` arrived together, so a consumer could present a view edit without draining the pty's
  replies (`justerm-core/tests/session.rs —
  send_pending_sends_the_pending_frame_and_leaves_replies_and_events_for_the_pump`).

## Where it will recur

Any new code that calls `drain_replies`, `drain_events` or `Session::pump`: it is now the only holder
of those bytes, and must write them back or answer them. Any new handle that exposes a `Session` or
`Engine` to a second party: say who drains, and give the other party a non-draining way to present.
