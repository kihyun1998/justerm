//! The reply surface: what the engine answers an application, and what it relays for the
//! consumer to answer. DSR, the kitty flags query and DECRQM queue their replies directly (DA1/DA2
//! are answered inline in `dispatch`); colour, colour-scheme and clipboard queries become
//! `TermEvent`s that the consumer answers through the `report_*` methods. Also the title stack and
//! XTWINOPS.

use vte::Params;

use crate::event::{ClipboardTarget, TermEvent, Terminator};
use crate::input::{MouseEncoding, MouseProtocol};

use super::{MAX_CLIPBOARD_BASE64, TITLE_STACK_DEPTH, Term, param_or};

impl Term {
    /// Queue a color-scheme report (`CSI ? 997 ; 1 n` dark / `; 2 n` light) on the
    /// reply channel. The consumer calls this to answer a `ColorSchemeQuery` event
    /// or, when its scheme changes and `color_scheme_updates()` is set, to send the
    /// unsolicited notification. The engine never stores or interprets the scheme.
    pub fn report_color_scheme(&mut self, dark: bool) {
        let ps = if dark { 1 } else { 2 };
        self.replies
            .extend_from_slice(format!("\x1b[?997;{ps}n").as_bytes());
    }

    /// OSC 10/11/12 set/query the default fg/bg/cursor colour, stacking the
    /// `;`-separated specs across the `[foreground, background, cursor]` slots —
    /// xterm's `ChangeColorsRequest` offset loop (`misc.c:3679`, walking
    /// `OSC_TEXT_FG` → `OSC_TEXT_BG` → `OSC_TEXT_CURSOR`, `ptyx.h:1018-1020`).
    /// OSC 10 starts at slot 0, OSC 11 at slot 1, OSC 12 at slot 2 (#137, #832).
    /// A `?` spec is a query.
    ///
    /// **An empty spec addresses its slot and leaves it alone** — neither a set
    /// nor a reset — and the stack still advances past it, so `OSC 10 ; ; <bg>`
    /// is how xterm reaches the background alone. That skip-and-advance is xterm's
    /// *implementation*, not documented behaviour: `ctlseqs.txt:2082` documents only
    /// the stack (*"each successive parameter changes the next color in the list"*)
    /// and expects at least one parameter. The two empty
    /// cases are the same rule from both ends: nothing left in the string yields
    /// no name (`misc.c:3684-3685`), and a separator where a name should be yields no
    /// name either (`misc.c:3687`) before the parse steps past it.
    ///
    /// That rule is not a hardening detail here, it is a precondition: the empty
    /// form is the *only* one real applications emit for OSC 12 (`nvim` sends
    /// `ESC ] 12 ; BEL` four to five times per session), so a cursor slot without
    /// it would relay a burst of empty-string colour changes every time a user
    /// opens an editor (#832).
    ///
    /// The stack ends after the cursor. xterm's next slots are the pointer
    /// colours (`OSC_MOUSE_FG` = 13, `OSC_MOUSE_BG` = 14), which justerm does not
    /// model — dropping a fourth spec is better than mis-addressing it.
    pub(super) fn special_color(&mut self, params: &[&[u8]], start: usize, terminator: Terminator) {
        for (i, &spec) in params[1..].iter().enumerate() {
            if spec.is_empty() {
                continue; // skip this slot, but still advance to the next
            }
            let event = match start + i {
                0 if spec == b"?" => TermEvent::QueryForeground { terminator },
                0 => TermEvent::SetForeground(String::from_utf8_lossy(spec).into_owned()),
                1 if spec == b"?" => TermEvent::QueryBackground { terminator },
                1 => TermEvent::SetBackground(String::from_utf8_lossy(spec).into_owned()),
                2 if spec == b"?" => TermEvent::QueryCursorColor { terminator },
                2 => TermEvent::SetCursorColor(String::from_utf8_lossy(spec).into_owned()),
                _ => break, // past [fg, bg, cursor] — the pointer colours are unmodelled
            };
            self.events.push(event);
        }
    }

