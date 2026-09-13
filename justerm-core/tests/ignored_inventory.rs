//! Which kinds of sequence each capture's **end state** does not depend on (#895).
//!
//! Every other capture test pins what a real stream does. None pins what it does nothing with,
//! so a handler whose effect reaches no existing golden — an input-side mode, a reply, an event —
//! can stop firing with every test green. This replays each capture once whole and once per
//! kind with every instance of that kind removed, and pins, per kind, **which observable
//! surfaces moved**. A row losing a surface is a handler that stopped firing; a row gaining one
//! is something implemented, and the golden moves in the same change.
//!
//! ## What a verdict means, and what it does not
//!
//! `-` means *the state at the end of this stream is identical without that kind*. It does not
//! mean the engine ignores it. State that is set and never exercised later reads `-` while fully
//! handled: a DECSC with no DECRC, an HTS with no HT, a designated charset nothing prints through,
//! an SGR a later full clear paints over. Tab stops, the saved cursor, the charsets and the scroll
//! region have no getter at all and are only visible through a later effect. Comparing the
//! trajectory instead of the end state would see them; that was weighed against its cost and
//! left out of #895's scope.
//!
//! ## What the surfaces can and cannot observe
//!
//! The surface list is hand-kept, so a mode added to the engine and left out of [`surfaces`]
//! reads `-` for ever. That is what
//! [`every_surface_sees_an_effect_somewhere_in_the_corpus`] exists for: a surface that no kind in
//! the whole corpus moves is a broken instrument, not a quiet engine.
//!
//! - **An alt-screen capture is cut at its first `CSI ? 1049 l`**, as `vttest.rs`'s capture tests
//!   do. An application tears the alt buffer down before it exits, so the state at EOF holds none
//!   of what it drew and every grid-affecting kind would read `-`, the controls included — the
//!   first thing the `probe/47-dogfood-inventory` probe got wrong.
//! - **Input-side state is read through the encoders, with modified keys.** Cursor-key, keypad,
//!   modifyOtherKeys and the kitty flags never reach the screen, and modifyOtherKeys is invisible
//!   to an unmodified key — the probe's second mistake.
//! - **Kinds are bracketed by this file's own scanner**, not by `vte`. A mis-bracketed token
//!   corrupts its neighbours when removed and manufactures a false effect, so
//!   [`the_scanner_brackets_every_capture_exactly`] holds every ESC to exactly one token, and no
//!   token to an ESC past its own opening and closing.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use justerm_core::{
    Engine, Key, KeyEvent, KeypadKey, Modifiers, MouseAction, MouseButton, MouseEvent,
};

/// One replay scenario. `parts` are fed in order with a resize to `resize` between them, which is
/// how the `alt_resize_*` pairs are consumed (`alt_no_reflow.rs`): `post` alone is an application
/// answering a resize that never happened.
struct Capture {
    name: &'static str,
    cols: usize,
    rows: usize,
    scrollback: Option<usize>,
    parts: &'static [&'static [u8]],
    resize: Option<(usize, usize)>,
    golden: &'static str,
}

macro_rules! capture {
    ($name:literal, $cols:expr, $rows:expr, $sb:expr, [$($part:literal),+], $resize:expr) => {
        Capture {
            name: $name,
            cols: $cols,
            rows: $rows,
            scrollback: $sb,
            parts: &[$(include_bytes!(concat!("fixtures/", $part, ".raw"))),+],
            resize: $resize,
            golden: include_str!(concat!("fixtures/", $name, ".ignored.golden")),
        }
    };
}

/// Geometry follows each capture's consuming test, because the end state is a function of it.
/// Kept as a list rather than a glob so a capture added later is a deliberate entry here.
const CAPTURES: &[Capture] = &[
    capture!(
        "alt_resize_htop",
        80,
        24,
        None,
        ["alt_resize_htop.pre", "alt_resize_htop.post"],
        Some((40, 24))
    ),
    capture!(
        "alt_resize_vim",
        80,
        24,
        None,
        ["alt_resize_vim.pre", "alt_resize_vim.post"],
        Some((40, 24))
    ),
    capture!(
        "cursor_color_nvim",
        80,
        24,
        None,
        ["cursor_color_nvim"],
        None
    ),
    capture!("htop", 80, 24, None, ["htop"], None),
    capture!(
        "hyperlink_combining",
        80,
        24,
        None,
        ["hyperlink_combining"],
        None
    ),
    capture!("less_softwrap", 80, 24, None, ["less_softwrap"], None),
    capture!("ls_hyperlink", 80, 24, None, ["ls_hyperlink"], None),
    capture!("neovim_kitty", 80, 24, None, ["neovim_kitty"], None),
    capture!("osc133_clear", 40, 10, Some(200), ["osc133_clear"], None),
    capture!("softwrap_shifts", 80, 24, None, ["softwrap_shifts"], None),
    capture!("softwrap_wide", 80, 24, None, ["softwrap_wide"], None),
    capture!("tmux_clipboard", 80, 24, None, ["tmux_clipboard"], None),
    capture!("top", 80, 24, None, ["top"], None),
    capture!("undercurl_matrix", 80, 10, None, ["undercurl_matrix"], None),
    capture!("vim_closed_loop", 80, 24, None, ["vim_closed_loop"], None),
    capture!("vim_redraw", 80, 24, None, ["vim_redraw"], None),
    capture!("vim_title_stack", 80, 24, None, ["vim_title_stack"], None),
    capture!("written_space", 80, 24, None, ["written_space"], None),
];

