//! Search results (see `docs/architecture.md` "Engine API": `search`).
//!
//! A `Match` is an inclusive range in **absolute buffer coordinates** (a line
//! index into `[scrollback ++ screen]`, the same coordinate the selection model
//! uses). The engine finds matches; the consumer drives next/prev navigation
//! (holding the `Vec<Match>` and calling `scroll_to_match`), mirroring
//! Alacritty's "engine finds, frontend navigates" split.

/// Whether `pattern` is a regex [`Term::search_with`](crate::Term::search_with) can run
/// (`opts.regex = true`) — a `true` guarantees `search_with` will *build* the pattern, and a
/// `false` is exactly the case it silently swallows into an empty result (#316 D2).
///
/// Validated under **case-insensitive** compilation, the most expansive: Unicode case-folding
/// grows the compiled program, so a `true` here holds whichever case mode smart-case / the
/// `case_sensitive` override later picks for the search. A case-*sensitive*-only check could pass
/// a pattern that then exceeds the `regex` size limit under `search_with`'s case-insensitive build
/// (`CompiledTooBig`), reintroducing the silent swallow for an all-lowercase near-limit pattern.
/// Grammar validity itself is case-flag-independent, so an invalid pattern (unbalanced group,
/// lookaround / backreferences the `regex` crate lacks) is rejected regardless.
///
/// A consumer surfaces invalid-regex with this rather than JS `RegExp`: the `regex` crate's grammar
/// differs (no lookaround/backreferences, Unicode-aware `\w \d \b`), so a JS-side check would
/// misjudge patterns and reproduce the D2 gap. Pattern-only (no `SearchOptions`) — the case flag
/// changes only compile size, covered here by validating the worst case.
pub fn is_valid_regex(pattern: &str) -> bool {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .is_ok()
}

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
    ///
    /// Caveats vs a JS `RegExp` (xterm.js): the `regex` crate has **no lookaround/backreferences**
    /// and its `\w \d \b` are **Unicode-aware** by default. An **invalid or unsupported pattern
    /// yields no matches** (an empty result) rather than an error — the current API has no error
    /// channel, so a consumer cannot distinguish a bad pattern from a genuine no-match (#314).
    /// Smart-case (see [`case_sensitive`](Self::case_sensitive)) infers case from the *raw* pattern,
    /// so an uppercase metacharacter (`\B`, `\D`, `\x1B`…) can flip case-sensitivity — set
    /// `case_sensitive` explicitly, or use an inline `(?i)`/`(?-i)`, to be sure.
    pub regex: bool,
    /// Match only where the run is bounded by non-word characters (a word char is alphanumeric or
    /// `_`) — the `\bword\b` sense, applied to both literal and regex queries.
    pub whole_word: bool,
    /// `None` = smart-case (case-insensitive iff the query has no uppercase); `Some(true)` =
    /// case-sensitive; `Some(false)` = force case-insensitive.
    pub case_sensitive: Option<bool>,
}
