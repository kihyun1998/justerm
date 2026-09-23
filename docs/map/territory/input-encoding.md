# Territory — input encoding

## What it is

The inverse of `feed`: a key, mouse, paste or focus event becomes the byte sequence a TUI expects on
its stdin. What those bytes are depends on **DEC modes the engine learned from the *output* stream** —
so the encoder cannot live where the events are.

It spans two crates by necessity, and the split is exact: the **web widget normalises DOM events into
intents**, and **core turns intents into bytes**. Neither half can do the other's job — the browser
has the event, the engine has the modes.

## Governing decisions

- [ADR-0016 — mouse mode as a wanted-events mask in the frame](../../adr/0016-mouse-mode-wanted-events-mask-in-frame.md)
  — how the consumer learns *which* events the application wants, so it can route rather than guess
  (wire v7→8). It decides the routing signal, not the encoding
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  the modes are engine state, so the encoding is engine work; the DOM event is the consumer's

Nothing governs the encoding itself.

## Design model

- **The engine owns the modes; the encoders are pure.** `event + modes → bytes`, so the consumer's
  I/O stays its own concern and the functions are testable without a PTY.
- **Modes are hidden state learned from output.** DECCKM, mouse tracking and encoding, focus
  reporting, bracketed paste — an application turns them on by *printing*, and the same keystroke
  therefore encodes differently depending on what was printed earlier. This is the sharpest instance
  of `architecture.md`'s "input encoding is mode-gated" entry.
