//! The terminal state model: a `vte::Perform` that maps parsed VT actions onto
//! the grid, cursor, and pen. This is where the "hidden VT state" lives —
//! pending-wrap, the wide-char spacer, and the pen (BCE seam).

use std::collections::VecDeque;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use vte::Params;

use crate::cell::{CellFlags, UnderlineStyle};
use crate::color::Color;
use crate::cursor::{Cursor, CursorShape, Pen};
use crate::damage::{LineBounds, LineDamage, ScrollOp, TermDamage};
use crate::event::TermEvent;
use crate::grid::{ExtAttrs, Grid, Row};
use crate::input::{
    KeyEvent, MouseEncoding, MouseEvent, MouseProtocol, encode_focus, encode_key, encode_mouse,
    encode_paste,
};
use crate::search::Match;
use crate::selection::{BufferPoint, Selection};
use crate::serialize::{Frame, FrameKind, MAX_SCROLL_COUNT, MarkerId, MarkerKind, Overlay, Span};

/// Buffer-walk primitives shared by every read surface (#585). A child module, so
/// it reaches `Term`'s private fields directly — no field is widened for it.
mod walk;

/// The search query surface (#586) — finding matches, and the highlight set the
/// consumer pushes back. Stands on `walk`, whose `pub(super)` reaches a sibling
/// module because both are descendants of `term`.
mod search;

/// The selection surface (#587) — gestures, the anchor fixups the write path drives,
/// and text extraction. Stands on `walk` like its siblings.
mod selection;

/// The decoration-marker surface (#588) — marks anchored to absolute buffer lines, the
/// OSC 133 command queries over them, and the anchor fixups the write path drives.
mod markers;

/// Viewport logical lines (#601) — the soft-wrap-joined text a consumer needs for URL
/// detection. The last read surface to leave this file under #584.
mod logical;

/// Tracked points (#691) — absolute positions the engine keeps on their content for a
/// holder that lives outside it, and the anchor fixups the write path drives.
mod tracked;

/// The VT dispatch surface — `vte::Perform` for `Term`, the DEC private modes and the VT52
/// sub-parser. A child module like its siblings, so it drives `Term`'s private write path
/// directly.
mod dispatch;

/// The reply surface — the queries the engine answers alone, the ones it relays as events for
/// the consumer to answer through `report_*`, and the title stack. `dispatch` calls into it.
mod replies;

/// The viewport surface — the scroll position, viewport-row to absolute-line conversion, and the
/// per-cell queries on what is on screen. `search`, `selection` and `markers` call into it.
mod viewport;

/// Owns the authoritative screen state and applies VT actions to it.
///
/// **No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)): nothing outside this crate has a reason to build one.** No
/// public function accepts it — the engine hands it out — and there are zero out-of-crate literal
/// sites, so the attribute would bind nothing it does not already bind.
pub struct Term {
    grid: Grid,
    /// The inactive screen. Swapped with `grid` on alt-screen enter/leave; holds
    /// whichever of primary/alternate is not currently shown. The alt screen has
    /// no scrollback (#3 only rings the primary).
    alt_grid: Grid,
    cursor: Cursor,
    /// Cursor saved on alt-screen enter (DEC 1049), restored on leave.
    saved_cursor: Cursor,
    /// Whether the alternate screen is currently active. Guards enter/leave so a
    /// double-enter or double-leave is a no-op.
    on_alt: bool,
    /// One flag per column: is there a tab stop here? Explicit per-column state
    /// (HTS sets, TBC clears), not a fixed modulo. Default = every 8th column.
    tabs: Vec<bool>,
    /// Which characters end a word for Word selection — consumer policy (ADR-0017), so it
    /// survives RIS. Defaults to [`DEFAULT_WORD_SEPARATORS`]; replaced through
    /// `set_word_separators`, which enforces the `' '` floor.
    word_separators: String,
    /// The window title the application last set (OSC 0/2), retained so a title pop has
    /// something to restore (#823). Application state, so it dies on RIS:
    /// `docs/map/invariant/ris-keeps-configuration-drops-coordinates.md`.
    window_title: String,
    /// The icon name, the second axis XTWINOPS addresses (#823). Written only by OSC 0 and the
    /// title stack's push and pop; OSC 1 is not parsed and there is no icon-name event, so no
    /// test observes it (`docs/architecture.md` § Hidden VT state).
    icon_name: String,
    /// The window-title stack (XTWINOPS `CSI 22 t` / `CSI 23 t`), bounded at
    /// [`TITLE_STACK_DEPTH`]; one of two independent stacks, one per axis. Why two:
    /// `docs/map/territory/vt-interpretation.md`.
    window_title_stack: Vec<String>,
    /// The icon-name stack — the other half of the pair above.
    icon_name_stack: Vec<String>,
    /// Origin mode (DECOM ?6): when set, cursor addressing is relative to the
    /// scroll region's top margin (and clamped to it).
    origin_mode: bool,
    /// Autowrap (DECAWM ?7): default on. When off, a glyph past the right margin
    /// pins the cursor to the last column and overwrites in place instead of
    /// wrapping to the next line (matches xterm.js) (#63).
    autowrap: bool,
    /// Insert mode (IRM, the non-private SM/RM mode 4): default off (replace).
    /// When on, a printed glyph shifts the row's tail right first (#64).
    insert_mode: bool,
    /// New-line mode (LNM, the non-private SM/RM mode 20): default off. When on,
    /// a line feed also carriage-returns (`convertEol`). Output-only — the Enter
    /// key still encodes CR, matching xterm.js (#71).
    newline_mode: bool,
    /// Reverse wraparound (DEC ?45): default off. When on (and with `?7h`), a step back at
    /// column 0 of a soft-wrapped row moves to the end of the previous row, and a step back
    /// from a parked cursor spends the deferred wrap instead of moving — `BS` and `CSI D`
    /// alike, through `Term::step_back` (#80, #873). The walk leaves the wrap link alone ([`Term::end_wrap`]).
    reverse_wraparound: bool,
    /// Bracketed-paste mode (DEC ?2004). The engine owns the flag; the input
    /// encoder (#11) reads it to decide whether to wrap pasted text in markers.
    bracketed_paste: bool,
    /// Synchronized output (DEC ?2026): the app brackets a frame of output so the
    /// renderer can paint it atomically. The engine only *tracks* the flag — the
    /// consumer owns the paint-hold and the spec-mandated timeout.
    synchronized_output: bool,
    /// Color-scheme-update notifications (DEC ?2031): the app asked to be told
    /// when the light/dark scheme changes. The engine is theme-agnostic — it only
    /// tracks the flag; the consumer (which knows the scheme) drives the ?997
    /// notification via `report_color_scheme` (#85).
    color_scheme_updates: bool,
    /// Grapheme-cluster mode (DEC ?2027, default OFF): the app opted into UAX #29 grapheme-cluster
    /// width — a ZWJ / skin-tone / flag / emoji+VS16 sequence is clustered into ONE cell instead of
    /// one cell per scalar (#295). OFF keeps the per-char (wcwidth-compatible) behaviour so the
    /// cursor stays in sync with wcwidth apps — clustering is opt-in for exactly that reason (#301).
    grapheme_clustering: bool,
    /// Where the last content-producing print landed — `(row, col)` of that cluster's lead
    /// cell — or `None` once anything but a print has run. `REP` reads the grapheme back off
    /// this cell (#825). Who sets and clears it, and why a position:
    /// `docs/map/territory/vt-interpretation.md`.
    repeat_anchor: Option<(usize, usize)>,
    /// Set while [`crate::Engine::feed`] advances the parser over a single `CAN`
    /// (`0x18`) or `SUB` (`0x1a`) byte. An `osc_dispatch` inside that advance is the
    /// OSC the byte cancels, and it has no effect (#970).
    pub(crate) cancel_byte_in_flight: bool,
    /// win32-input-mode (DEC ?9001): the app asked for keys as raw Windows
    /// key-records. The engine only *tracks* the flag — the raw record encoding
    /// (`CSI Vk;Sc;Uc;Kd;Cs;Rc _`) is a non-goal (raw passthrough, no semantic
    /// conversion), left to the ConPTY consumer; `encode_key` is unchanged (#86).
    win32_input_mode: bool,
    /// Application cursor keys (DECCKM ?1): when set, cursor keys / Home / End
    /// encode as SS3 rather than CSI (see `input.rs`).
    app_cursor_keys: bool,
    /// Application keypad mode (DECNKM ?66 / DECKPAM `ESC =` / DECKPNM `ESC >`):
    /// tracked for protocol completeness + DECRQM, but NOT yet acted on in key
    /// encoding — xterm.js tracks it the same way and never reads it (#74).
    application_keypad: bool,
    /// VT52 compatibility mode (DECANM ?2 *reset*): when set, `esc_dispatch` is
    /// re-routed into the pre-ANSI VT52 dialect (`ESC A`-style sequences) instead
    /// of the ANSI meaning. `ESC <` clears it. Default off (ANSI). (#84)
    vt52_mode: bool,
    /// VT52 `ESC Y row col` direct-addressing state (#84). vte tokenizes `ESC Y`
    /// as a final and returns to ground, so the two coordinate bytes arrive as
    /// `print()` calls — not part of the escape sequence. This counts them down
    /// (2 → 1 → 0; 0 = not addressing) and `vt52_y_row` parks the first (row)
    /// until the second (col) lands. Each byte decodes as `value - 0x20`.
    vt52_y_pending: u8,
    vt52_y_row: usize,
    /// Mouse tracking mode — what events the app asked to be reported
    /// (?1000/?1002/?1003). `Off` by default.
    mouse_protocol: MouseProtocol,
    /// Mouse coordinate encoding (default X10 vs ?1006 SGR).
    mouse_encoding: MouseEncoding,
    /// Focus in/out reporting (?1004): emit `CSI I`/`CSI O` on focus change.
    focus_events: bool,
    /// Kitty keyboard-protocol progressive-enhancement flags currently in effect
    /// (bit0 disambiguate, bit1 report-events, bit2 alt-keys, bit3 all-as-escape,
    /// bit4 associated-text). 0 = legacy. `encode_key` consults these (#23).
    kitty_flags: u8,
    /// Saved `kitty_flags` for the protocol's push/pop stack (`CSI > u` pushes,
    /// `CSI < u` pops). Capped depth — overflow drops the oldest entry.
    kitty_stack: Vec<u8>,
    /// The other screen's `kitty_flags` and `kitty_stack`: each screen keeps its own, and
    /// [`Self::swap_kitty_keyboard`] exchanges them when the screen changes.
    kitty_flags_inactive: u8,
    kitty_stack_inactive: Vec<u8>,
    /// xterm's `modifyOtherKeys` at level 2 or above, set by `CSI > 4 ; Pv m` (XTMODKEYS,
    /// #890). `encode_key` consults it after `kitty_flags`. Both resets clear it:
    /// `docs/map/invariant/ris-keeps-configuration-drops-coordinates.md`.
    modify_other_keys_2: bool,
    /// Consumer events (title / bell / cwd) accumulated since the last
    /// `drain_events` (#12). Pull, not push — see `event.rs`.
    events: Vec<TermEvent>,
    /// Outbound reply bytes (DA/DSR/DECRQM query answers, #27) accumulated
    /// during `feed` for the consumer to write back to the PTY. Raw bytes →
    /// PTY, kept separate from typed `events` → UI.
    replies: Vec<u8>,
    /// The hyperlink currently open (OSC 8 with a URI), stamped onto every glyph written until
    /// closed (OSC 8 with empty URI). Ambient pen-like state — not part of the pen/SGR, and
    /// not cleared by an SGR reset.
    current_link: Option<std::sync::Arc<str>>,
    /// Live OSC 8 `id=` groups: `"id;;uri"` → the allocation that key already named, held
    /// weakly so a group never outlives its cells (#635, `docs/map/territory/hyperlinks.md`).
    link_ids: std::collections::HashMap<String, std::sync::Weak<str>>,
    /// Map length at which [`Self::link_ids`] is swept for dangling keys, doubling each time
    /// so the sweep is amortised O(1) per open and dead keys stay O(live).
    link_ids_sweep_at: usize,
    /// Scroll region top/bottom margins (DECSTBM), 0-based inclusive. A
    /// line-feed at `scroll_bottom` scrolls only rows `[scroll_top..=scroll_bottom]`.
    /// Default = the full screen.
    scroll_top: usize,
    scroll_bottom: usize,
    /// Lines that have scrolled off the top of the primary screen, oldest at the
    /// front. Accrues only on a top-anchored, primary-screen scroll.
    scrollback: VecDeque<Row>,
    /// How many lines the viewport is scrolled up from the bottom. 0 = following
    /// the live screen; clamped to `[0, scrollback.len()]`.
    display_offset: usize,
    /// Maximum scrollback lines retained; the oldest are evicted past this.
    scrollback_limit: usize,
    /// A spare row buffer recycled across full-screen scrolls: the cap-evicted
    /// oldest line is parked here and reused as the next scroll's blank bottom,
    /// so a steady-state flood allocates nothing (ADR-0009).
    recycled_row: Option<Row>,
    /// Per-line damage bounds since the last `reset_damage` (ack), one per row.
    line_damage: Vec<LineBounds>,
    /// A first-class scroll recorded since the last `reset_damage`.
    scroll: Option<ScrollOp>,
    /// The whole screen changed (alt switch / clear / later resize+flood) — the
    /// renderer must redraw everything.
    full_damage: bool,
    /// The cursor `(row, col)` at the last `reset_damage` (ack) — where the
    /// consumer last saw the caret. A pure cursor move records no content
    /// damage, so `damage()` folds this *old* cell plus the current one into the
    /// frame; without it a cell-invert caret ghosts at the old spot (mirrors
    /// Alacritty's `last_cursor`). #38.
    prev_cursor: (usize, usize),
    /// The live selection, in absolute buffer coordinates. `None` when nothing
    /// is selected. See `selection.rs`.
    selection: Option<Selection>,
    /// The search highlights the consumer asked to paint (#108). Search
    /// matches are consumer-owned (it drives next/prev), so the engine holds only
    /// the set handed back via `set_search_highlights`, and `frame()` projects it
    /// onto the viewport — the same anchoring path as the selection.
    search_highlights: Vec<Match>,
    /// The active (current) search match (#428), stored as its absolute span (#436) and
    /// designated by the consumer by index or by span. Voided with the set by
    /// `set_search_highlights` and `invalidate_search_highlights`:
    /// `docs/map/territory/search.md`.
    active_search_highlight: Option<Match>,
    /// Engine-owned decoration markers (#118), split per buffer (#177). The active list is
    /// selected by `on_alt` through `markers`/`markers_mut`; `alt_markers` holds plain anchors
    /// only (#187, #192) and is disposed on alt-leave. `next_marker_id` is shared by both
    /// buffers so ids never alias.
    normal_markers: VecDeque<Marker>,
    alt_markers: VecDeque<Marker>,
    next_marker_id: u32,
    /// The basis that keeps a pulled marker index valid without re-pulling (#490), reported by
    /// [`Term::marker_index`] and the frame header. `evicted_total` counts lines popped off the
    /// front of scrollback since startup or RIS (by the cap, `ED 3` and [`Term::clear`], #936);
    /// `marker_epoch` moves when a surviving marker's line moved for a reason no single offset
    /// repairs. Disposal is not a bump. See `docs/map/territory/marker.md`.
    evicted_total: u64,
    marker_epoch: u32,
    /// Positions the engine keeps on their content for a holder that lives
    /// *outside* it (#691). Split per buffer and re-anchored by the same fixups as
    /// the markers beside them; the difference is that nothing here reaches a
    /// frame — a tracked point is answered on request, never projected.
    normal_tracked: Vec<TrackedPoint>,
    alt_tracked: Vec<TrackedPoint>,
    next_tracked_id: u32,
    /// Cursor state saved by DECSC (ESC 7), restored by DECRC (ESC 8). A slot
    /// separate from `saved_cursor` (which is the alt-screen save). Defaults to
    /// home/default so a DECRC with no prior DECSC restores a sane state.
    decsc: SavedCursor,
    /// SCS-designated character sets G0..G3 (#62). `gl` indexes the active (GL)
    /// set, switched by SI (→G0) / SO (→G1). First cut uses G0/G1.
    charsets: [Charset; 4],
    gl: usize,
}

/// A character set designated by SCS (#62). First cut: ASCII (default), DEC
/// Special Graphics (line-drawing), and UK. G2/G3 and the GR half are later.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Charset {
    #[default]
    Ascii,
    DecSpecialGraphics,
    Uk,
}

impl Charset {
    /// Map one GL byte (a `char` in the 7-bit range) through this set. ASCII and
    /// any out-of-range char pass through; UK swaps `#`→£; DEC Special Graphics
    /// translates `_`..`~` to the line-drawing / symbol glyphs.
    fn map(self, c: char) -> char {
        match self {
            Charset::Ascii => c,
            Charset::Uk if c == '#' => '£',
            Charset::Uk => c,
            Charset::DecSpecialGraphics => dec_special_graphics(c),
        }
    }
}

/// The VT100 DEC Special Graphics set: `` ` ``..`~` (0x60..0x7E) map to the box-drawing
/// and symbol glyphs; anything else, `_` included, passes through unchanged.
fn dec_special_graphics(c: char) -> char {
    // `_` (0x5F) is absent on purpose: `docs/map/territory/vt-interpretation.md`.
    match c {
        '`' => '◆',
        'a' => '▒',
        'b' => '␉',
        'c' => '␌',
        'd' => '␍',
        'e' => '␊',
        'f' => '°',
        'g' => '±',
        'h' => '␤',
        'i' => '␋',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        other => other,
    }
}

/// Default scrollback retention when not specified.
const DEFAULT_SCROLLBACK: usize = 10_000;

/// The narrowest screen the engine represents: **two columns**, because a width-2 glyph
/// needs a lead cell and a spacer ([ADR-0025](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0025-row-and-wide-pair-cell-state-ownership.md) D4).
/// `Term::with_scrollback` and [`Term::resize`] clamp `cols` up to this.
///
/// The clamp is **silent and pull-only**: a `resize(1, rows)` is widened, not rejected, and
/// no event reports it. Read the width back from [`Term::grid`] / the frame header and size
/// the PTY from that, never from the value you requested.
pub const MIN_COLUMNS: usize = 2;

/// The built-in word-boundary set for Word (semantic) selection — the default value of
/// [`Term::set_word_separators`], and policy the consumer may replace ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)).
///
/// Space, tab, U+3000 IDEOGRAPHIC SPACE and a punctuation set without `.`, `/` or `-`, so a
/// path or URL stays one word. A literal set rather than the Unicode `White_Space`
/// property, so a no-break space does not end a word. [`Term::set_word_separators`]
/// additionally forces `' '` into whatever it is given.
pub const DEFAULT_WORD_SEPARATORS: &str = ",│`|:\"' ()[]{}<>\t\u{3000}";

