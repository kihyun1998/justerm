//! The VT dispatch surface: `vte::Perform` for `Term` — the parser's callbacks for printable
//! runs, C0 controls, CSI, ESC and OSC — plus the DEC private modes, the VT52 sub-parser and the
//! OSC 8 link registry lookup those callbacks drive.

use vte::{Params, Perform};

use crate::event::{NotificationSequence, TermEvent, Terminator};
use crate::input::{MouseEncoding, MouseProtocol};
use crate::serialize::MarkerKind;

use super::{Charset, LINK_IDS_FIRST_SWEEP, Term, param_or, version_number};

/// The `id=` value out of an OSC 8 `params` field, or `None` when it is absent or empty.
///
/// `params` is a `:`-separated key=value list; the first `id=` anywhere in it is the one
/// consulted, and an empty value is no id. The xterm.js rows each rule is taken from:
/// `docs/agents/reference-facts.md` § How xterm.js parses `id=` (#635).
fn osc8_link_id(params: &[u8]) -> Option<&[u8]> {
    params
        .split(|b| *b == b':')
        .find_map(|kv| kv.strip_prefix(b"id="))
        .filter(|value| !value.is_empty())
}

/// The most OSC fields `vte` hands to `osc_dispatch` (its private `MAX_OSC_PARAMS`,
/// 0.15.0); fields past it are dropped before dispatch.
const VTE_OSC_FIELD_CAP: usize = 16;

/// `Pp` of the secondary DA report: 1 = VT220 (`ctlseqs.txt:825`), the same terminal DA1
/// advertises as level 62. Why: `docs/architecture.md` § Hidden VT state (#824).
const DA2_TERMINAL_TYPE: u32 = 1;

/// `Pc` of the secondary DA report: 0, the absence value for a ROM cartridge registration
/// number. Why this and not alacritty's 1: `docs/architecture.md` § Hidden VT state (#824).
const DA2_ROM_CARTRIDGE: u32 = 0;

/// `Pv` of the secondary DA report: this build's crate version, folded at compile time.
const DA2_VERSION: u32 = version_number(env!("CARGO_PKG_VERSION"));

