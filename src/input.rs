//! Input encoding (#11): consumer events → the bytes an application expects.
//!
//! The inverse of `feed` — a key/mouse/paste/focus event becomes the byte
//! sequence a TUI app reads on its stdin, decided by the DEC modes the engine
//! tracks from the *output* stream (DECCKM, mouse tracking/encoding, focus,
//! bracketed paste). The engine owns the modes; these functions are pure
//! (event + modes → bytes), so the consumer's I/O stays its own concern.
//!
//! This is the **legacy xterm** baseline (the common-90% every TUI speaks). The
//! kitty keyboard protocol (`CSI u` + a negotiated progressive-flag stack) is a
//! stateful superset deferred to #23.

use bitflags::bitflags;

bitflags! {
    /// Modifier keys held during an event. The bit values follow the **kitty**
    /// scheme (the superset): Shift=1, Alt=2, Ctrl=4, Super=8, Hyper=16, Meta=32,
    /// CapsLock=64, NumLock=128. Legacy xterm can only express the first three
    /// plus Meta-at-8, so `csi_param` remaps; kitty uses the bits directly (#23).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Modifiers: u8 {
        const SHIFT     = 1;
        const ALT       = 2;
        const CTRL      = 4;
        const SUPER     = 8;
        const HYPER     = 16;
        const META      = 32;
        const CAPS_LOCK = 64;
        const NUM_LOCK  = 128;
    }
}

impl Modifiers {
    /// The legacy xterm CSI modifier parameter (`1 + bitmask`, Shift=1/Alt=2/
    /// Ctrl=4/Meta=8), or `None` when none of the legacy-expressible modifiers is
    /// held. Super/Hyper/CapsLock/NumLock have no legacy form and are dropped.
    fn csi_param(self) -> Option<u8> {
        let mut bits = 0u8;
        if self.contains(Modifiers::SHIFT) {
            bits |= 1;
        }
        if self.contains(Modifiers::ALT) {
            bits |= 2;
        }
        if self.contains(Modifiers::CTRL) {
            bits |= 4;
        }
        if self.contains(Modifiers::META) {
            bits |= 8;
        }
        if bits == 0 { None } else { Some(1 + bits) }
    }

    /// The kitty CSI modifier parameter (`1 + bits`) — the bit values already
    /// match the kitty scheme, so all eight modifiers are expressible (#23).
    fn kitty_param(self) -> Option<u8> {
        if self.is_empty() {
            None
        } else {
            Some(1 + self.bits())
        }
    }
}

/// A logical key press from the consumer (already decoded from the platform's
/// keyboard event — justerm does not read hardware).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A printable character (the consumer's already-composed text).
    Char(char),
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Enter,
    Tab,
    Backspace,
    Escape,
    /// Function key `F(n)`, `n` in 1..=12.
    F(u8),
}

/// Press / repeat / release. Legacy reports only presses; the kitty protocol's
/// "report event types" flag (bit 1) carries repeat and release too (#23).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyAction {
    #[default]
    Press,
    Repeat,
    Release,
}

/// A key event: a key, the modifiers held with it, its press/repeat/release type
/// (defaults to `Press`), and consumer-supplied extras the kitty protocol's
/// alternate-keys / associated-text flags report (all `None` for legacy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub mods: Modifiers,
    pub action: KeyAction,
    /// Kitty alternate-keys (bit 2): the codepoint Shift would produce, if it
    /// differs from `key`.
    pub shifted_key: Option<char>,
    /// Kitty alternate-keys (bit 2): the codepoint at this key's position on the
    /// base (standard) layout, if it differs from `key`.
    pub base_key: Option<char>,
    /// Kitty associated-text (bit 4): the text the key actually produced
    /// (composed input / dead keys).
    pub text: Option<char>,
}

impl Default for KeyEvent {
    fn default() -> Self {
        KeyEvent {
            key: Key::Char('\0'),
            mods: Modifiers::empty(),
            action: KeyAction::Press,
            shifted_key: None,
            base_key: None,
            text: None,
        }
    }
}