/// A declared OSC 8 hyperlink, as handed to a consumer.
///
/// Owned rather than borrowed, so a caller can hold it across the next `feed()`. Cloning is a
/// refcount bump; the allocation is shared with every cell of the same OSC 8 open and
/// released when the last row holding it dies. Two opens of an identical URI are two links,
/// and no accessor answers link identity yet.
///
/// No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)): no public
/// function accepts one, so the attribute would bind nothing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hyperlink {
    uri: std::sync::Arc<str>,
}

impl Hyperlink {
    pub(crate) fn new(uri: std::sync::Arc<str>) -> Self {
        Hyperlink { uri }
    }

    /// The link target, exactly as the application declared it — never validated,
    /// never resolved. Whether it is a URL a consumer is willing to open is that
    /// consumer's policy ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)), the same way colour resolution is.
    ///
    /// **One exception to "exactly":** a URI containing 14 or more unencoded `;` arrives
    /// cut short, because the parser this engine builds on passes at most 16 OSC fields.
    /// The shorter URI is not marked as cut. A percent-encoded `%3B` is unaffected.
    pub fn uri(&self) -> &str {
        &self.uri
    }
}

/// Length at which the `id=` group map is first swept for dangling keys, doubling from
/// there. Small enough that a session declaring a handful of ids never pays a sweep,
/// large enough that the sweep is not the common path.
const LINK_IDS_FIRST_SWEEP: usize = 16;

/// How deep an XTWINOPS title stack goes before a push starts dropping the oldest entry.
/// Why ten, and why the oldest: `docs/architecture.md` § Hidden VT state (#823).
const TITLE_STACK_DEPTH: usize = 10;

/// The widest grid the engine will hold: the frame header stores `cols` as `u16`, so a
/// wider grid could not be described to a consumer. Far past any real terminal, so a
/// backstop rather than a policy. The clamp is silent and pull-only on the same terms as
/// [`MIN_COLUMNS`] — read the size back from [`Term::grid`] rather than trusting the value
/// you passed in.
pub const MAX_COLUMNS: usize = u16::MAX as usize;

/// The tallest grid the engine will hold. The row half of [`MAX_COLUMNS`] — same
/// `u16` header field, same reasoning, same silent-clamp contract.
pub const MAX_ROWS: usize = u16::MAX as usize;

/// The most live markers one buffer will hold: the marker group's counts are `u16` on the
/// wire, and markers are allocated by the untrusted stream (OSC 133), so a line-less stream
/// could otherwise accumulate them without bound. A backstop, not a policy: ordinary shell
/// integration tops out near 40 000 in a default-scrollback session. Overflow disposes the
/// oldest marker and announces it through `TermEvent::MarkerDisposed`.
pub const MAX_MARKERS: usize = u16::MAX as usize;

/// The longest command text an OSC-133 `OutputStart` mark will freeze, in `char`s. A longer
/// command is captured truncated to this many characters, at a `char` boundary.
///
/// The text spans `[B, C)` and the stream decides how far apart those are, so the frozen
/// copy needs a bound; a prefix is a usable answer where an absent one is not. No ordinary
/// command reaches it.
pub const MAX_COMMAND_TEXT: usize = 4096;

/// The longest `OSC 52` base64 payload the engine will decode, in bytes. A longer one is
/// **dropped whole**, never truncated: a truncated clipboard is text the user would paste
/// believing it complete.
///
/// It bounds the engine's own copies, not the parser's: `vte` has already buffered the
/// payload before the handler runs. It does not bound the event queue. Sized so that no
/// real copy reaches it.
pub const MAX_CLIPBOARD_BASE64: usize = 16 * 1024 * 1024;

/// The state DECSC (ESC 7) saves and DECRC (ESC 8) restores: position, pen/SGR,
/// pending-wrap, and origin mode (per ADR-0004 — DECRC restores origin mode,
/// which Alacritty omits). Cursor *visibility* is deliberately not part of this
/// (DECTCEM is separate from DECSC).
#[derive(Clone, Copy, Default)]
struct SavedCursor {
    row: usize,
    col: usize,
    pen: Pen,
    pending_wrap: bool,
    origin_mode: bool,
    /// SCS charset state at save time — DECSC/DECRC round-trip the designated
    /// sets and the active GL shift (#62).
    charsets: [Charset; 4],
    gl: usize,
}

/// An engine-owned decoration marker (#118): a stable id bound to an absolute
/// buffer line. The line shifts in lockstep with eviction/region scroll/reflow
/// (the same coordinate moves the selection anchor tracks); the marker is
/// dropped when its line leaves the buffer — **or when `ED` blanks the whole row
/// it stands on** (#750), which is the one death that is not the buffer moving.
struct Marker {
    id: MarkerId,
    line: usize,
    /// The cursor column at emit time (#166): bounds the typed command for OSC-133
    /// `CommandStart`/`OutputStart` marks; `0` for plain `add_marker` decorations. Domain
    /// `[0, cols]`, a bound rather than a cell (#562, `docs/map/territory/marker.md`).
    col: usize,
    /// Plain for a `add_marker` decoration; a command-boundary role for an
    /// OSC 133 mark (#158). All kinds share the anchor/eviction machinery.
    kind: MarkerKind,
    /// What an `OutputStart` mark froze about the command it closes (#750); `None` on every
    /// other kind. Boxed, and on the marker rather than in a side table:
    /// `docs/map/territory/marker.md`.
    command: Option<Box<CommandRecord>>,
}

/// The part of a command that is not in the buffer, frozen on its `OutputStart` mark: the
/// command `text` when `C` arrives, and the `exit` code when `D` is parsed (#750). Neither
/// can be recovered from cells afterwards: `docs/map/territory/marker.md`.
struct CommandRecord {
    text: Box<str>,
    exit: Option<i32>,
}

/// A stable handle to a tracked buffer position, handed out by
/// [`Term::track_point`].
///
/// It is deliberately **not** a [`MarkerId`]: a marker is a decoration anchor and
/// rides two frame groups, so every marker a consumer registers is something the
/// renderer paints. A tracked point is private to whoever asked for it.
///
/// **No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)): the attribute is already implied.** The field is `pub(crate)`,
/// so no literal is possible outside this crate however many fields it grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrackedId(pub(crate) u32);

/// One tracked position: an absolute buffer `(line, col)` the write path keeps on
/// its content (#691). The element type of `normal_tracked` / `alt_tracked`.
struct TrackedPoint {
    id: TrackedId,
    line: usize,
    col: usize,
}

/// One live marker, as the pull query reports it: its stable id, its absolute
/// `[scrollback ++ screen]` line, and its kind. `kind` rides here rather than on the frame
/// because it never changes after the marker is made.
///
/// No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)): no public
/// function accepts one, so the attribute would bind nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerEntry {
    pub id: MarkerId,
    pub line: u32,
    pub kind: MarkerKind,
}

/// The answer to [`Term::marker_index`]: every live marker of the active buffer, plus the
/// basis that says how long the answer stays usable.
///
/// Keep it and rebase per frame — `current = line - (evicted_total_now - evicted_total)` —
/// for exactly as long as `epoch` is unchanged; when the epoch moves, ask again. An
/// alt-screen switch moves the epoch too, because an absolute line means a different thing
/// on each screen.
///
/// No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)): no public
/// function accepts one, so the attribute would bind nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerIndex {
    pub markers: Vec<MarkerEntry>,
    pub evicted_total: u64,
    pub epoch: u32,
}

/// One executed shell command recovered from OSC-133 marks, for screen-reader command
/// navigation: the consumer jumps prompt to prompt and announces `command` with a
/// success/fail signal from `exit`.
///
/// No `#[non_exhaustive]` ([#844](https://github.com/kihyun1998/justerm/issues/844)): no public
/// function accepts one, so the attribute would bind nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    /// The command's jump anchor as a *document* line: the logical-line index of its
    /// `CommandStart` mark within [`Term::accessible_text`], where soft-wrapped rows collapse to
    /// one line.
    ///
    /// **Meaningful only with the document it indexes** — the one [`Term::accessible_text`]
    /// returns at the same instant, on the primary screen. No published scalar dates a
    /// document line (eviction and a row's wrap bit both move it independently of the absolute
    /// lines), and while the alt screen is up `accessible_text` returns the other document, into
    /// which this index can still resolve onto unrelated content. So ask for both together, keep
    /// them together, and re-ask rather than rebase ([ADR-0029](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0029-a-published-coordinate-carries-its-instant-or-is-re-asked.md)).
    /// `EL`/`ECH` retire no mark, so a line can still name a row they blanked.
    pub line: usize,
    /// The typed command text, prompt- and output-excluded (`B`→`C` columns), frozen when the
    /// command's `133;C` arrived rather than re-read from cells later. Bounded at
    /// [`MAX_COMMAND_TEXT`] `char`s.
    pub command: String,
    /// The `CommandFinished` (`133;D`) exit code, if the shell reported one and the command has
    /// finished; recorded onto the closing mark when `D` is parsed.
    pub exit: Option<i32>,
}

/// Collect per-line damage bounds into damaged `LineDamage` spans (undamaged
/// lines dropped). Shared by `damage` (content-only) and `frame_damage`
/// (content + cursor cells).
fn bounds_to_lines(bounds: &[LineBounds]) -> Vec<LineDamage> {
    bounds
        .iter()
        .enumerate()
        .filter(|(_, b)| b.is_damaged())
        .map(|(line, b)| {
            let (left, right) = b.span();
            LineDamage { line, left, right }
        })
        .collect()
}

