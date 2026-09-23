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
/// Three rules, each taken from xterm.js's `_createHyperlink` verbatim rather than from
/// the spec prose, because each is a place a reasonable reading goes wrong
/// (`src/common/InputHandler.ts:3128-3131` at the pinned SHA `699f5537b023`):
///
/// - **`:`-separated**, not `;` — `params` is one OSC argument holding a key=value list
///   (`id=xyz123:foo=bar:baz=quux`), so the split is on colons (`params.split(':')`).
/// - **`id` may sit anywhere in it** (`findIndex(e => e.startsWith('id='))`), so matching
///   only a leading `id=` is the wrong parse and passes a single-parameter test.
/// - **an empty value is not an id** (`slice(3) || undefined`). This is the one with teeth:
///   an empty key would group every `id=`-with-no-value link in a session into one link
///   across unrelated URIs, and it is a wrong answer that grows with uptime.
///
/// Only the first `id=` is consulted, matching `findIndex` — an empty first one yields
/// `None` rather than searching on for a non-empty sibling.
fn osc8_link_id(params: &[u8]) -> Option<&[u8]> {
    params
        .split(|b| *b == b':')
        .find_map(|kv| kv.strip_prefix(b"id="))
        .filter(|value| !value.is_empty())
}

/// The most OSC fields `vte` hands to `osc_dispatch` (its private `MAX_OSC_PARAMS`,
/// 0.15.0); fields past it are dropped before dispatch.
const VTE_OSC_FIELD_CAP: usize = 16;

/// `Pp` of the secondary DA report: 1 = VT220, from the spec's closed table
/// (`ctlseqs.txt:825`). It names the same terminal DA1 already advertises —
/// `CSI ? 62 ; 22 c`, where 62 *is* VT220 (`ctlseqs.txt:778`).
///
/// The two are not forced to agree in general: DA1's first parameter is an
/// operating **level** and `Pp` a device **type**, and xterm can decouple them
/// through DECTID. justerm implements neither DECTID nor DECSCL, so nothing
/// here can decouple them — which is why one terminal identity is the only
/// coherent answer, not because disagreeing would be malformed.
///
/// ghostty is the convergence check: it derives both from one device type
/// (`src/terminal/device_attributes.zig:82`, `:161` @ `e6e26e1`) and pairs a
/// level-62 DA1 with `Pp = 1` (`src/termio/stream_handler.zig:843`). Its DA1 is
/// *not* byte-identical to justerm's at its default — `clipboard-write` is
/// `.allow` (`src/config/Config.zig:2380`), which appends `;52`; the identical
/// string is its deny branch. alacritty pairs `?6c` (VT102) with `Pp = 0` and
/// xterm.js `?1;2c` (VT100) with `Pp = 0`, both of which are what this same
/// rule produces at those levels — so neither is a counterexample, and no
/// reference in the corpus pairs a level-62 DA1 with a `Pp` other than 1.
const DA2_TERMINAL_TYPE: u32 = 1;

/// `Pc` of the secondary DA report: 0.
///
/// The ground is first-principles and needs no reference: `Pc` is a **ROM
/// cartridge registration number**, justerm has no cartridge, and 0 is the
/// absence value. That is how both implementations that comment the field read
/// it — xterm writes it as `/* options (none) */` (`charproc.c:4267`) and
/// ghostty as *"Always 0 for emulators"* (`src/terminal/device_attributes.zig:88`
/// @ `e6e26e1`). xterm.js sends 0 in all three of its branches; alacritty alone
/// sends 1 (`alacritty_terminal/src/term/mod.rs:1267` @ `852e971`).
///
/// **What does *not* carry this choice, stated because it looks like it should.**
/// `ctlseqs.txt:839` says `Pc` *"is always zero"* only of a **DEC terminal** —
/// a description of hardware in xterm's own documentation, not a requirement on
/// emulators. And ADR-0004 tie-breaks the spec against alacritty where alacritty
/// *"merely omits or under-implements"*; here alacritty **contradicts**, which
/// is neither of its branches, and its other branch (genuine ambiguity → follow
/// alacritty) would give 1. So the value rests on the argument above and on the
/// 3-1 head count, not on a spec mandate that does not exist.
const DA2_ROM_CARTRIDGE: u32 = 0;

