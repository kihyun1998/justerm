//! #1038 — what the stream sizes is bounded, not only what it counts.
//!
//! #721 capped how *many* markers a stream can allocate. Each place below already had a
//! count bound and no size bound, so a few kilobytes of input retained hundreds of
//! megabytes (measured on 727865d: 2.9 KB → 445 MiB through combining marks × REP,
//! 10 MiB → 236 MiB through the title stack, 0.74 MiB → 202 MiB through OSC 133 command
//! text, and 103 MiB of hyperlink URIs kept after every cell holding them was erased).
//!
//! The first half pins each bound as behaviour. The second half measures retained heap
//! with a counting allocator — this file is its own test binary, so the allocator sees
//! only this file's tests, and they serialise on `HEAP` so no two measure at once.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};

use justerm_core::{
    Engine, MAX_CLUSTER_TAIL, MAX_COMMAND_TEXT, MAX_COMMAND_TEXT_TOTAL, MAX_LINK_ID, MAX_LINK_URI,
    MAX_TITLE, TermEvent,
};

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
static ALLOCS: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        LIVE.fetch_add(l.size() as isize, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        LIVE.fetch_add(l.size() as isize, Relaxed);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        LIVE.fetch_add(n as isize - l.size() as isize, Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Held by every test in this file, so the live-byte counter is never read while another
/// test allocates.
static HEAP: Mutex<()> = Mutex::new(());

fn heap() -> std::sync::MutexGuard<'static, ()> {
    HEAP.lock().unwrap_or_else(|p| p.into_inner())
}

fn live() -> isize {
    LIVE.load(Relaxed)
}

const MIB: isize = 1024 * 1024;

fn row0(e: &Engine) -> String {
    e.accessible_text().lines().next().unwrap_or("").to_owned()
}

fn titles(e: &mut Engine) -> Vec<String> {
    e.drain_events()
        .into_iter()
        .filter_map(|ev| match ev {
            TermEvent::Title(t) => Some(t),
            _ => None,
        })
        .collect()
}

/// One OSC 133 command: prompt `A`, `B`, the command typed, then `C`, `D`, newline.
fn command(text: &str) -> Vec<u8> {
    format!("\x1b]133;A\x07$ \x1b]133;B\x07{text}\x1b]133;C\x07\r\n\x1b]133;D;0\x07").into_bytes()
}

// ---- combining marks ------------------------------------------------------------------

#[test]
fn a_cell_keeps_at_most_max_cluster_tail_marks() {
    let _g = heap();
    let mut e = Engine::new(80, 24);
    e.feed(format!("a{}", "\u{301}".repeat(MAX_CLUSTER_TAIL + 20)).as_bytes());
    assert_eq!(
        row0(&e),
        format!("a{}", "\u{301}".repeat(MAX_CLUSTER_TAIL)),
        "marks past MAX_CLUSTER_TAIL must be dropped, and the first ones kept"
    );
}

#[test]
fn the_longest_rgi_emoji_survives_whole_in_mode_2027() {
    let _g = heap();
    // kiss: man, man, light skin tone — 10 code points, the longest RGI sequence (Emoji 18.0).
    let kiss =
        "\u{1F468}\u{1F3FB}\u{200D}\u{2764}\u{FE0F}\u{200D}\u{1F48B}\u{200D}\u{1F468}\u{1F3FB}";
    let mut e = Engine::new(80, 24);
    e.feed(format!("\x1b[?2027h{kiss}x").as_bytes());
    assert_eq!(row0(&e), format!("{kiss}x"));
}

#[test]
fn a_scalar_dropped_by_the_cap_does_not_change_the_cell_width() {
    let _g = heap();
    // Eight marks and a ZWJ fill the tail; the second smiley would make the cluster an emoji
    // ZWJ sequence (width 2), but it is the tenth scalar after the base and is dropped.
    let mut e = Engine::new(80, 24);
    let full = format!("\u{263A}{}\u{200D}", "\u{301}".repeat(MAX_CLUSTER_TAIL - 1));
    e.feed(format!("\x1b[?2027h{full}\u{263A}x").as_bytes());
    assert_eq!(row0(&e), format!("{full}x"));
    assert_eq!(
        e.cursor().col,
        2,
        "the stored cluster is width 1, so `x` lands in column 1 and the cursor stops at 2"
    );
}

#[test]
fn an_unbounded_scrollback_limit_scrolls() {
    let _g = heap();
    let mut e = Engine::with_scrollback(80, 24, usize::MAX);
    e.feed(&b"x\r\n".repeat(100));
    assert_eq!(e.scrollback_len(), 100 - 23);
}

#[test]
fn rep_under_an_unbounded_scrollback_limit_repeats() {
    let _g = heap();
    let mut e = Engine::with_scrollback(80, 24, usize::MAX);
    e.feed(b"a\x1b[3b");
    assert_eq!(row0(&e), "aaaa");
}

#[test]
fn rep_repeats_the_capped_cluster() {
    let _g = heap();
    let mut e = Engine::new(80, 24);
    e.feed(format!("a{}\x1b[2b", "\u{301}".repeat(MAX_CLUSTER_TAIL + 5)).as_bytes());
    let cluster = format!("a{}", "\u{301}".repeat(MAX_CLUSTER_TAIL));
    assert_eq!(row0(&e), cluster.repeat(3));
}

// ---- titles ---------------------------------------------------------------------------

#[test]
fn a_title_past_max_title_is_cut_at_a_char_boundary() {
    let _g = heap();
    let mut e = Engine::new(80, 24);
    // A 3-byte char, so a byte-length cut would split one.
    e.feed(format!("\x1b]2;{}\x07", "한".repeat(MAX_TITLE + 7)).as_bytes());
    let got = titles(&mut e);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].chars().count(), MAX_TITLE);
    assert!(got[0].chars().all(|c| c == '한'));
}