impl Term {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_scrollback(cols, rows, DEFAULT_SCROLLBACK)
    }

    pub fn with_scrollback(cols: usize, rows: usize, scrollback_limit: usize) -> Self {
        // Both clamps mirror `resize`, so a screen cannot be born at a size a resize would refuse:
        // the width floor (#547) and ceiling (#621), and a terminal is never 0-tall.
        let cols = cols.clamp(MIN_COLUMNS, MAX_COLUMNS);
        let rows = rows.clamp(1, MAX_ROWS);
        Term {
            grid: Grid::new(cols, rows),
            alt_grid: Grid::new(cols, rows),
            cursor: Cursor::default(),
            saved_cursor: Cursor::default(),
            on_alt: false,
            origin_mode: false,
            autowrap: true,
            insert_mode: false,
            newline_mode: false,
            reverse_wraparound: false,
            bracketed_paste: false,
            synchronized_output: false,
            color_scheme_updates: false,
            grapheme_clustering: false,
            repeat_anchor: None,
            cancel_byte_in_flight: false,
            win32_input_mode: false,
            app_cursor_keys: false,
            application_keypad: false,
            vt52_mode: false,
            vt52_y_pending: 0,
            vt52_y_row: 0,
            mouse_protocol: MouseProtocol::Off,
            mouse_encoding: MouseEncoding::Default,
            focus_events: false,
            kitty_flags: 0,
            kitty_stack: Vec::new(),
            kitty_flags_inactive: 0,
            kitty_stack_inactive: Vec::new(),
            modify_other_keys_2: false,
            events: Vec::new(),
            replies: Vec::new(),
            current_link: None,
            link_ids: std::collections::HashMap::new(),
            link_ids_sweep_at: LINK_IDS_FIRST_SWEEP,
            tabs: default_tabs(cols),
            word_separators: DEFAULT_WORD_SEPARATORS.to_owned(),
            window_title: String::new(),
            icon_name: String::new(),
            window_title_stack: Vec::new(),
            icon_name_stack: Vec::new(),
            scroll_top: 0,
            scroll_bottom: rows - 1,
            scrollback: VecDeque::new(),
            display_offset: 0,
            scrollback_limit,
            recycled_row: None,
            line_damage: vec![LineBounds::undamaged(cols); rows],
            scroll: None,
            full_damage: false,
            prev_cursor: (0, 0), // matches the default cursor's home position
            selection: None,
            search_highlights: Vec::new(),
            active_search_highlight: None,
            normal_markers: VecDeque::new(),
            alt_markers: VecDeque::new(),
            next_marker_id: 0,
            evicted_total: 0,
            marker_epoch: 0,
            normal_tracked: Vec::new(),
            alt_tracked: Vec::new(),
            next_tracked_id: 0,
            decsc: SavedCursor::default(),
            charsets: [Charset::Ascii; 4],
            gl: 0,
        }
    }

    /// What changed since the last `reset_damage()` — line ranges, each with a
    /// changed column span. See [ADR-0003](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0003-damage-model-incremental-bounds.md).
    pub fn damage(&self) -> TermDamage {
        if self.full_damage {
            return TermDamage::Full;
        }
        // Scrolled up under follow-bottom "stay": the viewport is frozen, so
        // screen changes below it are not visible — report nothing. (A user
        // scroll that moves the viewport sets full_damage above.)
        if self.display_offset > 0 {
            return TermDamage::Partial(Vec::new());
        }
        TermDamage::Partial(bounds_to_lines(&self.line_damage))
    }

    /// Render damage: content damage plus the old (last-acked) and current cursor cells, folded
    /// in only when the cursor moved, for [`Term::frame`] (#38). [`Term::damage`] stays
    /// content-only. Mirrors alacritty's `last_cursor`.
    fn frame_damage(&self) -> TermDamage {
        if self.full_damage {
            return TermDamage::Full;
        }
        if self.display_offset > 0 {
            return TermDamage::Partial(Vec::new());
        }
        let cur = self.cursor.point();
        if cur == self.prev_cursor {
            return TermDamage::Partial(bounds_to_lines(&self.line_damage));
        }
        let mut bounds = self.line_damage.clone();
        bounds[cur.0].expand(cur.1, cur.1);
        let pr = self.prev_cursor.0.min(self.grid.rows() - 1);
        let pc = self.prev_cursor.1.min(self.grid.cols() - 1);
        bounds[pr].expand(pc, pc);
        TermDamage::Partial(bounds_to_lines(&bounds))
    }

    /// Clear accumulated damage. The consumer calls this after applying a frame
    /// (the ack); the next `damage()` reflects only changes since.
    pub fn reset_damage(&mut self) {
        for b in &mut self.line_damage {
            b.reset();
        }
        self.scroll = None;
        self.full_damage = false;
        // The consumer has now seen the caret at the current position; the next
        // frame's cursor-move damage is measured from here (#38).
        self.prev_cursor = self.cursor.point();
    }

    /// Mark the whole screen damaged (alt switch / clear / flood, and a consumer
    /// reattach that needs a full re-sync — see [`crate::Engine::mark_fully_damaged`]).
    pub fn mark_fully_damaged(&mut self) {
        self.full_damage = true;
    }

    /// Record that columns `[left, right]` of `row` changed. Both columns are clamped to the
    /// last column and asserted in debug (#536); `row` is left to panic on the index. Why each:
    /// `docs/map/territory/damage.md`.
    fn damage_span(&mut self, row: usize, left: usize, right: usize) {
        let last = self.grid.cols().saturating_sub(1);
        debug_assert!(
            left <= right && right <= last,
            "damage_span({row}, {left}, {right}) is not a span inside [0, {last}]"
        );
        self.line_damage[row].expand(left.min(last), right.min(last));
    }

    /// The first-class scroll recorded since the last `reset_damage`, if any. `None` while
    /// scrolled up — a content scroll must not shift the frozen viewport.
    ///
    /// The count is capped at the region's own height, since a larger shift names nothing a
    /// consumer can act on, and it saturates rather than wrapping where a region is taller than
    /// the wire's `i16` count can hold.
    pub fn scroll_delta(&self) -> Option<ScrollOp> {
        if self.display_offset > 0 {
            return None;
        }
        self.scroll.map(cap_scroll)
    }

    /// Build a serializable [`Frame`] from the current damage and the viewport. `Full` ships
    /// every row; `Partial` ships the damaged spans. Each distinct OSC 8 open a shipped cell
    /// references gets one entry in the frame's own link table.
    pub fn frame(&self) -> Frame {
        let cols = self.grid.cols();
        let rows = self.grid.rows();
        let (kind, line_spans): (FrameKind, Vec<(usize, usize, usize)>) = match self.frame_damage()
        {
            TermDamage::Full => (
                FrameKind::Full,
                (0..rows).map(|l| (l, 0, cols - 1)).collect(),
            ),
            TermDamage::Partial(lines) => (
                FrameKind::Partial,
                lines
                    .into_iter()
                    .map(|d| (d.line, d.left, d.right))
                    .collect(),
            ),
        };

        // Frame-local link numbering, keyed by the URI's `Arc` identity and sized by this frame
        // (#26, #628, `docs/map/territory/hyperlinks.md`).
        let mut link_table: Vec<String> = Vec::new();
        let mut link_remap: std::collections::HashMap<*const u8, u32> =
            std::collections::HashMap::new();
        // Cells come from the viewport at `display_offset`, not the live grid:
        // viewport row `line` is absolute buffer line `top + line` (scrollback
        // when scrolled up, the live grid when `display_offset == 0`, where
        // `top == scrollback.len()` and this is identical to reading the grid).
        // Without this, a wire consumer — cells reach it only through `frame()` —
        // could never display scrollback (#48).
        let top = self.scrollback.len() - self.display_offset;
        let mut spans = Vec::with_capacity(line_spans.len());
        for (line, left, right) in line_spans {
            let mut cells = Vec::with_capacity(right - left + 1);
            let mut combining = std::collections::BTreeMap::new();
            let mut links = std::collections::BTreeMap::new();
            let mut ucolors = std::collections::BTreeMap::new();
            let row = self.abs_row(top + line);
            let last_col = row.len().saturating_sub(1);
            for col in left..=right {
                let mut cell = row[col];
                // Soft-wrap is a row property (#538), but the wire has no per-row slot — so it is
                // *derived* back onto the last cell's WRAPLINE bit here, which keeps the format
                // byte-identical and is why moving the storage needed no VERSION bump. The bit is
                // therefore wire-only: on a live grid it is never set, and `Row::is_wrapped` is
                // the question to ask.
                if col == last_col && row.is_wrapped() {
                    cell.insert_flags(CellFlags::WRAPLINE);
                }
                // Combining clusters and hyperlinks live in the row's maps; each
                // tagged cell contributes its reference to the frame, recorded on
                // the span by span-relative column (the cell holds only the bit).
                if let Some(marks) = row.combining_at(col) {
                    // The cluster itself, at its column — no side table and no index
                    // since v14 (#621). Nothing interned these (this push was
                    // unconditional), so the index only ever bought indirection.
                    combining.insert(col - left, marks.to_vec());
                }
                if let Some(uri) = row.link_at(col) {
                    // Number each distinct open once per frame (only referenced URIs
                    // ship). The wire keeps its interning — #621 measured inlining a URI
                    // per linked cell at +171…403% — so this stays an index into
                    // `link_table`; only the *engine* side stopped being a table.
                    let key = std::sync::Arc::as_ptr(uri) as *const u8;
                    let next = link_table.len() as u32 + 1;
                    let fidx = *link_remap.entry(key).or_insert_with(|| {
                        link_table.push(uri.to_string());
                        next
                    });
                    let fidx = core::num::NonZeroU32::new(fidx)
                        .expect("frame-local link indices are 1-based");
                    links.insert(col - left, fidx);
                }
                // Underline colour (SGR 58, #520): a colour reference, not a
                // side-table index, so it rides the span inline. `ucolor_at` is
                // flag-gated + already Default-filtered (the stamp only fires on an
                // underlined cell), so a present entry is a real non-default colour.
                if let Some(color) = row.ucolor_at(col) {
                    ucolors.insert(col - left, color);
                }
                cells.push(cell);
            }
            spans.push(Span {
                line: line as u16,
                left: left as u16,
                right: right as u16,
                cells,
                combining,
                links,
                ucolors,
            });
        }

        Frame {
            cols: cols as u16,
            rows: rows as u16,
            kind,
            // The live cursor: position in screen coords + DECTCEM visibility.
            // Reported, not drawn — the consumer renders the caret (#38).
            cursor_row: self.cursor.row as u16,
            cursor_col: self.cursor.col as u16,
            // Hidden while scrolled up: the live cursor is off the frozen
            // viewport, and a cell-invert caret would otherwise ink over
            // scrollback. Consistent with the frozen-damage policy (no cursor
            // damage is emitted while scrolled) and with xterm.js / alacritty,
            // which hide the caret when it falls outside the visible rows (#48).
            cursor_visible: self.cursor.visible && self.display_offset == 0,
            cursor_shape: self.cursor.shape,
            cursor_blink: self.cursor.blink,
            // Viewport scroll position for the consumer's scrollbar (ADR-0013).
            display_offset: self.display_offset as u32,
            scrollback_len: self.scrollback.len() as u32,
            evicted_total: self.evicted_total,
            marker_epoch: self.marker_epoch,
            // The active buffer's population, which is what `marker_index` reports and
            // therefore what a consumer's held index is compared against (#490).
            marker_count: self.markers().len() as u32,
            // The mouse tracking mode as a routing mask (#129): which mouse events
            // the app wants, derived from the protocol by the single source
            // `encode_mouse` shares. The consumer routes app-vs-local on it.
            mouse_events: self.mouse_protocol.wanted_events(),
            // Alt-screen flag (#149): buffer-global state the consumer can't
            // derive from viewport damage; the a11y announce policy gates on it.
            alt_screen: self.on_alt,
            // Which modified C0 keys reach the application under the current keyboard
            // modes (#941), derived from the encoder `encode_key` runs.
            modified_keys: crate::input::modified_keys(
                self.app_cursor_keys,
                self.application_keypad,
                self.kitty_flags,
                self.modify_other_keys_2,
            ),
            scroll: self.scroll_delta(),
            spans,
            link_table,
            // Interaction overlays projected onto this viewport (#108): the
            // engine-owned selection and the consumer-supplied search highlights,
            // each re-projected here so the scroll offset is applied once, by the
            // same authority that projects the cells.
            overlay: Overlay {
                selection: self.selection_range(),
                matches: self
                    .search_highlights
                    .iter()
                    .flat_map(|m| self.match_spans(m))
                    .collect(),
                // The consumer-designated active match (#428), projected through
                // the same `match_spans` math — usually also present in `matches`
                // above (the renderer's ranking resolves the overlap, #424), but
                // a span designation may sit OUTSIDE a capped hand-over (#436).
                active_match: self
                    .active_search_highlight
                    .as_ref()
                    .map(|m| self.match_spans(m))
                    .unwrap_or_default(),
                markers: self.marker_positions(),
            },
        }
    }

    /// Record a scroll of rows `[top, bottom]` by `count` (positive = up).
    ///
    /// Damage is indexed by row position, so it must follow the content the
    /// scroll just moved: rotate the bounds the same way and mark the newly
    /// exposed line fully damaged (it is new blank content for the consumer).
    fn record_scroll(&mut self, top: usize, bottom: usize, count: isize) {
        let cols = self.grid.cols();
        match count {
            1 => {
                self.line_damage[top..=bottom].rotate_left(1);
                self.line_damage[bottom] = LineBounds::fully_damaged(cols);
            }
            -1 => {
                self.line_damage[top..=bottom].rotate_right(1);
                self.line_damage[top] = LineBounds::fully_damaged(cols);
            }
            _ => {}
        }
        // Accumulate repeated scrolls of the same region into one op (flow
        // control). A *different* region cannot be expressed as one op, so
        // degrade to full rather than silently dropping the earlier scroll.
        match self.scroll {
            Some(op) if op.top == top && op.bottom == bottom => {
                self.scroll = Some(ScrollOp {
                    top,
                    bottom,
                    count: op.count + count,
                });
            }
            None => self.scroll = Some(ScrollOp { top, bottom, count }),
            Some(_) => {
                self.scroll = None;
                self.mark_fully_damaged();
            }
        }
    }

    /// Replace the word-boundary set used by Word (semantic) selection — the policy half of
    /// `selection_begin(.., SelectionType::Word)` ([ADR-0017](https://github.com/kihyun1998/justerm/blob/master/docs/adr/0017-core-consumer-boundary-mechanism-vs-policy.md)).
    /// The default is [`DEFAULT_WORD_SEPARATORS`].
    ///
    /// **`' '` is forced into whatever you pass**: a blank cell is a space, so it is what ends
    /// the walk at the end of a row's text and keeps a double-click from starting on a wide
    /// separator's spacer. The set is also the only bound on the walk — one that omits the
    /// separators present in the buffer makes a double-click walk the whole soft-wrap run.
    pub fn set_word_separators(&mut self, separators: &str) {
        let mut set: String = separators.to_owned();
        if !set.contains(' ') {
            set.push(' ');
        }
        self.word_separators = set;
    }

    /// The word-boundary set currently in force — what was passed to
    /// [`Term::set_word_separators`] plus the forced `' '`, or
    /// [`DEFAULT_WORD_SEPARATORS`] if it was never called.
    pub fn word_separators(&self) -> &str {
        &self.word_separators
    }

    /// Number of lines currently held in scrollback history.
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    /// Whether the app has an open synchronized-output block (DEC ?2026).
    pub fn synchronized_output(&self) -> bool {
        self.synchronized_output
    }

    /// Whether the app enabled color-scheme-update notifications (DEC ?2031).
    pub fn color_scheme_updates(&self) -> bool {
        self.color_scheme_updates
    }

    /// Whether the app enabled grapheme-cluster mode (DEC ?2027): emoji ZWJ / skin-tone /
    /// flag / VS16 sequences are clustered into one cell. OFF (default) is per-char, wcwidth-compat.
    pub fn grapheme_clustering(&self) -> bool {
        self.grapheme_clustering
    }

    /// Whether the app enabled win32-input-mode (DEC ?9001). The engine does
    /// not encode the raw key-records itself (a non-goal); a ConPTY consumer reads
    /// this to decide whether to emit them.
    pub fn win32_input_mode(&self) -> bool {
        self.win32_input_mode
    }

    // ---- selection -----------------------------------------------------------

    /// Resize the screen to `cols` x `rows`, reflowing soft-wrapped lines on the primary
    /// screen; rows dropped off the top enter scrollback. The whole screen is damaged.
    ///
    /// `cols` is clamped to [`MIN_COLUMNS`]..=[`MAX_COLUMNS`] and `rows` to `1..=`[`MAX_ROWS`].
    pub fn resize(&mut self, cols: usize, rows: usize) {
        // Not a print: a reflow moves the cell `REP` would read back (#825).
        self.repeat_anchor = None;
        // The same clamps as the constructor (#547, #621).
        let cols = cols.clamp(MIN_COLUMNS, MAX_COLUMNS);
        let rows = rows.clamp(1, MAX_ROWS);
        let old_cols = self.grid.cols();
        let old_rows = self.grid.rows();
        let limit = self.scrollback_limit;

        // One marker-epoch bump for the whole reflow, gated on a dimension change (#490,
        // `docs/map/territory/marker.md`).
        if (cols != old_cols || rows != old_rows)
            && (!self.normal_markers.is_empty() || !self.alt_markers.is_empty())
        {
            self.bump_marker_epoch();
        }

        // A reflow moves match coordinates, so the query-derived highlights are invalidated; the
        // selection is user-authored and re-anchors below.
        self.invalidate_search_highlights();

        // ...except on the alt screen, where any geometry change drops it (#660):
        // `docs/map/territory/selection.md`.
        if self.on_alt && (cols != old_cols || rows != old_rows) {
            self.selection = None;
        }

        // Both screens are resized; scrollback pairs with the primary screen, whichever is
        // active. `reflow: true` is a constant, not `self.autowrap` — reflow is not gated on
        // DECAWM: `docs/map/territory/reflow.md`.
        let dims = ReflowDims {
            old_cols,
            cols,
            rows,
            limit,
            reflow: true,
        };
        let scrollback = std::mem::take(&mut self.scrollback);
        if self.on_alt {
            // Active = alt (cursor, no scrollback); inactive = primary. The alt pane is re-fit, not
            // reflowed (#567); its markers and tracked points are stored as `base + alt_row`, so
            // convert to alt-local rows and re-anchor on the new base afterwards — the primary
            // scrollback may rewrap even when the alt grid does not move.
            let old_base = scrollback.len();
            let mut alt_pts: Vec<(usize, usize)> = self
                .alt_markers
                .iter()
                .map(|m| (m.line - old_base, m.col))
                .collect();
            // Alt-scoped tracked points convert the same way (#691).
            let alt_tracked_off = alt_pts.len();
            alt_pts.extend(
                self.alt_tracked
                    .iter()
                    .map(|p| (p.line.saturating_sub(old_base), p.col)),
            );
            let alt = self.grid.take_lines();
            let r_alt = reflow_pane(
                alt,
                VecDeque::new(),
                self.cursor.point(),
                &alt_pts,
                ReflowDims {
                    limit: 0,
                    reflow: false,
                    ..dims
                },
            );
            self.grid.set_screen(r_alt.screen, cols, rows);
            self.cursor.set_point(r_alt.cursor, rows, cols);

            // Primary is inactive here, but markers anchor primary content, so they reflow with it,
            // carrying `(line, col)` (#166). No primary selection exists while `on_alt`.
            let mut marker_pts: Vec<(usize, usize)> = self
                .normal_markers
                .iter()
                .map(|m| (m.line, m.col))
                .collect();
            // Primary-scoped tracked points reflow with this pane too (#691).
            let tracked_off = marker_pts.len();
            marker_pts.extend(self.normal_tracked.iter().map(|p| (p.line, p.col)));
            let primary = self.alt_grid.take_lines();
            let r = reflow_pane(
                primary,
                scrollback,
                self.saved_cursor.point(),
                &marker_pts,
                dims,
            );
            self.alt_grid.set_screen(r.screen, cols, rows);
            self.scrollback = r.scrollback;
            self.saved_cursor.set_point(r.cursor, rows, cols);
            for (i, m) in self.normal_markers.iter_mut().enumerate() {
                m.line = r.extras[i].0.saturating_sub(r.evicted);
                m.col = r.extras[i].1;
            }
            // Released rather than clamped when the reflow evicted its line (#691).
            let mut ti = 0;
            let evicted = r.evicted;
            let extras = &r.extras;
            self.normal_tracked.retain_mut(|p| {
                let (line, col) = extras[tracked_off + ti];
                ti += 1;
                match line.checked_sub(evicted) {
                    Some(line) => {
                        p.line = line;
                        p.col = col;
                        true
                    }
                    None => false,
                }
            });
            // The alt half's `extras` count from the top of the alt pane's own history, which is
            // empty (limit `0`); a marker whose row went with the shrink is disposed, not moved to
            // row 0: `docs/map/territory/marker.md`.
            let new_base = self.scrollback.len();
            let mut alt_disposed = Vec::new();
            let mut i = 0;
            self.alt_markers.retain_mut(|m| {
                let (line, col) = r_alt.extras[i];
                i += 1;
                match line.checked_sub(r_alt.evicted) {
                    Some(row) if row < rows => {
                        m.line = new_base + row;
                        // The column rides along as in the primary half (unpinned on alt).
                        m.col = col;
                        true
                    }
                    _ => {
                        alt_disposed.push(m.id);
                        false
                    }
                }
            });
            for id in alt_disposed {
                self.events.push(TermEvent::MarkerDisposed(id));
            }
            // The alt half's tracked points, on the alt marker's rule: a row the shrink pushed off
            // is gone, so the point is released (#691). `row < rows` is kept for parity with the
            // marker loop, though no measured input reaches it.
            let mut ai = 0;
            let alt_extras = &r_alt.extras;
            let alt_evicted = r_alt.evicted;
            self.alt_tracked.retain_mut(|p| {
                let (line, col) = alt_extras[alt_tracked_off + ai];
                ai += 1;
                match line.checked_sub(alt_evicted) {
                    Some(row) if row < rows => {
                        p.line = new_base + row;
                        p.col = col;
                        true
                    }
                    _ => false,
                }
            });
        } else {
            // Active = primary (cursor, scrollback); inactive = alt. The selection anchors reflow
            // alongside the cursor so they keep their content across a column change.
            let sel_pts: Vec<(usize, usize)> = self
                .selection
                .as_ref()
                .map(|s| {
                    vec![
                        (s.anchor.point.line, s.anchor.point.col),
                        (s.focus.point.line, s.focus.point.col),
                    ]
                })
                .unwrap_or_default();
            // Markers ride after the selection points, carrying `(line, col)` (#166, #118).
            let mut pts = sel_pts.clone();
            pts.extend(self.normal_markers.iter().map(|m| (m.line, m.col)));
            // Tracked points ride after the markers, by the same offset idiom (#691).
            let tracked_off = pts.len();
            pts.extend(self.normal_tracked.iter().map(|p| (p.line, p.col)));

            let primary = self.grid.take_lines();
            let r = reflow_pane(primary, scrollback, self.cursor.point(), &pts, dims);
            self.grid.set_screen(r.screen, cols, rows);
            self.scrollback = r.scrollback;
            self.cursor.set_point(r.cursor, rows, cols);
            if let Some(sel) = &mut self.selection {
                // A selection endpoint is UI state, so a `col == cols` result (#562) is clamped into the
                // grid: UI state may not move the application's content to make room for itself.
                sel.anchor.point = BufferPoint {
                    line: r.extras[0].0.saturating_sub(r.evicted),
                    col: r.extras[0].1.min(cols - 1),
                };
                sel.focus.point = BufferPoint {
                    line: r.extras[1].0.saturating_sub(r.evicted),
                    col: r.extras[1].1.min(cols - 1),
                };
            }
            let marker_off = sel_pts.len();
            for (i, m) in self.normal_markers.iter_mut().enumerate() {
                m.line = r.extras[marker_off + i].0.saturating_sub(r.evicted);
                m.col = r.extras[marker_off + i].1;
            }
            // A tracked point whose line was evicted is released, not saturated like the markers
            // above (#691, `docs/map/territory/marker.md`).
            let mut i = 0;
            let evicted = r.evicted;
            let extras = &r.extras;
            self.normal_tracked.retain_mut(|p| {
                let (line, col) = extras[tracked_off + i];
                i += 1;
                match line.checked_sub(evicted) {
                    Some(line) => {
                        p.line = line;
                        p.col = col;
                        true
                    }
                    None => false,
                }
            });

            let alt = self.alt_grid.take_lines();
            let r = reflow_pane(
                alt,
                VecDeque::new(),
                (0, 0),
                &[],
                ReflowDims {
                    limit: 0,
                    reflow: false,
                    ..dims
                },
            );
            self.alt_grid.set_screen(r.screen, cols, rows);
        }

        // Carry the deferred wrap across the resize — it is cursor state (#848). Where the reflow
        // leaves the cursor short of the last column, the one-past position is representable, so
        // the flag is cleared and the cursor takes it (ghostty's rule; the reference rows are in
        // `docs/agents/reference-facts.md`). `col + 1` cannot overflow: the branch requires
        // `col != cols - 1`.
        if self.cursor.pending_wrap && self.cursor.col != cols - 1 {
            self.cursor.pending_wrap = false;
            self.cursor.col += 1;
        }
        // The scroll region is a range over the current screen, so a geometry change discards
        // it; a resize to the current size is not a geometry change, and `resize` has no early
        // return (`docs/map/territory/reflow.md`).
        if cols != old_cols || rows != old_rows {
            self.scroll_top = 0;
            self.scroll_bottom = rows - 1;
        }
        // Extend, never rebuild and never trim (#849): new columns take the default ladder at
        // their absolute index, so `tabs.len() >= cols` rather than equal
        // (`docs/map/territory/reflow.md`).
        if cols > self.tabs.len() {
            let mut index = self.tabs.len();
            self.tabs.resize_with(cols, || {
                let is_stop = is_default_tab_stop(index);
                index += 1;
                is_stop
            });
        }
        self.display_offset = self.display_offset.min(self.scrollback.len());

        // Damage tracking is sized to the screen; a resize repaints everything,
        // so drop any pending scroll op (it points at the old rows).
        self.line_damage = vec![LineBounds::undamaged(cols); rows];
        self.scroll = None;
        self.mark_fully_damaged();
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// Whether bracketed-paste mode (DEC ?2004) is enabled. The input encoder
    /// reads this to decide whether to wrap pasted text in markers.
    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    // ---- input encoding (#11) ------------------------------------------------

    /// Encode a key event to bytes using every mode that decides one: the active
    /// cursor-key mode (DECCKM), application keypad, the kitty keyboard-protocol
    /// flags and `modifyOtherKeys` level 2. `encode_key` consults all four,
    /// and asks kitty first.
    pub fn encode_key(&self, ev: KeyEvent) -> Option<Vec<u8>> {
        encode_key(
            &ev,
            self.app_cursor_keys,
            self.application_keypad,
            self.kitty_flags,
            self.modify_other_keys_2,
        )
    }

    /// Encode a mouse event using the active tracking mode + encoding. `None`
    /// when reporting is off or the event is filtered by the mode.
    pub fn encode_mouse(&self, ev: MouseEvent) -> Option<Vec<u8>> {
        encode_mouse(&ev, self.mouse_protocol, self.mouse_encoding)
    }

    /// Encode pasted text, wrapping it in bracketed-paste markers when ?2004 is
    /// on.
    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        encode_paste(text, self.bracketed_paste)
    }

    /// Encode a focus change (`CSI I`/`CSI O`), or `None` when focus reporting
    /// (?1004) is off.
    pub fn encode_focus(&self, focused: bool) -> Option<Vec<u8>> {
        encode_focus(focused, self.focus_events)
    }

    // ---- cursor / scroll primitives ------------------------------------------

    /// An ordinary line feed — `LF`/`VT`/`FF`, `IND` and `NEL`, none of which serves a wrap.
    /// Clears the deferred wrap, as every acting positioner does (see
    /// [`Cursor::pending_wrap`]); the wrap machinery drives [`Term::linefeed_inner`] directly
    /// and consumes the flag instead (#848).
    fn linefeed(&mut self) {
        self.linefeed_inner(false);
        self.cursor.pending_wrap = false;
    }

    /// Move down one line: at the bottom margin scroll the region instead, below the region
    /// just descend. Column unchanged. `serves_wrap` says the auto-wrap asked for it, which
    /// exempts the region's bottom seam in `shift_region` — the blank landing there is where
    /// the wrapped text goes (xterm.js threads the same fact through `BufferService.scroll`).
    fn linefeed_inner(&mut self, serves_wrap: bool) {
        // New-line mode (LNM ?20): a line feed also returns to column 0 (#71).
        if self.newline_mode {
            self.carriage_return();
        }
        if self.cursor.row == self.scroll_bottom {
            // A top-anchored primary-screen scroll pushes the evicted top line
            // into scrollback history.
            if self.scroll_top == 0 && !self.on_alt {
                // A top-anchored primary scroll accrues scrollback; only a full-screen one takes the
                // O(1) ring handshake, a sub-region copies and region-scrolls (ADR-0009).
                let evicted = if self.scroll_bottom == self.grid.rows() - 1 {
                    // Full-screen hot path: move the evicted top row out, install
                    // a recycled blank as the new bottom (zero-alloc steady state).
                    let blank = self
                        .recycled_row
                        .take()
                        .unwrap_or_else(|| Row::from_cells(Vec::with_capacity(self.grid.cols())));
                    let evicted = self.grid.scroll_up_recycle(blank);
                    // The one row-shift not routed through `shift_region`, so it records its own scroll op;
                    // it owes no seam clear — the top row enters scrollback and the bottom is the wrap's.
                    self.record_scroll(self.scroll_top, self.scroll_bottom, 1);
                    evicted
                } else {
                    // Top-anchored sub-region: copy row 0, then region-scroll `[0..=scroll_bottom]`. The
                    // fixed rows below keep their grid position while scrollback grows, so their absolute
                    // index shifts +1: re-anchor the content-tracking anchors and invalidate the highlights
                    // (#449, #108).
                    let below = self.scrollback.len() + self.scroll_bottom + 1;
                    self.selection_shift_below_margin(below);
                    self.markers_shift_below_margin(below);
                    self.tracked_shift_below_margin(below);
                    self.invalidate_search_highlights();
                    let evicted = self.grid.row_owned(0);
                    self.shift_region(
                        self.scroll_top,
                        self.scroll_bottom,
                        false,
                        true,
                        serves_wrap,
                    );
                    evicted
                };
                self.scrollback.push_back(evicted);
                // Follow-bottom = stay: if the user is scrolled up, bump the
                // offset so the same lines stay in view instead of being yanked
                // to the bottom.
                if self.display_offset > 0 {
                    self.display_offset = (self.display_offset + 1).min(self.scrollback.len());
                }
                // Cap: evict the oldest line past the limit. The view is anchored
                // to history, so dropping the front shifts the offset down too
                // (xterm.js trims ybase and ydisp together) — also keeps the
                // offset within `[0, len]`. The evicted row is parked for reuse.
                if self.scrollback.len() > self.scrollback_limit {
                    self.recycled_row = self.scrollback.pop_front();
                    self.lines_left_the_front(1);
                    if self.display_offset > 0 {
                        // Scrolled up: evicting the oldest line advanced the
                        // viewport, so it must be repainted (the "frozen while
                        // scrolled" rule does not apply when the view itself moved).
                        self.display_offset -= 1;
                        self.mark_fully_damaged();
                    }
                }
            } else {
                // Region (top margin > 0) or alt-screen scroll: the evicted line
                // does NOT enter scrollback, so content moves *within* the screen
                // and absolute indices in the region shift. Rotate the selection
                // up so it follows; an endpoint on the dropped line clears it.
                let base = self.scrollback.len();
                self.selection_rotate_region(
                    base + self.scroll_top,
                    base + self.scroll_bottom,
                    true,
                );
                // Rotate the active buffer's markers with the content (#187):
                // per-buffer storage (#186) scopes them, so an alt scroll rotates
                // *alt* marks and leaves the frozen primary list untouched — no
                // guard needed. `markers_rotate_region` routes via `markers_mut`.
                self.markers_rotate_region(base + self.scroll_top, base + self.scroll_bottom, true);
                self.tracked_rotate_region(base + self.scroll_top, base + self.scroll_bottom, true);
                self.invalidate_search_highlights();
                self.shift_region(
                    self.scroll_top,
                    self.scroll_bottom,
                    false,
                    false,
                    serves_wrap,
                );
            }
        } else if self.cursor.row + 1 < self.grid.rows() {
            self.cursor.row += 1;
        }
    }

    /// DECSTBM (CSI r): set the top/bottom scroll margins (1-based inclusive).
    /// An invalid region (top ≥ bottom) is ignored.
    fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let bottom = bottom.min(self.grid.rows());
        if top >= bottom {
            return;
        }
        self.scroll_top = top - 1;
        self.scroll_bottom = bottom - 1;
        self.goto(0, 0); // DECSTBM homes the cursor (absolute)
    }

    // ---- alt screen (DEC 1049) -----------------------------------------------

    /// Save the cursor into the alt-screen slot — `?1048` set, and the first half of `?1049`
    /// enter (#72).
    fn save_alt_cursor(&mut self) {
        self.saved_cursor = self.cursor;
    }

    /// Restore the cursor from the alt-screen slot — `?1048` reset, and the
    /// second half of `?1049` leave. DECTCEM visibility is a standalone mode, not
    /// part of the save, so preserve it across the restore (#38/#72).
    fn restore_alt_cursor(&mut self) {
        let visible = self.cursor.visible;
        self.cursor = self.saved_cursor;
        self.cursor.visible = visible;
        self.settle_restored_wrap();
    }

    /// Switch to the (cleared) alternate buffer without touching the cursor —
    /// `?47`/`?1047` set, and the second half of `?1049` enter (#72).
    fn switch_to_alt(&mut self) {
        if self.on_alt {
            return;
        }
        // The pulled index reports the ACTIVE buffer, so a swap changes what the
        // consumer's held answer even describes — with no line having moved (#490).
        // Gated on a marker existing on either side, since an empty index is already
        // correct for both buffers.
        if !self.normal_markers.is_empty() || !self.alt_markers.is_empty() {
            self.bump_marker_epoch();
        }
        std::mem::swap(&mut self.grid, &mut self.alt_grid);
        self.grid.clear();
        self.swap_kitty_keyboard();
        self.on_alt = true;
        self.display_offset = 0; // the alt screen has no scrollback to view
        self.selection = None; // a selection cannot survive a screen swap
        self.invalidate_search_highlights(); // matches index the primary buffer
        self.mark_fully_damaged();
    }

    /// Switch back to the primary buffer without touching the cursor —
    /// `?47`/`?1047` reset, and the first half of `?1049` leave (#72).
    fn switch_to_primary(&mut self) {
        if !self.on_alt {
            return;
        }
        // Dispose the alt buffer's markers on leave, as xterm's `activateNormalBuffer` →
        // `clearAllMarkers` does (#177, #187).
        for m in self.alt_markers.drain(..) {
            self.events.push(TermEvent::MarkerDisposed(m.id));
        }
        // Alt-scoped tracked points die the same way, unannounced — `tracked_point` answers
        // `None` (#691). The epoch bump asks only `normal_markers`: the drain above emptied the
        // alt list, and what can still be stale is the primary population (#490).
        if !self.normal_markers.is_empty() {
            self.bump_marker_epoch();
        }
        self.alt_tracked.clear();
        std::mem::swap(&mut self.grid, &mut self.alt_grid);
        self.swap_kitty_keyboard();
        self.on_alt = false;
        self.display_offset = 0; // return to the primary at its bottom
        self.selection = None; // a selection cannot survive a screen swap
        self.invalidate_search_highlights(); // matches index the swapped-out buffer
        self.mark_fully_damaged();
    }

    /// Exchange the active screen's kitty keyboard flags and stack with the other screen's.
    fn swap_kitty_keyboard(&mut self) {
        std::mem::swap(&mut self.kitty_flags, &mut self.kitty_flags_inactive);
        std::mem::swap(&mut self.kitty_stack, &mut self.kitty_stack_inactive);
    }

    /// Enter the alternate screen: save the cursor, swap in the other grid, and
    /// clear it.
    fn enter_alt_screen(&mut self) {
        if self.on_alt {
            return;
        }
        self.save_alt_cursor();
        self.switch_to_alt();
    }

    /// Leave the alternate screen: swap the primary grid back in and restore the
    /// saved cursor.
    fn leave_alt_screen(&mut self) {
        if !self.on_alt {
            return;
        }
        self.switch_to_primary();
        self.restore_alt_cursor();
    }

    /// RI (ESC M): move up one line. At the top margin, scroll the region down
    /// instead.
    fn reverse_index(&mut self) {
        if self.cursor.row == self.scroll_top {
            // RI never enters scrollback; the region scrolls down within the
            // screen, so absolute indices in it shift down. Rotate the selection.
            let base = self.scrollback.len();
            self.selection_rotate_region(base + self.scroll_top, base + self.scroll_bottom, false);
            // Rotate the active buffer's markers (#187) — alt-scoped on the alt
            // screen, so no guard (see `linefeed`).
            self.markers_rotate_region(base + self.scroll_top, base + self.scroll_bottom, false);
            self.tracked_rotate_region(base + self.scroll_top, base + self.scroll_bottom, false);
            self.invalidate_search_highlights();
            self.shift_region(self.scroll_top, self.scroll_bottom, true, false, false);
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
        }
        // Cleared in both branches: both are the verb acting, and xterm and ghostty clear
        // unconditionally here too (see [`Cursor::pending_wrap`]).
        self.cursor.pending_wrap = false;
    }

    // ---- cursor save/restore (DECSC / DECRC) ---------------------------------

    /// DECSC (ESC 7): save the cursor position, pen, pending-wrap, and origin
    /// mode. Visibility is not saved (DECTCEM is separate).
    fn save_cursor(&mut self) {
        self.decsc = SavedCursor {
            row: self.cursor.row,
            col: self.cursor.col,
            pen: self.cursor.pen,
            pending_wrap: self.cursor.pending_wrap,
            origin_mode: self.origin_mode,
            charsets: self.charsets,
            gl: self.gl,
        };
    }

    /// DECRC (ESC 8): restore what DECSC saved. Origin mode is restored (per
    /// ADR-0004); visibility is left as-is. The position is clamped to the
    /// current screen in case it shrank since the save.
    fn restore_cursor(&mut self) {
        let s = self.decsc;
        self.cursor.row = s.row.min(self.grid.rows() - 1);
        self.cursor.col = s.col.min(self.grid.cols() - 1);
        self.cursor.pen = s.pen;
        self.cursor.pending_wrap = s.pending_wrap;
        self.origin_mode = s.origin_mode;
        self.charsets = s.charsets;
        self.gl = s.gl;
        self.settle_restored_wrap();
    }

    /// A restored deferred wrap is only meaningful at the last column; anywhere else the
    /// logical position is representable and the cursor takes it — `resize`'s translation,
    /// applied to the saved slots at restore (#848, `docs/map/territory/cursor-position.md`).
    fn settle_restored_wrap(&mut self) {
        if self.cursor.pending_wrap && self.cursor.col + 1 < self.grid.cols() {
            self.cursor.pending_wrap = false;
            self.cursor.col += 1;
        }
    }

    /// RIS (ESC c) — full reset to the power-on state (#53): rebuild `Term` from the
    /// constructor at the current dimensions and scrollback cap, and signal a full repaint.
    /// Carried across: the consumer-bound `replies`/`events`, the embedder's
    /// `word_separators`, and the tracked-point and marker id counters and marker epoch. The
    /// title stacks and retained strings are dropped and the palette is not announced (#823,
    /// #835). Which survives and why: `docs/map/invariant/ris-keeps-configuration-drops-coordinates.md`.
    fn full_reset(&mut self) {
        let replies = std::mem::take(&mut self.replies);
        let mut events = std::mem::take(&mut self.events);
        // Announce every marker's disposal; the events survive the reset below (#118).
        events.extend(
            self.normal_markers
                .iter()
                .chain(&self.alt_markers)
                .map(|m| TermEvent::MarkerDisposed(m.id)),
        );
        let (cols, rows) = (self.grid.cols(), self.grid.rows());
        // Consumer policy, not terminal state (#545).
        let word_separators = std::mem::take(&mut self.word_separators);
        // Tracked points die, but their id counter rides across: they have no disposal event,
        // so a reissued id would answer a stale ask with another point (#691).
        let next_tracked_id = self.next_tracked_id;
        // The marker epoch rides across and then moves, so a consumer re-pulls (#490).
        let marker_epoch = self.marker_epoch;
        // Marker ids ride across so a stale `MarkerDisposed` cannot drop a reissued id.
        let next_marker_id = self.next_marker_id;
        *self = Term::with_scrollback(cols, rows, self.scrollback_limit);
        self.replies = replies;
        self.events = events;
        self.word_separators = word_separators;
        self.next_tracked_id = next_tracked_id;
        self.next_marker_id = next_marker_id;
        self.marker_epoch = marker_epoch;
        self.bump_marker_epoch();
        self.mark_fully_damaged();
    }

    /// DECSTR (CSI ! p) — soft reset (#53): return a defined subset of modes to their
    /// defaults without touching screen content, scrollback, the cursor position or mouse
    /// and focus reporting. Autowrap returns to on, the xterm default. The pen resets; the
    /// palette is not announced, as for RIS.
    fn soft_reset(&mut self) {
        self.cursor.visible = true;
        self.cursor.pen = Pen::default();
        self.cursor.shape = None; // the application's caret shape and blink mode (#927)
        self.cursor.blink = false;
        self.scroll_top = 0;
        self.scroll_bottom = self.grid.rows() - 1;
        self.origin_mode = false;
        self.app_cursor_keys = false;
        self.bracketed_paste = false;
        self.modify_other_keys_2 = false; // xterm clears the modify resources on DECSTR too (#890)
        self.grapheme_clustering = false; // ?2027 back to the wcwidth-compat default (#295)
        self.autowrap = true; // xterm default is ON (not the VT100 "off")
        self.insert_mode = false;
        self.charsets = [Charset::Ascii; 4];
        self.gl = 0;
        self.decsc = SavedCursor::default();
    }

    fn carriage_return(&mut self) {
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
    }

    /// DECSCUSR (CSI Ps SP q): set the caret shape + blink (#89). 1/2 =
    /// blinking/steady block; 3/4 = blinking/steady underline; 5/6 =
    /// blinking/steady bar (odd = blink). 0 clears the shape to `None` — the
    /// consumer's default shape (#927) — and turns the blink mode off. An unknown
    /// param leaves the style unchanged.
    fn set_cursor_style(&mut self, param: u16) {
        let (shape, blink) = match param {
            0 => (None, false),
            1 => (Some(CursorShape::Block), true),
            2 => (Some(CursorShape::Block), false),
            3 => (Some(CursorShape::Underline), true),
            4 => (Some(CursorShape::Underline), false),
            5 => (Some(CursorShape::Bar), true),
            6 => (Some(CursorShape::Bar), false),
            _ => return,
        };
        self.cursor.shape = shape;
        self.cursor.blink = blink;
    }

    /// Backspace (BS, 0x08): one step back.
    fn backspace(&mut self) {
        self.step_back();
    }

    /// One step back, shared by `BS` and `CSI D` (#873; maintainer's call, 2026-09-08). With
    /// reverse wraparound (`?45` and `?7h`) a step at column 0 of a soft-wrapped row moves to
    /// the last column of the previous row; a hard line does not reverse. The references:
    /// `docs/agents/reference-facts.md`, reverse wraparound.
    fn step_back(&mut self) {
        // A parked cursor is logically one past its column, so under `?45` + `?7h` the park is
        // spent as the first unit of the move (#80). Under `?7l` it is spent by moving, 3-0.
        if self.reverse_wraparound && self.autowrap && self.cursor.pending_wrap {
            self.cursor.pending_wrap = false;
            return;
        }
        self.cursor.pending_wrap = false;
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
            return;
        }
        // The walk needs autowrap too, for the same reason as the spend.
        if self.reverse_wraparound
            && self.autowrap
            && self.cursor.row > self.scroll_top
            && self.cursor.row <= self.scroll_bottom
        {
            let prev = self.cursor.row - 1;
            let last = self.grid.cols() - 1;
            if self.grid.row_ref(prev).is_wrapped() {
                // The wrap link survives the walk: the rows still hold one logical line, and every
                // reader asks this flag. xterm.js clears it; xterm and ghostty do not.
                self.cursor.row = prev;
                self.cursor.col = last;
            }
        }
    }

    /// Auto-wrap at end of line: line-feed then return to column 0.
    fn wrapline(&mut self) {
        self.linefeed_inner(true);
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
    }

    // ---- tab stops (HT / HTS / TBC) ------------------------------------------

    /// HT: advance to the next set tab stop, or the last column if none remain (no wrap).
    /// The deferred wrap is cleared only when the walk moves: at the last column this verb
    /// changes nothing and leaves the flag armed (#848; the reference rows are in
    /// `docs/agents/reference-facts.md`).
    fn put_tab(&mut self) {
        let cols = self.grid.cols();
        let mut col = self.cursor.col;
        while col + 1 < cols {
            col += 1;
            if self.tabs[col] {
                break;
            }
        }
        if col != self.cursor.col {
            self.cursor.col = col;
            self.cursor.pending_wrap = false;
        }
    }

    /// CHT (CSI Ps I): [`Term::put_tab`] repeated `n` times, stopping at the first
    /// one that does not move — so the deferred-wrap rule is `put_tab`'s, and the
    /// work is bounded by the row rather than by the parameter (#898). The break
    /// is defensive only: a `put_tab` that did not move will not move on a repeat,
    /// so removing it changes no outcome and no test can redden it.
    fn put_forward_tabs(&mut self, n: usize) {
        for _ in 0..n {
            let col = self.cursor.col;
            self.put_tab();
            if self.cursor.col == col {
                break;
            }
        }
    }

    /// CBT (CSI Ps Z): step back `n` tab stops, or to column one if fewer remain — the mirror
    /// of [`Term::put_tab`] over the same table, repeating the walk `n` times. Clamps within
    /// the line and clears the deferred wrap, the second a deliberate divergence from all four
    /// references (#826, `docs/map/territory/cursor-position.md`).
    fn put_back_tab(&mut self, n: usize) {
        let mut col = self.cursor.col;
        for _ in 0..n {
            if col == 0 {
                break;
            }
            while col > 0 {
                col -= 1;
                if self.tabs[col] {
                    break;
                }
            }
        }
        self.cursor.col = col;
        self.cursor.pending_wrap = false;
    }

    /// HTS (ESC H): set a tab stop at the cursor column.
    fn set_tab_stop(&mut self) {
        let col = self.cursor.col;
        self.tabs[col] = true;
    }

    /// TBC (CSI g): clear the tab stop at the cursor (mode 0) or all stops
    /// (mode 3).
    fn clear_tab_stop(&mut self, mode: u16) {
        match mode {
            0 => {
                let col = self.cursor.col;
                self.tabs[col] = false;
            }
            3 => self.tabs.iter_mut().for_each(|t| *t = false),
            _ => {}
        }
    }

    // ---- printing ------------------------------------------------------------

    /// The extended attributes the pen stamps onto a cell it writes: the open OSC 8 link and
    /// a non-default underline colour, the colour only while `UNDERLINE` is set (#520). Which
    /// sites build from here and which must not: `docs/map/territory/pen.md`.
    fn pen_ext_attrs(&self) -> ExtAttrs {
        let ucolor = self.cursor.pen.underline_color;
        let armed =
            ucolor != Color::Default && self.cursor.pen.flags.contains(CellFlags::UNDERLINE);
        ExtAttrs::from_pen(self.current_link.clone(), armed.then_some(ucolor))
    }

    /// Free a cell that has stopped being part of a glyph — the no-orphan repair an overwrite,
    /// erase or row-shift owes when it destroys one half of a width-2 glyph, plus the spacer a
    /// mode-2027 demotion no longer needs. It damages the cell, which is the half every site
    /// used to forget (#530).
    ///
    /// The cell becomes a blank carrying the pen's background only (xterm.js's
    /// `_eraseAttrData()`), not the whole pen and not the cell's own attributes — maintainer's
    /// call, 2026-07-24. With DECSCA the freed cell would lose its protection; DECSCA is not
    /// implemented. See `docs/map/territory/wide-glyph.md`.
    fn free_cell(&mut self, row: usize, col: usize) {
        let bg = self.cursor.pen.bg;
        let cell = self.grid.cell_mut(row, col);
        cell.reset();
        cell.set_bg(bg);
        // As in `clear_cells`: the bits are gone, so release what they gated (#628).
        self.grid.row_mut(row).purge_side_maps(col..col + 1);
        self.damage_span(row, col, col);
    }

    /// Will the next `wrapline()` actually reach another row? It does when the cursor is at the
    /// region's bottom or has a row below it; parked below a DECSTBM region on the last row it
    /// stays put. Both wide-at-boundary paths ask first, since both destroy content on the
    /// assumption that the row changes. Mirrors `linefeed`'s own condition.
    fn wrapline_advances(&self) -> bool {
        self.cursor.row == self.scroll_bottom || self.cursor.row + 1 < self.grid.rows()
    }

    /// Blank the last column as the soft-wrap artefact it is, when a width-2 glyph could not
    /// fit there (#528): a pen blank marked as a leading spacer, so text readers skip it, and
    /// the row marked soft-wrapped. Shared by `write_glyph`'s wide-at-boundary wrap and
    /// `relocate_cluster_wide`. Why written rather than flagged:
    /// `docs/map/territory/wide-glyph.md`.
    fn vacate_for_wrap(&mut self, row: usize, col: usize) {
        // An overwrite like any other: a spacer here strands its lead unless the lead is freed.
        if col > 0 && self.grid.cell(row, col).is_wide_spacer() {
            self.free_cell(row, col - 1);
        }
        let mut vacated = self.cursor.pen.cell(' ');
        vacated.set_leading_spacer();
        *self.grid.cell_mut(row, col) = vacated;
        self.begin_wrap(row);
        let ext = self.pen_ext_attrs();
        self.grid.row_mut(row).set_ext_attrs(col, ext);
        // The cell changed, so it is damaged (ADR-0003); the repaired lead is `free_cell`'s.
        self.damage_span(row, col, col);
    }

    /// Place one already-charset-translated scalar: join it to the previous cluster under mode
    /// 2027, attach it as a combining mark, or write it as a new glyph. Split out of `print` so
    /// `REP` re-enters below the VT52 intercept and the charset translation (#825). The three
    /// arms that place content are exactly the sites that arm [`Term::repeat_anchor`].
    fn place_grapheme(&mut self, c: char) {
        // Grapheme-cluster mode (DEC ?2027, #295): if `c` extends the previous cell's cluster,
        // join it there instead of placing a new cell. OFF → the per-char (wcwidth) path below.
        if self.grapheme_clustering
            && let Some(at) = self.try_grapheme_join(c)
        {
            self.repeat_anchor = Some(at);
            return;
        }
        match c.width() {
            // Zero-width (combining marks): a zero-width code point is a combining
            // mark — attach it to the previous base glyph rather than dropping it.
            Some(0) => self.repeat_anchor = Some(self.push_combining(c)),
            // `DEL` (0x7F): `vte` prints it, and its width is `None`. It writes no cell, so it clears
            // the anchor.
            None => self.repeat_anchor = None,
            // Coerced to a pair — the only multi-column shape the cell model has (ADR-0025); a width
            // of 3 is unrepresentable (#595, `docs/map/territory/wide-glyph.md`).
            Some(width) => self.repeat_anchor = self.write_glyph(c, width.min(2)),
        }
    }

    /// Write one glyph at the cursor, handling deferred wrap and the wide-char
    /// spacer, then advance the cursor (deferring the wrap if it hits the edge).
    /// Returns where the glyph landed — `(row, col)` of its lead cell — or `None` on
    /// the one path that writes nothing: a width-2 glyph that cannot fit the last column
    /// with autowrap off is dropped. [`Term::repeat_anchor`] is set from this, so a
    /// print that placed no cell must not arm the repeat.
    fn write_glyph(&mut self, c: char, width: usize) -> Option<(usize, usize)> {
        // The caller coerces widths past 2 (#595); asserted where every wide branch assumes it.
        debug_assert!(
            width <= 2,
            "write_glyph({c:?}, {width}) — the cell model represents at most a pair"
        );
        let cols = self.grid.cols();

        // Resolve a deferred last-column wrap before placing the next glyph.
        // The row being left soft-wrapped: mark its last cell so reflow (#7) can
        // tell it from a hard CR/LF line-end.
        if self.cursor.pending_wrap && !self.autowrap {
            // Under `?7l` the park is spent in place rather than wrapping — the only place DECAWM is
            // tested (#869, `docs/map/territory/cursor-position.md`).
            self.cursor.pending_wrap = false;
        }
        if self.cursor.pending_wrap {
            let row = self.cursor.row;
            // Claim the wrap only if there will be a next row to continue into (#540).
            if self.wrapline_advances() {
                self.begin_wrap(row);
            }
            self.wrapline();
        }

        // A width-2 glyph that cannot fit in the last column wraps first — unless
        // autowrap is off, in which case it is dropped (xterm.js `continue`), not
        // squeezed or wrapped.
        if width == 2 && self.cursor.col + 1 >= cols {
            if !self.autowrap {
                return None; // dropped: nothing written, so nothing to repeat
            }
            // …but only if the wrap actually happens: vacating for a wrap that never occurs
            // blanks a column holding a live glyph.
            if self.wrapline_advances() {
                self.vacate_for_wrap(self.cursor.row, cols - 1);
            }
            self.wrapline();
        }

        // Insert mode (IRM): open a `width`-wide gap at the cursor first, shifting
        // the row's tail right (off-edge cells discarded, wide halves repaired),
        // then write into the gap — mirrors xterm.js's insertCells (#64).
        if self.insert_mode {
            self.insert_chars(width);
        }

        let (row, col) = (self.cursor.row, self.cursor.col);

        // Overwriting either half of a pair that wrapped from the row above voids the row above's
        // artefact record (#534) — except an in-place same-width overwrite, ghostty's
        // `if (cell.wide != wide)` escape, which is why this is asked before the write.
        if col <= 1 && self.wrapped_pair_at_row_start(row) && !(col == 0 && width == 2) {
            self.void_wrap_artefact_above(row);
        }

        // Overwriting one half of an existing wide glyph orphans the other —
        // clear it so no stray lead/spacer is left behind.
        let last = col + width - 1;
        if col > 0 && self.grid.cell(row, col).is_wide_spacer() {
            self.free_cell(row, col - 1);
        }
        if last + 1 < cols && self.grid.cell(row, last).is_wide() {
            self.free_cell(row, last + 1);
        }

        let mut cell = self.cursor.pen.cell(c);
        if width == 2 {
            cell.insert_flags(CellFlags::WIDE_CHAR);
        }
        *self.grid.cell_mut(row, col) = cell;
        // Stamp the pen's extended attrs — the open hyperlink (#26/#46) and a non-default
        // underline colour (#520) — into the row's side maps.
        let ext = self.pen_ext_attrs();
        // `.clone()` is a refcount bump on the link rider (#628); the spacer stamps the same value.
        self.grid.row_mut(row).set_ext_attrs(col, ext.clone());

        // The trailing column of a wide glyph carries a distinct spacer marker —
        // and the same link + underline colour, so a hover/selection/underline over
        // either half agrees.
        if width == 2 && col + 1 < cols {
            let mut spacer = self.cursor.pen.cell(' ');
            spacer.insert_flags(CellFlags::WIDE_CHAR_SPACER);
            *self.grid.cell_mut(row, col + 1) = spacer;
            self.grid.row_mut(row).set_ext_attrs(col + 1, ext);
        }

        // Record damage for the cell(s) just written.
        self.damage_span(row, col, col + width - 1);

        // Advance. Reaching/passing the last column sets pending-wrap instead of
        // wrapping eagerly — the cursor parks on the last column.
        let new_col = col + width;
        if new_col >= cols {
            self.cursor.col = cols - 1;
            // The park is taken whatever the mode says (#869); under `?7l` the consume site above
            // spends it in place (#63), and re-enabling the mode first makes it wrap, as all four
            // references do.
            self.cursor.pending_wrap = true;
        } else {
            self.cursor.col = new_col;
        }
        Some((row, col))
    }

    /// The column, on the cursor's row, of the cluster the cursor last printed into, or `None`
    /// when nothing precedes it on this row — shared by the combining-mark attach point and the
    /// mode-2027 join point.
    ///
    /// Parked ([`Cursor::pending_wrap`]), the cluster is at the cursor; otherwise one column
    /// left. Either way, once more left over a `WIDE_CHAR_SPACER` to reach its lead. The parked
    /// reading holds under `?7l` too since #869 armed the park unconditionally (#865 had worked
    /// around the old arming here). After a bare cursor move the references give four answers
    /// and this keeps the plurality's (reference rows in `docs/agents/reference-facts.md`). `REP`
    /// does not use this: it needs where a print wrote, even after the cursor moved.
    fn cursor_cluster_col(&self) -> Option<usize> {
        let row = self.cursor.row;
        // The pinned case. The wide arm is the same cell reached from its spacer: a
        // pair that fills the row leaves the cursor on the trailing spacer, one past
        // the anchored lead.
        let col = if self.cursor.pending_wrap {
            self.cursor.col
        } else if self.cursor.col == 0 {
            return None;
        } else {
            self.cursor.col - 1
        };
        Some(if self.grid.cell(row, col).is_wide_spacer() {
            col.saturating_sub(1)
        } else {
            col
        })
    }

    /// The text the cell at `(row, col)` holds, in print order: its base glyph followed by any
    /// combining marks — one grapheme cluster, what a single print produced. A `String`
    /// because both callers want text. Off the per-scalar path since #867.
    fn cluster_text(&self, row: usize, col: usize) -> String {
        let mut out = String::new();
        out.push(self.grid.cell(row, col).c());
        if let Some(marks) = self.grid.row_ref(row).combining_at(col) {
            out.extend(marks.iter().copied());
        }
        out
    }

    /// REP (CSI Ps b): repeat the preceding grapheme `count` times (#825). The caller owns the
    /// armed check (the `'b'` arm of `csi_dispatch`).
    ///
    /// The repeat re-enters the print path, so wrap, wide pairing, mode 2027, the pen, insert
    /// mode and the scroll region apply as for typed text. What is repeated is the cluster at
    /// [`Term::repeat_anchor`], read off the cell — a product judgement where the references
    /// give three answers, the maintainer's (2026-09-07). The count is bounded twice: a progress
    /// guard that stops once an iteration places no new cell (the load-bearing one), and a cap
    /// of a buffer's worth of cells. Why each: `docs/architecture.md` § Hidden VT state.
    fn repeat_last(&mut self, count: usize) {
        let Some((row, col)) = self.repeat_anchor else {
            return;
        };
        // Snapshot once: the repeats move the anchor, so re-reading it per iteration
        // would repeat the growing run rather than the grapheme.
        let cluster = self.cluster_text(row, col);
        let cap = (self.scrollback_limit + self.grid.rows()).saturating_mul(self.grid.cols());
        for _ in 0..count.min(cap.max(1)) {
            let before = self.repeat_anchor;
            for c in cluster.chars() {
                self.place_grapheme(c);
            }
            if self.repeat_anchor == before {
                break; // placed no new cell — see "the progress guard" above
            }
        }
    }

    /// Attach a combining mark (width-0 code point) to the cluster the cursor last printed
    /// into ([`Term::cursor_cluster_col`]), in the row's combining map.
    fn push_combining(&mut self, c: char) -> (usize, usize) {
        let row = self.cursor.row;
        // A mark with no base attaches to column 0; the join path declines that case instead.
        let col = self.cursor_cluster_col().unwrap_or(0);
        // Append the mark to the row's combining map at this column (setting the
        // cell's combining bit). No global pool — the cluster rides the row.
        self.grid.row_mut(row).push_combining(col, c);
        self.damage_span(row, col, col);
        (row, col)
    }

    /// Mode 2027 (#295): if `c` extends the previous cell's grapheme cluster (UAX #29), append
    /// it to that cell's side table — no new cell, no cursor advance — and return where the
    /// cluster ended up; otherwise `None`. O(1) in the cluster's length (#867): the break
    /// question reads a bounded tail of the side table, and the width oracle is asked only
    /// when [`crate::grapheme::width_may_change`] says the answer can have moved.
    fn try_grapheme_join(&mut self, c: char) -> Option<(usize, usize)> {
        let row = self.cursor.row;
        // Locate the previous cluster's base cell; `None` means nothing precedes it on
        // this row, so there is nothing to extend.
        let Some(col) = self.cursor_cluster_col() else {
            return None; // nothing precedes on this row
        };
        // The cell is read, never rebuilt into a string (#867): both the break question and the
        // width question are answered from the base scalar, the side table's length, and — only
        // when the width can actually move — the cluster text itself.
        let base = self.grid.cell(row, col).c();
        let base_is_wide = self.grid.cell(row, col).is_wide();
        let (joins, first_join) = {
            let marks = self.grid.row_ref(row).combining_at(col).unwrap_or(&[]);
            (
                crate::grapheme::joins_cluster(base, marks, c),
                marks.is_empty(),
            )
        };
        if !joins {
            return None;
        }
        // Width promotion: a flag's second regional indicator, or a text-base + VS16, grows the
        // cluster to width 2. `UnicodeWidthStr` over the whole cluster remains the authority for
        // that; `width_may_change` only decides whether it has to be asked, so a cluster that
        // keeps growing stops paying for an answer that cannot have moved. Read BEFORE the push,
        // because `cluster_text` reads the side table this join is about to extend.
        let cluster_w = if crate::grapheme::width_may_change(c, first_join, base_is_wide) {
            let mut prev = self.cluster_text(row, col);
            prev.push(c);
            UnicodeWidthStr::width(prev.as_str())
        } else if base_is_wide {
            2
        } else {
            1
        };
        // Join: ride the side-table (no new cell).
        self.grid.row_mut(row).push_combining(col, c);
        // Where the cluster ends up, which is not always where it was joined: a promotion at
        // the last column relocates it to the next row (#303), and the anchor has to follow or
        // it names a column `vacate_for_wrap` just blanked.
        let mut at = (row, col);
        if cluster_w == 2 && !self.grid.cell(row, col).is_wide() {
            at = self.promote_cluster_to_wide(row, col);
        } else if cluster_w == 1 && self.grid.cell(row, col).is_wide() {
            // The mirror case: a default-wide emoji + VS15 (text selector) shrinks to width 1.
            self.demote_cluster_to_narrow(row, col);
        }
        self.damage_span(row, col, col);
        Some(at)
    }

    /// Shrink a wide cluster cell back to a single-width cell (#295): a default-wide emoji joined by
    /// VS15 (U+FE0E, the text selector) requests text presentation → width 1. Remove `WIDE_CHAR`,
    /// free the spacer, and back the cursor up over it (the inverse of `promote_cluster_to_wide`).
    fn demote_cluster_to_narrow(&mut self, row: usize, col: usize) {
        let cols = self.grid.cols();
        self.grid
            .cell_mut(row, col)
            .remove_flags(CellFlags::WIDE_CHAR);
        if col + 1 < cols {
            self.free_cell(row, col + 1); // free the now-unused spacer
        }
        // The cluster shrank 2→1: the cursor sat just past the wide cell (col+2, or pending-wrap on
        // the last column); it now sits just past the single-width cell at col+1.
        self.cursor.pending_wrap = false;
        self.cursor.col = (col + 1).min(cols - 1);
        self.damage_span(row, col, (col + 1).min(cols - 1));
    }

    /// Widen a narrow base cell to a double-width cluster in place (#295): set `WIDE_CHAR`, write
    /// its spacer, and step the cursor over it. Only reached when a joining scalar (flag's 2nd RI,
    /// VS16) promotes the cluster to width 2. A base pinned at the last column has no room for a
    /// spacer — relocation is a later step; until then it stays narrow (rare, renders single-width).
    fn promote_cluster_to_wide(&mut self, row: usize, col: usize) -> (usize, usize) {
        let cols = self.grid.cols();
        if col + 1 >= cols {
            // No spacer room at the last column: relocate the whole cluster to the next line as a
            // wide cell (the row soft-wraps), mirroring write_glyph's wide-at-boundary wrap (#303).
            return self.relocate_cluster_wide(row, col);
        }
        // A spacer written over a wide glyph's lead orphans its far half; free it, as
        // `write_glyph` does.
        if self.grid.cell(row, col + 1).is_wide() && col + 2 < cols {
            self.free_cell(row, col + 2);
        }
        self.grid
            .cell_mut(row, col)
            .insert_flags(CellFlags::WIDE_CHAR);
        // The spacer takes the lead's extended attributes, not the pen's (#521, ADR-0025 D4).
        let ext = self.grid.row_ref(row).ext_attrs_at(col);
        // …and the lead's underline style, carried on the packed cell (#829).
        let lead_style = self.grid.cell(row, col).underline_style();
        let mut spacer = self.cursor.pen.cell(' ');
        spacer.insert_flags(CellFlags::WIDE_CHAR_SPACER);
        spacer.set_underline_style(lead_style);
        *self.grid.cell_mut(row, col + 1) = spacer;
        self.grid.row_mut(row).set_ext_attrs(col + 1, ext);
        // The cursor sat at col+1 (just past the narrow base); move it over the new spacer, applying
        // the same last-column pending-wrap rule as a wide write.
        let new_col = col + 2;
        if new_col >= cols {
            self.cursor.col = cols - 1;
            self.cursor.pending_wrap = true;
        } else {
            self.cursor.col = new_col;
        }
        self.damage_span(row, col, col + 1);
        // Promoted in place: the lead did not move.
        (row, col)
    }

    /// Relocate a last-column narrow cluster to the next line as a wide cell (#303): its base
    /// and marks move to `(next_row, 0..=1)` and the vacated column becomes the soft-wrap
    /// artefact, as `write_glyph` wraps a wide glyph that cannot fit. With autowrap off, or no
    /// next row, it stays narrow. The destination owes the no-orphan repair:
    /// `docs/map/territory/wide-glyph.md`.
    fn relocate_cluster_wide(&mut self, row: usize, col: usize) -> (usize, usize) {
        let cols = self.grid.cols();
        if cols < 2 || !self.autowrap || !self.wrapline_advances() {
            // Nowhere to place a wide cell; with no next row it would overwrite this row's 0-1.
            return (row, col);
        }
        // Capture the base, its marks and its extended attributes before vacating: `wrapline()`
        // may scroll, after which the source row is a different `Row` (#521).
        let base = *self.grid.cell(row, col);
        let marks: Vec<char> = self
            .combining_at(row, col)
            .map(<[char]>::to_vec)
            .unwrap_or_default();
        let ext = self.grid.row_ref(row).ext_attrs_at(col);
        // The same vacate `write_glyph` takes (#528).
        self.vacate_for_wrap(row, col);
        // Advance to the next line (scrolls if at the bottom); cursor lands at col 0.
        self.wrapline();
        let nr = self.cursor.row;
        // The destination is an overwrite: free the far half of a wide glyph the spacer lands
        // on (#529, D4). `2 < cols` is a live bound after an alt-screen truncation (#567).
        // Why this site and not ghostty's reach-back: `docs/map/territory/wide-glyph.md`.
        if 2 < cols && self.grid.cell(nr, 1).is_wide() {
            self.free_cell(nr, 2);
        }
        // Re-place the base as a wide lead + spacer, re-attaching the marks fresh (drop the combining
        // bit so push_combining starts a clean cluster at the new column).
        let mut lead = base;
        lead.set_combined(false);
        lead.insert_flags(CellFlags::WIDE_CHAR);
        *self.grid.cell_mut(nr, 0) = lead;
        for m in marks {
            self.grid.row_mut(nr).push_combining(0, m);
        }
        // Re-attach the extended attributes to both halves: `lead` copied the presence bits but
        // not the map entries.
        self.grid.row_mut(nr).set_ext_attrs(0, ext.clone());
        // …and the underline style from the relocated LEAD, for the reason the sibling site above
        // states (#829, ADR-0025 D4).
        let lead_style = self.grid.cell(nr, 0).underline_style();
        let mut spacer = self.cursor.pen.cell(' ');
        spacer.insert_flags(CellFlags::WIDE_CHAR_SPACER);
        spacer.set_underline_style(lead_style);
        *self.grid.cell_mut(nr, 1) = spacer;
        self.grid.row_mut(nr).set_ext_attrs(1, ext);
        // Cursor just past the wide cell (pending-wrap if it fills a 2-column row).
        if cols <= 2 {
            self.cursor.col = cols - 1;
            self.cursor.pending_wrap = true;
        } else {
            self.cursor.col = 2;
            self.cursor.pending_wrap = false;
        }
        self.damage_span(nr, 0, 1);
        // The cluster's new home. Callers anchor on this, not on the vacated column.
        (nr, 0)
    }

    // ---- cursor movement (CSI A/B/C/D/E/F/G/d/e/H/f) -------------------------

    /// Up `n` rows — CUU, and through it VT52 `ESC A` and CPL — stopping at the top
    /// margin when the cursor is at or below it and at the screen top otherwise.
    fn move_up(&mut self, n: usize) {
        let floor = if self.cursor.row >= self.scroll_top {
            self.scroll_top
        } else {
            0
        };
        self.cursor.row = self.cursor.row.saturating_sub(n).max(floor);
        self.cursor.pending_wrap = false;
    }

    /// Down `n` rows — CUD, and through it VT52 `ESC B` and CNL — stopping at the
    /// bottom margin when the cursor is at or above it and at the screen bottom otherwise.
    fn move_down(&mut self, n: usize) {
        let ceiling = if self.cursor.row <= self.scroll_bottom {
            self.scroll_bottom
        } else {
            self.grid.rows() - 1
        };
        self.cursor.row = (self.cursor.row + n).min(ceiling);
        self.cursor.pending_wrap = false;
    }

    fn move_forward(&mut self, n: usize) {
        self.cursor.col = (self.cursor.col + n).min(self.grid.cols() - 1);
        self.cursor.pending_wrap = false;
    }

    fn move_back(&mut self, n: usize) {
        // Under `?45`, `n` applications of the step `BS` takes (#873,
        // `docs/map/territory/cursor-position.md`); off it, one saturating subtraction.
        if self.reverse_wraparound {
            for _ in 0..n {
                self.step_back();
            }
        } else {
            self.cursor.col = self.cursor.col.saturating_sub(n);
        }
        // Cleared here too, so a zero count still clears, as every positioning verb does.
        self.cursor.pending_wrap = false;
    }

    fn set_col(&mut self, col: usize) {
        self.cursor.col = col.min(self.grid.cols() - 1);
        self.cursor.pending_wrap = false;
    }

    fn set_row(&mut self, row: usize) {
        self.cursor.row = row.min(self.grid.rows() - 1);
        self.cursor.pending_wrap = false;
    }

    fn goto(&mut self, row: usize, col: usize) {
        let (offset, max_row) = self.addressable_rows();
        self.cursor.row = (row + offset).min(max_row);
        self.cursor.col = col.min(self.grid.cols() - 1);
        self.cursor.pending_wrap = false;
    }

    /// The row addressing origin and the last addressable row. Origin mode
    /// addresses rows relative to the scroll region's top margin and clamps to its
    /// bottom; otherwise rows are absolute to the screen.
    fn addressable_rows(&self) -> (usize, usize) {
        if self.origin_mode {
            (self.scroll_top, self.scroll_bottom)
        } else {
            (0, self.grid.rows() - 1)
        }
    }

    /// VPR (CSI Ps e): the current row plus `n`, positioned as CUP positions a row
    /// — bounded by the last addressable row, not by the scroll margin CUD stops at.
    fn vertical_position_relative(&mut self, n: usize) {
        let (_, max_row) = self.addressable_rows();
        self.cursor.row = (self.cursor.row + n).min(max_row);
        self.cursor.pending_wrap = false;
    }

    // ---- erase (CSI J / K) ---------------------------------------------------

    /// Clear cells `from..to` on `row` with Background Color Erase: erased cells carry the
    /// current SGR background only (xterm, alacritty). Callers pass a non-empty range — with
    /// `from == to` the lead at `from - 1` would be freed and its spacer left standing.
    fn clear_cells(&mut self, row: usize, from: usize, to: usize) {
        let cols = self.grid.cols();
        // Erasing either half of a pair that wrapped from the row above voids that row's artefact
        // record (#534); `from <= 1` because erasing column 1 frees the lead too.
        if from <= 1 && to > from && self.wrapped_pair_at_row_start(row) {
            self.void_wrap_artefact_above(row);
        }
        // Don't orphan a wide char straddling the erase boundary.
        if from > 0 && self.grid.cell(row, from).is_wide_spacer() {
            self.free_cell(row, from - 1);
        }
        if to > from && to < cols && self.grid.cell(row, to - 1).is_wide() {
            self.free_cell(row, to);
        }

        let bg = self.cursor.pen.bg;
        for col in from..to {
            let cell = self.grid.cell_mut(row, col);
            cell.reset();
            cell.set_bg(bg);
        }
        // `reset` cleared the presence bits; this releases what they gated (#628).
        self.grid.row_mut(row).purge_side_maps(from..to);
        if to > from {
            self.damage_span(row, from, to - 1);
        }
    }

    /// Mark `row` as soft-wrapping into the next one, and damage the cell the bit rides on.
    ///
    /// The mirror of [`Term::end_wrap`]: the flag lives on the `Row` (#538) and reaches a
    /// consumer only as the last cell's `WRAPLINE`, derived at encode time, so no cell write
    /// carries it and a `Partial` frame would never ship it without this damage (#557).
    fn begin_wrap(&mut self, row: usize) {
        self.grid.row_mut(row).set_wrapped(true);
        let last = self.grid.cols() - 1;
        self.damage_span(row, last, last);
    }

    /// End `row`'s soft wrap, because something just destroyed the content that continued onto
    /// the next row — and damage the cell the bit rides on (#540).
    ///
    /// Which verbs owe this is a per-verb rule, not derivable from the erased range. Each that
    /// does destroys content from the cursor rightward: `EL 0`, `ECH` and `DCH`, plus `EL 2`,
    /// a deliberate divergence from xterm and ghostty. `EL 1`, `ICH` and a reverse-wrap walk
    /// (#873) do not. The table with its reference sites, and why `EL 2` diverges:
    /// `docs/map/territory/soft-wrap.md`.
    fn end_wrap(&mut self, row: usize) {
        self.grid.row_mut(row).set_wrapped(false);
        // The bit rides the wire on the last cell, so that cell is damaged (#540).
        let last = self.grid.cols() - 1;
        self.damage_span(row, last, last);
        // The wrap artefact goes with the wrap (ADR-0025 D3, `docs/map/territory/wide-glyph.md`).
        self.grid.cell_mut(row, last).clear_leading_spacer();
    }

    /// The pair that wrapped into `row` is about to be destroyed or moved, so the artefact record
    /// on the row above is void — drop it. **Call before the mutation.** The record survives only
    /// an in-place same-width overwrite: ghostty gates the same way on the pre-write state, and
    /// alacritty, with no width-unchanged escape, is the outlier (#534). At grid row 0 on the
    /// primary the row above is the last scrollback row. See `docs/map/territory/wide-glyph.md`.
    fn void_wrap_artefact_above(&mut self, row: usize) {
        if row > 0 {
            let last = self.grid.cols() - 1;
            if self.grid.cell(row - 1, last).is_leading_spacer() {
                self.grid.cell_mut(row - 1, last).clear_leading_spacer();
                self.damage_span(row - 1, last, last);
            }
        } else if !self.on_alt
            && let Some(cell) = self.scrollback.back_mut().and_then(|r| r.last_mut())
        {
            cell.clear_leading_spacer();
        }
    }

    /// Is a wide pair standing at columns 0..=1 of `row` — i.e. is there a record for
    /// `void_wrap_artefact_above` to void? A cheap pre-mutation test the four call sites share, so
    /// the rule lives in one place rather than being re-derived per verb (ADR-0025 D2).
    fn wrapped_pair_at_row_start(&self, row: usize) -> bool {
        self.grid.cell(row, 0).is_wide()
    }

    /// Drop a wide-wrap artefact marker that has outlived the wrap it belonged to, without
    /// touching the wrap itself.
    ///
    /// The mirror of the marker clean-up inside `end_wrap`, for the verbs that erase *leftward*:
    /// `EL 1` and `ED 1` correctly leave the wrap alone (the row's tail still flows onward), but
    /// they can still clear the last column, and then the artefact's blank turns into visible
    /// text that a reflow bakes in permanently. Only the marker goes; the wrap is the caller's
    /// business.
    fn drop_artefact_if_erased(&mut self, row: usize, from: usize, to: usize) {
        let last = self.grid.cols() - 1;
        if from <= last && to > last {
            self.grid.cell_mut(row, last).clear_leading_spacer();
        }
    }

    /// Shift `[top..=bottom]` by one line — up unless `down` — and end the wraps the shift
    /// falsified. Every row-shifting verb (IL/DL/SU/SD and the region paths in LF/RI) goes
    /// through here so the repair cannot be forgotten at a call site (ADR-0025 D2).
    ///
    /// Rotating whole rows keeps a wrap's adjacency claim true inside the region; only the two
    /// seams falsify it — `top - 1`, and the row that lost its continuation to the blank.
    /// `evicts_to_scrollback` exempts the top seam and `serves_wrap` the bottom one (#557);
    /// both are caller facts. Derived, not ported — no reference implements this rule — and
    /// sound only without DECSLRM. See ADR-0025.
    fn shift_region(
        &mut self,
        top: usize,
        bottom: usize,
        down: bool,
        evicts_to_scrollback: bool,
        serves_wrap: bool,
    ) {
        if down {
            self.grid.scroll_down_region(top, bottom);
        } else {
            self.grid.scroll_up_region(top, bottom);
        }
        // Record the scroll op before any seam clear: `record_scroll` rotates `line_damage` with
        // the content, so damage recorded earlier would land on the wrong row (ADR-0025).
        self.record_scroll(top, bottom, if down { -1 } else { 1 });
        if top > 0 {
            self.end_wrap(top - 1);
        } else if !evicts_to_scrollback && !self.on_alt {
            // At `top == 0` the row above is the last scrollback row on the primary; a linefeed
            // (`evicts_to_scrollback`) keeps the adjacency, and the alt screen has none. Cleared with
            // its artefact marker and without damage (`docs/map/territory/soft-wrap.md`).
            if let Some(row) = self.scrollback.back_mut() {
                row.set_wrapped(false);
                if let Some(cell) = row.last_mut() {
                    cell.clear_leading_spacer();
                }
            }
        }
        // The row that lost its continuation: `bottom` going down, `bottom - 1` going up — but
        // not at the screen's bottom edge, where the wrap is waiting for the row the shift makes,
        // and never when the shift serves a wrap (#557, ADR-0025).
        let orphaned = if serves_wrap {
            // The exposed blank is the continuation the wrap is waiting for.
            None
        } else if down {
            Some(bottom)
        } else if bottom + 1 < self.grid.rows() {
            bottom.checked_sub(1)
        } else {
            None
        };
        if let Some(row) = orphaned {
            self.end_wrap(row);
        }
    }

    fn erase_display(&mut self, mode: u16) {
        let (cols, rows) = (self.grid.cols(), self.grid.rows());
        let (cr, cc) = (self.cursor.row, self.cursor.col);
        match mode {
            0 => {
                // Erases this row's tail and every row below, so nothing can continue from here
                // — and the rows below cannot continue either.
                self.clear_cells(cr, cc, cols);
                self.end_wrap(cr);
                for row in (cr + 1)..rows {
                    self.clear_cells(row, 0, cols);
                    self.end_wrap(row);
                    self.dispose_markers_on_row(row);
                }
            }
            1 => {
                // Leftward: this row's tail survives, so its own wrap does. The rows *above* are
                // gone entirely.
                for row in 0..cr {
                    self.clear_cells(row, 0, cols);
                    self.end_wrap(row);
                    self.dispose_markers_on_row(row);
                }
                self.clear_cells(cr, 0, cc + 1);
                self.drop_artefact_if_erased(cr, 0, cc + 1);
                // Covering the whole row means nothing continues from it, as xterm.js's `ED 1` arm says
                // (`InputHandler.ts:1248-1252`); `EL 1` has no such arm there or here.
                if cc + 1 == cols {
                    self.end_wrap(cr);
                }
            }
            2 => {
                for row in 0..rows {
                    self.clear_cells(row, 0, cols);
                    self.end_wrap(row);
                    self.dispose_markers_on_row(row);
                }
            }
            // xterm's addition: erase saved lines. The screen and the cursor are untouched.
            3 => self.erase_history(),
            _ => {}
        }
    }

    /// Drop every scrollback line — `ED 3`, and the first step of [`Term::clear`]
    /// (#936). The lines leave the *front* of the buffer, so every holder of an
    /// absolute line is repaired exactly as the scrollback cap repairs it, `n` lines
    /// at once: the selection clamps, markers and tracked points on the dropped lines
    /// go, search highlights are dropped, and `evicted_total` advances by `n`. The
    /// view returns to the bottom, since the history it was showing is gone.
    ///
    /// On the alt screen it drops the primary's history underneath, as xterm does;
    /// every alt line sits above the dropped ones, so its holders only shift.
    fn erase_history(&mut self) {
        let n = self.scrollback.len();
        if n == 0 {
            return;
        }
        self.scrollback.clear();
        self.lines_left_the_front(n);
        self.display_offset = 0;
        self.mark_fully_damaged();
    }

    /// Repair every holder of an absolute line after `n` lines left the front of the buffer —
    /// the one funnel for the scrollback cap, `ED 3` and [`Term::clear`] — and count them in
    /// `evicted_total` (#490). Reflow is not counted here (`docs/map/territory/marker.md`). The
    /// selection re-anchors, search highlights are dropped, markers and tracked points shift
    /// and those on a dropped line go (#118, #691). The view is the caller's.
    fn lines_left_the_front(&mut self, n: usize) {
        self.evicted_total += n as u64;
        self.selection_evict_oldest(n);
        self.invalidate_search_highlights();
        self.markers_evict_oldest(n);
        self.tracked_evict_oldest(n);
    }

    /// Clear the primary screen and its scrollback, keeping the cursor's line
    /// — a terminal's Clear command, out of band: the parser and whatever it
    /// holds mid-sequence are untouched. The cursor's logical line, from its first
    /// row on screen down to the cursor's row, moves to the top with the cursor on it
    /// at the same column; the rows above it and all of history are dropped, and the
    /// rows below the cursor are blanked. The view returns to the bottom and the next
    /// frame is `Full`.
    ///
    /// The selection and the search highlights are cleared. A marker on a kept row
    /// stays on it; every other marker is disposed and announced. Tracked points on
    /// dropped lines go; the rest shift with the kept rows.
    ///
    /// Returns `false` and changes nothing on the alt screen.
    pub fn clear(&mut self) -> bool {
        if self.on_alt {
            return false;
        }
        let (cols, cursor) = (self.grid.cols(), self.cursor.row);
        // The kept line is the cursor's whole logical line up to the cursor, so a prompt
        // and a command that wrapped keep their start — as far up as the screen reaches.
        let mut top = cursor;
        while top > 0 && self.grid.is_row_wrapped(top - 1) {
            top -= 1;
        }
        // Move the rows above it into history, so dropping history takes them too: their
        // absolute lines are unchanged by the move, so no holder moves.
        for _ in 0..top {
            let row = self.grid.scroll_up_recycle(Row::blank(cols));
            self.scrollback.push_back(row);
        }
        let last = cursor - top;
        self.cursor.row = last;
        self.erase_history();
        self.selection = None;
        self.invalidate_search_highlights();
        for row in last + 1..self.grid.rows() {
            let blanked = self.grid.row_mut(row);
            blanked.blank_in_place();
            blanked.purge_side_maps(0..cols);
            self.dispose_markers_on_row(row);
        }
        // Nothing follows the cursor's row now, so it cannot continue onto the next row.
        self.end_wrap(last);
        // `REP` reads back the cell the last print wrote. That cell is on the cursor's row
        // whenever the anchor is armed, so it moves up with it; anywhere else it is gone.
        self.repeat_anchor = self
            .repeat_anchor
            .filter(|&(row, _)| row == cursor)
            .map(|(_, col)| (last, col));
        self.scroll = None;
        self.mark_fully_damaged();
        true
    }

    /// Erase in line (EL): 0 = cursor→end, 1 = start→cursor, 2 = whole line.
    fn erase_line(&mut self, mode: u16) {
        let cols = self.grid.cols();
        let (cr, cc) = (self.cursor.row, self.cursor.col);
        match mode {
            // Erase right — ends the wrap at any column (xterm's `ClearRight`).
            0 => {
                self.clear_cells(cr, cc, cols);
                self.end_wrap(cr);
            }
            // Erase left — the tail survives, so the wrap does. The artefact marker does not:
            // if the erase reached the last column it just blanked the cell the marker described.
            1 => {
                self.clear_cells(cr, 0, cc + 1);
                self.drop_artefact_if_erased(cr, 0, cc + 1);
            }
            // Erase the whole line — ends the wrap, a deliberate divergence from xterm
            // (`docs/map/territory/soft-wrap.md`).
            2 => {
                self.clear_cells(cr, 0, cols);
                self.end_wrap(cr);
            }
            _ => {}
        }
    }

    // ---- intra-line editing (ICH / DCH / ECH) --------------------------------

    /// ECH (CSI Pn X): erase `n` cells in place from the cursor — no shift.
    /// BCE-filled (via `clear_cells`); pending-wrap is left untouched.
    fn erase_chars(&mut self, n: usize) {
        let cols = self.grid.cols();
        let (row, col) = (self.cursor.row, self.cursor.col);
        let to = (col + n).min(cols);
        self.clear_cells(row, col, to);
        // Destroys content from the cursor rightward, so the row stops continuing at any column
        // (`docs/map/territory/soft-wrap.md`).
        self.end_wrap(row);
    }

    /// ICH (CSI Pn @): insert `n` blanks at the cursor, shifting the rest of the
    /// line right; cells pushed past the right edge are lost. The opened gap is
    /// BCE-filled; pending-wrap is left untouched.
    fn insert_chars(&mut self, n: usize) {
        let cols = self.grid.cols();
        let (r, col) = (self.cursor.row, self.cursor.col);
        let n = n.min(cols - col);
        if n == 0 {
            return;
        }
        // Shifting a wrapped pair out of columns 0/1 voids the row above's artefact record
        // (#534) — asked before the shift, since IRM routes here right after `vacate_for_wrap`
        // set that record.
        if col <= 1 && self.wrapped_pair_at_row_start(r) {
            self.void_wrap_artefact_above(r);
        }
        let bg = self.cursor.pen.bg;
        let row = self.grid.row_mut(r);
        // Shift [col .. cols-n) right by n; the tail falls off the edge. The
        // combining map follows the moved cells (the bit travels with the raw
        // copy, the cluster data must too).
        row.copy_within(col..cols - n, col + n);
        row.move_maps(col..cols - n, col + n);
        for cell in &mut row[col..col + n] {
            cell.reset();
            cell.set_bg(bg);
        }
        // Repair wide-char halves split at the seams (no-orphan invariant):
        // a lead just before the gap lost its spacer; the first shifted cell may
        // be a spacer whose lead did not move.
        if col > 0 && self.grid.cell(r, col - 1).is_wide() {
            self.free_cell(r, col - 1);
        }
        if col + n < cols && self.grid.cell(r, col + n).is_wide_spacer() {
            self.free_cell(r, col + n);
        }
        // A lead shifted to the last column lost its spacer off the edge.
        if self.grid.cell(r, cols - 1).is_wide() {
            self.free_cell(r, cols - 1);
        }
        // A right shift pushes the last column off the edge, so this row's marker is discarded,
        // never carried inward (`docs/map/territory/wide-glyph.md`).
        self.damage_span(r, col, cols - 1);
    }

    /// DCH (CSI Pn P): delete `n` cells at the cursor, shifting the tail left; the
    /// vacated cells at the right are BCE-blanked. Pending-wrap is left untouched.
    fn delete_chars(&mut self, n: usize) {
        let cols = self.grid.cols();
        let (r, col) = (self.cursor.row, self.cursor.col);
        let n = n.min(cols - col);
        if n == 0 {
            return;
        }
        // The row stops continuing — ended **before** the shift, which would otherwise carry the
        // artefact marker inward (#534, `docs/map/territory/wide-glyph.md`).
        self.end_wrap(r);
        // Deleting a wrapped pair out of columns 0/1 voids the row above's record — asked before
        // the shift, which can pull the next wide glyph into column 0 (#534).
        if col <= 1 && self.wrapped_pair_at_row_start(r) {
            self.void_wrap_artefact_above(r);
        }
        let bg = self.cursor.pen.bg;
        let row = self.grid.row_mut(r);
        // Shift [col+n .. cols) left to [col ..); BCE-fill the vacated tail. The
        // combining map follows the moved cells.
        row.copy_within(col + n..cols, col);
        row.move_maps(col + n..cols, col);
        for cell in &mut row[cols - n..cols] {
            cell.reset();
            cell.set_bg(bg);
        }
        // Repair wide-char halves split by the deletion (no-orphan invariant):
        // a lead just before the cut lost its spacer; the cell now at the cursor
        // may be a spacer whose lead was deleted.
        if col > 0 && self.grid.cell(r, col - 1).is_wide() {
            self.free_cell(r, col - 1);
        }
        if self.grid.cell(r, col).is_wide_spacer() {
            self.free_cell(r, col);
        }
        self.damage_span(r, col, cols - 1);
    }

    // ---- line/region editing (IL / DL / SU / SD) -----------------------------

    /// Scroll rows `[top..=bottom]` by `n` lines, BCE-filling the exposed lines.
    /// `down` inserts blanks at the top (content moves down); otherwise content
    /// moves up and blanks appear at the bottom. Reuses the one-line region scroll
    /// primitives (so damage + scroll-op accumulation come for free), then fills
    /// the exposed lines with the current SGR background.
    fn scroll_region_lines(&mut self, top: usize, bottom: usize, n: usize, down: bool) {
        let height = bottom - top + 1;
        let n = n.min(height);
        if n == 0 {
            return;
        }
        // Anchors (selection #3, markers #118/#158) live at absolute buffer lines;
        // SU/SD/IL/DL don't accrue scrollback, so `base` is stable across the loop.
        let base = self.scrollback.len();
        for _ in 0..n {
            self.shift_region(top, bottom, down, false, false);
            // Rotate anchors with the content, like `linefeed`/`reverse_index`
            // (#162). `up` = content moved up = the non-`down` case. Markers rotate
            // with the active buffer (#187) — alt-scoped on the alt screen, so no
            // guard. **The selection is unguarded here for the same reason, not because
            // "it is cleared on alt enter"** — that was this comment's claim until #660 and
            // it is false: a selection made while the alt screen is up is ordinary, it does
            // reach this line, and rotating it is correct, because the content really did
            // move under it. The code was right; only its stated reason was wrong.
            self.selection_rotate_region(base + top, base + bottom, !down);
            self.markers_rotate_region(base + top, base + bottom, !down);
            self.tracked_rotate_region(base + top, base + bottom, !down);
        }
        self.invalidate_search_highlights();
        // BCE-fill the n exposed lines (the primitives blank to default).
        let bg = self.cursor.pen.bg;
        let (fill_top, fill_end) = if down {
            (top, top + n)
        } else {
            (bottom + 1 - n, bottom + 1)
        };
        let cols = self.grid.cols();
        for r in fill_top..fill_end {
            for c in 0..cols {
                let cell = self.grid.cell_mut(r, c);
                cell.reset();
                cell.set_bg(bg);
            }
        }
    }

    /// SU (CSI Pn S): scroll the scroll region up by `n`.
    fn scroll_up_lines(&mut self, n: usize) {
        self.scroll_region_lines(self.scroll_top, self.scroll_bottom, n, false);
    }

    /// SD (CSI Pn T): scroll the scroll region down by `n`.
    fn scroll_down_lines(&mut self, n: usize) {
        self.scroll_region_lines(self.scroll_top, self.scroll_bottom, n, true);
    }

    /// IL (CSI Pn L): insert `n` blank lines at the cursor, scrolling
    /// `[cursor..=scroll_bottom]` down. A no-op when the cursor is outside the
    /// scroll region.
    fn insert_lines(&mut self, n: usize) {
        let cur = self.cursor.row;
        if cur < self.scroll_top || cur > self.scroll_bottom {
            return;
        }
        // Clears the deferred wrap (#848; `docs/map/territory/cursor-position.md`).
        self.cursor.pending_wrap = false;
        self.scroll_region_lines(cur, self.scroll_bottom, n, true);
    }

    /// DL (CSI Pn M): delete `n` lines at the cursor, scrolling
    /// `[cursor..=scroll_bottom]` up. A no-op when the cursor is outside the
    /// scroll region.
    fn delete_lines(&mut self, n: usize) {
        let cur = self.cursor.row;
        if cur < self.scroll_top || cur > self.scroll_bottom {
            return;
        }
        // As `insert_lines`.
        self.cursor.pending_wrap = false;
        self.scroll_region_lines(cur, self.scroll_bottom, n, false);
    }

    // ---- SGR (CSI m) ---------------------------------------------------------

    fn sgr(&mut self, params: &Params) {
        let pen = &mut self.cursor.pen;
        let mut iter = params.iter();
        while let Some(param) = iter.next() {
            let code = param.first().copied().unwrap_or(0);
            match code {
                0 => pen.reset(),
                1 => pen.flags.insert(CellFlags::BOLD),
                2 => pen.flags.insert(CellFlags::DIM),
                3 => pen.flags.insert(CellFlags::ITALIC),
                // SGR 4 and its colon sub-parameter form (#829): `4:0` is off, an unrecognised
                // sub-style degrades to a single underline, and every style is stored and drawn (#830).
                4 => {
                    let style = match param.get(1) {
                        None | Some(1) => UnderlineStyle::Single,
                        Some(0) => UnderlineStyle::None,
                        Some(2) => UnderlineStyle::Double,
                        Some(3) => UnderlineStyle::Curly,
                        Some(4) => UnderlineStyle::Dotted,
                        Some(5) => UnderlineStyle::Dashed,
                        Some(_) => UnderlineStyle::Single,
                    };
                    pen.flags.set_underline_style(style);
                }
                5 => pen.flags.insert(CellFlags::BLINK),
                7 => pen.flags.insert(CellFlags::INVERSE),
                8 => pen.flags.insert(CellFlags::HIDDEN),
                9 => pen.flags.insert(CellFlags::STRIKETHROUGH),
                // The legacy double underline (#830), on the same field so `24` clears both spellings.
                // Decided by the spec (`ctlseqs.txt:1200`) over `vte`'s `CancelBold` reading; `CSI 21m`
                // meaning "stop bold" gets a double underline and keeps its bold. Reference rows in
                // `docs/agents/reference-facts.md`.
                21 => pen.flags.set_underline_style(UnderlineStyle::Double),
                22 => pen.flags.remove(CellFlags::BOLD | CellFlags::DIM),
                23 => pen.flags.remove(CellFlags::ITALIC),
                // Clears the style, not just the derived flag (#829) — removing `UNDERLINE` alone
                // would leave a styled-but-not-underlined pen, the disagreement this model exists
                // to make unrepresentable.
                24 => pen.flags.set_underline_style(UnderlineStyle::None),
                25 => pen.flags.remove(CellFlags::BLINK),
                27 => pen.flags.remove(CellFlags::INVERSE),
                28 => pen.flags.remove(CellFlags::HIDDEN),
                29 => pen.flags.remove(CellFlags::STRIKETHROUGH),
                30..=37 => pen.fg = Color::Indexed((code - 30) as u8),
                38 => {
                    if let Some(c) = parse_extended_color(param, &mut iter) {
                        pen.fg = c;
                    }
                }
                39 => pen.fg = Color::Default,
                40..=47 => pen.bg = Color::Indexed((code - 40) as u8),
                48 => {
                    if let Some(c) = parse_extended_color(param, &mut iter) {
                        pen.bg = c;
                    }
                }
                49 => pen.bg = Color::Default,
                // Underline colour (SGR 58 / 59, #520) — same extended-colour grammar
                // as 38/48 (colon `58:2:r:g:b` / `58:5:n`, or legacy semicolon), so it
                // reuses `parse_extended_color` verbatim. 59 returns to "follow the fg".
                58 => {
                    if let Some(c) = parse_extended_color(param, &mut iter) {
                        pen.underline_color = c;
                    }
                }
                59 => pen.underline_color = Color::Default,
                // bright foreground/background (aixterm) → palette 8..=15.
                90..=97 => pen.fg = Color::Indexed((code - 90 + 8) as u8),
                100..=107 => pen.bg = Color::Indexed((code - 100 + 8) as u8),
                _ => {}
            }
        }
    }
}