/// The `Pv` this build reports, folded at compile time so the doc-comment above
/// is literally true and the query path does no arithmetic.
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

    /// A DCS is terminated: not a print, so the repeat is disarmed. This method
    /// exists for that alone — the payload is otherwise unhandled — and it is reachable
    /// in ordinary use: with DA2 answered, `vim` follows up with XTGETTCAP
    /// (`DCS + q <hex> ST`) queries this engine does not answer. Both halves of that
    /// are pinned on recorded bytes rather than asserted — `tests/closed_loop_capture.rs`.
    ///
    /// The end of the DCS and not its start, which is both xterm's rule (its gate fires
    /// when the parser returns to the ground state) and the only half that can be shown
    /// to matter: no CSI can arrive between `hook` and here, so a disarm in `hook` is a
    /// guard no mutation can redden.
    ///
    /// Which DCS terminator is fed decides whether this line is load-bearing at all,
    /// measured by deleting it (`-`, a DCS, then `CSI 3 b`, counting dashes):
    ///
    /// | terminator | dashes without this line |
    /// |---|---|
    /// | `ESC \` (7-bit ST) | 1 — `esc_dispatch` disarms on the `\` |
    /// | `0x9C` (8-bit ST, which DCS accepts where OSC refuses it) | 4 |
    /// | none; aborted by the `ESC` of the next sequence | 4 |
    ///
    /// So a test that feeds only `ESC \` proves nothing here, which is what the first
    /// version of `rep_after_a_dcs_repeats_nothing` did.
    ///
    /// The last row is a **deliberate divergence from xterm**, in the safe direction:
    /// an unterminated DCS never returns xterm's parser to the ground state, so xterm
    /// would still repeat, while `vte` calls this on the abort and this engine does
    /// not. Disarming too eagerly can only turn `REP` into a no-op.
    fn unhook(&mut self) {
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
        // DA2 (secondary device attributes, CSI > c) — the query vim uses to
        // fill `v:termresponse` and identify what it is talking to (#824).
        //
        // What answering it actually buys, measured as a control pair on a real
        // pty (RHEL 9.2, vim 8.2, TERM=xterm-256color, 24x80; every other query
        // answered identically in both arms, controls run before and after):
        //
        //     no reply           ->  ttymouse=xterm
        //     ESC[>1;1500;0c     ->  ttymouse=sgr
        //
        // The mouse protocol is what the version number buys *directly*: legacy
        // `xterm` encoding cannot report a column past 223 and cannot report a
        // release, `sgr` has neither limit.
        //
        // It is not the only effect, and the second one is the larger. Answering
        // also makes vim **ask ten more questions** — `DCS + q <hex> ST`
        // (XTGETTCAP) for `Co`, `ku`, `kd`, `kl`, `kr`, `k1`, `#2`, `#4`, `%i`
        // and `*7`: the colour count and the arrow / function / shifted key
        // codes. vim's own `term.txt` gates that on the reply indicating
        // "patchlevel 141 or higher", and its point is that a terminal produces
        // different key codes in different modes, so it asks instead of
        // guessing. Measured rather than taken from the doc: the requests appear
        // at `Pv` 276 and 1500 and are absent at 1, 94 and 95, which brackets the
        // gate to (95, 276] and is consistent with 141 without pinning it. What
        // is stable across runs is whether they appear at all; *how many* arrive
        // is not — one arm sent each capability once where every other sent it
        // twice. **justerm answers none of those today** — they fall to the
        // same intermediate catch-all this block sits above — so the capability
        // is unlocked and then unanswered. That is the honest state, and it is
        // #47 tail rather than this slice.
        //
        // modifyOtherKeys is *not* gated on any of it: vim emits `CSI > 4 ; 2 m`
        // about 180 bytes before it asks.
        //
        // `>` reaches us as an *intermediate*, so DA2 was not "unhandled" but
        // unreachable: the catch-all below returns before the final is ever
        // examined. This opened exactly one route through it — the `>` prefix
        // alone, with the `c` final. **A second one is open now**: the `m`
        // final, for XTMODKEYS (#890), in the block immediately below this one.
        //
        // It is one route out of ten `>` finals xterm routes, and the choice is
        // reach, not completeness: across this repo's capture corpus `CSI > c`
        // occurs 5 times and XTMODKEYS `CSI > m` 10 (re-measured 2026-09-11 after
        // #891 added a twentieth fixture; they read 4 and 7 when #890 chose on
        // them, and the order the choice turned on is unchanged) — the latter is
        // the highest-reach `>` sequence justerm did not route, which is why it
        // was the next one taken (#890) rather than a later one — it no longer falls
        // through here. XTVERSION `CSI > q` occurs **once**, in `tmux_clipboard.raw`.
        //
        // Both numbers moved after this paragraph was written, and the second one
        // changed sign: it said `> q` occurred *zero* times, which was true on
        // 2026-09-01 and false on 2026-09-02, when #842 checked in a tmux capture
        // that contains one (re-measured 2026-09-11, and a fresh tmux attach
        // recorded the same day emits it too — tmux asks unconditionally). A count
        // taken from a corpus is only ever true of one revision of it.
        //
        // And it is a floor rather than a measurement of reach, because all but one
        // capture is **open-loop** — recorded under `script(1)` or a bare `expect`,
        // both of which answer nothing, so no sequence an application only sends *after* a reply can
        // appear in it. The signature is in the corpus: answering DA2 makes vim ask
        // ten `DCS + q` XTGETTCAP questions, and `DCS + q` occurs **zero** times
        // across every open-loop fixture, four of which ask DA2. The exception is
        // `vim_closed_loop.raw`, which holds all ten (#891) — what it cost to record
        // one, and why its bytes are a function of a consumer policy as well as of
        // vim, is in `docs/map/territory/vt-interpretation.md`.
        //
        // The match is on the whole slice rather than `.first()`, so
        // `CSI > $ c` is not DA2. That is 3-1: xterm drops it
        // (`VTPrsTbl.c:4747`, `$` is CASE_CSI_IGNORE inside `dec2_table`),
        // ghostty drops it (`src/terminal/stream.zig:1612`, `else => null`) and
        // xterm.js drops it (its handler key packs prefix *and* intermediates,
        // `InputHandler.ts:233`), while alacritty **would answer** it
        // (`vte-0.15.0/src/ansi.rs:1572` passes `intermediates.first()`). No
        // producer of that form exists in any pinned corpus, so this is a
        // divergence with no measured reach — pinned by a test regardless,
        // because the predicate is otherwise unfalsifiable.
        if intermediates == [b'>'] && action == 'c' {
            // Only `Ps = 0` or omitted is a request; a qualifier we do not
            // recognise is answered with silence rather than with a report that
            // does not address it. xterm (`charproc.c:4220`), xterm.js
            // (`InputHandler.ts:1738`) and alacritty (`ansi.rs:1572`) all gate
            // this way; ghostty reads no parameter at all and has no test that
            // would notice.
            if param_or(params, 0, 0) == 0 {
                self.replies.extend_from_slice(
                    format!("\x1b[>{DA2_TERMINAL_TYPE};{DA2_VERSION};{DA2_ROM_CARTRIDGE}c")
                        .as_bytes(),
                );
            }
            return;
        }
        // XTMODKEYS (`CSI > Pp ; Pv m`) — the second route through the `>` guard, and
        // the highest-reach one: 10 occurrences across this repo's captures against
        // DA2's 5, all of them `Pp = 4` (modifyOtherKeys). `vim` sets it at startup
        // and clears it on exit, and the clear is the more frequent of the two.
        //
        // **Only `Pp = 4` is routed**, of the eight resources xterm keys off this one final;
        // `CSI > m` is deliberately not honoured, because an omitted `Pp` is measurably
        // indistinguishable from one aimed at another resource. **`Pv >= 2`, not `== 2`**:
        // level 2 is what separates `Ctrl+I` from `Tab`, and 3 asks for more than 2 rather
        // than for nothing. Both, with the reference sites, are in
        // `docs/agents/reference-facts.md` (#890).
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
                // The three lines are one rule and their order is the rule: put back
                // what this dispatch took, repeat, then clear what the repeats re-armed
                // through the print path. That is xterm's lifecycle — the byte
                // completing `CSI b` returns the parser to the ground state with nothing
                // printed, so a second `CSI b` repeats nothing. ghostty is the outlier
                // and re-arms (`printRepeat` calls `print`); pinned by
                // `rep_does_not_rearm_itself` (#825).
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

    /// OSC dispatch (the event surface): title (0/2), cwd (7). OSC 8 hyperlink
    /// is per-cell state, handled in its own slice, not here.
    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        // Not a print: the repeat is disarmed (#825, [`Term::repeat_anchor`]).
        self.repeat_anchor = None;
        // Ended by `CAN` / `SUB`, which cancel the sequence rather than end it
        // (#970, [`Term::cancel_byte_in_flight`]).
        if self.cancel_byte_in_flight {
            return;
        }
        // Which byte ended the sequence decides which byte ends its reply, and
        // this is the only place it is observable — vte hands it over per
        // dispatch and keeps nothing (#836). It rides outward on the query
        // events rather than being remembered: `drain_events` is a batch, so
        // two queries can be outstanding at once and one stored scalar could
        // not say which exchange it belonged to.
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
            // OSC 0 = icon + window title, OSC 2 = window title. Both set title.
            // Since #823 the string is also *retained*, because a title pop has
            // nothing to restore otherwise. OSC 0 writes both axes and OSC 2
            // only the window one — the distinction was invisible while the
            // engine merely forwarded, and becomes observable the moment an
            // axis-limited push/pop pair (which `vim` emits) is answered.
            //
            // The payload is `params[1..]` **rejoined**, not `params[1]`: `vte` splits on
            // every `;` and a title may legally contain one, so reading a single field cut
            // `make -j8; ./run` down to `make -j8` and announced the short string as the
            // real one (#880). Same read-site rule #650 established for `OSC 8`. The guard
            // is the *slice* being non-empty, not the string: a fieldless `OSC 2` must stay
            // ignored where `OSC 2 ;` clears the title, and `params.get(1..)` answers
            // `Some(&[])` for the first, whose join is indistinguishable from the second.
            //
            // The rejoin recovers only what `vte` hands over, which is at most 16 fields: a
            // title with 15 or more `;` still arrives cut, and cannot be told from a complete
            // one here (#840, closed not planned; see the VT interpretation map note).
            b"0" | b"2" => {
                if let Some(fields) = params.get(1..).filter(|f| !f.is_empty()) {
                    let title = String::from_utf8_lossy(&fields.join(&b';')).into_owned();
                    if number == b"0" {
                        self.icon_name.clone_from(&title);
                    }
                    self.set_window_title(title);
                }
            }
            // OSC 7 = current working directory (a file:// URI). Rejoined for the reason
            // on the title arm above — `;` is a legal byte in a path and in a URI, and the
            // engine hands the value over as declared (#880, ADR-0017), up to the same
            // 16-field bound as the title.
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
                // One allocation per *open*, shared by that open's cells and dropped
                // with the last row holding it (#628 — there is no pool). Two opens of
                // an identical URI stay two links, deliberately: merging them would
                // override a distinction the application controls through `id=`. That
                // parameter is the *only* dedup performed, which is one rule and not
                // two — xterm.js states it as "links with no id will only ever be
                // registered a single time" beside a lookup keyed on id-plus-uri
                // (`OscLinkService.ts:34`, `:49-54`).
                // The URI is `params[2..]` **rejoined**, not `params[2]` (#650). vte splits the
                // OSC payload on `;`, so a URI carrying an unencoded `;` arrives in pieces and
                // reading only the first dropped the rest — silently, with no error. Measured,
                // `]8;;https://x/a;b=c` arrives as `["8", "", "https://x/a", "b=c"]`. xterm.js
                // special-cases the same thing from the other side, splitting on the *first* `;`
                // only and taking all the rest as the URI, *"to support unencoded semi-colons in
                // the URIs"* (`InputHandler.ts:3106-3112`). `?a=1;b=2` is a legal query string.
                //
                // Nothing is lost at the parser **up to 16 fields**, and past that the tail is
                // gone before this arm runs: `vte` records at most 16 field boundaries, so a URI
                // with 14 or more `;` resolves to a shorter one that is indistinguishable from a
                // complete link (#840, closed not planned; see the VT interpretation map note).
                //
                // The close survives this: `]8;;` arrives as `["8", "", ""]`, whose rejoin is
                // empty, and an empty URI still closes. Never decoded — a `%3B` stays `%3B`,
                // because the engine hands the target over exactly as declared (ADR-0017).
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
            // OSC 4 = set/query an ANSI palette entry: `OSC 4 ; index ; spec`
            // (#122). The engine forwards index + raw spec; the consumer applies
            // it to its palette (theme-agnostic — the cell keeps `Indexed`).
            //
            // **An empty spec relays nothing, and drops only its own pair** (#834).
            // The engine cannot tell a *malformed* colour from a good one — it
            // never parses one, which is its identity and not a gap — but "this
            // field is empty" needs no parser, and forwarding `""` hands the
            // consumer a value it must invent a policy for.
            //
            // Dropping *the rest of the sequence* was the alternative, and it is
            // what xterm does: `ChangeOneAnsiColor` returns negative and that hits
            // `/* stop on any error */ break` (`misc.c:3013-3016`, pair loop at
            // `:2993`; chain `AllocateAnsiColor` → `xtermAllocColor` → `-1` at
            // `:2918`).
            //
            // **This is a deliberate divergence from the ADR-0004 tie-breaker, not
            // a case the tie-breaker fails to reach.** An earlier draft of this
            // comment claimed the latter and was wrong, which is worth stating
            // because the wrong version is the intuitive one: xterm makes the
            // *same* observation this engine makes — `strlen(spec) == 0`, no
            // parser, `misc.c:3105-3107` — and `XParseColor` at `:3111` is in the
            // `else if`, so it is never reached for an empty spec. The trigger IS
            // available here and "follow xterm" IS well defined. It is declined
            // because reading a blank field as evidence that structurally
            // well-formed pairs are corrupt is an inference about *application
            // intent*, which ADR-0017 puts on the consumer's side, and because it
            // would discard a value the application explicitly sent. That call is
            // the maintainer's, recorded on #834 with its grounds, and theirs to
            // reverse.
            //
            // **The references are 2–2 on this input, not 1–1**, and the two that
            // answer as this engine does are the two that keep walking: xterm.js
            // (`InputHandler.ts:3073`, loop `:3064`) and alacritty via `vte`
            // (`vte-0.15.0/src/ansi.rs:1372-1389` — a failed `xparse_color` falls
            // to `unhandled` with no `break`). ghostty relays nothing here, by a
            // *third* mechanism rather than by agreeing: `tokenizeScalar` drops
            // the empty token (`color.zig:130`) so the pairing re-aligns, and then
            // `RGB.parse("2")` fails into `catch return result` (`:210`), yielding
            // the accumulated — empty — list. Its re-alignment only becomes
            // *visible* where the re-aligned pair parses: `OSC 4 ; 1 ; ; #fff`
            // sets index 1 there and nothing anywhere else.
            //
            // The guard tests **emptiness only**, deliberately (#834 Out of
            // Scope). A space-only spec is relayed verbatim; TAB and NUL are C0
            // bytes `vte` drops inside an OSC string, so those fields arrive
            // genuinely empty and *are* dropped. Both are pinned by tests, so a
            // later "align with the reference" widening to whitespace reddens.
            b"4" => {
                // One event per `index ; spec` pair (xterm's `while slots > 1`).
                // The walk advances two fields whether or not a pair produces an
                // event, so dropping one cannot misalign the pairs after it.
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
            // OSC 104 = reset palette entries (#122): an **empty payload** resets
            // the whole table, else one event per named index.
            //
            // Empty means both `OSC 104` and `OSC 104 ;` (#832). vte hands those
            // over as `["104"]` and `["104", ""]`, and testing only the first left
            // the second falling into the index loop, where `"".parse::<u8>()`
            // fails and the reset evaporated silently. xterm tests the payload
            // string rather than the field count — `if (*buf != '\0')`
            // (`misc.c:3057`), whose else-branch is *"resetting all colors"*
            // (`misc.c:3077`) — and xterm.js gates on the same emptiness
            // (`InputHandler.ts:3223-3224`, a slot-less RESTORE).
            //
            // The test is `params[1]` being the *whole* payload, not "every field
            // is empty": for `OSC 104 ; ;` xterm's buf is `";"`, which is not
            // empty, so that form takes the index path. (ghostty differs here —
            // `tokenizeScalar` drops both separators and it resets everything —
            // but it agrees on the form that matters, `misc.c` and xterm.js do
            // not, and ADR-0004 puts the spec proxy on top.)
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
            // OSC 110 / 111 / 112 = reset the default foreground / background /
            // cursor colour (#122, #832). One slot each, never a stack — xterm's
            // reset path resolves a single index from the code itself and walks
            // nothing (`misc.c:3729`).
            b"110" => self.events.push(TermEvent::ResetForeground),
            b"111" => self.events.push(TermEvent::ResetBackground),
            b"112" => self.events.push(TermEvent::ResetCursorColor),
            _ => {} // other OSCs are later slices
        }
    }
}
