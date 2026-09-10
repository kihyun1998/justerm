//! THROWAWAY PROBE (#47) — not for commit.
//!
//! What a key encodes before and after an application asks for modifyOtherKeys.

use justerm_core::{Engine, Key, KeyAction, KeyEvent, Modifiers};

fn enc(e: &Engine, key: Key, mods: Modifiers) -> String {
    match e.encode_key(KeyEvent {
        key,
        mods,
        action: KeyAction::Press,
        shifted_key: None,
        base_key: None,
        text: None,
    }) {
        Some(b) => format!("{:?}", String::from_utf8_lossy(&b)).replace('\u{1b}', "<ESC>"),
        None => "(none)".into(),
    }
}

fn main() {
    let keys = [
        ("Ctrl+i", Key::Char('i'), Modifiers::CTRL),
        ("Tab", Key::Tab, Modifiers::empty()),
        ("Ctrl+[", Key::Char('['), Modifiers::CTRL),
        ("Esc", Key::Escape, Modifiers::empty()),
        ("Ctrl+m", Key::Char('m'), Modifiers::CTRL),
        ("Enter", Key::Enter, Modifiers::empty()),
        ("Ctrl+a", Key::Char('a'), Modifiers::CTRL),
    ];
    let mut before = Engine::new(80, 24);
    let mut after = Engine::new(80, 24);
    after.feed(b"\x1b[>4;2m"); // exactly what vim sends at startup
    println!("{:<8} {:<14} {:<14} same?", "key", "before", "after >4;2m");
    for (name, k, m) in keys {
        let b = enc(&before, k, m);
        let a = enc(&after, k, m);
        println!(
            "{:<8} {:<14} {:<14} {}",
            name,
            b,
            a,
            if a == b { "yes" } else { "NO" }
        );
    }
    let _ = &mut before;
}