/// Cap a recorded scroll to what a consumer can act on and what the wire can carry (#661);
/// see [`Term::scroll_delta`]. A free function so the wire bound is testable without
/// building a screen taller than 32 767 rows.
fn cap_scroll(op: ScrollOp) -> ScrollOp {
    let height = op.bottom.saturating_sub(op.top).saturating_add(1) as isize;
    let bound = height.min(MAX_SCROLL_COUNT);
    ScrollOp {
        count: op.count.clamp(-bound, bound),
        ..op
    }
}

/// Parse `38`/`48`/`58` extended colour (foreground / background / underline, #520), in
/// either form: the colon sub-parameter form inline in `param` (`38:5:n`, `38:2:r:g:b`, or
/// `38:2:cs:r:g:b` with a colorspace slot, counted by length), or the legacy semicolon form
/// pulled from `iter`. The short colon form is tolerated on purpose:
/// `docs/map/territory/pen.md`.
fn parse_extended_color<'a, I>(param: &[u16], iter: &mut I) -> Option<Color>
where
    I: Iterator<Item = &'a [u16]>,
{
    if param.len() > 1 {
        // Colon sub-parameter form: kind is param[1].
        match param[1] {
            2 => {
                // 38:2:r:g:b (len 5) or 38:2:cs:r:g:b (len 6, colorspace skipped).
                let off = if param.len() >= 6 { 3 } else { 2 };
                let r = *param.get(off)? as u8;
                let g = *param.get(off + 1)? as u8;
                let b = *param.get(off + 2)? as u8;
                Some(Color::Rgb(r, g, b))
            }
            5 => Some(Color::Indexed(*param.get(2)? as u8)),
            _ => None,
        }
    } else {
        // Legacy semicolon form: kind, then its operands, are separate params.
        match iter.next()?.first().copied()? {
            2 => {
                let r = iter.next()?.first().copied()? as u8;
                let g = iter.next()?.first().copied()? as u8;
                let b = iter.next()?.first().copied()? as u8;
                Some(Color::Rgb(r, g, b))
            }
            5 => Some(Color::Indexed(iter.next()?.first().copied()? as u8)),
            _ => None,
        }
    }
}

