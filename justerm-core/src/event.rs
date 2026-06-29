//! Consumer event surface (#12): point-in-time notifications the engine
//! accumulates while parsing, for the consumer to drain.
//!
//! Pull, not push — the engine queues events during `feed` and the consumer
//! takes them with `drain_events`, mirroring the rest of the pull cadence
//! (`damage` / `frame` / `reset_damage`). No callback is injected across the
//! boundary, so the engine stays decoupled from the consumer's event loop
//! (unlike alacritty's `EventListener`, whose push model would couple them).
//!
//! OSC 8 hyperlinks are deliberately absent — a hyperlink is per-cell state
//! (which cells are links), not a point-in-time event, so it is modelled like
//! graphemes in its own slice (#26), not here.

use crate::serialize::MarkerId;

/// A consumer-facing event emitted while parsing the VT stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEvent {
    /// The window/icon title was set (OSC 0 or OSC 2).
    Title(String),
    /// The terminal bell rang (BEL, `0x07`).
    Bell,
    /// The working directory was reported (OSC 7), e.g. `file://host/path`.
    Cwd(String),
    /// The app requested 80/132-column mode (DECCOLM `?3`). justerm is
    /// dimension-free, so this is a *request* — the consumer may honor it by
    /// calling `resize(cols, rows)`, or ignore it. `cols` is 80 or 132 (#82).
    ColumnMode { cols: usize },
    /// The app queried the light/dark color scheme (DSR `CSI ? 996 n`). justerm
    /// is theme-agnostic, so the consumer (which knows the scheme) answers by
    /// calling `Engine::report_color_scheme` (#85).
    ColorSchemeQuery,
    /// The app set ANSI palette entry `index` to `spec` (OSC 4). One event per
    /// `index ; spec` pair in the sequence. The cell still references
    /// `Indexed(index)` — only the consumer's palette[index] changes, so the
    /// engine stays theme-agnostic (#122).
    SetPaletteColor { index: u8, spec: String },
    /// The app set the default foreground colour (OSC 10). Raw spec, forwarded
    /// for the consumer to apply — theme-agnostic, like [`SetBackground`](Self::SetBackground) (#122).
    SetForeground(String),
    /// The app set the default background colour (OSC 11). The engine is
    /// theme-agnostic, so it forwards the raw spec string (`rgb:…`/`#…`) for the
    /// consumer to parse and apply to its palette — it never holds hex (#122).
    SetBackground(String),
    /// The app reset palette entries to the theme default (OSC 104). `None` =
    /// the whole table (no argument); `Some(index)` = one entry, one event per
    /// index given. The consumer restores its palette (#122).
    ResetPaletteColor(Option<u8>),
    /// The app queried ANSI palette entry `index` (OSC 4 with `?` for that pair);
    /// the consumer answers with `report_palette_color` (#122).
    QueryPaletteColor { index: u8 },
    /// The app reset the default foreground to the theme default (OSC 110, #122).
    ResetForeground,
    /// The app reset the default background to the theme default (OSC 111, #122).
    ResetBackground,
    /// The app queried the default foreground colour (OSC 10 with `?`); the
    /// consumer answers with `report_foreground` (#122).
    QueryForeground,
    /// The app queried the default background colour (OSC 11 with `?`). The
    /// theme-agnostic engine relays it; the consumer answers with
    /// `report_background` (#122), mirroring `ColorSchemeQuery`.
    QueryBackground,
    /// A decoration marker's line left the buffer — evicted past the scrollback
    /// cap, or scrolled out of an in-screen region (#118). The handle is now
    /// dead; the consumer drops the decoration bound to it. This is the
    /// frame-mode equivalent of xterm's `IMarker.onDispose` — disposal is a
    /// point-in-time fact (a marker absent from a frame may merely be scrolled
    /// off-screen), so it rides the event queue, not the frame overlay.
    MarkerDisposed(MarkerId),
}