impl Term {
    /// Apply one DEC private mode set (`'h'`) or reset (`'l'`). DECSET/DECRST
    /// carry a list of modes, so `csi_dispatch` folds this over every parameter
    /// (#56); each mode is an independent toggle, not a stack.
    fn set_dec_private_mode(&mut self, action: char, mode: u16) {
        match (action, mode) {
            ('h', 1049) => self.enter_alt_screen(),
            ('l', 1049) => self.leave_alt_screen(),
            // Legacy alt-screen variants (#72): ?47/?1047 switch the buffer
            // without saving the cursor; ?1048 saves/restores the cursor without
            // switching. ?1049 is the two combined.
            ('h', 47) | ('h', 1047) => self.switch_to_alt(),
            ('l', 47) | ('l', 1047) => self.switch_to_primary(),
            ('h', 1048) => self.save_alt_cursor(),
            ('l', 1048) => self.restore_alt_cursor(),
            ('h', 6) => {
                // DECOM: set homes the cursor to the region top.
                self.origin_mode = true;
                self.goto(0, 0);
            }
            ('l', 6) => self.origin_mode = false, // unset leaves the cursor put
            ('h', 7) => self.autowrap = true,     // DECAWM
            ('l', 7) => self.autowrap = false,
            ('h', 45) => self.reverse_wraparound = true, // reverse wraparound (#80)
            ('l', 45) => self.reverse_wraparound = false,
            // DECCOLM (#82): the engine is dimension-free, so emit a request the
            // consumer may honor by resizing — no screen/cursor/margin change here.
            ('h', 3) => self.events.push(TermEvent::ColumnMode { cols: 132 }),
            ('l', 3) => self.events.push(TermEvent::ColumnMode { cols: 80 }),
            ('h', 25) => self.cursor.visible = true, // DECTCEM show
            ('l', 25) => self.cursor.visible = false, // DECTCEM hide
            ('h', 12) => self.cursor.blink = true,   // att610 cursor blink (#81)
            ('l', 12) => self.cursor.blink = false,
            ('h', 2004) => self.bracketed_paste = true,
            ('l', 2004) => self.bracketed_paste = false,
            ('h', 2026) => self.synchronized_output = true, // synchronized output (#73)
            ('l', 2026) => self.synchronized_output = false,
            ('h', 2027) => self.grapheme_clustering = true, // grapheme-cluster mode (#295)
            ('l', 2027) => self.grapheme_clustering = false,
            ('h', 2031) => self.color_scheme_updates = true, // color-scheme notifications (#85)
            ('l', 2031) => self.color_scheme_updates = false,
            ('h', 9001) => self.win32_input_mode = true, // win32-input-mode (#86)
            ('l', 9001) => self.win32_input_mode = false,

            // Input-encoding modes (#11): DECCKM, mouse tracking + encoding,
            // focus reporting. Each set assigns the level; each reset clears
            // it (apps enable/disable the same mode, not a stack).
            ('h', 1) => self.app_cursor_keys = true, // DECCKM
            ('l', 1) => self.app_cursor_keys = false,
            ('h', 66) => self.application_keypad = true, // DECNKM (#74)
            ('l', 66) => self.application_keypad = false,
            // DECANM (#84): set = ANSI (the normal state); reset enters VT52. Only
            // the reset is meaningful — `?2h` is a no-op (already ANSI).
            ('l', 2) => self.vt52_mode = true,
            ('h', 9) => self.mouse_protocol = MouseProtocol::X10, // X10 mouse (#70)
            ('h', 1000) => self.mouse_protocol = MouseProtocol::Normal,
            ('h', 1002) => self.mouse_protocol = MouseProtocol::ButtonEvent,
            ('h', 1003) => self.mouse_protocol = MouseProtocol::AnyEvent,
            ('l', 9) | ('l', 1000) | ('l', 1002) | ('l', 1003) => {
                self.mouse_protocol = MouseProtocol::Off
            }
            ('h', 1006) => self.mouse_encoding = MouseEncoding::Sgr,
            ('l', 1006) => self.mouse_encoding = MouseEncoding::Default,
            ('h', 1015) => self.mouse_encoding = MouseEncoding::Urxvt,
            ('l', 1015) => self.mouse_encoding = MouseEncoding::Default,
            ('h', 1005) => self.mouse_encoding = MouseEncoding::Utf8,
            ('l', 1005) => self.mouse_encoding = MouseEncoding::Default,
            ('h', 1016) => self.mouse_encoding = MouseEncoding::SgrPixels,
            ('l', 1016) => self.mouse_encoding = MouseEncoding::Default,
            ('h', 1004) => self.focus_events = true,
            ('l', 1004) => self.focus_events = false,

            _ => {} // other DEC modes are later slices
        }
    }

    /// Dispatch one VT52 escape sequence (`ESC <final>`), reached only while
    /// `vt52_mode` is set (#84). VT52 is a pre-ANSI dialect: the cursor/erase
    /// finals map to the same `Term` primitives the ANSI path uses. `ESC <`
    /// returns to ANSI. Unknown finals are ignored.
    fn vt52_dispatch(&mut self, byte: u8) {
        match byte {
            b'A' => self.move_up(1),         // cursor up
            b'B' => self.move_down(1),       // cursor down
            b'C' => self.move_forward(1),    // cursor right
            b'D' => self.move_back(1),       // cursor left
            b'H' => self.goto(0, 0),         // cursor home
            b'I' => self.reverse_index(),    // reverse line feed
            b'J' => self.erase_display(0),   // erase cursor → end of screen
            b'K' => self.erase_line(0),      // erase cursor → end of line
            b'Y' => self.vt52_y_pending = 2, // direct address: two coord bytes follow
            // Identify (DECID): reply `ESC / Z` — "I am a VT52".
            b'Z' => self.replies.extend_from_slice(b"\x1b/Z"),
            b'=' => self.application_keypad = true, // enter alternate keypad
            b'>' => self.application_keypad = false, // exit alternate keypad
            b'<' => self.vt52_mode = false,         // exit VT52, return to ANSI
            // RIS (`ESC c`) is honored even here: it is a hard "recover from any
            // state" reset, and `full_reset` rebuilds `Term` with `vt52_mode`
            // cleared, so RIS always escapes VT52 back to ANSI. VT52 defines no
            // other meaning for `ESC c`.
            b'c' => self.full_reset(),
            // Graphics mode (`ESC F`/`ESC G`) is a documented non-goal: the VT52
            // graphics glyph set differs from DEC Special Graphics, so reusing that
            // charset would render the wrong glyphs. No-op rather than approximate.
            b'F' | b'G' => {}
            _ => {} // unknown VT52 finals are ignored
        }
    }

