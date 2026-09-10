//! THROWAWAY PROBE (#47) — not for commit.
//!
//! Differential replay over a real capture: for each *kind* of escape sequence the
//! stream contains, feed the stream with every instance of that kind removed and
//! compare a full-frame digest against the unmodified baseline. IDENTICAL means the
//! engine's observable state does not depend on that kind anywhere in this stream.
//!
//! The digest is the wire frame + accessible text + drained events + replies, not a
//! char grid: a char-only snapshot cannot see a colour or a link, so it would report
//! "ignored" for anything the engine handles but does not print (#554's trap).
//!
//! Positive control: `CSI m` and `CSI H` must come back DIFFERS. If they do not, the
//! instrument is broken and every IDENTICAL row below it is meaningless.

use justerm_core::{Engine, Key, KeyAction, KeyEvent, KeypadKey, Modifiers};

/// One scanned token: its byte range and its kind key.
struct Tok {
    start: usize,
    end: usize,
    kind: String,
}

/// Minimal ANSI scanner — enough to bracket whole sequences so removing one leaves
/// valid bytes on both sides. Anything not introduced by ESC is plain text (and C0
/// controls, which get their own kinds).
fn scan(b: &[u8]) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        match b[i] {
            0x1b => {
                i += 1;
                if i >= b.len() {
                    break;
                }
                match b[i] {
                    b'[' => {
                        // CSI: params/intermediates then a final in 0x40..=0x7e.
                        i += 1;
                        let ps = i;
                        while i < b.len() && (0x20..0x40).contains(&b[i]) {
                            i += 1;
                        }
                        let inter: String = b[ps..i]
                            .iter()
                            .filter(|c| !c.is_ascii_digit() && **c != b';' && **c != b':')
                            .map(|c| *c as char)
                            .collect();
                        if i < b.len() {
                            let fin = b[i] as char;
                            i += 1;
                            out.push(Tok {
                                start,
                                end: i,
                                kind: format!("CSI {inter}{fin}"),
                            });
                            continue;
                        }
                    }
                    b']' => {
                        // OSC: terminated by BEL or ST (ESC \ or 0x9c).
                        i += 1;
                        let ps = i;
                        while i < b.len()
                            && b[i] != 0x07
                            && !(b[i] == 0x1b && b.get(i + 1) == Some(&b'\\'))
                            && b[i] != 0x9c
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
                        out.push(Tok {
                            start,
                            end: i,
                            kind: format!("OSC {}", if num.is_empty() { "?" } else { &num }),
                        });
                        continue;
                    }
                    b'P' | b'X' | b'^' | b'_' => {
                        // DCS / SOS / PM / APC: string until ST.
                        let intro = b[i] as char;
                        i += 1;
                        while i < b.len() && !(b[i] == 0x1b && b.get(i + 1) == Some(&b'\\')) {
                            i += 1;
                        }
                        if i < b.len() {
                            i += 2;
                        }
                        out.push(Tok {
                            start,
                            end: i,
                            kind: format!("ESC {intro} (string)"),
                        });
                        continue;
                    }
                    _ => {
                        // Plain ESC: optional intermediates then a final byte.
                        let ps = i;
                        while i < b.len() && (0x20..0x30).contains(&b[i]) {
                            i += 1;
                        }
                        let inter: String = b[ps..i].iter().map(|c| *c as char).collect();
                        if i < b.len() {
                            let fin = b[i] as char;
                            i += 1;
                            out.push(Tok {
                                start,
                                end: i,
                                kind: format!("ESC {inter}{fin}"),
                            });
                            continue;
                        }
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
                continue;
            }
            _ => {
                i += 1;
                continue;
            }
        }
    }
    out
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Everything the engine exposes to a consumer, hashed.
fn digest(input: &[u8]) -> u64 {
    let mut e = Engine::with_scrollback(80, 24, 10_000);
    e.feed(input);
    let frame = e.frame();
    let wire = justerm_core::encode(&frame);
    let events = format!("{:?}", e.drain_events());
    let replies = e.drain_replies();
    let text = e.accessible_text();
    // State that never reaches the screen is still state. Keypad/cursor-key mode only
    // shows up in what a keypress *encodes*, so ask the encoder rather than the grid —
    // without this the probe reports IDENTICAL for every input-side mode and calls it
    // "ignored".
    let probe_keys: Vec<u8> = [
        (Key::Up, Modifiers::empty()),
        (Key::Down, Modifiers::empty()),
        (Key::Home, Modifiers::empty()),
        (Key::End, Modifiers::empty()),
        (Key::F(1), Modifiers::empty()),
        (Key::Char('a'), Modifiers::empty()),
        // modifyOtherKeys is about *modified* keys: these are the pairs it exists to
        // disambiguate, and an unmodified probe key cannot see it at all.
        (Key::Char('i'), Modifiers::CTRL),
        (Key::Char('['), Modifiers::CTRL),
        (Key::Char('m'), Modifiers::CTRL),
        (Key::Char('a'), Modifiers::CTRL),
        (Key::Char('a'), Modifiers::ALT),
        (Key::Char('a'), Modifiers::CTRL | Modifiers::SHIFT),
    ]
    .into_iter()
    .flat_map(|(k, mods)| {
        e.encode_key(KeyEvent {
            key: k,
            mods,
            action: KeyAction::Press,
            shifted_key: None,
            base_key: None,
            text: None,
        })
        .unwrap_or_default()
    })
    .collect();
    let keypad: Vec<u8> = [KeypadKey::Enter, KeypadKey::Add, KeypadKey::Digit(5)]
        .into_iter()
        .flat_map(|k| {
            e.encode_key(KeyEvent {
                key: Key::Keypad(k),
                mods: Modifiers::empty(),
                action: KeyAction::Press,
                shifted_key: None,
                base_key: None,
                text: None,
            })
            .unwrap_or_default()
        })
        .collect();
    let cur = format!(
        "{:?}|{}|{}|bp={}|w32={}|sync={}|cs={}|keys={:?}|kp={:?}",
        e.cursor(),
        e.scrollback_len(),
        e.viewport_logical_lines().len(),
        e.bracketed_paste(),
        e.win32_input_mode(),
        e.synchronized_output(),
        e.color_scheme_updates(),
        probe_keys,
        keypad,
    );
    fnv(&wire)
        ^ fnv(events.as_bytes()).rotate_left(11)
        ^ fnv(&replies).rotate_left(23)
        ^ fnv(text.as_bytes()).rotate_left(37)
        ^ fnv(cur.as_bytes()).rotate_left(51)
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: probe_ignored <raw>");
    let mut bytes = std::fs::read(&path).expect("read capture");
    // An alt-screen app tears the alt buffer down before it exits, so the state at EOF
    // shows none of what it drew — every kind then reads IDENTICAL, including the
    // controls. Cut at the teardown, which is what the repo's own capture tests do.
    let mut cut = None;
    for pat in [b"[?1049l".as_slice(), b"[?1047l", b"[?47l"] {
        if let Some(p) = bytes.windows(pat.len()).rposition(|w| w == pat) {
            cut = Some(cut.map_or(p, |c: usize| c.min(p)));
        }
    }
    if let Some(c) = cut {
        eprintln!(
            "(truncated at alt-screen teardown: {c} of {} bytes)",
            bytes.len()
        );
        bytes.truncate(c);
    }
    let toks = scan(&bytes);

    let mut kinds: Vec<String> = toks.iter().map(|t| t.kind.clone()).collect();
    kinds.sort();
    kinds.dedup();

    let base = digest(&bytes);
    println!(
        "== {path} ({} bytes, {} kinds) ==",
        bytes.len(),
        kinds.len()
    );

    let mut rows: Vec<(String, usize, bool)> = Vec::new();
    for k in &kinds {
        let mut stripped = Vec::with_capacity(bytes.len());
        let mut last = 0;
        let mut n = 0;
        for t in toks.iter().filter(|t| &t.kind == k) {
            stripped.extend_from_slice(&bytes[last..t.start]);
            last = t.end;
            n += 1;
        }
        stripped.extend_from_slice(&bytes[last..]);
        rows.push((k.clone(), n, digest(&stripped) != base));
    }

    rows.sort_by_key(|(k, n, differs)| (*differs, std::cmp::Reverse(*n), k.clone()));
    for (k, n, differs) in &rows {
        println!(
            "{:<18} x{:<6} {}",
            k,
            n,
            if *differs { "DIFFERS" } else { "IDENTICAL" }
        );
    }
}