/// The fixed dimensions a resize reflows toward.
#[derive(Clone, Copy)]
struct ReflowDims {
    old_cols: usize,
    cols: usize,
    rows: usize,
    limit: usize,
    /// Whether a column change may re-split this pane's content, or only re-fit its rows —
    /// false for the alt screen (#567, `docs/map/territory/reflow.md`).
    reflow: bool,
}

/// The result of reflowing one pane.
struct PaneReflow {
    screen: Vec<Row>,
    scrollback: VecDeque<Row>,
    /// The cursor's new screen-relative position.
    cursor: (usize, usize),
    /// Each tracked extra point's new position in this pane's own `[history ++ screen]` frame,
    /// index-aligned with `extra_abs`, before any history the caller discards. Raw, with
    /// `evicted` beside it, because the primary and alt callers translate differently.
    extras: Vec<(usize, usize)>,
    /// Rows that left the buffer entirely off the front of this pane's history. For the primary
    /// that is the scrollback cap's eviction; for the alt pane, whose limit is `0` because it has
    /// no history, it is every row the shrink pushed off the top. An extra whose raw line is below
    /// this **is not in the buffer any more** — the caller decides what that means for its kind.
    evicted: usize,
}

/// Reflow one pane (its `scrollback` joined with `screen`) to `dims`, tracking the
/// screen-relative cursor `point` plus any `extra_abs` points in absolute
/// `[scrollback ++ screen]` coordinates. Returns the new screen rows, the scrollback capped
/// to `dims.limit`, and where each point landed.
fn reflow_pane(
    screen: Vec<Row>,
    scrollback: VecDeque<Row>,
    point: (usize, usize),
    extra_abs: &[(usize, usize)],
    dims: ReflowDims,
) -> PaneReflow {
    let scroll_len = scrollback.len();
    let mut all: Vec<Row> = scrollback.into();
    all.extend(screen);

    // The cursor is screen-relative; lift it to absolute, then track it together
    // with the already-absolute extras.
    let mut pts: Vec<(usize, usize)> = Vec::with_capacity(1 + extra_abs.len());
    pts.push((scroll_len + point.0, point.1));
    pts.extend_from_slice(extra_abs);

    let pts = if dims.reflow && dims.cols != dims.old_cols {
        let (reflowed, np) = crate::grid::reflow(all, dims.cols, &pts);
        all = reflowed;
        np
    } else {
        pts
    };

    // A cursor one row past the content (#562) buys that row from history when the pane can
    // pay (`limit > 0`): `docs/map/territory/reflow.md`.
    let cursor_abs = pts[0].0 + usize::from(pts[0].1 == dims.cols);
    if dims.limit > 0 {
        while all.len() <= cursor_abs {
            all.push(Row::blank(dims.cols));
        }
    }

    let split = all.len().saturating_sub(dims.rows);
    let history: Vec<Row> = all.drain(0..split).collect();
    let mut sb: VecDeque<Row> = history.into();
    let mut dropped = 0usize;
    while sb.len() > dims.limit {
        sb.pop_front();
        dropped += 1;
    }

    // `reflow` may answer `col == cols` — "just after the last cell", which is a real place in the
    // logical line and no place in the grid (#562). The **cursor's** reading of it is the next
    // *write* position, so a full row means the start of the row after; the caller's row fit
    // provides that row (`Grid::set_screen` pads at the bottom). A mark reads the same value the
    // opposite way and keeps it verbatim — see `Term::resize`.
    let cursor_row = pts[0].0.saturating_sub(split);
    let cursor = if pts[0].1 == dims.cols {
        (cursor_row + 1, 0)
    } else {
        (cursor_row, pts[0].1)
    };

    // Bound tracked lines to this pane's last addressable line, here where the final
    // geometry is known (#562).
    let max_line = split + dims.rows - 1;

    // The cursor returns to screen-relative (its absolute index minus the history split). The
    // extras stay in this pane's frame — see the field docs for why they are not shifted here.
    PaneReflow {
        cursor,
        extras: pts[1..]
            .iter()
            .map(|&(l, c)| (l.min(max_line), c))
            .collect(),
        evicted: dropped,
        screen: all,
        scrollback: sb,
    }
}