    /// Consume one `ESC Y` coordinate byte (#84). The first byte is the row, the
    /// second the column; each decodes as `value - 0x20`. On the second byte the
    /// cursor is addressed (`goto` clamps out-of-range coordinates). Reached only
    /// from `print` while `vt52_y_pending > 0`.
    fn vt52_take_coord(&mut self, c: char) {
        let coord = (c as usize).saturating_sub(0x20);
        if self.vt52_y_pending == 2 {
            self.vt52_y_row = coord;
            self.vt52_y_pending = 1;
        } else {
            self.vt52_y_pending = 0;
            self.goto(self.vt52_y_row, coord);
        }
    }

    /// The allocation an OSC 8 `id=` names: the live one if that id already named a link
    /// with this same URI, else a fresh one recorded under the key (#635).
    ///
    /// Keyed on **id and URI together**, mirroring xterm.js's `_getEntryIdKey`
    /// (`` `${id};;${uri}` ``, `OscLinkService.ts:87`). Keying on the id alone would follow
    /// a reused id to a stale target — an application saying "same link" about two
    /// different destinations has not said anything the engine should honour.
    fn link_for_id(&mut self, id: &str, uri: &str) -> std::sync::Arc<str> {
        let key = format!("{id};;{uri}");
        // A key whose link has left the buffer is *absent*, not stale — the group it named
        // is gone, so this open starts a new one. That is xterm.js's behaviour too, reached
        // by deleting the entry rather than by letting a reference die.
        if let Some(live) = self.link_ids.get(&key).and_then(std::sync::Weak::upgrade) {
            return live;
        }
        // Amortised sweep before inserting, so dangling keys stay O(live) rather than
        // O(ids ever declared). Doubling the threshold keeps it O(1) per open.
        if self.link_ids.len() >= self.link_ids_sweep_at {
            self.link_ids.retain(|_, weak| weak.strong_count() > 0);
            self.link_ids_sweep_at = (self.link_ids.len() * 2).max(LINK_IDS_FIRST_SWEEP);
        }
        let fresh: std::sync::Arc<str> = std::sync::Arc::from(uri);
        self.link_ids.insert(key, std::sync::Arc::downgrade(&fresh));
        fresh
    }
}

impl Perform for Term {
    fn print(&mut self, c: char) {
        // VT52 `ESC Y` direct addressing (#84): vte delivers the two coordinate
        // bytes here (it returned to ground after the `Y` final), so intercept
        // them before they would be written as glyphs.
        if self.vt52_y_pending > 0 {
            self.vt52_take_coord(c);
            return;
        }
        // Translate through the active (GL) character set first (#62): under DEC
        // Special Graphics a printable byte becomes a line-drawing glyph.
        let c = self.charsets[self.gl].map(c);
        self.place_grapheme(c);
    }

    /// A DCS is terminated or aborted: not a print, so the repeat is disarmed. The payload
    /// is otherwise unhandled.
    fn unhook(&mut self) {
        // Why it exists and when it is load-bearing (#825):
        // `docs/map/territory/vt-interpretation.md`.
        self.repeat_anchor = None;
    }