    /// OSC 52 (`OSC 52 ; Pc ; Pd`) — an application asking to put text on a
    /// selection, or to read one back (#828).
    ///
    /// The engine's half is *mechanism* and nothing else: recognise the
    /// sequence, resolve `Pc` to a [`ClipboardTarget`], decode `Pd`, and relay.
    /// It never touches a clipboard, never holds one, and carries no allow/deny
    /// knob — under ADR-0017 that gate is the consumer's, and a consumer that
    /// drops the event has refused the request. alacritty puts a gate at the
    /// equivalent site (`alacritty_terminal/src/term/mod.rs:1706`) — **not because
    /// it is the whole terminal, which is what this comment used to say and is
    /// not the distinction (#841)**: that gate is in alacritty's *engine* crate
    /// too, reading a policy the application injects through `Config`
    /// (`:353`, written at `alacritty/src/config/ui_config.rs:125`). The reason
    /// this crate has none is that it holds no clipboard at all, so a gate in
    /// front of a relay refuses nothing a dropped event does not already refuse.
    ///
    /// **An absent target field means the clipboard, and that is a divergence
    /// from the spec taken deliberately.** `ctlseqs.txt:2161` says *"If the
    /// parameter is empty, xterm uses s 0, to specify the configurable
    /// primary/clipboard selection and cut-buffer 0"*, and `misc.c:3359` is that
    /// sentence in code. Neither half is representable here: `s` is whichever
    /// selection a *user resource* has configured — policy, which ADR-0017 puts
    /// in the consumer — and cut buffers are not modelled at all. So "follow the
    /// spec" is not a well-defined instruction, exactly as it was not for #834's
    /// empty colour spec, where xterm's trigger (*a colour that failed to parse*)
    /// is a condition this engine structurally cannot observe.
    ///
    /// Two qualifications, because the short version of this argument overclaims
    /// in both directions. **The cut buffer is what is unrepresentable; `s` is
    /// merely unmodelled** — xterm resolves it through the `selectToClipboard`
    /// resource (`button.c:2081`), and DECSET 1041 sets that same resource *from
    /// the stream* (`ctlseqs.txt:1008`), so an engine tracking 1041 could
    /// resolve it. justerm declines to model 1041; that is a choice, not an
    /// impossibility. And **xterm-as-shipped reads the empty field as PRIMARY**,
    /// since the resource defaults to false — so this diverges from what the
    /// reference does by default, not merely from a sentence in its manual.
    ///
    /// What decides it is the other two lines of evidence. **Independent
    /// lineages, and there are fewer than a naive count gives**: alacritty never
    /// sees an empty field at all, because `vte` substitutes `b'c'` first
    /// (`vte-0.15.0/src/ansi.rs:1488`) — those two are *one* lineage, not two.
    /// The genuinely separate ones are ghostty, by an explicit byte-scan branch
    /// pinned under a test (`clipboard_operation.zig:24`, `:64`), and xterm.js's
    /// clipboard addon, whose browser provider ignores the selector entirely and
    /// always writes `navigator.clipboard`
    /// (`addons/addon-clipboard/src/ClipboardAddon.ts:77`). Three lineages, one
    /// answer. **And the emitter's own documentation agrees**: `tmux`'s
    /// `set-clipboard` is documented as setting *the terminal clipboard* — never
    /// the primary selection — and `tmux` 3.2a is measured sending exactly this
    /// empty form.
    ///
    /// **A missing payload *field* is not an empty payload.** `OSC 52 ; c`
    /// arrives as two fields and does nothing; `OSC 52 ; c ;` arrives as three,
    /// the third empty, and is a store of the empty string — which is how the
    /// sequence clears a selection. xterm makes the same split by construction,
    /// its whole handler sitting inside `if (*buf == ';')` (`misc.c:3353`), and
    /// ghostty rejects the payload-less form for the same reason
    /// (`clipboard_operation.zig:20`).
    ///
    /// **The payload is `params[2..]` rejoined**, the rule #650 established for
    /// OSC 8: `vte` splits the whole OSC body on `;`, so reading `params[2]`
    /// alone would take a payload containing a `;` and decode its first piece —
    /// which for a well-formed prefix is a *successful* decode of a truncated
    /// clipboard, the one failure mode this handler must not have. Rejoined, the
    /// stray `;` reaches the decoder and is refused there.
    ///
    /// **Malformed in, nothing out**, which is the second deliberate divergence
    /// — and it is narrower than the spec sentence makes it look. The spec ends
    /// a payload that is *"neither a base64 string nor ?"* by clearing the
    /// selection (`ctlseqs.txt:2174`); this drops it silently. Clearing is
    /// destructive, and inferring one from bytes the engine could not parse
    /// means corruption on the wire wipes what the user copied by hand.
    ///
    /// The family, counted rather than asserted: **three drop** — alacritty
    /// (`alacritty_terminal/src/term/mod.rs:1717`), ghostty, which returns on a
    /// decode failure (`src/Surface.zig:2186`) and states the rule beside its
    /// test as *"Read requests and malformed base64 must never reach the
    /// callback"* (`src/terminal/c/terminal.zig:2961`), and this. **One clears**
    /// — xterm.js's addon, deliberately: *"Clear clipboard if text is not a
    /// base64 encoded string"* (`addons/addon-clipboard/src/ClipboardAddon.ts:55`).
    /// **And one neither** — xterm, below.
    ///
    /// What the divergence actually is, stated precisely because reading the
    /// spec alone gets it wrong: **xterm has no validator to disagree with.**
    /// `AppendToSelectionBuffer` (`button.c:4679`) decodes one character at a
    /// time and `return`s on any byte outside the alphabet (`:4698`), so xterm
    /// *filters* rather than rejects — `Zm9v-Zm9v` yields `foofoo` there — and
    /// since the store path clears the buffer first (`misc.c:3410`), the spec's
    /// "cleared" is what falls out when the filter finds nothing to keep. So the
    /// disagreement is about what to **accept**, not about what to do on
    /// refusal, and this engine is the stricter of the two on purpose: a filter
    /// hands the consumer text assembled from bytes the application did not
    /// send.
    ///
    /// Non-UTF-8 is refused on the same principle one level up: every text
    /// surface this crate publishes is UTF-8, and a lossy conversion would hand
    /// the consumer characters the application never sent.
    pub(super) fn clipboard(&mut self, params: &[&[u8]], terminator: Terminator) {
        let Some(&field) = params.get(1) else {
            return;
        };
        let target = match field {
            // Empty and `c` are the same answer; see the divergence note above.
            b"" | b"c" => ClipboardTarget::Clipboard,
            // `p` and `s` are NOT the same answer, and the first draft made them
            // one. See `ClipboardTarget`: a collapse here would put a selector
            // the application never wrote into the reply.
            b"p" => ClipboardTarget::Primary,
            b"s" => ClipboardTarget::Selection,
            // Anything else — an unmodelled target like `q` or a cut buffer, and
            // also a *multi*-target list like `pc`, which the spec permits
            // (`ctlseqs.txt:2156`) and this engine cannot express. Both are
            // dropped rather than approximated: honouring one target of two is
            // the same defect as truncating a payload, one axis over, and
            // `vte`/alacritty's first-byte-wins would do exactly that. ghostty
            // rejects a multi-byte field too (`clipboard_operation.zig:36`).
            _ => return,
        };
        // Two fields is `OSC 52 ; c` — no payload field at all, not an empty one.
        if params.len() < 3 {
            return;
        }
        // The bound is checked on the fields, BEFORE the join — `join` is itself
        // an unconditional full copy, so checking after it would let a hostile
        // payload buy a second allocation the size of the first. `+ len - 3` is
        // the separators the join puts back.
        let fields = &params[2..];
        if fields.iter().map(|f| f.len()).sum::<usize>() + fields.len() - 1 > MAX_CLIPBOARD_BASE64 {
            return;
        }
        let payload: Vec<u8> = fields.join(&b';');
        if payload == b"?" {
            self.events
                .push(TermEvent::QueryClipboard { target, terminator });
            return;
        }
        if let Some(bytes) = crate::base64::decode(&payload)
            && let Ok(text) = String::from_utf8(bytes)
        {
            self.events.push(TermEvent::ClipboardStore { target, text });
        }
    }