/// Default tab stops: one every 8 columns (incl. column 0), matching xterm.
fn default_tabs(cols: usize) -> Vec<bool> {
    (0..cols).map(is_default_tab_stop).collect()
}

/// Whether `col` carries a stop in the default ladder.
///
/// Shared by the table the constructor builds and the extension [`Term::resize`]
/// performs, which fills at the *absolute* column index. The two must not drift,
/// and a second literal `8` is exactly how they would.
fn is_default_tab_stop(col: usize) -> bool {
    col.is_multiple_of(8)
}

/// First sub-parameter of CSI param `idx`, or `default` when absent or zero
/// (a zero/omitted numeric param means "1" for cursor movement and "0" for
/// erase — callers pass the right default).
fn param_or(params: &Params, idx: usize, default: u16) -> u16 {
    match params.iter().nth(idx).and_then(|p| p.first().copied()) {
        Some(v) if v != 0 => v,
        _ => default,
    }
}

/// The `Pv` field of the secondary device-attributes report (#824): the crate version,
/// padded base-100 (`1.2.3` → `10203`), the pre-release suffix cut at the first hyphen.
/// The base is load-bearing — vim gates its mouse protocol and XTGETTCAP on this number:
/// `docs/map/territory/vt-interpretation.md`.
const fn version_number(version: &str) -> u32 {
    let bytes = version.as_bytes();
    let mut parts = [0u32; 3];
    let mut part = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'-' || b == b'+' {
            break;
        } else if b == b'.' {
            part += 1;
            if part >= 3 {
                break;
            }
        } else if b.is_ascii_digit() {
            parts[part] = parts[part] * 10 + (b - b'0') as u32;
        }
        i += 1;
    }
    parts[0] * 10_000 + parts[1] * 100 + parts[2]
}