/// Which mouse button an event concerns. `None` on a [`MouseEvent`] means bare
/// motion with no button held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    /// Horizontal scroll / tilt-wheel — xterm buttons 6 and 7, encoded in the
    /// same 64-base wheel group as up/down.
    WheelLeft,
    WheelRight,
}

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

/// A mouse event in viewport cell coordinates (0-based — the encoding shifts to
/// 1-based on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    /// The button, or `None` for bare motion (no button held).
    pub button: Option<MouseButton>,
    pub action: MouseAction,
    pub col: usize,
    pub row: usize,
    /// 0-based pixel coordinates, used only by the `?1016` SGR-pixels encoding —
    /// the consumer (which has the window geometry) supplies them; the engine
    /// only formats them. Ignored by the cell-based encodings.
    pub px: usize,
    pub py: usize,
    pub mods: Modifiers,
}

/// Mouse tracking mode — *what* the app asked to be reported (DEC `?1000` /
/// `?1002` / `?1003`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseProtocol {
    /// No reporting (default). `encode_mouse` returns `None`.
    #[default]
    Off,
    /// `?1000` — button press and release only.
    Normal,
    /// `?1002` — also motion while a button is held (drag).
    ButtonEvent,
    /// `?1003` — also motion with no button held.
    AnyEvent,
}

/// Mouse coordinate encoding — *how* a report is framed (default X10 vs DEC
/// `?1006` SGR).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseEncoding {
    /// X10 `CSI M Cb Cx Cy`, each value offset by 32 — breaks past column 223.
    #[default]
    Default,
    /// `?1006` SGR `CSI < Cb ; Cx ; Cy M|m` — coords unbounded, release distinct.
    Sgr,
    /// `?1015` urxvt `CSI Cb ; Cx ; Cy M` — the Default byte semantics (Cb with
    /// the +32 base, release loses button identity) as decimal params, always
    /// terminated by `M`. Unbounded coords, no separate release form.
    Urxvt,
    /// `?1005` UTF-8 — the Default `CSI M Cb Cx Cy` framing but each value
    /// UTF-8-encoded, so values past 127 become multi-byte (extends the range).
    Utf8,
    /// `?1016` SGR-pixels — the SGR framing but the coordinates are pixels
    /// (from `MouseEvent::px`/`py`) instead of cells.
    SgrPixels,
}

const ESC: u8 = 0x1b;

/// Bit 0 of the kitty progressive-enhancement flags: disambiguate escape codes.
const KITTY_DISAMBIGUATE: u8 = 0b1;

/// Encode a key event to bytes, given whether DECCKM (application cursor keys)
/// is active and the kitty keyboard-protocol flags. Returns `None` only for keys
/// with no defined encoding.
pub fn encode_key(ev: &KeyEvent, app_cursor: bool, kitty_flags: u8) -> Option<Vec<u8>> {
    // Under the kitty protocol, events legacy cannot express (a modifier on a
    // text key, a release/repeat) take the `CSI unicode ; mods : event u` form;
    // everything else falls through to legacy (#23).
    if kitty_flags != 0
        && let Some(bytes) = kitty_encode(ev, kitty_flags)
    {
        return Some(bytes);
    }
    match ev.key {
        Key::Char(c) => Some(encode_char(c, ev.mods)),
        Key::Up => Some(cursor_key(b'A', ev.mods, app_cursor)),
        Key::Down => Some(cursor_key(b'B', ev.mods, app_cursor)),
        Key::Right => Some(cursor_key(b'C', ev.mods, app_cursor)),
        Key::Left => Some(cursor_key(b'D', ev.mods, app_cursor)),
        Key::Home => Some(cursor_key(b'H', ev.mods, app_cursor)),
        Key::End => Some(cursor_key(b'F', ev.mods, app_cursor)),
        Key::Insert => Some(tilde_key(2, ev.mods)),
        Key::Delete => Some(tilde_key(3, ev.mods)),
        Key::PageUp => Some(tilde_key(5, ev.mods)),
        Key::PageDown => Some(tilde_key(6, ev.mods)),
        Key::Enter => Some(vec![b'\r']),
        Key::Backspace => Some(vec![0x7f]), // DEL, the PC-keyboard convention
        Key::Escape => Some(vec![ESC]),
        Key::Tab => {
            if ev.mods.contains(Modifiers::SHIFT) {
                Some(vec![ESC, b'[', b'Z']) // back-tab (CBT)
            } else {
                Some(vec![b'\t'])
            }
        }
        Key::F(n) => function_key(n, ev.mods),
    }
}