/// Every surface a consumer can read, by name. Order is the golden's column order.
const SURFACES: &[&str] = &[
    "frame", "text", "attrs", "events", "replies", "keys", "paste", "mouse", "focus", "modes",
    "marks",
];

// ---------------------------------------------------------------------------------------------
// Scanner

#[derive(Debug)]
struct Tok {
    start: usize,
    end: usize,
    kind: String,
}

/// Brackets whole sequences so removing one leaves valid bytes on both sides. A parameter stays
/// in the kind where it names a function rather than a quantity: all of a mode set/reset (`?1h`
/// and `?1049h` share nothing), and the first of XTWINOPS `t`, DSR `n`, DECRQM `$p` and any `>`
/// sequence (`22t` pushes a title, `23t` pops one). Every other CSI drops its digits, so
/// `CSI 1;2H` and `CSI H` are one kind.
fn scan(b: &[u8]) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        match b[i] {
            0x1b if i + 1 < b.len() => {
                i += 1;
                match b[i] {
                    b'[' => {
                        i += 1;
                        let ps = i;
                        while i < b.len() && (0x20..0x40).contains(&b[i]) {
                            i += 1;
                        }
                        if i >= b.len() {
                            break;
                        }
                        let fin = b[i];
                        i += 1;
                        let params = &b[ps..i - 1];
                        let symbols: String = params
                            .iter()
                            .filter(|c| !c.is_ascii_digit() && **c != b';' && **c != b':')
                            .map(|c| *c as char)
                            .collect();
                        let first: String = params
                            .iter()
                            .skip_while(|c| !c.is_ascii_digit())
                            .take_while(|c| c.is_ascii_digit())
                            .map(|c| *c as char)
                            .collect();
                        let shown = if fin == b'h' || fin == b'l' {
                            params.iter().map(|c| *c as char).collect()
                        } else if matches!(fin, b't' | b'n')
                            || params.first() == Some(&b'>')
                            || params.last() == Some(&b'$') && fin == b'p'
                        {
                            format!("{}{first}", symbols.trim_end_matches('$'))
                                + if symbols.ends_with('$') { "$" } else { "" }
                        } else {
                            symbols
                        };
                        out.push(Tok {
                            start,
                            end: i,
                            kind: format!("CSI {shown}{}", fin as char),
                        });
                    }
                    b']' => {
                        i += 1;
                        let ps = i;
                        while i < b.len()
                            && b[i] != 0x07
                            && !(b[i] == 0x1b && b.get(i + 1) == Some(&b'\\'))
                        {
                            i += 1;
                        }
                        let num: String = b[ps..i]
                            .iter()
                            .take_while(|c| c.is_ascii_digit())
                            .map(|c| *c as char)
                            .collect();
                        if i < b.len() {
                            i += if b[i] == 0x1b { 2 } else { 1 };
                        }
                        let num = if num.is_empty() { "?".to_string() } else { num };
                        out.push(Tok {
                            start,
                            end: i,
                            kind: format!("OSC {num}"),
                        });
                    }
                    intro @ (b'P' | b'X' | b'^' | b'_') => {
                        i += 1;
                        let ps = i;
                        while i < b.len() && !(b[i] == 0x1b && b.get(i + 1) == Some(&b'\\')) {
                            i += 1;
                        }
                        let head: String = b[ps..i]
                            .iter()
                            .filter(|c| !c.is_ascii_digit() && **c != b';')
                            .take(2)
                            .map(|c| *c as char)
                            .collect();
                        if i < b.len() {
                            i += 2;
                        }
                        let name = match intro {
                            b'P' => "DCS",
                            b'X' => "SOS",
                            b'^' => "PM",
                            _ => "APC",
                        };
                        out.push(Tok {
                            start,
                            end: i,
                            kind: format!("{name} {head}"),
                        });
                    }
                    _ => {
                        let ps = i;
                        while i < b.len() && (0x20..0x30).contains(&b[i]) {
                            i += 1;
                        }
                        if i >= b.len() {
                            break;
                        }
                        i += 1;
                        let seq: String = b[ps..i].iter().map(|c| *c as char).collect();
                        out.push(Tok {
                            start,
                            end: i,
                            kind: format!("ESC {seq}"),
                        });
                    }
                }
            }
            c if c < 0x20 || c == 0x7f => {
                i += 1;
                out.push(Tok {
                    start,
                    end: i,
                    kind: format!("C0 {c:#04x}"),
                });
            }
            _ => i += 1,
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Replay

/// The cut applied to a single-part capture: everything before the first alt-screen teardown.
fn cut(bytes: &[u8]) -> &[u8] {
    match bytes.windows(8).position(|w| w == b"\x1b[?1049l") {
        Some(p) => &bytes[..p],
        None => bytes,
    }
}

fn parts_of(c: &Capture) -> Vec<&'static [u8]> {
    c.parts.iter().map(|p| cut(p)).collect()
}