#[test]
fn a_title_at_max_title_is_kept_whole_through_push_and_pop() {
    let _g = heap();
    let mut e = Engine::new(80, 24);
    let t = "x".repeat(MAX_TITLE);
    e.feed(format!("\x1b]2;{t}\x07\x1b[22;2t\x1b]2;short\x07\x1b[23;2t").as_bytes());
    let lens: Vec<usize> = titles(&mut e).iter().map(String::len).collect();
    assert_eq!(lens, vec![MAX_TITLE, 5, MAX_TITLE]);
}

// ---- command text ---------------------------------------------------------------------

#[test]
fn live_command_text_stops_at_the_total_budget() {
    let _g = heap();
    let mut e = Engine::new(MAX_COMMAND_TEXT + 8, 4);
    let long = "c".repeat(MAX_COMMAND_TEXT);
    let n = MAX_COMMAND_TEXT_TOTAL / MAX_COMMAND_TEXT + 3;
    for _ in 0..n {
        e.feed(&command(&long));
    }
    let lines = e.command_lines();
    let total: usize = lines.iter().map(|c| c.command.len()).sum();
    assert_eq!(lines.len(), n, "every command is still listed");
    assert_eq!(
        total, MAX_COMMAND_TEXT_TOTAL,
        "the text fills the budget and stops there"
    );
    assert!(
        lines.last().unwrap().command.is_empty(),
        "a capture past the budget keeps nothing"
    );
}

#[test]
fn a_disposed_command_returns_its_text_to_the_budget() {
    let _g = heap();
    let mut e = Engine::new(MAX_COMMAND_TEXT + 8, 4);
    let long = "c".repeat(MAX_COMMAND_TEXT);
    for _ in 0..MAX_COMMAND_TEXT_TOTAL / MAX_COMMAND_TEXT + 3 {
        e.feed(&command(&long));
    }
    // ED 3 drops history and the marks in it; the budget must come back with them.
    e.feed(b"\x1b[3J\x1b[2J\x1b[H");
    e.feed(&command(&long));
    let lines = e.command_lines();
    assert_eq!(
        lines.last().map(|c| c.command.as_str()),
        Some(long.as_str())
    );
}

