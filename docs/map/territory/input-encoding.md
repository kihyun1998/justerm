# Territory — input encoding

## What it is

The inverse of `feed`: a key, mouse, paste or focus event becomes the byte sequence a TUI expects on
its stdin. What those bytes are depends on **DEC modes the engine learned from the *output* stream**,
so the encoder cannot live where the events are. It spans two crates, and the split is exact: the
**web widget turns DOM events into intents** (`input.ts`, `pointer.ts`, and `composition.ts` for an
IME commit), and **core turns intents into bytes** (`input.rs`, under the modes `Term` holds). The
browser has the event, the engine has the modes. Writing the bytes to the child is the consumer's
(or `justerm-pty`'s `Pty::write`).

## Governing decisions

- [ADR-0016 — mouse mode as a wanted-events mask in the frame](../../adr/0016-mouse-mode-wanted-events-mask-in-frame.md)
  — how the consumer learns which pointer events the application wants, so it can route rather than
  guess (wire v7→8). It decides the routing signal, and "mouse routing consults the same bits"; not
  the encoding.
- [ADR-0017 — mechanism vs policy](../../adr/0017-core-consumer-boundary-mechanism-vs-policy.md) —
  the modes are engine state, so the encoding is engine work; the DOM event, a paste filter and a
  key substitution are the consumer's.

Nothing governs the encoding itself.

## Design model

From the source and the dated calls below.

**Core: event + modes → bytes**
- **The engine owns the modes; the encoders are pure.** `encode_key`, `encode_mouse`, `encode_paste`
  and `encode_focus` in `input.rs` are free functions of `event + modes`; `Term::encode_*` (forwarded
  by `Engine::encode_*`) supplies the live modes. So the functions are testable without a PTY and the
  consumer's I/O stays its own.
- **Modes are hidden state learned from output.** DECCKM, application keypad, mouse tracking and
  encoding, focus reporting, bracketed paste, the kitty flags and modifyOtherKeys — an application
  turns them on by *printing*, so the same keystroke encodes differently depending on what was printed
  earlier (`docs/architecture.md` §Hidden VT state, "input encoding is mode-gated").
- **The legacy xterm baseline plus two negotiated extensions, asked in a fixed order** (`encode_key`).
  The kitty keyboard protocol (`CSI u` plus a progressive-flag stack, #23) is a **stateful superset**:
  it replaces the legacy form for what legacy cannot express, and is asked first (`kitty_encode`; its
  private-use codepoints for functional keys are not done). modifyOtherKeys (#890) is not a superset
  — it rewrites one case *inside* legacy, and is asked after. (This territory said kitty was
  "deliberately deferred" until 2026-09-11, two months after #23 shipped it, in two places at once.)
- **modifyOtherKeys rewrites legacy from the inside** (#890). `CSI > 4 ; 2 m` makes a modified
  character — and a modified Tab / Enter / Escape / Backspace, whose bare forms are C0 controls —
  encode as `CSI 27 ; <1+mods> ; <codepoint> ~`, which is how `Ctrl+I` stops being Tab
  (`modify_other_key`). Its placement after kitty and inside legacy is the one ghostty states a reason
  for: traditional encoding, modifyOtherKeys and fixterms are extensions that combine.
  - **The gate on which characters qualify is this territory's sharpest divergence, forced by the
    seam.** The widget normalises a DOM event into a produced *character* plus the modifiers it saw,
    so a capital arrives as `Char('A') + SHIFT`, where a keysym-based terminal has already spent the
    Shift. xterm's `0x40..=0x7f` clause, which xterm and ghostty use, would turn every capital into
    an escape sequence here; `char_qualifies` drops it.
  - **The gate asks the *parameter*, not the raw bits.** `csi_param` drops Super / Hyper / CapsLock /
    NumLock, so a gate on the bitflags admitted a chord it could not describe — `Shift+Super+A` came
    out as `CSI 27;2;65~`, byte-identical to a bare `Shift+A`. Reachable through the widget, which maps
    `metaKey` to `SUPER` unconditionally (`modsOf`): every macOS `Cmd+Shift+<letter>` while the mode is
    on. The reference masks first for the same reason.
  - **Two values are ours, not the reference's.** The shape is `CSI 27 ; <mods> ; <code> ~` and not the
    `CSI <code> ; <mods> u` alternative, which is byte-for-byte what the kitty path already produces —
    two protocols a consumer negotiates separately must not be indistinguishable on the wire. A
    modified Backspace carries **127**, not 8, because 127 is what this encoder gives a bare Backspace
    (the PC-keyboard convention), and the two must name the same key; ghostty's table spells the same.
    `Delete` and the other named keys are deliberately out — they already have an unambiguous
    modified form.
- **Legacy Alt is an ESC prefix on the key's own encoding, named keys included** (#941). Enter / Tab /
  Backspace / Escape used to drop Alt, so `Alt+Backspace` reached readline as a bare DEL rather than
  `M-DEL`. The prefix now goes on whatever the key otherwise encodes (`Alt+Shift+Tab` is `ESC CSI Z`,
  `alt_prefixes_esc_to_the_c0_named_keys`). Under modifyOtherKeys 2 and kitty the negotiated form
  answers first and Alt stays a parameter. Tab is where the references split: xterm under
  `metaSendsEscape`, alacritty and ghostty prefix it; xterm.js alone sends a bare HT — the prefix
  follows the majority and this encoder's own character rule, a derivation, not a call. Only Alt:
  `Ctrl+Backspace` stays DEL, where xterm.js and ghostty send BS; not examined.
- **Each screen keeps its own kitty flags and stack** (#941). Flags an application pushed on the
  alternate screen and never popped (a crash) used to keep the shell in `CSI u`. They are now swapped
  when the screen actually changes; the alternate screen's survive between visits and RIS clears both,
  as all three references do (`flags_pushed_on_the_alternate_screen_do_not_outlive_it`,
  `ris_clears_both_screens_flags`). The push stack is capped at 16 and drops the oldest
  (`Term::kitty_dispatch`).
- **What the modes decide reaches the consumer as a derived mask** (#941, wire v18). A consumer that
  substitutes a key — PenTerm types `\` + CR for Shift+Enter, because legacy sends it as a plain CR —
  must stop once the application asked for an encoding that carries Shift, and must know before the
  key is encoded. The frame's `modified_keys` answers per modifier (Shift / Alt / Ctrl) and per C0 key
  (Enter / Tab / Backspace / Escape): does the modified press encode differently from the bare one?
  **It is computed by running `encode_key` on both** (`derive_modified_keys`), ADR-0016's
  mask-over-enum argument one step further: the encoder itself is the table, so the two cannot
  disagree (`assert_mask_agrees_with_encoder`). The raw modes would mislead, measured: kitty flags
  with report-events alone are non-zero and still leave Shift+Enter a plain CR
  (`kitty_flags_without_disambiguate_still_drop_shift_on_enter`). The per-C0-key shape is the
  **maintainer's call** (2026-09-18, shown against a single Shift+Enter bit and the raw state);
  splitting it by modifier is a derivation. The derivation is memoized process-wide over its 2 048
  mode combinations (`modified_keys`), because both placements that re-ran it were measured to cost:
  per frame, a 1-cell frame went ~190 ns → 1–3 µs; per mode write, a stream of kitty push/pop went
  ~4 → ~290 ms/MiB.
- **A paste sends one `\r` per line ending, and inside a bracket no ESC** (#1036). `encode_paste` is
  the one encoder every consumer shares, so the floor lives there: a Windows clipboard's `\r\n` sent
  as-is reaches readline inside the bracket as two line breaks, and an `ESC[201~` in the payload closes
  the bracket and turns the rest into typed input. ESC becomes `␛` (U+241B) only inside the bracket —
  outside one nothing can tell paste from typing. **Stripping other control characters is policy and
  stays with the consumer** (PenTerm's paste setting; xterm keeps it under a user switch). No option,
  the maintainer's call (2026-10-08), made on the four-reference table linked below.
- **Mouse reports are gated on the live mask at encode time.** `encode_mouse` drops any event whose
  category `MouseProtocol::wanted_events` does not contain — the same table the frame carries — and a
  wheel release always. So a release or drag arriving after the application stopped tracking is
  dropped in core, whatever the widget routed.
- **win32-input-mode (`?9001`) is tracked, not encoded** (#86). The flag answers DECRQM, but the raw
  key-record encoding is passthrough the ConPTY consumer owns; `encode_key` is deliberately unchanged
  (`tests/win32_input.rs`).

**Web: DOM event → intent**
- **The web half normalises, it does not encode.** `Intent` mirrors `input.rs` as a contract (key,
  mouse, paste, focus, and `text` for an IME commit); the protocol bytes are the backend's job. A
  consumer that encoded in the browser would have to replicate the mode tracking, which it cannot see.
- **The input target is a hidden textarea, not the canvas** (#116) — a canvas cannot receive IME
  events. Focus restoration must go through the widget's own `focus()`; focusing the canvas kills
  typing and IME together. So `element` does **not** have to be focusable and the widget never makes it
  so (#649: the option's doc had claimed otherwise, and no `tabIndex` was ever written) — but a
  focusable element's pointer-down must have its default cancelled, because the browser's focusing
  steps run after the widget's handler and would blur the textarea it just focused. Since #902 the
  widget cancels every press it acts on; a press it does not act on is the consumer's to cancel.
  xterm.js has the same pairing (`preventDefault()` then focus).
- **The textarea's identity is `INPUT_ATTRIBUTE` (`data-justerm-input`), published as a contract**
  (#903). A host asking "is the keyboard in a text field?" reads any focused `<textarea>` as a form
  field, so a terminal pane looked like typing. Neither thing the element already had is an identity —
  `aria-label` is accessible text, and its position under `element` is not promised. A data attribute
  rather than xterm.js's `xterm-helper-textarea` class, because a consumer's CSS cannot collide with
  it and the widget already marks its scrollbar that way (`SCROLLBAR_ATTRIBUTE`, read internally, not
  exported). **No accessor on `Terminal`**, unlike xterm.js's `readonly textarea`: the attribute
  answers both a host's questions (`hasAttribute`, `element.querySelector`), and an accessor adds a
  contract about when it is `undefined`. The maintainer's call, made with both shown.
- **An IME confirmation is a raw `text` intent, not a paste** — bracketed-paste markers would tell the
  application something untrue about where the text came from. `CompositionController` reads the
  textarea's *value* after the native event settles (never the event `data`), and reports a backspace
  while an IME is active as one DEL (`\x7f`, xterm's `C0.DEL`), ported from xterm's
  `CompositionHelper`.
- **A consumer claims a key through `TerminalOptions.beforeKey`, asked after the IME gate** (#901). A
  composition key never reaches the consumer, and a key that finalizes a composition has committed its
  text before the consumer is asked. This reverses xterm.js, which asks its custom key handler first;
  under that order every consumer owns a composition guard, and a claimed Enter skips the finalize, so
  the commit goes out at `compositionend` after whatever the consumer sent. **A claimed key keeps its
  browser default**, as in xterm.js: an un-cancelled paste chord goes on to fire `paste`, which the
  widget sends as a paste intent. A capture-phase listener with `stopPropagation` could claim keys
  before this existed, but it hides the event from every listener below and has no ordering against
  the IME. A Ctrl chord mid-composition was measured with the Windows Korean IME: the IME finalizes on
  Ctrl, so the letter arrives as an ordinary key the consumer can claim. A Shift chord is unmeasured.
- **Shift+Insert and Ctrl+Insert are left to the browser, not encoded** (#1068). On Windows and Linux
  they are the system paste and copy, and both work only through the default the keydown triggers.
  `isSystemClipboardChord` is asked after `beforeKey`, so a consumer can still claim either chord, and
  Shift+Insert's paste goes through the existing `paste` listener. xterm.js does the same
  (`Keyboard.ts` `case 45`) with a wider predicate — Shift or Ctrl, whatever else is held; here it is
  exactly one of the two with no Alt or Meta. No platform branch, as at xterm.js. **The widget leaves
  the `copy` event alone — copying is the consumer's.** xterm.js answers it with
  `clipboardData.setData(selectionText)`, which needs the text synchronously during dispatch; here
  `Terminal` holds no `SelectionPort`, `SelectionPort.text()` is async, and the widget holds no
  clipboard. So with nothing listening Ctrl+Insert copies nothing — it simply no longer sends
  `ESC [ 2 ; 5 ~`. The maintainer's call (2026-10-15), made against a widget copy hook.
- **The widget owns the pointer, and the route is decided at the press** (#902). `PointerRouter` sends
  a press to the application when the mask's DOWN bit is set and Shift is not held (`pressGoesToApp`),
  and otherwise to `TerminalOptions.selection`, the consumer's selection controller handed over (and,
  since #934, to the link tracker). That ownership is the **maintainer's call** (2026-09-15), made on a
  lens + refuter pass over four shapes: a consumer consulting an exported verdict, the controller
  consulting an injected mask, the widget swallowing app-bound presses in the capture phase, and the
  widget owning dispatch. It chose the last. It did not cover same-cell motion dedup, nor whether a
  press routed to the application should clear a selection.
  - **Deciding at the press is enough because core filters at encode time** (above). The widget owes
    two things only: a local press never has its release reported, and a reported press never reaches
    the selection.
  - **Shift forces a press local on every platform, and is not an option** (maintainer's call,
    2026-09-15). alacritty and ghostty use Shift everywhere; xterm.js alone uses Alt on macOS behind
    `macOptionClickForcesSelection`, and Alt here already means block selection and alt-click cursor
    move. That a forced press **anchors** rather than extends is a derivation: the engine drops a
    selection on every screen swap, so an extend of nothing selects nothing. Same as xterm.js, unlike
    ghostty, which extends.
  - **The gesture is followed on `window`** only while one is live, and a reported gesture ends when no
    button is held. Bare motion is listened for on `element` and never while a gesture is live, so a
    drag is not reported twice. A reported gesture whose release never arrived ends at the next
    buttonless move, or at a press with no other button held.
  - **The DOM back/forward buttons (3/4) are not reported.** The DOM→intent map names left, middle and
    right only, so they arrive as `null`, and `PointerRouter` refuses a buttonless press or release —
    core would encode it as code 3, the legacy *release*. The intent type and core do have
    `back`/`forward` (#52); mapping them is not done. That, not making core refuse a buttonless press,
    is the maintainer's call (2026-09-15); core still accepts that input.
  - **A scrollbar inside `element` is not the grid.** `Terminal` routes no press or motion whose target
    is inside `[data-justerm-scrollbar]` — PenTerm mounts its track inside the pane. Skipped by target
    rather than by the thumb stopping propagation, which also hid the press from PenTerm's pane host.
  - `CaptureOptions.mouseReporting` survives for a consumer building its own widget from the parts;
    `Terminal` does not pass it.
- **A pointer coordinate is clamped by its converter** (`cellEvent`): col/row to the grid, px/py floored
  and bounded to the last pixel inside it, because an unbounded value would wrap in core's `usize`
  (#266, #907) — see the invariant below.
- **A bad `CellGeometry` is reported, not refused** (`geometryViolations`, `checkGeometry`). Every
  length is CSS px and `NaN` in any field poisons every pointer event silently, so the preconditions
  are *signalled*. xterm refuses instead, but its refusal is half of a repair loop that re-measures,
  and this widget does not measure: the geometry arrives per event from the consumer (#578, ADR-0017).
  (A clause once said copying xterm's guard "buys the drop without the recovery"; measured false in a
  real browser — `getGeometry` is pulled per event, so a refused gesture resumes on the correct cell
  once the box returns. Its scope is the *cell*; an absent *box* is refused where it is measured,
  #819.) The signal is a bare `console.warn`, deduped per field because the reach is every pointer
  event.
- **`CellGeometry` is CSS px, and saying so is load-bearing.** The published README's example once
  built it from `renderer.cellSize()` — device px — so every click resolved to the wrong cell at
  `devicePixelRatio !== 1`; nothing type-checks a unit.
- **The preedit run, the textarea anchor and the composition origin are the widget's, not this
  territory's.** They push to the renderer rather than producing an intent (`preeditIntent`,
  `preeditLatch`, `textareaMove` in `composition.ts`); see [widget lifecycle](widget-lifecycle.md) and
  the composition invariant.

**Inputs and their bounds**
- One DOM event yields at most one intent; a paste intent carries the clipboard's text whole, and
  `encode_paste` allocates its length plus 12 bytes. Nothing bounds a paste's size here; a consumer
  that limits one does so before encoding.

## Code

- `justerm-core/src/input.rs` — `Modifiers` (`csi_param`, `kitty_param`), `Key`, `KeypadKey`,
  `KeyAction`, `KeyEvent`, `MouseButton`, `MouseAction`, `MouseEvent`, `MouseProtocol::wanted_events`,
  `MouseEvents`, `MouseEncoding`, `ModifiedKeys`, `modified_keys`, `derive_modified_keys`, `encode_key`,
  `kitty_encode`, `modify_other_key`, `char_qualifies`, `encode_char`, `encode_mouse`, `event_category`,
  `encode_paste`, `encode_focus`
- `justerm-core/src/term.rs` — `Term::encode_key`, `Term::encode_mouse`, `Term::encode_paste`,
  `Term::encode_focus`, `Term::bracketed_paste`; the kitty per-screen swap
  (`kitty_flags_inactive`, `kitty_stack_inactive`)
- `justerm-core/src/term/replies.rs` — `Term::kitty_dispatch` (push / pop / set / query)
- `justerm-core/src/lib.rs` — `Engine::encode_key`, `Engine::encode_mouse`, `Engine::encode_paste`,
  `Engine::encode_focus`, `Engine::win32_input_mode`
- `justerm-core/tests/input.rs` · `kitty.rs` · `modify_other_keys.rs` · `modified_keys.rs` ·
  `mouse_encoding.rs` · `mouse_mode.rs` · `keypad.rs` · `win32_input.rs`
- `justerm-web/src/input.ts` — `Intent`, `InputSink`, `KeyEvent`, `MouseEvent`, `CellGeometry`,
  `geometryViolations`, `checkGeometry`, `clampTo`, `mouseFromDom`, `wheelMouseFromDom`, `keyFromDom`,
  `isSystemClipboardChord`, `CaptureOptions`, `captureInput`, `isUserInput`, `MouseEvents`,
  `ModifiedKeys`, `Mod`
- `justerm-web/src/pointer.ts` — `PointerRouter` (`down`, `move`, `hover`, `up`), `pressGoesToApp`,
  `LocalPointer`, `PointerRouterDeps`
- `justerm-web/src/composition.ts` — `CompositionController` (`keydown`, `compositionStart`,
  `compositionEnd`), the IME commit as a `text` intent and the IME delete as one DEL
- `justerm-web/src/terminal.ts` — `makeHiddenTextarea`, `INPUT_ATTRIBUTE`, `Terminal.attach` (binds the
  listeners, the IME gate before `beforeKey`, the selection tick timer)

## Reference behaviour

In `docs/agents/reference-facts.md` — linked, never restated.

- [modifyOtherKeys](../../agents/reference-facts.md#modifyotherkeys--which-level-breaks-the-control-aliases-and-the-predicate-that-would-break-plain-typing-890-verified-2026-09-11)
  — which level breaks the control aliases and why xterm's character clause cannot be copied (#890);
  the first time these encoders were read against the trees
- [The kitty flag stack per screen, and legacy Alt on the C0 named keys](../../agents/reference-facts.md#the-kitty-flag-stack-per-screen-and-legacy-alt-on-the-c0-named-keys-941-verified-2026-09-18)
  (#941)
- [What a paste sends — line endings and ESC](../../agents/reference-facts.md#what-a-paste-sends--line-endings-and-esc-1036-verified-2026-10-08)
  (#1036)
- [A consumer's veto over a key](../../agents/reference-facts.md#a-consumers-veto-over-a-key--where-it-sits-against-the-ime-and-who-owns-the-default-it-leaves-901-verified-2026-09-15)
  — where `beforeKey` sits against the IME and who owns the default it leaves (#901)
- [Who bounds a pointer coordinate](../../agents/reference-facts.md#who-bounds-a-pointer-coordinate--the-producer-not-the-engine-667-verified-2026-07-31)
  — the producer, not the engine (#667)

Everything else is unpinned: the legacy baseline is described as xterm's, and the IME delete cites
xterm's `C0.DEL` in a comment — an implementation claim about a named reference with no pinned row,
where a wrong byte is invisible until an application misbehaves.

## Cross-cutting invariants

- [the cell size is derived state](../invariant/cell-size-is-derived-state.md)
  — `CellGeometry` is a cell divisor with a lifetime and a unit, and nothing type-checks either (#578)
- [a pointer coordinate is bounded by the converter that produces it](../invariant/pointer-coordinates-are-bounded-by-their-producer.md)
  — first discovered here (#266, against `encode_mouse`'s `usize` wrap), where it read as a
  mouse-reporting fact, which is how the sibling converter went four issues without it (#667)
- [an IME composition is browser-owned state the engine never sees](../invariant/composition-is-browser-owned-state.md)
  — this territory owns the mechanism: `composition.ts`, the hidden textarea as the input target, and
  a commit as a raw `text` intent. True since #116 and recorded nowhere until the invariant, so #592
  (the caret) and #631 (the anchor) each derived it locally. What reaches other territories: a
  composition has no frame to key on

## Blast radius

- [browser proof harness](browser-proof-harness.md) — `captureInput` is browser-only glue the unit
  suite cannot run (`environment: "node"`; its doc says "not unit-tested"), so which DOM default a key
  keeps or cancels — a `paste`, a `copy`, a composition — is proven only by `justerm-web/e2e/demo.spec.ts`
  (#1068's Shift+Insert / Ctrl+Insert test is one).
- [frame](frame.md) — `mouse_events` (ADR-0016) and `modified_keys` (#941) are header values derived
  from the encoders' own tables; a change to what `encode_key` emits for a modified Enter / Tab /
  Backspace / Escape moves a header value and the `*.ignored.golden` rows of the mode writes.
- [events & replies](events-and-replies.md) — encoded input and the engine's replies are one byte
  stream to the child with no framing between them, so their hand-over order is the only separation.
- [accessibility](accessibility.md) — the hidden textarea is both the input target and part of the
  accessibility tree, so a change to its focus handling or labelling reaches both.
- [selection](selection.md) — a press goes to the selection only when the mask's DOWN bit is clear or
  Shift is held; the selection assumes it never sees a reported press.
- [viewport](viewport.md) — since #913 an input can move the view: typing while scrolled up asks for
  the live edge. A change to what becomes an intent, to the IME gate or to `beforeKey`'s verdict
  changes when the viewport moves, and a key the IME swallows produces no intent yet still snaps.
- [widget lifecycle](widget-lifecycle.md) — the IME gate and the anchor/preedit writers share
  `CompositionController`'s `composing` / `active` predicates; a change to either moves the other.
- [hyperlinks](hyperlinks.md) — `PointerRouter` hands a local primary press to the link tracker too;
  a change to the routing predicate changes which presses can open a link.

## Known holes / open

- **Zero governing records for the encoding**, in a territory where being wrong produces a
  misbehaving application rather than an error.
- **Shift+Insert and Ctrl+Insert on macOS are unmeasured** (#1068). If a Mac browser reports a PC
  keyboard's Insert as `key === "Insert"`, both chords are left to a default Blink's mac key table may
  not have, so they do nothing — before #1068 they reached the application as keys. A key log on a
  Mac with a PC keyboard settles it.
- **No same-cell motion dedup** (#902 left it out, maintainer's call). All three references drop a
  repeated motion report, and core is stateless, so `?1003` repeats a cell at pointer rate. The key is
  unresolved: a cell key drops the sub-cell motion `?1016` exists to carry, a pixel key duplicates cell
  reports, and the widget cannot choose because ADR-0016 kept the coordinate encoding off the wire.
- **A reported press can also act elsewhere.** A link controller wired by the consumer on the same
  element still opens on its modified click, and a right press still fires `contextmenu`, so a
  tracking application gets the press while the page acts on it. The references split three ways.
  Found by #902's lens, unmeasured beyond reading. Narrower since #934: the widget's own link
  tracker is handed only presses that stay local, so the remaining case is a consumer-wired one.
- **A press reported while the viewport is scrolled into history carries a viewport row.**
  `encode_mouse` takes viewport coordinates and the offset resets only on a screen swap, so on the
  normal screen the row is not the one the application drew there. The wheel's report already had
  this; #902 made presses reach it.
- **`?1016` pixels are CSS px, and nothing pairs that with a cell pixel size.** An application turns
  pixels into cells by dividing by a size it learned elsewhere, and core answers no pixel query
  (`window_ops` handles 22/23 only), so the only source is the PTY winsize. All three references report
  pointer pixels and size in one unit; here the widget publishes CSS px (#907) while `cellSize()` is
  device px, so a consumer filling `ws_xpixel` from `cellSize()` would be off by `dpr`. Unreached
  today: `justerm-pty`'s `pty_size` sends `pixel_width: 0`, and PenTerm's local panes run on it.
  Found by #907's lens.
- **Whether a press routed to the application clears a selection** is undecided (#902 did not cover
  it). A selection made before an application took the mouse on the normal screen stays highlighted
  while its clicks are reported; core already clears on a screen swap.
- **A consumer's substitution decided from `modified_keys` cannot be corrected at encode time** (#941).
  The mask is the last frame's, the key is encoded against the live engine, and the window is up to one
  frame cadence after a kitty push/pop or XTMODKEYS. The mouse mask has the same lag but is safe
  because `encode_mouse` re-checks the live mask; a substitution is sent as typed text, so core never
  sees the key it replaced. Found by #941's lens, unmeasured. **Handed to PenTerm** (maintainer,
  2026-09-18): its backend holds the engine and encodes every key, so it can decide at encode time.
- **Two mode sets have to agree across a crate boundary.** The web mirrors `input.rs`'s intent types
  and masks by hand, the same ungated mirroring `types.ts` does for the frame.
- **A C1 CSI inside a bracketed paste is not neutralised** (#1036 left it out). U+009B goes out as UTF-8
  `C2 9B`; whether any receiver reads the decoded character as CSI and so closes the bracket is
  unmeasured. Raised for Windows Terminal's paste filter (microsoft/terminal#9034), which strips C1;
  none of the four references guards it inside the bracket.