/// Bit 1 of the kitty flags: report event types (repeat / release).
const KITTY_REPORT_EVENTS: u8 = 0b10;
/// Bit 2 of the kitty flags: report alternate (shifted / base-layout) keys.
const KITTY_ALTERNATE_KEYS: u8 = 0b100;
/// Bit 3 of the kitty flags: report all keys (incl. printable) as escape codes.
const KITTY_ALL_AS_ESCAPE: u8 = 0b1000;
/// Bit 4 of the kitty flags: report the text a key produced.
const KITTY_ASSOCIATED_TEXT: u8 = 0b10000;

/// Kitty `CSI unicode ; mods : event u` encoding. Returns `None` to fall through
/// to legacy when this event needs no kitty form (a plain press of an
/// unmodified key under disambiguate, etc.). The functional-key codepoint table
/// and the remaining flags grow this in later slices.
fn kitty_encode(ev: &KeyEvent, flags: u8) -> Option<Vec<u8>> {
    // Event sub-parameter — only reported when the report-events flag is on, and
    // a plain press is the omitted default.
    let event = if flags & KITTY_REPORT_EVENTS != 0 {
        match ev.action {
            KeyAction::Press => None,
            KeyAction::Repeat => Some(2),
            KeyAction::Release => Some(3),
        }
    } else {
        None
    };
    let modified = ev.mods.kitty_param();
    let disambiguate = flags & KITTY_DISAMBIGUATE != 0;

    // Functional keys (arrows / nav / F-keys) keep their legacy escape form but
    // gain the kitty `;mods:event` parameter when modified or evented; an
    // unmodified press stays legacy.
    if let Some((number, terminator)) = functional_key(ev.key) {
        if event.is_none() && modified.is_none() {
            return None; // legacy form
        }
        return Some(kitty_seq(number, modified, event, terminator));
    }

    // Codepoint keys. Escape is ambiguous (introduces sequences) → disambiguated
    // even unmodified. Enter/Tab/Backspace are the documented *exceptions*: legacy
    // unless modified or carrying a non-press event.
    let codepoint = match ev.key {
        Key::Escape => 27,
        Key::Enter => 13,
        Key::Tab => 9,
        Key::Backspace => 127,
        Key::Char(c) => c as u32,
        _ => return None,
    };
    // Escape disambiguates even unmodified. All-as-escape sends *every* key in
    // CSI u form — by here, functional keys are already handled, so the rest
    // (Esc/Enter/Tab/Backspace/Char) all qualify. Otherwise a modifier or a
    // non-press event is needed.
    let all_as_escape = flags & KITTY_ALL_AS_ESCAPE != 0;
    let always = (disambiguate && ev.key == Key::Escape) || all_as_escape;
    if !always && event.is_none() && !(disambiguate && modified.is_some()) {
        return None;
    }
    Some(kitty_csi_u(ev, codepoint, modified, event, flags))
}