    /// Answer an OSC 52 [`TermEvent::QueryClipboard`]: base64-encode the
    /// consumer's text into the OSC 52 reply envelope, ST-terminated.
    ///
    /// The consumer hands the target back rather than the engine remembering
    /// which one was asked about — the same shape as
    /// [`Term::report_palette_color`], which takes its `index` back for the same
    /// reason. alacritty is the alternative: its query captures the target and
    /// the terminator in a closure the consumer later calls
    /// (`alacritty_terminal/src/term/mod.rs:1740`), which is one more piece of
    /// hidden state and one more question ("what if replies interleave?") bought
    /// for nothing the consumer does not already hold.
    ///
    /// **Answering is optional, and that is the security property.** The engine
    /// holds no clipboard, so a query it is never asked to answer reveals
    /// nothing; a consumer refuses a *read* simply by not calling this, whatever
    /// it does about *writes*.
    ///
    /// **The selector round-trips.** `c` / `p` / `s` in, the same one out, which
    /// is why [`ClipboardTarget`] keeps `p` and `s` apart: every reference echoes
    /// the field the application wrote — xterm the recognised list
    /// (`misc.c:3384`), alacritty the raw byte (`…/term/mod.rs:1744`), ghostty
    /// its three locations (`src/Surface.zig:5954`) — and it is the one field a
    /// client can pair a reply on. The single exception is an **empty** field,
    /// answered naming `c`, which is what alacritty also sends once `vte` has
    /// defaulted it: the reply says what the engine understood, and there is no
    /// selector to echo.
    ///
    /// The reply echoes the terminator the query arrived with, like every other
    /// reply this crate queues — settled for the whole channel rather than for
    /// this sequence alone.
    pub fn report_clipboard(
        &mut self,
        target: ClipboardTarget,
        text: &str,
        terminator: Terminator,
    ) {
        let field = match target {
            ClipboardTarget::Clipboard => 'c',
            ClipboardTarget::Primary => 'p',
            ClipboardTarget::Selection => 's',
        };
        let data = crate::base64::encode(text.as_bytes());
        self.replies
            .extend_from_slice(format!("\x1b]52;{field};{data}").as_bytes());
        self.replies.extend_from_slice(terminator.bytes());
    }