- **A mode can rewrite legacy from the inside, and modifyOtherKeys is the one that does** (#890).
  `CSI > 4 ; 2 m` makes a modified character — and a modified `Tab` / `Enter` / `Escape` /
  `Backspace`, whose bare forms are C0 controls — encode as `CSI 27 ; <1+mods> ; <codepoint> ~`, which
  is how `Ctrl+I` stops being `Tab`. It sits *after* the kitty check and *inside* the legacy arm,
  the placement ghostty states a reason for: traditional encoding, modifyOtherKeys and fixterms
  are extensions that do not change existing behaviour, so they combine. **The gate on which keys
  qualify is this territory's sharpest divergence and it is forced by the seam above**: because the
  widget normalises a DOM event into a produced *character* plus the modifiers it saw, a capital
  arrives as `Char('A') + SHIFT` — where a keysym-based terminal has already spent the Shift. So
  xterm's own `0x40..=0x7f` clause, which both it and ghostty use, would turn every capital into an
  escape sequence here. Rows in [`reference-facts.md`](../../agents/reference-facts.md).
  **And the gate asks that of the *parameter*, not of the raw bits**, which is the whole of
  it: `csi_param` drops Super / Hyper / CapsLock / NumLock, so a gate on the bitflags admits
  a chord it cannot then describe — `Shift+Super+A` passed and came out as
  `CSI 27;2;65~`, byte-identical to what a bare `Shift+A` would have to mean. Reachable
  through the widget, which maps `metaKey` to `SUPER` unconditionally: every macOS
  `Cmd+Shift+<letter>` while the mode is on. The reference masks first for the same reason.
- **Two values in that encoding are ours, not the reference's, and both read as arbitrary at
  the call site.** The shape is `CSI 27 ; <mods> ; <code> ~` and **not** the
  `CSI <code> ; <mods> u` form the reference offers as its alternative — that one is
  byte-for-byte what this engine's *kitty* path already produces, and two protocols a
  consumer negotiates separately must not be indistinguishable on the wire. And a modified
  `Backspace` carries code **127**, not the `8` a keysym-based terminal would send, because
  `127` is the byte this encoder gives a bare Backspace (the PC-keyboard convention) —
  the two have to agree about which key they are naming. ghostty's table spells the same.
- **This is the legacy xterm baseline** — the common 90% every TUI speaks — with **two**
  negotiated extensions on it, which behave differently and are asked in a fixed order. The
  kitty keyboard protocol (`CSI u` plus a progressive-flag stack, #23) is a **stateful
  superset**: it *replaces* the legacy form for what legacy cannot express, and it is asked
  first. `modifyOtherKeys` (#890) is not a superset — it rewrites one case *inside* legacy,
  and is asked after. (This bullet said the kitty half was *"deliberately deferred"* until
  2026-09-11, some two months after #23 shipped it; the same sentence survived a second time
  under `## Known holes`, which is what a claim held in two places does.)
- **Legacy Alt is an ESC prefix on the key's own encoding — named keys included** (#941). A
  character always had it; Enter / Tab / Backspace / Escape dropped Alt, so `Alt+Backspace` reached
  readline as a bare DEL rather than `M-DEL` (backward-kill-word). The prefix now goes on whatever
  the key otherwise encodes (`Alt+Shift+Tab` is `ESC CSI Z`). Under modifyOtherKeys 2 and kitty the
  negotiated form answers first and Alt stays a parameter. **Tab is where the references split**:
  xterm under `metaSendsEscape`, alacritty and ghostty prefix it; xterm.js alone sends a bare HT.
  The prefix follows the majority and this encoder's own character rule — a derivation, not a call.
  Only Alt: `Ctrl+Backspace` stays DEL, where xterm.js and ghostty send BS; not examined here.
- **What the modes decide reaches the consumer as a derived mask** (#941, wire v18). A consumer
  that substitutes a key — PenTerm types `\` + CR for Shift+Enter, because legacy sends it as a
  plain CR — must not do so once the application asked for an encoding that carries Shift, and it
  has to know before the key is encoded. The frame's `modified_keys` answers per modifier
  (Shift / Alt / Ctrl) and per key whose bare form is a C0 control (Enter / Tab / Backspace /
  Escape): *does the modified press encode differently from the bare one?* **It is computed by
  running `encode_key` on both**, which is ADR-0016's mask-over-enum argument taken one step
  further — there the table is shared, here the encoder itself is the table, so the two cannot
  disagree. The raw modes would mislead in a way that was measured: kitty flags with
  report-events alone are non-zero and still leave Shift+Enter a plain CR. The per-C0-key shape is
  the **maintainer's call** (2026-09-18, shown against a single Shift+Enter bit and the raw
  state); splitting it by modifier is a derivation. The derivation is memoized process-wide over
  its 2 048 input combinations, because both placements that re-ran it were measured to cost:
  per frame, a 1-cell frame went ~190 ns → 1–3 µs; per mode write, a stream of kitty push/pop
  went ~4 → ~290 ms/MiB, which any application's output can emit.
- **The web half normalises, it does not encode.** Its intent types mirror `input.rs` as a contract;
  the protocol bytes are the backend's job. A consumer that encoded in the browser would have to
  replicate the mode tracking, which it cannot see.
- **The input target is a hidden textarea, not the canvas** — a canvas cannot receive IME events at
  all. Focus restoration must go through the widget's own `focus()`; focusing the canvas kills typing
  and IME together. So `element` does **not** have to be focusable and the widget never makes it so
  (#649) — but a focusable element's pointer-down must have its default cancelled, because the
  browser's focusing steps run *after* the widget's handler and would blur the textarea it just
  focused. Since #902 the widget cancels every press it acts on (reported, or handed to
  `TerminalOptions.selection`); a press it does not act on is still the consumer's to cancel.
  xterm.js has the same pairing (`preventDefault()` then focus).
- **The textarea's identity is `INPUT_ATTRIBUTE` (`data-justerm-input`), published as a contract**
  (#903). A host asking *"is the keyboard in a text field?"* reads any focused `<textarea>` as a form
  field, so a terminal pane looked like typing: global shortcuts stood down and focus claims would not
  move off it. Neither thing the element already had is an identity — `aria-label` is accessible text,
  not a name a host may key on, and its position under `element` is not promised (PenTerm matched a direct
  child meanwhile). A data attribute rather than xterm.js's `xterm-helper-textarea` class, because a
  consumer's CSS cannot collide with it and the widget already marks its scrollbar that way
  (`data-justerm-scrollbar`, #902 — read internally, not exported, so this is the first published one). **No accessor on `Terminal`**, unlike xterm.js's `readonly
  textarea`: the attribute answers both questions a host has — *is this element a terminal's input*
  (`hasAttribute`) and *where is this widget's* (`element.querySelector`) — and an accessor adds a
  second contract about when it is `undefined` (before mount, output-only, after dispose). **That was
  the maintainer's call**, made with the accessor and its lifecycle cost shown side by side.
- **A composition freezes the anchor for every writer, forced or not** (#637 for the frame stream,
  #649 for the point-of-use re-sync). The predicate is "a candidate window is open" — `isComposing`,
  not the broader `active`, which outlives it by one deferred read and so would swallow the
  `compositionstart` re-sync in continuous CJK.
- **A composition's origin comes from the composition before it, not from the frame stream, whenever
  a commit is still in flight** (#911, ADR-0028 D4). The frame stream is not merely behind at that
  instant — the committed text has not been sent yet — so the value read is the previous composition's
  own run end, which is `setPreedit`'s returned caret column and not an advance computed here (the
  widget has no `wcwidth`). The predicate is `active`, the mirror of the bullet above: the window
  #649 had to keep the guard OUT of is exactly the window this has to act IN.
- **An IME confirmation is a raw text intent**, not a paste — bracketed-paste markers would tell the
  application something untrue about where the text came from.
- **A consumer claims a key through `TerminalOptions.beforeKey`, asked after the IME gate** (#901).
  A composition key never reaches the consumer, and a key that finalizes a composition has committed
  its text before the consumer is asked. This is the reverse of xterm.js, which asks its custom key
  handler first; under that order every consumer owns a composition guard, and a claimed `Enter`
  skips the finalize, so the commit goes out at `compositionend` — after whatever the consumer sent
  for the key. Reordered, not lost. **A claimed key keeps its
  browser default**, as in xterm.js: an un-cancelled paste chord goes on to fire `paste`, which the
  widget sends as a paste intent, so the consumer cancels any default it replaces. A capture-phase
  listener on an ancestor with `stopPropagation` could claim keys before this existed, but it hides
  the event from every listener below that ancestor and has no ordering against the IME. A **Ctrl** chord
  pressed mid-composition was measured with the Windows Korean IME: the IME finalizes on `Ctrl`, so
  the letter arrives as an ordinary key and the consumer can claim it. A Shift chord is still
  unmeasured (see the rows). Rows in
  [`reference-facts.md`](../../agents/reference-facts.md).
- **The widget owns the pointer, and the route is decided at the press** (#902, implementing
  ADR-0016's *"mouse routing consults the same bits"*, which until then only the wheel did).
  `PointerRouter` sends a press to the application when the mask's DOWN bit is set and Shift is not
  held, and otherwise to `TerminalOptions.selection` — the consumer's `SelectionController`, handed
  over rather than wired by the consumer. That ownership is the **maintainer's call** (2026-09-15),
  made on a lens + refuter pass over four shapes: a consumer consulting an exported verdict before
  calling its controller (what an unmigrated or forgetful wiring gets wrong silently — it selects
  *and* reports), the controller consulting an injected mask, the widget swallowing app-bound presses
  in the capture phase (the #901 mechanism, rejected above, and it would kill a scrollbar thumb inside
  `element`), and the widget owning dispatch. It chose the last. What that decision did **not** cover:
  the dedup below, and whether a press routed to the application should clear a selection.
  - **Deciding at the press is enough because core filters at encode time.** `encode_mouse` gates on
    the live `wanted_events()`, so a release or drag after the application stopped tracking is
    dropped there. The widget owes two things only: a local press never has its release reported,
    and a reported press never reaches the selection.
  - **Shift forces a press local on every platform, and is not an option** (maintainer's call,
    2026-09-15). alacritty and ghostty use Shift everywhere; xterm.js alone uses Alt on macOS behind
    `macOptionClickForcesSelection`, and Alt here already means block selection and alt-click cursor
    move. That a forced Shift press **anchors** rather than extends (`mouseDown(ev, detail, forced)`)
    is a **derivation, not part of that call**: the engine drops a selection on every screen swap, so
    the one the controller remembers is usually gone by the time an application takes the mouse, and
    an extend of nothing selects nothing. It anchors on the normal screen too, where the remembered
    selection survives — the same as xterm.js, whose selection service does not extend while mouse
    events are active, and unlike ghostty, which extends.
  - **The gesture is followed on `window`**, only while one is live, and a reported gesture ends when
    no button is held. Bare motion (MOVE) is listened for on `element` and never while a gesture is
    live, so a drag is not reported twice.
  - **A reported gesture whose release never arrived ends** at the next buttonless move, or at a
    press with no other button held — otherwise it would keep claiming presses, Shift included.
  - **The DOM back/forward buttons (3/4) are not reported.** The widget's DOM→intent map does not name
    them, so they arrive as `null`, and core encodes a buttonless press as code 3 — the legacy
    *release*. The intent type and core do have `back`/`forward` (#52); mapping them is not done.
    Leaving the drop, not mapping the buttons and not making core refuse a buttonless press, is the
    **maintainer's call** (2026-09-15), shown both alternatives. Core still accepts that input.
  - **A scrollbar inside `element` is not the grid.** `Scrollbar` marks its track
    (`SCROLLBAR_ATTRIBUTE`) and `Terminal` routes no press or motion whose target is inside it —
    PenTerm mounts its track inside the pane, over the canvas's last columns. Skipped by target rather
    than by the thumb stopping propagation: that was the first version, and it also hid the press from
    every ancestor, which PenTerm's pane host uses to take the keyboard. xterm.js's slider reaches the
    "not the grid" half through `pointerdown.preventDefault()`, which suppresses the compatibility
    `mousedown` altogether.
  - `CaptureOptions.mouseReporting` survives for a consumer building its own widget from the parts;
    `Terminal` no longer passes it.
- **A bad `CellGeometry` is reported, not refused** (`geometryViolations`). Every length is CSS px and
  `NaN` in any field poisons every pointer event silently, so the preconditions are *signalled*. xterm
  refuses instead, but its refusal is half of a repair loop — the same predicate triggers a
  re-measure — and this widget does not measure: the geometry arrives per event from the consumer
  (#578, ADR-0017). alacritty's casts saturate and ghostty's cell is an integer type, so neither
  transfers. (A clause here once said copying xterm's guard "buys the drop without the recovery"; it
  measured false in a real browser — `getGeometry` is pulled per event, so a refused gesture resumes
  on the correct cell once the box returns. The rest stands; its scope is the *cell*, and an absent
  *box* is refused where it is measured — see the absent-box invariant.) The signal is a bare
  `console.warn`, as xterm's own converter does rather than its off-by-default `LogService`, and it
  is **deduped per field**, unlike xterm, because the reach here is every pointer event at `mousemove`
  rate. Rows: [who bounds a pointer coordinate](../../agents/reference-facts.md#who-bounds-a-pointer-coordinate--the-producer-not-the-engine-667-verified-2026-07-31).
- **`CellGeometry` is CSS px, and saying so is load-bearing.** The published README's example once
  built it from `renderer.cellSize()` — device px — so every click resolved to the wrong cell at
  `devicePixelRatio !== 1`; nothing type-checks a unit.

## Code

- `justerm-core/src/input.rs` — the mode flags and the encoders' shared types
- `justerm-core/src/term.rs` — `Term::encode_key`, `encode_mouse`, `encode_paste`, `encode_focus`,
  and the mode flags they read (`bracketed_paste`, and the DEC modes tracked from output)
- `justerm-web/src/input.ts` — DOM events → intent objects; the intent types mirror the backend
  contract
- `justerm-web/src/pointer.ts` — `PointerRouter` (press/drag/release/bare-motion routing) and
  `pressGoesToApp`; `Terminal.attach` binds its listeners and owns the selection tick timer
- `justerm-web/src/terminal.ts` — `makeHiddenTextarea` (the input target, marked with
  `INPUT_ATTRIBUTE`) and `Terminal.attach`, which mounts it inside `element`
- `justerm-web/src/composition.ts` — IME composition, including the backspace-during-composition case
  reported as one delete

## Reference behaviour

**Two sections** in `docs/agents/reference-facts.md` — modifyOtherKeys (#890), which is also the
first time this territory's encoders were read against the trees rather than described, and the
kitty stack per screen plus legacy Alt on the C0 named keys (#941). Everything else is still
unpinned: the encoders are described as the legacy xterm
baseline, and the IME delete case cites xterm's `C0.DEL` in a comment — an implementation claim about
a named reference with no pinned row, in the area where a wrong byte is invisible until an
application misbehaves.

## Cross-cutting invariants

- [the cell size is derived state](../invariant/cell-size-is-derived-state.md)
  — `CellGeometry` is a cell divisor with a lifetime and a unit, and nothing type-checks either
  (#578)
- [a pointer coordinate is bounded by the converter that produces it](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — **this territory is where the rule was first discovered** (#266, against `encode_mouse`'s
  `usize` wrap) and where it read as a mouse-reporting fact rather than a shared one, which is how
  the sibling converter went four issues without it (#667)
- [an IME composition is browser-owned state the engine never sees](../invariant/composition-is-browser-owned-state.md)
  — **this territory owns the mechanism the invariant is about**: `composition.ts`, the hidden textarea
  as the real input target, and the decision that a confirmation is a raw `text` intent. The fact has
  been true since #116 and was recorded nowhere, so the behaviours downstream of it (#592 the caret,
  #631 the anchor) each derived it locally. The consequence that reaches other territories is the one
  worth carrying out of here: a composition has **no frame to key on**, and the frame stream keeps
  describing a cursor that knows nothing about it

## Blast radius

- [frame](frame.md) — `mouse_events` is a header scalar (ADR-0016), and it exists so the consumer can
  route an event to the application or keep it local; `modified_keys` (#941) is its keyboard
  counterpart, so a change to what `encode_key` emits for a modified Enter / Tab / Backspace /
  Escape moves a header value and the `*.ignored.golden` inventory rows of the mode writes
- [events & replies](events-and-replies.md) — replies travel the same direction and share the
  consumer's PTY write path, but are generated by parsing rather than by a user action
- [accessibility](accessibility.md) — the hidden textarea is both the input target and
  part of the accessibility surface, so a change to focus handling reaches both
- [selection](selection.md) — mouse intents drive selection when the application has *not* asked for
  mouse events; the wanted-events mask is what decides which
- [viewport](viewport.md) — since #913 an input intent can also **move the view**: typing while
  scrolled up asks the consumer for the live edge. So a change to what becomes an intent, to the IME
  gate, or to `beforeKey`'s verdict now changes when the viewport moves, and the two paths are not
  symmetric — a key the IME **swallows** produces no intent at all yet still snaps, so it is handled
  at the gate rather than at the sink

## Known holes / open

- **Zero governing records for the encoding**, in a territory where being wrong produces a
  misbehaving application rather than an error.
- **No same-cell motion dedup** (#902 left it out, maintainer's call). All three references drop a
  repeated motion report, and core is stateless, so `?1003` repeats a cell at pointer rate. The key is
  the unresolved part: a cell key drops the sub-cell motion `?1016` exists to carry and a pixel key
  duplicates cell reports, and the widget cannot choose because ADR-0016 kept the coordinate encoding
  off the wire.
- **A reported press can also act elsewhere.** A link controller the consumer wires on the same element
  still opens on its modified click, and a right press still fires `contextmenu`, so a tracking
  application gets the press while the page acts on it. The widget owns neither; the references split
  three ways. Found by #902's lens, unmeasured beyond reading.
- **A press reported while the viewport is scrolled into history carries a viewport row.**
  `encode_mouse` takes viewport coordinates and the offset resets only on a screen swap, so on the
  normal screen the row is not the one the application drew there. The wheel's report already had
  this; #902 made presses reach it.
- **`?1016` pixels are CSS px, and nothing pairs that with a cell pixel size.** An application turns
  reported pixels into cells by dividing by a pixel size it learned elsewhere, and core answers no
  pixel query (`window_ops` handles 22/23 only), so the only source is the consumer's PTY winsize.
  All three references report the mouse pixels and the size in **one** unit; here the widget
  publishes CSS px for the pointer (#907) while `cellSize()` is device px, so a consumer filling
  `ws_xpixel` from `cellSize()` would be off by `dpr`. Unreached in the first consumer, which sends
  `pixel_width: 0` (PenTerm `src-tauri/src/pty/manager.rs`, measured 2026-09-15). Found by #907's lens.
- **Whether a press routed to the application clears a selection** is undecided (#902 did not cover
  it). A selection made before an application took the mouse on the normal screen stays highlighted
  while its clicks are reported; core already clears on a screen swap.
- ~~**The kitty keyboard protocol is deferred, not decided.**~~ — **closed by #23, and this line
  outlived it by a long way.** The flag stack, the push/pop/set forms, the query reply and the
  `CSI u` encoding all ship (`input.rs::kitty_encode`, `tests/kitty.rs`); what the bullet described
  as *"a design sketch with no record and no issue-level commitment"* has been running code for
  long enough that #890 measured against it. The hole it was pointing at is real and narrower:
  there is still **no decision record** for the encoding, which is the first bullet in this
  section, not a second one.
- **A consumer's substitution decided from `modified_keys` cannot be corrected at encode time**
  (#941). The mask is the last frame's, the key is encoded against the live engine, and the window
  is up to one frame cadence after a kitty push/pop or XTMODKEYS. The mouse mask has the same lag
  but is safe because `encode_mouse` re-checks the live mask; a substitution is sent as typed text,
  so core never sees the key it replaced. Unmeasured beyond reading; found by #941's lens. **Handed
  to PenTerm** (maintainer, 2026-09-18): its backend holds the engine and encodes every key, so it
  can decide at encode time; nothing in core closes it.
- ~~**One kitty flag stack serves both screens.**~~ — **closed in #941.** Flags an application
  pushed on the alternate screen and never popped (a crash) kept the shell in `CSI u`, and since
  `modified_keys` kept `SHIFT_ENTER` set there. Each screen now keeps its own flags and stack, swapped
  when the screen actually changes; the alternate screen's survive between visits and RIS clears
  both, as all three references do (rows in `reference-facts.md`). The mask could not have caught
  it — it shares the encoder's model by construction — so the tests are on the encoder
  (`tests/kitty.rs`). Found by #941's lens and refuter.
- **Two mode sets have to agree across a crate boundary.** The web mirrors `input.rs`'s intent types
  by hand, the same ungated mirroring `types.ts` does for the frame.
- ~~**In-progress IME composition is not rendered inline in the grid**~~ — **closed by #249**
  (2026-08-03, ADR-0028). The composition is drawn by `justerm-renderer` as a pass over the composed
  cells; the widget pushes it from `compositionupdate.data` (never the textarea value, which lags it
  by one event) and re-aims the anchor at the run's end. The hole this leaves is smaller and named:
  the drawn run and the eventual commit can disagree for Korean, because `data` is what the IME is
  showing and `value` is what will be committed — that is the IME's behaviour, not a defect to fix.
- **The IME anchor's *other* readers are only partly known.** Measured (#649): the browser's focus
  steps are a real second reader — focusing the textarea scrolls the nearest scrollable ancestor, and
  the destination tracks the anchor 1:1, so a stale anchor scrolls the page proportionally wrong.
  Whether an AT tool or magnifier is a *third* reader is still unmeasured, and it is the open question
  on spine #640 that decides whether the focus-time re-sync can be dropped for xterm's
  `focus({ preventScroll: true })`.