fn key(key: Key, mods: Modifiers) -> KeyEvent {
    KeyEvent {
        key,
        mods,
        ..Default::default()
    }
}

/// Every consumer-observable surface after replaying `parts`, one string per [`SURFACES`] entry.
fn surfaces(c: &Capture, parts: &[Vec<u8>]) -> Vec<String> {
    let mut e = match c.scrollback {
        Some(sb) => Engine::with_scrollback(c.cols, c.rows, sb),
        None => Engine::new(c.cols, c.rows),
    };
    for (i, p) in parts.iter().enumerate() {
        if i > 0
            && let Some((cols, rows)) = c.resize
        {
            e.resize(cols, rows);
        }
        e.feed(p);
    }
    let (cols, rows) = c
        .resize
        .filter(|_| parts.len() > 1)
        .unwrap_or((c.cols, c.rows));

    e.mark_fully_damaged();
    let frame = justerm_core::encode(&e.frame());

    let mut attrs = String::new();
    for r in 0..rows {
        for col in 0..cols {
            let u = e.underline_color_at(r, col);
            let l = e.link_at(r, col);
            if u != justerm_core::Color::Default || l.is_some() {
                let _ = write!(attrs, "{r},{col}:{u:?}:{l:?};");
            }
        }
    }

    let keys: Vec<String> = [
        (Key::Up, Modifiers::empty()),
        (Key::Home, Modifiers::empty()),
        (Key::F(1), Modifiers::empty()),
        (Key::Enter, Modifiers::empty()),
        (Key::Backspace, Modifiers::empty()),
        (Key::Char('a'), Modifiers::empty()),
        (Key::Left, Modifiers::SHIFT),
        (Key::Char('i'), Modifiers::CTRL),
        (Key::Char('['), Modifiers::CTRL),
        (Key::Char('a'), Modifiers::CTRL),
        (Key::Char('a'), Modifiers::ALT),
        (Key::Char('a'), Modifiers::CTRL | Modifiers::SHIFT),
        (Key::Keypad(KeypadKey::Enter), Modifiers::empty()),
        (Key::Keypad(KeypadKey::Digit(5)), Modifiers::empty()),
    ]
    .into_iter()
    .map(|(k, m)| format!("{:?}", e.encode_key(key(k, m))))
    .collect();

    let mouse: Vec<String> = [
        (
            Some(MouseButton::Left),
            MouseAction::Press,
            Modifiers::empty(),
        ),
        (
            Some(MouseButton::Left),
            MouseAction::Release,
            Modifiers::empty(),
        ),
        (None, MouseAction::Motion, Modifiers::empty()),
        (
            Some(MouseButton::Left),
            MouseAction::Motion,
            Modifiers::empty(),
        ),
        (
            Some(MouseButton::WheelUp),
            MouseAction::Press,
            Modifiers::CTRL,
        ),
    ]
    .into_iter()
    .map(|(button, action, mods)| {
        format!(
            "{:?}",
            e.encode_mouse(MouseEvent {
                button,
                action,
                col: 300,
                row: 2,
                px: 7,
                py: 9,
                mods
            })
        )
    })
    .collect();

    vec![
        format!("{frame:?}"),
        e.accessible_text(),
        attrs,
        format!("{:?}", e.drain_events()),
        format!("{:?}", e.drain_replies()),
        keys.join("|"),
        format!("{:?}", e.encode_paste("x")),
        mouse.join("|"),
        format!("{:?}|{:?}", e.encode_focus(true), e.encode_focus(false)),
        format!(
            "sync={} w32={} scheme={} sb={}",
            e.synchronized_output(),
            e.win32_input_mode(),
            e.color_scheme_updates(),
            e.scrollback_len(),
        ),
        format!("{:?}|{:?}", e.command_marks(), e.command_lines()),
    ]
}