    /// Answer an OSC 4 palette query: wrap the consumer-supplied spec for
    /// `index` in the OSC 4 reply envelope.
    ///
    /// The reply echoes the terminator the query arrived with, which the
    /// consumer takes off the `Query…` event and hands back here: the
    /// spec says a terminal *"uses the same terminator used in a query"*
    /// (`ctlseqs.txt:2020`), and the engine cannot choose on the consumer's
    /// behalf because only the parser ever saw which byte arrived.
    pub fn report_palette_color(&mut self, index: u8, spec: &str, terminator: Terminator) {
        self.replies
            .extend_from_slice(format!("\x1b]4;{index};{spec}").as_bytes());
        self.replies.extend_from_slice(terminator.bytes());
    }

    /// Answer an OSC 10 foreground query: wrap the consumer-supplied spec
    /// in the OSC 10 reply envelope.
    ///
    /// The reply echoes the terminator the query arrived with, which the
    /// consumer takes off the `Query…` event and hands back here: the
    /// spec says a terminal *"uses the same terminator used in a query"*
    /// (`ctlseqs.txt:2020`), and the engine cannot choose on the consumer's
    /// behalf because only the parser ever saw which byte arrived.
    pub fn report_foreground(&mut self, spec: &str, terminator: Terminator) {
        self.replies
            .extend_from_slice(format!("\x1b]10;{spec}").as_bytes());
        self.replies.extend_from_slice(terminator.bytes());
    }