/// The `CSI u` codepoint form, including the alternate-keys and associated-text
/// sub-fields when their flags are active:
/// `CSI codepoint[:shifted[:base]] [; mods[:event] [; text]] u`.
fn kitty_csi_u(
    ev: &KeyEvent,
    codepoint: u32,
    modified: Option<u8>,
    event: Option<u8>,
    flags: u8,
) -> Vec<u8> {
    let mut s = format!("\x1b[{codepoint}");

    // Alternate keys (bit 2): codepoint : shifted : base.
    if flags & KITTY_ALTERNATE_KEYS != 0 && (ev.shifted_key.is_some() || ev.base_key.is_some()) {
        s.push(':');
        if let Some(sh) = ev.shifted_key {
            s.push_str(&(sh as u32).to_string());
        }
        if let Some(b) = ev.base_key {
            s.push(':');
            s.push_str(&(b as u32).to_string());
        }
    }

    // The text sub-parameter (bit 4) forces the modifier field to be present.
    let text = if flags & KITTY_ASSOCIATED_TEXT != 0 {
        ev.text
    } else {
        None
    };
    if modified.is_some() || event.is_some() || text.is_some() {
        s.push(';');
        s.push_str(&modified.unwrap_or(1).to_string());
        if let Some(e) = event {
            s.push(':');
            s.push_str(&e.to_string());
        }
    }
    if let Some(txt) = text {
        s.push(';');
        s.push_str(&(txt as u32).to_string());
    }

    s.push('u');
    s.into_bytes()
}

/// A functional key's legacy CSI form: `(leading number, terminator)` — e.g. Up
/// is `(1, b'A')` → `CSI 1 A`, Delete is `(3, b'~')` → `CSI 3 ~`. `None` for keys
/// that take the `CSI u` codepoint form instead.
fn functional_key(key: Key) -> Option<(u32, u8)> {
    Some(match key {
        Key::Up => (1, b'A'),
        Key::Down => (1, b'B'),
        Key::Right => (1, b'C'),
        Key::Left => (1, b'D'),
        Key::Home => (1, b'H'),
        Key::End => (1, b'F'),
        Key::Insert => (2, b'~'),
        Key::Delete => (3, b'~'),
        Key::PageUp => (5, b'~'),
        Key::PageDown => (6, b'~'),
        Key::F(1) => (1, b'P'),
        Key::F(2) => (1, b'Q'),
        Key::F(3) => (1, b'R'),
        Key::F(4) => (1, b'S'),
        Key::F(5) => (15, b'~'),
        Key::F(6) => (17, b'~'),
        Key::F(7) => (18, b'~'),
        Key::F(8) => (19, b'~'),
        Key::F(9) => (20, b'~'),
        Key::F(10) => (21, b'~'),
        Key::F(11) => (23, b'~'),
        Key::F(12) => (24, b'~'),
        _ => return None,
    })
}

/// Build `CSI <number> [; <param> [: <event>]] <terminator>` — the shared shape
/// of both the `CSI u` codepoint form and the functional-key legacy form. The
/// `;param` is emitted when modified or evented (param defaults to 1).
fn kitty_seq(number: u32, modified: Option<u8>, event: Option<u8>, terminator: u8) -> Vec<u8> {
    let mut s = format!("\x1b[{number}");
    if modified.is_some() || event.is_some() {
        s.push(';');
        s.push_str(&modified.unwrap_or(1).to_string());
        if let Some(e) = event {
            s.push(':');
            s.push_str(&e.to_string());
        }
    }
    let mut v = s.into_bytes();
    v.push(terminator);
    v
}

/// A printable character with modifiers. Ctrl folds an ASCII letter to its
/// control code; Alt (meta-sends-escape) prefixes ESC.
fn encode_char(c: char, mods: Modifiers) -> Vec<u8> {
    let mut out = Vec::new();
    if mods.contains(Modifiers::ALT) {
        out.push(ESC);
    }
    if mods.contains(Modifiers::CTRL) {
        // Ctrl+letter → 0x01..=0x1a; Ctrl+@/[/\/]/^/_ → 0x00..0x1f.
        let code = match c {
            'a'..='z' => Some((c as u8 - b'a') + 1),
            'A'..='Z' => Some((c as u8 - b'A') + 1),
            '@' => Some(0),
            '[' => Some(0x1b),
            '\\' => Some(0x1c),
            ']' => Some(0x1d),
            '^' => Some(0x1e),
            '_' => Some(0x1f),
            ' ' => Some(0),
            _ => None,
        };
        if let Some(b) = code {
            out.push(b);
            return out;
        }
    }
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    out
}