/// The inventory of one capture, as the golden spells it.
fn inventory(c: &Capture) -> String {
    let parts = parts_of(c);
    let base = surfaces(c, &parts.iter().map(|p| p.to_vec()).collect::<Vec<_>>());
    let toks: Vec<Vec<Tok>> = parts.iter().map(|p| scan(p)).collect();

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for t in toks.iter().flatten() {
        *counts.entry(&t.kind).or_default() += 1;
    }

    let total: usize = parts.iter().map(|p| p.len()).sum();
    let mut out = format!(
        "# {} {}x{} bytes={} kinds={}\n",
        c.name,
        c.cols,
        c.rows,
        total,
        counts.len()
    );
    for (kind, n) in &counts {
        let stripped: Vec<Vec<u8>> = parts
            .iter()
            .zip(&toks)
            .map(|(p, ts)| {
                let mut s = Vec::with_capacity(p.len());
                let mut last = 0;
                for t in ts.iter().filter(|t| t.kind == *kind) {
                    s.extend_from_slice(&p[last..t.start]);
                    last = t.end;
                }
                s.extend_from_slice(&p[last..]);
                s
            })
            .collect();
        let moved: Vec<&str> = surfaces(c, &stripped)
            .iter()
            .zip(&base)
            .zip(SURFACES)
            .filter(|((a, b), _)| a != b)
            .map(|(_, name)| *name)
            .collect();
        let verdict = if moved.is_empty() {
            "-".to_string()
        } else {
            moved.join(" ")
        };
        let _ = writeln!(out, "{kind:<22} x{n:<5} {verdict}");
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Tests

#[test]
fn each_capture_matches_its_inventory() {
    let mut failures = String::new();
    for c in CAPTURES {
        let got = inventory(c);
        if got != c.golden.replace("\r\n", "\n") {
            let _ = write!(
                failures,
                "\n===== {}.ignored.golden =====\n{got}===== end =====\n",
                c.name
            );
        }
    }
    assert!(
        failures.is_empty(),
        "inventory drifted; the actual tables follow:{failures}"
    );
}

#[test]
fn every_surface_sees_an_effect_somewhere_in_the_corpus() {
    let mut seen: BTreeMap<&str, Vec<String>> = SURFACES.iter().map(|s| (*s, Vec::new())).collect();
    for c in CAPTURES {
        for line in c.golden.lines().filter(|l| !l.starts_with('#')) {
            let kind = line.split(" x").next().unwrap_or("").trim_end();
            for name in line
                .split_whitespace()
                .skip_while(|w| !w.starts_with('x'))
                .skip(1)
            {
                if let Some(v) = seen.get_mut(name) {
                    v.push(format!("{}:{kind}", c.name));
                }
            }
        }
    }
    let blind: Vec<&&str> = seen
        .iter()
        .filter(|(_, v)| v.is_empty())
        .map(|(k, _)| k)
        .collect();
    assert!(
        blind.is_empty(),
        "surfaces no kind in the corpus moves (a broken instrument): {blind:?}"
    );
}

#[test]
fn the_scanner_brackets_every_capture_exactly() {
    for c in CAPTURES {
        for part in parts_of(c) {
            let toks = scan(part);
            let mut covered = vec![false; part.len()];
            let mut last = 0;
            for t in &toks {
                assert!(
                    t.start >= last && t.end > t.start,
                    "{}: overlapping token {t:?}",
                    c.name
                );
                covered[t.start..t.end].iter_mut().for_each(|b| *b = true);
                last = t.end;
            }
            for (i, b) in part.iter().enumerate() {
                assert!(
                    *b != 0x1b || covered[i],
                    "{}: ESC at byte {i} belongs to no token — the scanner mis-bracketed it",
                    c.name
                );
            }
            // The failure that coverage cannot see: a token that runs long and swallows the next
            // sequence still covers every ESC. A token holds an ESC only where it opens, and a
            // string sequence also where its ST closes it.
            for t in &toks {
                let body = &part[t.start + 1..t.end];
                let st = body.ends_with(b"\x1b\\") as usize * 2;
                assert!(
                    !body[..body.len() - st].contains(&0x1b),
                    "{}: token {:?} at {}..{} swallowed another sequence",
                    c.name,
                    t.kind,
                    t.start,
                    t.end
                );
            }
        }
    }
}