// ---- hyperlinks -----------------------------------------------------------------------

#[test]
fn a_uri_past_max_link_uri_opens_no_link() {
    let _g = heap();
    let mut e = Engine::new(80, 24);
    let at = "u".repeat(MAX_LINK_URI);
    let past = "u".repeat(MAX_LINK_URI + 1);
    e.feed(format!("\x1b]8;;{at}\x07A\x1b]8;;\x07\x1b]8;;{past}\x07B\x1b]8;;\x07").as_bytes());
    assert_eq!(e.link_at(0, 0).map(|l| l.uri().len()), Some(MAX_LINK_URI));
    assert!(
        e.link_at(0, 1).is_none(),
        "an oversized URI must not open a link"
    );
    assert_eq!(row0(&e), "AB", "the text under it still prints");
}

#[test]
fn the_uri_bound_is_on_the_stored_text_not_the_raw_bytes() {
    let _g = heap();
    let mut e = Engine::new(80, 24);
    // Each invalid byte decodes to U+FFFD, three bytes: under the cap raw, over it stored.
    let mut b = b"\x1b]8;;".to_vec();
    b.extend(std::iter::repeat_n(0xFF, MAX_LINK_URI / 3 + 1));
    b.extend(b"\x07A");
    e.feed(&b);
    assert!(e.link_at(0, 0).is_none());
}

#[test]
fn an_oversized_uri_leaves_the_open_link_open() {
    let _g = heap();
    let mut e = Engine::new(80, 24);
    let past = "u".repeat(MAX_LINK_URI + 1);
    e.feed(format!("\x1b]8;;https://a\x07x\x1b]8;;{past}\x07y\x1b]8;;\x07z").as_bytes());
    let at = |c| e.link_at(0, c).map(|l| l.uri().to_owned());
    assert_eq!(
        at(1),
        Some("https://a".to_owned()),
        "the oversized sequence is ignored whole"
    );
    assert_eq!(at(2), None);
}

#[test]
fn an_id_past_max_link_id_still_opens_its_link() {
    let _g = heap();
    let mut e = Engine::new(80, 24);
    let id = "i".repeat(MAX_LINK_ID + 1);
    e.feed(format!("\x1b]8;id={id};https://x\x07A\x1b]8;;\x07").as_bytes());
    assert_eq!(
        e.link_at(0, 0).map(|l| l.uri().to_owned()),
        Some("https://x".to_owned())
    );
}

// ---- retained heap --------------------------------------------------------------------

/// Bytes still live after `f` ran on a fresh 80×24 engine and every event was drained.
fn retained(f: impl FnOnce(&mut Engine)) -> isize {
    let mut e = Engine::new(80, 24);
    e.feed(b"warm\r\n");
    drop(e.drain_events());
    let base = live();
    f(&mut e);
    drop(e.drain_events());
    live() - base
}

#[test]
fn marks_times_rep_retain_a_bounded_amount() {
    let _g = heap();
    // Positive control: the counter sees an ordinary full scrollback (~11 MiB at 80 cols).
    let plain = retained(|e| e.feed(&b"$ ls\r\n".repeat(10_100)));
    assert!(
        plain > 9 * MIB,
        "control: a full scrollback must register, got {plain}"
    );

    let mut unit = b"a".to_vec();
    unit.extend("\u{301}".repeat(100).as_bytes());
    unit.extend(b"\x1b[65535b");
    let r = retained(|e| e.feed(&unit.repeat(14)));
    // 10 024 rows × 80 cells, each holding at most MAX_CLUSTER_TAIL 4-byte marks plus the
    // map entry around them: well under 100 MiB. Unbounded, this was 445 MiB.
    assert!(r < 100 * MIB, "retained {} MiB", r / MIB);
}