#[cfg(test)]
mod tests {
    use super::cap_scroll;
    use super::version_number;
    use crate::Engine;
    use crate::damage::ScrollOp;
    use crate::serialize::MAX_SCROLL_COUNT;

    /// #824 — the `Pv` mapping, in-crate because the seam cannot reach it.
    ///
    /// `tests/reply.rs` asserts the reply carries the number *this* crate
    /// version maps to, which is the assertion that fires the moment a release
    /// drifts from its report. What it cannot observe is the mapping itself:
    /// there is only ever one crate version at run time, so a hand-written
    /// literal equal to today's number is byte-identical to the derivation at
    /// that seam. Measured, by replacing the call site with `1500`:
    /// `cargo test --workspace` stayed green, and `cargo clippy --workspace
    /// --all-targets -- -D warnings` **failed** — `version_number` loses its
    /// only non-test caller and `dead_code` is an error under the gate. So the
    /// test suite alone cannot see it and the gate can; these cases are what
    /// make the mapping itself falsifiable rather than assumed.
    #[test]
    fn version_number_pads_semver_base_100() {
        // Each place is two decimal digits wide, so a higher version always
        // reports a higher number — which is the only property `Pv` promises.
        assert_eq!(version_number("0.0.1"), 1);
        assert_eq!(version_number("0.15.0"), 15_00);
        assert_eq!(version_number("1.2.3"), 1_02_03);
        assert_eq!(version_number("999.99.99"), 9_99_99_99);
        assert!(version_number("0.16.0") > version_number("0.15.9"));
        assert!(version_number("1.0.0") > version_number("0.99.99"));
    }

