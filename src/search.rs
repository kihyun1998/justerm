//! Search results (see `docs/architecture.md` "Engine API": `search`).
//!
//! A `Match` is an inclusive range in **absolute buffer coordinates** (a line
//! index into `[scrollback ++ screen]`, the same coordinate the selection model
//! uses). The engine finds matches; the consumer drives next/prev navigation
//! (holding the `Vec<Match>` and calling `scroll_to_match`), mirroring
//! Alacritty's "engine finds, frontend navigates" split.

/// One literal match, inclusive on both ends, in absolute buffer coordinates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Match {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}