#[test]
fn a_huge_title_and_its_stack_retain_a_bounded_amount() {
    let _g = heap();
    let r = retained(|e| {
        let mut b = b"\x1b]0;".to_vec();
        b.extend(std::iter::repeat_n(b'y', 10 << 20));
        b.push(7);
        b.extend(b"\x1b[22t".repeat(10));
        b.extend(b"\x1b]0;short\x07");
        // The parser's own OSC buffer is #1039's, not this file's: measure past it by
        // feeding the payload, then asking only what the Term kept.
        e.feed(&b);
    });
    // vte keeps its OSC buffer's capacity (#1039): about 16 MiB for a 10 MiB payload.
    // Everything above that is the engine's own, and the cap leaves it at kilobytes.
    assert!(r < 17 * MIB, "retained {} MiB", r / MIB);
}

#[test]
fn an_erased_hyperlink_keeps_no_copy_of_its_uri() {
    let _g = heap();
    const N: isize = 5000;
    const LEN: isize = 2048;
    let uri = "u".repeat(LEN as usize);
    let erase = |e: &mut Engine| e.feed(b"\x1b[3J\x1b[2J\x1b[H");
    // Control: the same lines with no link — what history's own capacity keeps after ED 3.
    let plain = retained(|e| {
        for _ in 0..N {
            e.feed(b"X\r\n");
        }
        erase(e);
    });
    let linked = retained(|e| {
        for i in 0..N {
            e.feed(format!("\x1b]8;id={i};{uri}\x07X\x1b]8;;\x07\r\n").as_bytes());
        }
        erase(e);
    });
    // The id registry keeps each dead link's entry until its amortised sweep: its id and a
    // `Weak`, a few dozen bytes. A `Weak` into an allocation holding the URI inline keeps
    // the URI too (N × LEN, 10 MiB); a key carrying the URI is another N × LEN.
    let links = linked - plain;
    assert!(
        links < N * 128,
        "erased links kept {} KiB beyond the control's {} KiB",
        links / 1024,
        plain / 1024
    );
}

#[test]
fn an_oversized_id_is_not_kept() {
    let _g = heap();
    let id = "i".repeat(100 * 1024);
    let r = retained(|e| {
        for i in 0..100 {
            e.feed(format!("\x1b]8;id={i}{id};https://x\x07X\x1b]8;;\x07\r\n").as_bytes());
        }
        e.feed(b"\x1b[3J\x1b[2J\x1b[H");
    });
    // Registered under its id, each open would keep 100 KiB of key: 10 MiB.
    assert!(r < MIB, "retained {} KiB of oversized ids", r / 1024);
}

#[test]
fn a_resize_round_trip_keeps_no_slack() {
    let _g = heap();
    let mut e = Engine::new(80, 24);
    // Lines at 60 % width, so a reflow's row building has room to over-allocate.
    let line = format!("{}\r\n", "w".repeat(48));
    e.feed(line.repeat(10_100).as_bytes());
    drop(e.drain_events());
    let before = live();
    e.resize(200, 24);
    e.resize(80, 24);
    e.feed(line.repeat(20_000).as_bytes());
    drop(e.drain_events());
    let grown = live() - before;
    assert!(
        grown < MIB / 4,
        "a round trip and 20 000 lines grew the heap by {} KiB",
        grown / 1024
    );
}

#[test]
fn a_huge_resize_gives_its_rows_back() {
    let _g = heap();
    let mut e = Engine::new(80, 24);
    let before = live();
    e.resize(65_535, 24);
    e.resize(80, 24);
    e.feed(&b"x\r\n".repeat(100));
    drop(e.drain_events());
    let grown = live() - before;
    assert!(
        grown < MIB,
        "80×24 after a 65 535-column resize keeps {} KiB more",
        grown / 1024
    );
}

#[test]
fn a_full_scrollback_scrolls_without_allocating() {
    let _g = heap();
    let mut e = Engine::new(80, 24);
    e.feed(&b"$ ls\r\n".repeat(10_100));
    drop(e.drain_events());
    let more = b"$ ls\r\n".repeat(10_000);
    let before = ALLOCS.load(Relaxed);
    e.feed(&more);
    let n = ALLOCS.load(Relaxed) - before;
    assert_eq!(n, 0, "10 000 lines past the cap allocated {n} times");
}