    fn execute(&mut self, byte: u8) {
        // Not a print, so the repeat is disarmed (#825, [`Term::repeat_anchor`]). This
        // is also where `CAN` and `SUB` land, which abort a sequence in every state.
        self.repeat_anchor = None;
        match byte {
            // LF, VT, FF all line-feed.
            b'\n' | 0x0b | 0x0c => self.linefeed(),
            b'\r' => self.carriage_return(),
            0x08 => self.backspace(),
            b'\t' => self.put_tab(),
            0x07 => self.events.push(TermEvent::Bell), // BEL (#12)
            0x0e => self.gl = 1,                       // SO (LS1): GL = G1 (#62)
            0x0f => self.gl = 0,                       // SI (LS0): GL = G0
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        // Every completed CSI disarms `REP` (#825, [`Term::repeat_anchor`]). It is
        // *taken* here rather than cleared on the way out because this function has
        // six early returns and a clear at the end would miss all of them; `REP` is
        // the one arm that needs the value, and it puts it back.
        let repeat_anchor = self.repeat_anchor.take();
        // Kitty keyboard-protocol negotiation: CSI > / = / < / ? ... u. The
        // leading intermediate distinguishes it from plain `CSI u` (SCORC) (#23).
        if action == 'u'
            && let Some(&lead) = intermediates.first()
            && matches!(lead, b'>' | b'<' | b'=' | b'?')
        {
            self.kitty_dispatch(lead, params);
            return;
        }
        // DEC private modes arrive with a '?' intermediate.
        if intermediates.first() == Some(&b'?') {
            // DECRQM (CSI ? Ps $ p) — report whether mode Ps is set. The '$'
            // intermediate distinguishes it from a plain `?...p`. It queries a
            // single mode, so it keys off the first parameter only.
            if action == 'p' && intermediates.contains(&b'$') {
                self.decrqm(param_or(params, 0, 0));
                return;
            }
            // Private DSR (CSI ? Ps n): ?996 = color-scheme query (#85). The
            // theme-agnostic engine relays it as an event for the consumer.
            if action == 'n' {
                if param_or(params, 0, 0) == 996 {
                    self.events.push(TermEvent::ColorSchemeQuery);
                }
                return;
            }
            // DECSET/DECRST carry a *list* of modes; apply set/reset to EVERY
            // parameter, not just the first — htop batches `?1006;1000h` into one
            // CSI, so folding only params[0] dropped the 1000 (#56).
            for mode in params.iter().filter_map(|p| p.first().copied()) {
                self.set_dec_private_mode(action, mode);
            }
            return;
        }
        // DECSTR soft reset: CSI ! p (#53).
        if intermediates.first() == Some(&b'!') && action == 'p' {
            self.soft_reset();
            return;
        }
        // DECSCUSR set cursor style: CSI Ps SP q (space intermediate) (#89). The raw
        // value is read, since `param_or` folds 0 to its default and 0 is the reset.
        // `CSI SP q` arrives from vte as an explicit 0; `unwrap_or(1)` covers a
        // params list with no entry at all.
        if intermediates.first() == Some(&b' ') && action == 'q' {
            let param = params.iter().next().and_then(|p| p.first().copied());
            self.set_cursor_style(param.unwrap_or(1));
            return;
        }
        // DA2 (secondary device attributes, CSI > c) (#824). A private prefix is an
        // intermediate, so the pair is matched on the whole slice (`CSI > $ c` is not DA2)
        // ahead of the intermediate catch-all. What answering buys, and the references:
        // `docs/map/territory/vt-interpretation.md`.
        if intermediates == [b'>'] && action == 'c' {
            // Only `Ps = 0` or omitted is a request; any other qualifier gets no reply.
            if param_or(params, 0, 0) == 0 {
                self.replies.extend_from_slice(
                    format!("\x1b[>{DA2_TERMINAL_TYPE};{DA2_VERSION};{DA2_ROM_CARTRIDGE}c")
                        .as_bytes(),
                );
            }
            return;
        }
        // XTMODKEYS (`CSI > Pp ; Pv m`) (#890): only `Pp = 4` (modifyOtherKeys) is routed,
        // and `Pv >= 2` sets it. Why: `docs/architecture.md` § Hidden VT state.
        if intermediates == [b'>'] && action == 'm' {
            if param_or(params, 0, 0) == 4 {
                self.modify_other_keys_2 = param_or(params, 1, 0) >= 2;
            }
            return;
        }
        // Other private/intermediate sequences are later slices; ignore them
        // rather than misinterpret.
        if !intermediates.is_empty() {
            return;
        }
        match action {
            'A' => self.move_up(param_or(params, 0, 1) as usize),
            'B' => self.move_down(param_or(params, 0, 1) as usize),
            'e' => self.vertical_position_relative(param_or(params, 0, 1) as usize),
            // CNL / CPL (CSI Ps E / F): CUD / CUU, then CR (#898).
            'E' => {
                self.move_down(param_or(params, 0, 1) as usize);
                self.carriage_return();
            }
            'F' => {
                self.move_up(param_or(params, 0, 1) as usize);
                self.carriage_return();
            }
            'C' | 'a' => self.move_forward(param_or(params, 0, 1) as usize),
            'D' => self.move_back(param_or(params, 0, 1) as usize),
            // CBT (CSI Ps Z): back-tab, the mirror of HT over the tab-stop
            // table. Cursor motion only — it writes no cell (#826).
            'Z' => self.put_back_tab(param_or(params, 0, 1) as usize),
            // CHT (CSI Ps I): forward tab, the counted HT (#898).
            'I' => self.put_forward_tabs(param_or(params, 0, 1) as usize),
            // REP (CSI Ps b): repeat the preceding grapheme. `param_or` folds an
            // absent parameter and an explicit zero to one, as everywhere else here.
            'b' => {
                // Put back what this dispatch took, repeat, then clear what the repeats re-armed
                // through the print path: a second `CSI b` repeats nothing (#825). Why:
                // `docs/map/territory/vt-interpretation.md`.
                self.repeat_anchor = repeat_anchor;
                self.repeat_last(param_or(params, 0, 1) as usize);
                self.repeat_anchor = None;
            }
            'G' | '`' => self.set_col(param_or(params, 0, 1) as usize - 1),
            'd' => self.set_row(param_or(params, 0, 1) as usize - 1),
            'H' | 'f' => {
                let row = param_or(params, 0, 1) as usize - 1;
                let col = param_or(params, 1, 1) as usize - 1;
                self.goto(row, col);
            }
            'J' => self.erase_display(param_or(params, 0, 0)),
            'K' => self.erase_line(param_or(params, 0, 0)),
            'X' => self.erase_chars(param_or(params, 0, 1) as usize),
            '@' => self.insert_chars(param_or(params, 0, 1) as usize),
            'P' => self.delete_chars(param_or(params, 0, 1) as usize),
            'S' => self.scroll_up_lines(param_or(params, 0, 1) as usize),
            'T' => self.scroll_down_lines(param_or(params, 0, 1) as usize),
            'L' => self.insert_lines(param_or(params, 0, 1) as usize),
            'M' => self.delete_lines(param_or(params, 0, 1) as usize),
            'g' => self.clear_tab_stop(param_or(params, 0, 0)),
            'r' => {
                let rows = self.grid.rows() as u16;
                let top = param_or(params, 0, 1) as usize;
                let bottom = param_or(params, 1, rows) as usize;
                self.set_scroll_region(top, bottom);
            }
            'm' => self.sgr(params),
            's' => self.save_cursor(), // SCOSC (CSI s) — alias of DECSC
            't' => self.window_ops(params), // XTWINOPS — only 22/23 (#823)
            'u' => self.restore_cursor(), // SCORC (CSI u) — alias of DECRC
            // DA1 (primary device attributes, CSI c): advertise VT220 + ANSI
            // colour — the levels justerm actually implements (#27).
            'c' => self.replies.extend_from_slice(b"\x1b[?62;22c"),
            'n' => self.device_status_report(param_or(params, 0, 0)),
            // Non-private SM/RM. Folded over every parameter (modes can batch,
            // like the private path #56). IRM (4) and LNM (20) so far.
            'h' => {
                for m in params.iter().filter_map(|p| p.first().copied()) {
                    match m {
                        4 => self.insert_mode = true,
                        20 => self.newline_mode = true,
                        _ => {}
                    }
                }
            }
            'l' => {
                for m in params.iter().filter_map(|p| p.first().copied()) {
                    match m {
                        4 => self.insert_mode = false,
                        20 => self.newline_mode = false,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        // Not a print: the repeat is disarmed (#825, [`Term::repeat_anchor`]).
        self.repeat_anchor = None;
        // VT52 mode (#84): the pre-ANSI dialect reuses the same `ESC <final>`
        // tokens vte already produces, but with different meanings, so it is a
        // mode-gated branch here rather than a separate parser. All VT52 sequences
        // are intermediate-free; anything with an intermediate is not VT52.
        if self.vt52_mode && intermediates.is_empty() {
            self.vt52_dispatch(byte);
            return;
        }
        if let Some(&i) = intermediates.first() {
            // SCS: designate a charset to G0 (`ESC ( F`) or G1 (`ESC ) F`) (#62).
            if matches!(i, b'(' | b')') {
                let set = match byte {
                    b'0' => Charset::DecSpecialGraphics,
                    b'A' => Charset::Uk,
                    b'B' => Charset::Ascii,
                    _ => return, // other sets are later slices
                };
                self.charsets[if i == b'(' { 0 } else { 1 }] = set;
            }
            // Other intermediates (G2/G3 designators, etc.) are later slices.
            return;
        }
        match byte {
            b'D' => self.linefeed(), // IND (line-feed without CR)
            b'E' => {
                // NEL (next line): carriage return + line-feed.
                self.carriage_return();
                self.linefeed();
            }
            b'H' => self.set_tab_stop(),             // HTS
            b'M' => self.reverse_index(),            // RI
            b'7' => self.save_cursor(),              // DECSC
            b'8' => self.restore_cursor(),           // DECRC
            b'c' => self.full_reset(),               // RIS (#53)
            b'=' => self.application_keypad = true,  // DECKPAM (#74)
            b'>' => self.application_keypad = false, // DECKPNM
            _ => {}
        }
    }

    /// OSC dispatch: titles (0/2), cwd (7), notifications (9/777), command marks (133),
    /// hyperlinks (8), palette and dynamic colours (4/104/10-12/110-112), clipboard (52).
    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        // Not a print: the repeat is disarmed (#825, [`Term::repeat_anchor`]).
        self.repeat_anchor = None;
        // Ended by `CAN` / `SUB`, which cancel the sequence rather than end it
        // (#970, [`Term::cancel_byte_in_flight`]).
        if self.cancel_byte_in_flight {
            return;
        }
        // The byte that ended the sequence, which decides the byte that ends its reply; it
        // rides outward on the query events (#836, `docs/map/territory/events-and-replies.md`).
        let terminator = if bell_terminated {
            Terminator::Bel
        } else {
            Terminator::St
        };
        // params[0] is the OSC number; params[1..] the payload fields.
        let Some(&number) = params.first() else {
            return;
        };
        match number {
            // OSC 0 = icon + window title, OSC 2 = window title; both retained for the title
            // stack (#823). The payload is `params[1..]` rejoined on `;` (#880), and a fieldless
            // OSC is ignored where an empty field clears. `vte` hands over at most 16 fields (#840).
            b"0" | b"2" => {
                if let Some(fields) = params.get(1..).filter(|f| !f.is_empty()) {
                    let title = String::from_utf8_lossy(&fields.join(&b';')).into_owned();
                    if number == b"0" {
                        self.icon_name.clone_from(&title);
                    }
                    self.set_window_title(title);
                }
            }
            // OSC 7 = current working directory (a file:// URI), rejoined like the title (#880).
            b"7" => {
                if let Some(fields) = params.get(1..).filter(|f| !f.is_empty()) {
                    let cwd = String::from_utf8_lossy(&fields.join(&b';')).into_owned();
                    self.events.push(TermEvent::Cwd(cwd));
                }
            }
            // OSC 9 / OSC 777 = a notification. The payload is relayed rejoined and
            // uninterpreted, with the same no-field guard as the arms above; reaching
            // the field cap marks it as possibly cut rather than dropping it.
            b"9" | b"777" => {
                if let Some(fields) = params.get(1..).filter(|f| !f.is_empty()) {
                    let sequence = if number == b"9" {
                        NotificationSequence::Osc9
                    } else {
                        NotificationSequence::Osc777
                    };
                    self.events.push(TermEvent::Notification {
                        sequence,
                        payload: String::from_utf8_lossy(&fields.join(&b';')).into_owned(),
                        maybe_truncated: params.len() >= VTE_OSC_FIELD_CAP,
                    });
                }
            }
            // OSC 133 = FinalTerm/iTerm2 shell-integration command marks (#158):
            // `A` prompt start, `B` command start, `C` output start, `D[;exit]`
            // command finished. Each anchors a kinded marker at the cursor line;
            // pairing + navigation is consumer policy (#160). Unknown subcommands
            // (or none) are ignored. `D`'s exit field parses to `i32`, else None.
            b"133" => match params.get(1).copied() {
                Some(b"A") => self.add_command_mark(MarkerKind::PromptStart),
                Some(b"B") => self.add_command_mark(MarkerKind::CommandStart),
                Some(b"C") => self.add_command_mark(MarkerKind::OutputStart),
                Some(b"D") => {
                    let exit = params
                        .get(2)
                        .and_then(|p| core::str::from_utf8(p).ok())
                        .and_then(|s| s.parse::<i32>().ok());
                    self.add_command_mark(MarkerKind::CommandFinished(exit));
                }
                _ => {}
            },
            // OSC 8 = hyperlink: `OSC 8 ; params ; URI`. A non-empty URI opens a
            // link (made current); an empty URI closes it. `params` carries the
            // optional `id=` that groups runs into one link (#635).
            b"8" => {
                // One allocation per open, shared by that open's cells (#628); `id=` is the only
                // dedup (#635, `docs/map/territory/hyperlinks.md`). The URI is `params[2..]` rejoined
                // on `;` and never decoded (#650, ADR-0017); an empty one closes the link.
                let uri: Vec<u8> = params.get(2..).unwrap_or_default().join(&b';');
                self.current_link = if uri.is_empty() {
                    None
                } else {
                    let uri = String::from_utf8_lossy(&uri);
                    Some(match osc8_link_id(params.get(1).copied().unwrap_or(b"")) {
                        // No id declared: fresh per open, the reference-correct default.
                        None => std::sync::Arc::from(&*uri),
                        Some(id) => self.link_for_id(&String::from_utf8_lossy(id), &uri),
                    })
                };
            }
            // OSC 4 = set/query an ANSI palette entry: `OSC 4 ; index ; spec` (#122). The engine
            // forwards index + raw spec; the consumer applies it. An empty spec drops only its own
            // pair (#834, a deliberate divergence from xterm): `docs/map/territory/vt-interpretation.md`.
            b"4" => {
                // One event per `index ; spec` pair, advancing two fields whether or not a pair emits.
                let mut rest = &params[1..];
                while let [idx, spec, tail @ ..] = rest {
                    rest = tail;
                    if let Ok(index) = String::from_utf8_lossy(idx).parse::<u8>() {
                        if *spec == b"?" {
                            self.events
                                .push(TermEvent::QueryPaletteColor { index, terminator });
                        } else if !spec.is_empty() {
                            self.events.push(TermEvent::SetPaletteColor {
                                index,
                                spec: String::from_utf8_lossy(spec).into_owned(),
                            });
                        }
                    }
                }
            }
            // OSC 104 = reset palette entries (#122): an empty payload (`OSC 104` or `OSC 104 ;`,
            // #832) resets the whole table, else one event per named index.
            b"104" => {
                if params.len() <= 1 || (params.len() == 2 && params[1].is_empty()) {
                    self.events.push(TermEvent::ResetPaletteColor(None));
                } else {
                    for &idx in &params[1..] {
                        if let Ok(index) = String::from_utf8_lossy(idx).parse::<u8>() {
                            self.events.push(TermEvent::ResetPaletteColor(Some(index)));
                        }
                    }
                }
            }
            // OSC 10/11/12 = set/query the default foreground/background/cursor
            // colour, stacking specs across the [fg, bg, cursor] slots (#122,
            // #137, #832). Each code names the slot the stack starts at. The
            // engine forwards raw specs (theme-agnostic).
            b"10" => self.special_color(params, 0, terminator),
            b"11" => self.special_color(params, 1, terminator),
            b"12" => self.special_color(params, 2, terminator),
            // OSC 52 = manipulate selection data (#828): `OSC 52 ; Pc ; Pd`.
            // The engine decodes `Pd` and relays the request; the *clipboard* is
            // the consumer's, and so is every policy about it.
            b"52" => self.clipboard(params, terminator),
            // OSC 110 / 111 / 112 = reset the default foreground / background / cursor colour
            // (#122, #832), one slot each, never a stack.
            b"110" => self.events.push(TermEvent::ResetForeground),
            b"111" => self.events.push(TermEvent::ResetBackground),
            b"112" => self.events.push(TermEvent::ResetCursorColor),
            _ => {} // other OSCs are later slices
        }
    }
}