/// Cursor keys and Home/End. Unmodified: SS3 under DECCKM, else CSI. Modified:
/// always the CSI `1;<mod>` form regardless of DECCKM (xterm rule).
fn cursor_key(final_byte: u8, mods: Modifiers, app_cursor: bool) -> Vec<u8> {
    match mods.csi_param() {
        Some(param) => {
            let mut v = vec![ESC, b'['];
            v.extend_from_slice(b"1;");
            v.extend_from_slice(param.to_string().as_bytes());
            v.push(final_byte);
            v
        }
        None if app_cursor => vec![ESC, b'O', final_byte],
        None => vec![ESC, b'[', final_byte],
    }
}

/// Keys encoded as `CSI <n> ~` (Insert/Delete/PageUp/PageDown and F5+), with an
/// optional `;<mod>` parameter.
fn tilde_key(n: u8, mods: Modifiers) -> Vec<u8> {
    let mut v = vec![ESC, b'['];
    v.extend_from_slice(n.to_string().as_bytes());
    if let Some(param) = mods.csi_param() {
        v.push(b';');
        v.extend_from_slice(param.to_string().as_bytes());
    }
    v.push(b'~');
    v
}

/// Function keys. F1–F4 are SS3 `P/Q/R/S` (CSI `1;<mod>` form when modified);
/// F5–F12 are tilde keys `15/17/18/19/20/21/23/24 ~`.
fn function_key(n: u8, mods: Modifiers) -> Option<Vec<u8>> {
    match n {
        1..=4 => {
            let letter = b'P' + (n - 1); // P, Q, R, S
            match mods.csi_param() {
                Some(param) => {
                    let mut v = vec![ESC, b'[', b'1', b';'];
                    v.extend_from_slice(param.to_string().as_bytes());
                    v.push(letter);
                    Some(v)
                }
                None => Some(vec![ESC, b'O', letter]),
            }
        }
        5 => Some(tilde_key(15, mods)),
        6 => Some(tilde_key(17, mods)),
        7 => Some(tilde_key(18, mods)),
        8 => Some(tilde_key(19, mods)),
        9 => Some(tilde_key(20, mods)),
        10 => Some(tilde_key(21, mods)),
        11 => Some(tilde_key(23, mods)),
        12 => Some(tilde_key(24, mods)),
        _ => None,
    }
}

