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

/// Search modes beyond the default literal + smart-case (see [`Term::search_with`](crate::Term::search_with)).
/// Mirrors xterm.js's `ISearchOptions` (#314). The default (all off / smart-case) is exactly
/// [`Term::search`](crate::Term::search).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SearchOptions {
    /// Treat the query as a regular expression (the `regex` crate) instead of a literal substring.
    pub regex: bool,
    /// Match only where the run is bounded by non-word characters (a word char is alphanumeric or
    /// `_`) — the `\bword\b` sense, applied to both literal and regex queries.
    pub whole_word: bool,
    /// `None` = smart-case (case-insensitive iff the query has no uppercase); `Some(true)` =
    /// case-sensitive; `Some(false)` = force case-insensitive.
    pub case_sensitive: Option<bool>,
}