    #[test]
    fn version_number_strips_a_pre_release_suffix() {
        // Semver starts the pre-release at the FIRST hyphen, so a two-part
        // suffix strips whole. alacritty cuts at the last and mis-parses this.
        assert_eq!(version_number("0.15.0-dev"), 15_00);
        assert_eq!(version_number("1.2.3-rc.1-dev"), 1_02_03);
        assert_eq!(version_number("1.2.3+build.5"), 1_02_03);
        // The cases above cannot observe the strip at all: their suffixes carry
        // no digits, so removing the `-` break leaves the number unchanged —
        // measured, by doing exactly that and watching them stay green. A digit
        // *inside* the suffix is what the strip is for.
        assert_eq!(version_number("0.15.0-rc2"), 15_00);
        assert_eq!(version_number("1.2.3-4"), 1_02_03);
    }

    #[test]
    fn version_number_tolerates_a_short_or_odd_version() {
        // A missing component reads as zero rather than panicking, and a fourth
        // component is ignored — the report must not be able to fail.
        assert_eq!(version_number("2"), 2_00_00);
        assert_eq!(version_number("2.7"), 2_07_00);
        assert_eq!(version_number("1.2.3.4"), 1_02_03);
        assert_eq!(version_number(""), 0);
    }

    /// #661 — the wire's `i16` bound, not the region-height one.
    ///
    /// In-crate on purpose, and the reason is cost rather than visibility: reaching
    /// this through `Engine` needs a screen taller than 32 767 rows *and* 32 768
    /// scrolls of it, each rotating a `line_damage` of that length. Measured at
    /// 15.8 s in a debug build for a single assertion — the whole `serialize` suite
    /// is 0.2 s without it. See `cap_scroll`'s note for how the coverage is split.
    #[test]
    fn a_region_taller_than_the_wire_field_truncates_rather_than_wraps() {
        // 40 000 rows: over i16::MAX, under MAX_ROWS (u16::MAX), so the region
        // height alone would let 35 000 through — and 35 000 as i16 is -30 536.
        let up = cap_scroll(ScrollOp {
            top: 0,
            bottom: 40_000,
            count: 35_000,
        });
        assert_eq!(up.count, MAX_SCROLL_COUNT, "capped, and still an up-scroll");

        let down = cap_scroll(ScrollOp {
            top: 0,
            bottom: 40_000,
            count: -35_000,
        });
        assert_eq!(down.count, -MAX_SCROLL_COUNT, "sign survives the cap");
    }

    /// The cap is a ceiling, not a rewrite: a count inside both bounds is reported
    /// exactly, and the region it names is untouched.
    #[test]
    fn a_scroll_inside_both_bounds_passes_through_unchanged() {
        let op = ScrollOp {
            top: 4,
            bottom: 9,
            count: -2,
        };
        assert_eq!(cap_scroll(op), op);
    }

    /// #628 — a hyperlink's storage is released once no live row references it.
    ///
    /// In-crate on purpose: this defect has **no public observable**, which is why it
    /// survived from #46 until #621's completeness pass went looking. Pool indices never
    /// cross the wire (`Term::frame` remaps them to frame-local `link_table` positions),
    /// and the one public reader takes an index the caller already holds — so from
    /// outside the crate a pool of 5 entries and a pool of 50 000 are indistinguishable.
    /// The assertion has to stand where the storage does.
    ///
    /// The fixture is a buffer that cannot hold what it is fed: 2 rows plus 2 lines of
    /// scrollback is four lines total, so by the end all but the last four opens have
    /// been evicted and nothing on screen or in history refers to them.
    #[test]
    fn a_link_evicted_from_the_buffer_stops_being_stored() {
        let mut e = Engine::with_scrollback(20, 2, 2);
        for i in 0..50 {
            e.feed(format!("\x1b]8;;https://example.com/{i}\x07L{i}\x1b]8;;\x07\r\n").as_bytes());
        }

        // The observable had to move with the storage — there is no pool left to count.
        // A `Weak` is the stronger form of the same claim anyway: a bounded count can be
        // bounded and still wrong, while a dead `Weak` says *this exact allocation* was
        // released.
        //
        // **`e2` must outlive the assertion, and that is the whole test.** The first
        // version of this scoped the engine to the block that built the `Weak`, so the
        // engine was dropped before the check and the `Weak` died for that reason
        // instead. Measured: with a deliberate leak reintroduced (a `Vec<Arc<str>>` on
        // `Term`, retaining every open), that version stayed **green** — a tautological
        // proof, confirming only that dropping an `Engine` frees its own memory. Keeping
        // the engine alive is what makes the assertion about reclamation.
        let mut e2 = Engine::with_scrollback(20, 2, 2);
        e2.feed(b"\x1b]8;;https://example.com/first\x07L\x1b]8;;\x07\r\n");
        let weak = {
            let arc = e2
                .term
                .grid
                .row_ref(0)
                .link_at(0)
                .expect("on screen")
                .clone();
            std::sync::Arc::downgrade(&arc)
        };
        // The live half first: a fix that simply never stored the URI would satisfy the
        // dead-`Weak` assertion below for the wrong reason.
        assert!(
            weak.upgrade().is_some(),
            "the URI must be alive while its cell is on screen",
        );
        for i in 0..50 {
            e2.feed(format!("filler {i}\r\n").as_bytes());
        }
        assert!(
            weak.upgrade().is_none(),
            "the first link scrolled out of a 4-line buffer and nothing should still \
             hold its URI — before #628 every OSC 8 open lived for the life of the Term",
        );

        // The whole-buffer form of the same claim: 50 distinct opens through a buffer
        // that holds four lines leaves at most four entries *owned*.
        //
        // `owned_link_count` and not `link_at`, and that distinction is the test. The
        // first version summed the gated reader, which counts **linked cells** — measured
        // on an erased screen it read 0 while every URI was still allocated, so it could
        // not fail for the property this test exists to assert.
        // Deduped by allocation: one open covering three cells is three map entries and
        // one URI, so counting entries would fail at 9 for a buffer holding four links.
        let owned: std::collections::HashSet<*const u8> = e
            .term
            .scrollback
            .iter()
            .chain((0..2).map(|r| e.term.grid.row_ref(r)))
            .flat_map(|r| r.owned_links())
            .map(|u| std::sync::Arc::as_ptr(u) as *const u8)
            .collect();
        assert!(
            owned.len() <= 4,
            "a 4-line buffer cannot own more than 4 distinct URIs, found {}",
            owned.len(),
        );
    }

    /// #628 — erasing a cell in place releases its URI, not just its presence bit.
    ///
    /// The sibling of the eviction test above, and the case that one structurally cannot
    /// see: `clear_cells` / `free_cell` blank a cell **without dropping its row**, so no
    /// row-lifetime event fires. Under `row-keyed-side-maps` rule 3 leaving the map entry
    /// is sanctioned — *"a write that clears the cell owes the bit, not the map"* — and
    /// that was exactly right while the value was a 4-byte index: a stale entry is
    /// unreadable through the gate and costs nothing.
    ///
    /// #628 changed what the entry *is*. The map now owns a heap string, so the same
    /// sanctioned line retains one. Rule 3 still holds as stated — purging is not the
    /// correctness step, and missing a site costs bounded retention rather than a wrong
    /// answer — but the optimisation it calls optional became worth taking here.
    /// All three references release at this point: alacritty's `Cell::reset` drops the
    /// `Option<Arc<CellExtra>>` outright, ghostty's ref-counted set frees at zero, and
    /// xterm.js's `_resetBufferLine` clears `_extendedAttrs` and disposes the line's
    /// markers so `OscLinkService` deletes the entry.
    #[test]
    fn an_erased_cell_releases_its_uri_not_only_its_bit() {
        let mut e = Engine::new(80, 24);
        e.feed(b"]8;;https://example.com/erasedL]8;;");
        let weak = {
            let a = e
                .term
                .grid
                .row_ref(0)
                .link_at(0)
                .expect("on screen")
                .clone();
            std::sync::Arc::downgrade(&a)
        };
        assert!(weak.upgrade().is_some(), "alive while on screen");

        e.feed(b"[2J"); // ED 2 — erases in place; no row is dropped or reused

        // The gated reader already says "no link", and so does the frame. Neither can
        // see the retention, which is why this assertion holds the `Weak` instead:
        // measured before the purge, both public views read 0 while the URI lived.
        assert!(e.link_at(0, 0).is_none(), "the presence bit is cleared");
        assert!(
            weak.upgrade().is_none(),
            "and the URI itself is released — before the purge the map kept owning it,              so an erased screen retained every link it had shown",
        );
    }

    /// `Arc`, not `Rc`, and this is what makes that a fact rather than a comment.
    ///
    /// #628 chose `Arc<str>` for the row's link map on the stated ground that `Engine` is
    /// `Send + Sync`; `Rc` would have removed both **silently** — no signature changes
    /// here, and a downstream `Mutex<Engine>` failing to compile instead. The claim was
    /// load-bearing and unpinned: a repo-wide grep for it found only prose.
    #[test]
    fn the_engine_stays_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Engine>();
    }

    /// Two OSC 8 opens of an identical URI are **two links**, not one.
    ///
    /// Deliberate, and the reason is #635: merging them would override the grouping the
    /// application controls through `id=`, which is the one dedup xterm.js performs.
    /// Asserted by allocation identity through the in-crate observer rather than through
    /// a public accessor — the behaviour is real now, a consumer asking about it is not.
    #[test]
    fn two_opens_of_one_uri_are_two_links() {
        let mut e = Engine::new(40, 2);
        // One open covering two cells, then a *separate* open of the very same URI.
        e.feed(b"]8;;https://example.com/xAB]8;;");
        e.feed(b"]8;;https://example.com/xC]8;;");

        let row = e.term.grid.row_ref(0);
        let ptr = |c: usize| std::sync::Arc::as_ptr(row.link_at(c).expect("linked")) as *const u8;
        assert_eq!(
            e.link_at(0, 0).map(|h| h.uri().to_owned()),
            e.link_at(0, 2).map(|h| h.uri().to_owned()),
            "the text is the same",
        );
        assert_eq!(ptr(0), ptr(1), "A and B are one open, so one allocation");
        assert_ne!(
            ptr(0),
            ptr(2),
            "…but C is a second open — merging the two would override the distinction              `id=` exists to express (#635)",
        );
    }

    /// The other half of the rule above: an `id=` the application declared **does** group
    /// (#635). One rule, not two — "never merge on URI alone, always merge on a declared
    /// id" is how xterm.js states it (`OscLinkService.ts:34`, `:51` at the pinned SHA), and
    /// justerm shipped the first half only because #26 ported `registerLink`'s id-minting
    /// and not its lookup.
    ///
    /// Grouping is asserted as **allocation identity**, which is not an implementation
    /// detail leaking into a test: since #628 the `Arc`'s address *is* link identity —
    /// `Term::frame` interns `link_table` by `Arc::as_ptr`, so one allocation is what makes
    /// two runs one link index on the wire, and that index is what a consumer groups by.
    #[test]
    fn the_same_id_and_uri_group_into_one_link() {
        let mut e = Engine::new(40, 2);
        // Two separate opens, same `id=` and same URI, on two different lines — the case
        // the parameter exists for (a link that cannot be one contiguous run).
        e.feed(b"\x1b]8;id=xyz;https://example.com/a\x07A\x1b]8;;\x07\r\n");
        e.feed(b"\x1b]8;id=xyz;https://example.com/a\x07B\x1b]8;;\x07");

        let ptr = |r: usize, c: usize| {
            std::sync::Arc::as_ptr(e.term.grid.row_ref(r).link_at(c).expect("linked")) as *const u8
        };
        assert_eq!(
            ptr(0, 0),
            ptr(1, 0),
            "the application said these two runs are one link, so they share one allocation",
        );

        // And the wire agrees, which is the half a consumer can actually see: one entry in
        // `link_table`, referenced by both spans. Two entries is the defect.
        let f = e.frame();
        assert_eq!(f.link_table.len(), 1, "one link ships once");
    }

    /// Keyed on `id` **and** URI, not on `id` alone — xterm.js's `_getEntryIdKey` is
    /// `` `${id};;${uri}` `` (`OscLinkService.ts:87`). An application reusing an id for a
    /// different target has not said "same link"; treating it as one would follow a stale
    /// declaration to the wrong URI.
    #[test]
    fn the_same_id_with_a_different_uri_stays_two_links() {
        let mut e = Engine::new(40, 2);
        e.feed(b"\x1b]8;id=xyz;https://example.com/a\x07A\x1b]8;;\x07\r\n");
        e.feed(b"\x1b]8;id=xyz;https://example.com/b\x07B\x1b]8;;\x07");

        let ptr = |r: usize, c: usize| {
            std::sync::Arc::as_ptr(e.term.grid.row_ref(r).link_at(c).expect("linked")) as *const u8
        };
        assert_ne!(
            ptr(0, 0),
            ptr(1, 0),
            "same id, different target — two links"
        );
        assert_eq!(e.frame().link_table.len(), 2, "and both ship");
    }

    /// `id=` with an **empty value** is no id at all, so the no-id rule applies and each
    /// open is its own link. xterm.js reaches this by `parsedParams[i].slice(3) || undefined`
    /// (`InputHandler.ts:3130`) — the `||` is the whole behaviour, and reading `slice(3)`
    /// alone gives the opposite answer.
    ///
    /// Worth a test rather than a comment because the empty-string key is the one that
    /// would group *every* `id=`-with-no-value link in a session into one, across unrelated
    /// URIs — a wrong answer that grows with uptime.
    #[test]
    fn an_empty_id_value_is_no_id_at_all() {
        let mut e = Engine::new(40, 2);
        e.feed(b"\x1b]8;id=;https://example.com/a\x07A\x1b]8;;\x07\r\n");
        e.feed(b"\x1b]8;id=;https://example.com/a\x07B\x1b]8;;\x07");

        let ptr = |r: usize, c: usize| {
            std::sync::Arc::as_ptr(e.term.grid.row_ref(r).link_at(c).expect("linked")) as *const u8
        };
        assert_ne!(
            ptr(0, 0),
            ptr(1, 0),
            "no id declared, so the reference-correct fresh-per-open rule still holds",
        );
    }

    /// `params` is a **`:`-separated** key=value list (`id=xyz123:foo=bar:baz=quux`), and
    /// `id` may sit anywhere in it — xterm.js scans with `findIndex(e =>
    /// e.startsWith('id='))` (`InputHandler.ts:3129`). Testing only a leading `id=` would
    /// pass with a `starts_with` on the whole field, which is the wrong parse.
    #[test]
    fn the_id_param_is_found_among_other_params() {
        let mut e = Engine::new(40, 2);
        e.feed(b"\x1b]8;foo=bar:id=xyz:baz=quux;https://example.com/a\x07A\x1b]8;;\x07\r\n");
        e.feed(b"\x1b]8;id=xyz;https://example.com/a\x07B\x1b]8;;\x07");

        let ptr = |r: usize, c: usize| {
            std::sync::Arc::as_ptr(e.term.grid.row_ref(r).link_at(c).expect("linked")) as *const u8
        };
        assert_eq!(
            ptr(0, 0),
            ptr(1, 0),
            "the id is the same whatever else rides beside it",
        );
    }

    /// The grouping registry must not become the pool #628 deleted.
    ///
    /// Whatever maps an `id=` to its link has to hold it **weakly**: a strong reference
    /// would make every id'd link immortal for the life of the `Term` — the exact defect
    /// #628 removed, re-entering through the door #635 opens. xterm.js's equivalent map is
    /// reclaimed rather than weak (`_entriesWithId.delete` when the entry's last line
    /// marker is disposed, `OscLinkService.ts:98-100`); justerm has no disposal hook by
    /// design, so `Weak` is how the same lifetime is expressed here.
    ///
    /// This is the test that discriminates the two, and nothing public can: both spellings
    /// group correctly, and they differ only in what stays alive afterwards.
    #[test]
    fn the_id_registry_does_not_keep_a_link_alive() {
        let mut e = Engine::new(80, 24);
        e.feed(b"\x1b]8;id=xyz;https://example.com/grouped\x07L\x1b]8;;\x07");
        let weak = {
            let a = e
                .term
                .grid
                .row_ref(0)
                .link_at(0)
                .expect("on screen")
                .clone();
            std::sync::Arc::downgrade(&a)
        };
        assert!(weak.upgrade().is_some(), "alive while on screen");

        e.feed(b"\x1b[2J"); // ED 2 — the in-place erase that releases the row's side maps

        assert!(
            weak.upgrade().is_none(),
            "the id registry must hold a Weak — a strong entry would outlive the screen and              rebuild #628's leak one id at a time",
        );
    }
}