/// Encode a mouse event, given the active tracking mode and encoding. Returns
/// `None` when reporting is off or the event is filtered out by the mode (e.g.
/// a bare move under `?1000`).
pub fn encode_mouse(ev: &MouseEvent, proto: MouseProtocol, enc: MouseEncoding) -> Option<Vec<u8>> {
    if proto == MouseProtocol::Off {
        return None;
    }
    // A wheel turn is a single press-like event; a release on a wheel button is
    // not a real report (it would leak a bogus SGR `m` / an identity-less X10
    // release), so drop it.
    if ev.action == MouseAction::Release
        && matches!(
            ev.button,
            Some(
                MouseButton::WheelUp
                    | MouseButton::WheelDown
                    | MouseButton::WheelLeft
                    | MouseButton::WheelRight
            )
        )
    {
        return None;
    }
    // Mode gates which events report at all.
    match ev.action {
        MouseAction::Press | MouseAction::Release => {}
        MouseAction::Motion => match (proto, ev.button) {
            // Drag (button held) needs ButtonEvent or AnyEvent.
            (MouseProtocol::ButtonEvent | MouseProtocol::AnyEvent, Some(_)) => {}
            // Bare motion needs AnyEvent.
            (MouseProtocol::AnyEvent, None) => {}
            _ => return None,
        },
    }

    // Low button bits + wheel base.
    let button_bits = match ev.button {
        Some(MouseButton::Left) => 0,
        Some(MouseButton::Middle) => 1,
        Some(MouseButton::Right) => 2,
        Some(MouseButton::WheelUp) => 64,
        Some(MouseButton::WheelDown) => 65,
        Some(MouseButton::WheelLeft) => 66,
        Some(MouseButton::WheelRight) => 67,
        None => 3, // motion with no button: the "no button" code
    };
    let motion = if ev.action == MouseAction::Motion {
        32
    } else {
        0
    };
    let mod_bits = (if ev.mods.contains(Modifiers::SHIFT) {
        4
    } else {
        0
    }) + (if ev.mods.contains(Modifiers::ALT) {
        8
    } else {
        0
    }) + (if ev.mods.contains(Modifiers::CTRL) {
        16
    } else {
        0
    });

    let col1 = ev.col + 1;
    let row1 = ev.row + 1;

    match enc {
        MouseEncoding::Sgr | MouseEncoding::SgrPixels => {
            // SGR framing; `?1016` swaps cell coords for the consumer's pixels.
            // SGR keeps the button identity on release; the terminator says which.
            let cb = button_bits + motion + mod_bits;
            let (x, y) = if enc == MouseEncoding::SgrPixels {
                (ev.px + 1, ev.py + 1)
            } else {
                (col1, row1)
            };
            let final_byte = if ev.action == MouseAction::Release {
                b'm'
            } else {
                b'M'
            };
            let mut v = vec![ESC, b'[', b'<'];
            v.extend_from_slice(cb.to_string().as_bytes());
            v.push(b';');
            v.extend_from_slice(x.to_string().as_bytes());
            v.push(b';');
            v.extend_from_slice(y.to_string().as_bytes());
            v.push(final_byte);
            Some(v)
        }
        MouseEncoding::Default => {
            // X10: release loses button identity (button bits = 3); all values +32.
            let base = if ev.action == MouseAction::Release {
                3
            } else {
                button_bits
            };
            let cb = base + motion + mod_bits + 32;
            let cx = (col1 + 32).min(255) as u8;
            let cy = (row1 + 32).min(255) as u8;
            Some(vec![ESC, b'[', b'M', cb as u8, cx, cy])
        }
        MouseEncoding::Urxvt => {
            // Default's Cb semantics (release → button 3, +32 base) but as decimal
            // params and always terminated by `M`.
            let base = if ev.action == MouseAction::Release {
                3
            } else {
                button_bits
            };
            let cb = base + motion + mod_bits + 32;
            let mut v = vec![ESC, b'['];
            v.extend_from_slice(cb.to_string().as_bytes());
            v.push(b';');
            v.extend_from_slice(col1.to_string().as_bytes());
            v.push(b';');
            v.extend_from_slice(row1.to_string().as_bytes());
            v.push(b'M');
            Some(v)
        }
        MouseEncoding::Utf8 => {
            // Default's CSI M framing, but each value UTF-8-encoded so it can
            // exceed one byte (the 223-column fix that predates SGR).
            let base = if ev.action == MouseAction::Release {
                3
            } else {
                button_bits
            };
            let mut v = vec![ESC, b'[', b'M'];
            push_utf8(&mut v, base + motion + mod_bits + 32);
            push_utf8(&mut v, col1 + 32);
            push_utf8(&mut v, row1 + 32);
            Some(v)
        }
    }
}

/// Append `val` UTF-8-encoded (a single code point) — the ?1005 coordinate
/// packing. Out-of-range values fall back to the replacement character.
fn push_utf8(out: &mut Vec<u8>, val: usize) {
    let c = char::from_u32(val as u32).unwrap_or('\u{fffd}');
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// Wrap pasted text in bracketed-paste markers when the mode is on, else return
/// it raw. The markers let the app treat the payload as literal text, never as
/// typed control sequences.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if !bracketed {
        return text.as_bytes().to_vec();
    }
    let mut v = Vec::with_capacity(text.len() + 12);
    v.extend_from_slice(b"\x1b[200~");
    v.extend_from_slice(text.as_bytes());
    v.extend_from_slice(b"\x1b[201~");
    v
}

/// Focus in/out report (`CSI I` / `CSI O`), or `None` when focus reporting
/// (`?1004`) is off.
pub fn encode_focus(focused: bool, enabled: bool) -> Option<Vec<u8>> {
    if !enabled {
        return None;
    }
    Some(vec![ESC, b'[', if focused { b'I' } else { b'O' }])
}
