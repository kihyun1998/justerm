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

    /// OSC 10/11/12: set/query the default fg/bg/cursor colour, stacking the `;`-separated
    /// specs across the `[foreground, background, cursor]` slots from `start` (#137, #832). A
    /// `?` spec is a query, an empty spec skips its slot and still advances, and the stack
    /// ends after the cursor. Why: `docs/map/territory/vt-interpretation.md`.
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

    /// OSC 52 (`OSC 52 ; Pc ; Pd`) (#828): resolve `Pc` to a [`ClipboardTarget`], decode `Pd`
    /// and relay the store, or a query for `?`. The engine holds no clipboard and no allow/deny
    /// gate; a consumer refuses by dropping the event. An empty `Pc` is the clipboard, and a
    /// payload that is not base64 or not UTF-8 is dropped rather than clearing. Why:
    /// `docs/map/territory/events-and-replies.md`.
    pub(super) fn clipboard(&mut self, params: &[&[u8]], terminator: Terminator) {
        let Some(&field) = params.get(1) else {
            return;
        };
        let target = match field {
            // Empty and `c` both name the clipboard.
            b"" | b"c" => ClipboardTarget::Clipboard,
            // `p` and `s` stay apart: the reply echoes the selector the application wrote.
            b"p" => ClipboardTarget::Primary,
            b"s" => ClipboardTarget::Selection,
            // Anything else — `q`, a cut buffer, or a multi-target list like `pc` — is dropped,
            // not approximated.
            _ => return,
        };
        // Two fields is `OSC 52 ; c` — no payload field at all, not an empty one.
        if params.len() < 3 {
            return;
        }
        // The bound is checked on the fields, before the join copies them; `+ len - 1` is the
        // separators the join puts back.
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

    /// Answer an OSC 52 [`TermEvent::QueryClipboard`]: base64-encode the consumer's text into
    /// the OSC 52 reply envelope.
    ///
    /// The consumer hands back the event's `target` rather than the engine remembering it, as
    /// [`Term::report_palette_color`] takes back its `index`. The selector round-trips — `c` /
    /// `p` / `s` in, the same one out — except an empty field, which is answered naming `c`.
    /// Answering is optional: the engine holds no clipboard, so a consumer refuses a read by
    /// not calling this.
    ///
    /// The reply ends with `terminator`, the one the query arrived with, taken off the
    /// `Query…` event.
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

    /// Answer an OSC 4 palette query: wrap the consumer-supplied spec for `index` in the
    /// OSC 4 reply envelope.
    ///
    /// The reply ends with `terminator`, the one the query arrived with, taken off the
    /// `Query…` event.
    pub fn report_palette_color(&mut self, index: u8, spec: &str, terminator: Terminator) {
        self.replies
            .extend_from_slice(format!("\x1b]4;{index};{spec}").as_bytes());
        self.replies.extend_from_slice(terminator.bytes());
    }

    /// Answer an OSC 10 foreground query: wrap the consumer-supplied spec in the OSC 10
    /// reply envelope.
    ///
    /// The reply ends with `terminator`, the one the query arrived with, taken off the
    /// `Query…` event.
    pub fn report_foreground(&mut self, spec: &str, terminator: Terminator) {
        self.replies
            .extend_from_slice(format!("\x1b]10;{spec}").as_bytes());
        self.replies.extend_from_slice(terminator.bytes());
    }

    /// Answer an OSC 11 background query: wrap the consumer-supplied spec in the OSC 11
    /// reply envelope. The engine formats the envelope only — it never knows the colour.
    ///
    /// The reply ends with `terminator`, the one the query arrived with, taken off the
    /// `Query…` event.
    pub fn report_background(&mut self, spec: &str, terminator: Terminator) {
        self.replies
            .extend_from_slice(format!("\x1b]11;{spec}").as_bytes());
        self.replies.extend_from_slice(terminator.bytes());
    }

    /// Answer an OSC 12 cursor-colour query: wrap the consumer-supplied spec in the OSC 12
    /// reply envelope. The engine never learns the colour.
    ///
    /// The reply ends with `terminator`, the one the query arrived with, taken off the
    /// `Query…` event.
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

    /// Set the window title and tell the consumer. The OSC 0/2 path and the XTWINOPS pop both
    /// come through here and fire the same `TermEvent::Title` (#823):
    /// `docs/map/territory/events-and-replies.md`.
    pub(super) fn set_window_title(&mut self, title: String) {
        self.window_title.clone_from(&title);
        self.events.push(TermEvent::Title(title));
    }

    /// XTWINOPS (`CSI Ps ; Ps ; Ps t`): push (22) and pop (23) the title, and nothing else
    /// (#823). The second parameter selects the axis — absent or `0` both, `1` the icon name,
    /// `2` the window title, anything else none. The third (direct stack access) is ignored.
    /// Why: `docs/architecture.md` § Hidden VT state.
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

/// Push onto an XTWINOPS title stack, bounded at [`TITLE_STACK_DEPTH`] (#823). At the
/// bound the oldest entry goes and the push still succeeds (`docs/architecture.md`
/// § Hidden VT state).
fn push_title(stack: &mut Vec<String>, value: String) {
    if stack.len() >= TITLE_STACK_DEPTH {
        stack.remove(0);
    }
    stack.push(value);
}