    /// Answer an OSC 11 background query: wrap the consumer-supplied spec
    /// (it knows its palette) in the OSC 11 reply envelope. The engine formats
    /// the envelope only — it never knows the colour.
    ///
    /// The reply echoes the terminator the query arrived with, which the
    /// consumer takes off the `Query…` event and hands back here: the
    /// spec says a terminal *"uses the same terminator used in a query"*
    /// (`ctlseqs.txt:2020`), and the engine cannot choose on the consumer's
    /// behalf because only the parser ever saw which byte arrived.
    pub fn report_background(&mut self, spec: &str, terminator: Terminator) {
        self.replies
            .extend_from_slice(format!("\x1b]11;{spec}").as_bytes());
        self.replies.extend_from_slice(terminator.bytes());
    }

    /// Answer an OSC 12 cursor-colour query: the same envelope one slot
    /// over, terminated like its siblings. The consumer supplies the spec — it
    /// owns the palette, and the engine never learns the colour.
    ///
    /// The reply echoes the terminator the query arrived with, which the
    /// consumer takes off the `Query…` event and hands back here: the
    /// spec says a terminal *"uses the same terminator used in a query"*
    /// (`ctlseqs.txt:2020`), and the engine cannot choose on the consumer's
    /// behalf because only the parser ever saw which byte arrived.
    pub fn report_cursor_color(&mut self, spec: &str, terminator: Terminator) {
        self.replies
            .extend_from_slice(format!("\x1b]12;{spec}").as_bytes());
        self.replies.extend_from_slice(terminator.bytes());
    }

