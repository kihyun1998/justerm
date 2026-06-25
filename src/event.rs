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
}