    /// Take the consumer events queued since the last drain, emptying the queue.
    pub fn drain_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut self.events)
    }

    /// Take the reply bytes queued since the last drain (DA/DSR/DECRQM answers),
    /// emptying the buffer. The consumer writes them back to the PTY.
    pub fn drain_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.replies)
    }

    /// Device Status Report (CSI Ps n): 6 = cursor position, 5 = operating
    /// status. Queues the reply for `drain_replies` (#27).
    pub(super) fn device_status_report(&mut self, param: u16) {
        match param {
            6 => {
                // CSI row;col R, 1-based — region-relative under origin mode
                // (the coordinate system the app is addressing in).
                let row = if self.origin_mode {
                    self.cursor.row.saturating_sub(self.scroll_top)
                } else {
                    self.cursor.row
                } + 1;
                let col = self.cursor.col + 1;
                self.replies
                    .extend_from_slice(format!("\x1b[{row};{col}R").as_bytes());
            }
            5 => self.replies.extend_from_slice(b"\x1b[0n"), // status: OK
            _ => {}
        }
    }

    /// Kitty keyboard-protocol negotiation (#23). `lead` is the leading CSI
    /// intermediate: `?` query, `>` push, `=` set, `<` pop.
    pub(super) fn kitty_dispatch(&mut self, lead: u8, params: &Params) {
        match lead {
            // Query → report the current flags as `CSI ? flags u` (#27 channel).
            b'?' => self
                .replies
                .extend_from_slice(format!("\x1b[?{}u", self.kitty_flags).as_bytes()),
            // Push: save the current flags, then set the new ones (default 0).
            b'>' => {
                const KITTY_STACK_CAP: usize = 16;
                if self.kitty_stack.len() >= KITTY_STACK_CAP {
                    self.kitty_stack.remove(0); // drop the oldest on overflow
                }
                self.kitty_stack.push(self.kitty_flags);
                self.kitty_flags = param_or(params, 0, 0) as u8;
            }
            // Pop `n` (default 1): restore from the stack, 0 once empty.
            b'<' => {
                for _ in 0..param_or(params, 0, 1) {
                    self.kitty_flags = self.kitty_stack.pop().unwrap_or(0);
                }
            }
            // Set in place (no push): mode 1 replace, 2 or-in, 3 and-not.
            b'=' => {
                let flags = param_or(params, 0, 0) as u8;
                self.kitty_flags = match param_or(params, 1, 1) {
                    1 => flags,
                    2 => self.kitty_flags | flags,
                    3 => self.kitty_flags & !flags,
                    _ => self.kitty_flags,
                };
            }
            _ => {}
        }
    }

    /// DECRQM (CSI ? Ps $ p): report whether DEC private mode `Ps` is set —
    /// `CSI ? Ps ; val $ y` with val 1=set, 2=reset, 0=not recognized (#27).
    pub(super) fn decrqm(&mut self, mode: u16) {
        let state = match mode {
            1 => Some(self.app_cursor_keys),
            // DECANM (#84): set = ANSI mode (the normal state), reset = VT52.
            2 => Some(!self.vt52_mode),
            6 => Some(self.origin_mode),
            // DECCOLM: derived from the actual width, never a tracked flag — a
            // flag would lie if the consumer ignored the resize request (#82).
            3 => Some(self.grid.cols() == 132),
            7 => Some(self.autowrap),
            45 => Some(self.reverse_wraparound),
            9 => Some(self.mouse_protocol == MouseProtocol::X10),
            66 => Some(self.application_keypad),
            12 => Some(self.cursor.blink),
            25 => Some(self.cursor.visible),
            // Mouse tracking is a single-state enum (the levels are mutually
            // exclusive — an app enables one), so querying ?1000 while ?1002 is
            // active reports "reset". Faithful to that model.
            1000 => Some(self.mouse_protocol == MouseProtocol::Normal),
            1002 => Some(self.mouse_protocol == MouseProtocol::ButtonEvent),
            1003 => Some(self.mouse_protocol == MouseProtocol::AnyEvent),
            1004 => Some(self.focus_events),
            1006 => Some(self.mouse_encoding == MouseEncoding::Sgr),
            1015 => Some(self.mouse_encoding == MouseEncoding::Urxvt),
            1005 => Some(self.mouse_encoding == MouseEncoding::Utf8),
            1016 => Some(self.mouse_encoding == MouseEncoding::SgrPixels),
            47 | 1047 | 1049 => Some(self.on_alt),
            2004 => Some(self.bracketed_paste),
            2026 => Some(self.synchronized_output),
            2027 => Some(self.grapheme_clustering),
            2031 => Some(self.color_scheme_updates),
            9001 => Some(self.win32_input_mode),
            _ => None,
        };
        let val = match state {
            Some(true) => 1,
            Some(false) => 2,
            None => 0,
        };
        self.replies
            .extend_from_slice(format!("\x1b[?{mode};{val}$y").as_bytes());
    }

    /// Set the window title and tell the consumer, in one place.
    ///
    /// Both writers come through here — the OSC 0/2 path and the XTWINOPS pop
    /// path — so the retained string and the event a consumer sees cannot
    /// disagree. A pop deliberately fires the **same** `TermEvent::Title` an
    /// ordinary title change does: both implementations that carry the stack do
    /// exactly this (xterm.js's `setTitle` fires `_onTitleChange`, alacritty's
    /// `pop_title` routes through `set_title`), and a second event would ask
    /// every consumer to learn a distinction it has no use for. The consequence
    /// worth stating: `Title` now means *"the title is this"*, not *"the
    /// application just set this"*.
    pub(super) fn set_window_title(&mut self, title: String) {
        self.window_title.clone_from(&title);
        self.events.push(TermEvent::Title(title));
    }

    /// XTWINOPS (`CSI Ps ; Ps ; Ps t`) — window manipulation, of which this
    /// engine implements exactly two operations (#823).
    ///
    /// It owns no window, so most of the family is meaningless here: 14/16 ask
    /// about pixels the engine has no concept of, and the resize/move/iconify
    /// operations are requests about a window the consumer owns. 22 (push
    /// title) and 23 (pop title) are different — they are pure VT state, and
    /// they were the single most-emitted unimplemented sequence in the capture
    /// sweep that produced #823. **This does not make `CSI t` a handled final**;
    /// every other first parameter still falls through and is ignored.
    ///
    /// The second parameter selects the axis: absent or `0` both, `1` the icon
    /// name, `2` the window title, anything else no axis at all. It is honoured
    /// rather than assumed away because real applications use it — `vim` emits
    /// a fully nested `22;0;0t · 22;2t · 22;1t … 23;2t · 23;1t · 23;0;0t`,
    /// which a single shared stack gets wrong.
    ///
    /// **The optional third parameter is deliberately ignored**, and that is a
    /// divergence from the spec rather than a simplification of it:
    /// `ctlseqs.txt:1698` gives a value in 1..10 *direct access to the stack*,
    /// storing or retrieving without pushing or popping, and xterm implements
    /// it (`charproc.c:9272`). Measured reach of that form is zero on every
    /// axis checked — no occurrence across seven programs recorded under real
    /// ptys, no file under `/usr/bin` or `/usr/lib64` containing it, no
    /// terminfo capability that emits `CSI 22/23 t` at all under any candidate
    /// `TERM`, and no other implementation honouring it (xterm.js ignores it,
    /// alacritty's dispatch never reads past the first parameter, ghostty
    /// carries the index and then drops the command). It entered xterm in patch
    /// #385 (2023-10-01) for symmetry with XTPUSHCOLORS, not because an
    /// application asked. So `CSI 22;2;3t` is an ordinary push here.
    pub(super) fn window_ops(&mut self, params: &Params) {
        // `param_or` folds an explicit 0 to the default, which is what both
        // axis and operation want: absent and `0` mean the same thing in each.
        let axis = param_or(params, 1, 0);
        let (window, icon) = (axis == 0 || axis == 2, axis == 0 || axis == 1);
        match param_or(params, 0, 0) {
            22 => {
                if window {
                    let title = self.window_title.clone();
                    push_title(&mut self.window_title_stack, title);
                }
                if icon {
                    let name = self.icon_name.clone();
                    push_title(&mut self.icon_name_stack, name);
                }
            }
            23 => {
                if window && let Some(title) = self.window_title_stack.pop() {
                    self.set_window_title(title);
                }
                // Restoring the icon name has no observable output: the engine
                // has no icon-name event. The stack is still popped so the two
                // axes stay aligned across mixed push/pop sequences.
                if icon && let Some(name) = self.icon_name_stack.pop() {
                    self.icon_name = name;
                }
            }
            _ => {}
        }
    }
}

/// First sub-parameter of CSI param `idx`, or `default` when absent or zero
/// (a zero/omitted numeric param means "1" for cursor movement and "0" for
/// erase — callers pass the right default).
/// Push onto an XTWINOPS title stack, bounded at [`TITLE_STACK_DEPTH`] (#823).
///
/// At the bound the **oldest** entry goes and the push still succeeds, which is
/// what both implementations carrying this feature do — xterm.js `shift()`s its
/// array, alacritty `remove(0)`s its `Vec`, and xterm wraps a fixed-size one.
/// The alternative (refuse the push) is worse in the case that actually occurs:
/// it breaks the pairing for the innermost nesting levels, and those are the
/// ones a user unwinds first.
fn push_title(stack: &mut Vec<String>, value: String) {
    if stack.len() >= TITLE_STACK_DEPTH {
        stack.remove(0);
    }
    stack.push(value);
}
