//! #844 — every published struct either carries `#[non_exhaustive]` or carries the reason it
//! does not.
//!
//! **The rule this executes** is the acceptance criterion #844 wrote for itself: *"nobody looked"
//! and *"looked and declined"* must not leave the same trace. A struct with no attribute and no
//! recorded reason is indistinguishable from one the sweep never reached, and the trade here is a
//! one-way door — the attribute is free to add while the crate is `0.x` and a breaking change
//! afterwards — so an unexamined type is the failure, not an unmarked one.
//!
//! **The criterion the reasons are written against** lives in
//! `docs/map/territory/published-surface.md` § "The `#[non_exhaustive]` question, for structs".
//! Short form: a `Default` is the *consumer-chosen* form of the same forward compatibility the
//! attribute imposes, so where a `Default` exists or is meaningful, #843's rule — an exhaustive
//! type preserves the caller's option to be forced — decides it the same way it decided the enums.
//!
//! **Why this is a test and not a roster.** `docs/map/README.md` records what a hand-copied roster
//! costs (`#552`: stale in five places three days after it was written), and the sibling scan for
//! #843 (`justerm-wasm-decode/tests/wire_enum_stays_exhaustive.rs`) records the failure one level
//! up from that: its list of **core source files** omitted `cell.rs`, so every type in that file
//! was outside the scan with no roster entry left behind to go missing.
//!
//! This scan cannot have that hole. The published set is not listed here — it is **derived from
//! `lib.rs`'s own re-exports**, and a name that resolves to no declaration in the sources below is
//! a hard failure rather than a silent absence. Adding a module without adding it to
//! `CORE_SOURCES` therefore reddens this test instead of shrinking it.
//!
//! **`include_str!` is load-bearing**, as in `readme_pins.rs`: a moved or renamed source fails to
//! COMPILE here rather than quietly scanning nothing.

/// The crate root — the authority on what is published, and itself a source that declares one
/// public struct (`Engine`).
const LIB: &str = include_str!("../src/lib.rs");

/// Every module `lib.rs` re-exports from. A name that resolves to none of these fails the scan.
const CORE_SOURCES: &[(&str, &str)] = &[
    ("lib.rs", LIB),
    ("cell.rs", include_str!("../src/cell.rs")),
    ("color.rs", include_str!("../src/color.rs")),
    ("cursor.rs", include_str!("../src/cursor.rs")),
    ("damage.rs", include_str!("../src/damage.rs")),
    ("event.rs", include_str!("../src/event.rs")),
    ("grid.rs", include_str!("../src/grid.rs")),
    ("input.rs", include_str!("../src/input.rs")),
    ("logical.rs", include_str!("../src/logical.rs")),
    ("search.rs", include_str!("../src/search.rs")),
    ("selection.rs", include_str!("../src/selection.rs")),
    ("serialize.rs", include_str!("../src/serialize.rs")),
    ("term.rs", include_str!("../src/term.rs")),
];

/// What a scan found about one declaration.
#[derive(Debug)]
struct Decl {
    file: &'static str,
    attributed: bool,
    noted: bool,
}

/// The type names `lib.rs` publishes: everything inside a `pub use ...::{ .. }` list, plus the
/// single-item form, plus anything `lib.rs` declares itself. Lower-case items (`decode`, `encode`,
/// `is_valid_regex`) and SCREAMING constants are filtered out by the leading-capital test, which is
/// also what keeps a renamed function from silently entering the roster.
fn published_type_names(lib: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = lib;
    while let Some(at) = rest.find("pub use ") {
        rest = &rest[at + "pub use ".len()..];
        let body = match rest.find('{') {
            // `pub use m::{A, B};` — take the braced list
            Some(open) if rest[..open].find(';').is_none() => {
                let close = rest.find('}').expect("a `pub use` list must close");
                let list = &rest[open + 1..close];
                rest = &rest[close..];
                list.to_string()
            }
            // `pub use m::A;` — take the single item
            _ => {
                let end = rest.find(';').expect("a `pub use` must terminate");
                let item = rest[..end]
                    .rsplit("::")
                    .next()
                    .unwrap_or_default()
                    .to_string();
                rest = &rest[end..];
                item
            }
        };
        for name in body.split([',', '\n', ' ']) {
            let name = name.trim();
            if name.chars().next().is_some_and(char::is_uppercase)
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                && !name
                    .chars()
                    .all(|c| c.is_uppercase() || c == '_' || c.is_numeric())
            {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Find `pub struct NAME` in one source and report what surrounds it.
///
/// The attribute is read from the declaring line *and* from the lines above it, because
/// `#[non_exhaustive] pub struct X {` is legal Rust and an intervening `#[derive(..)]` must not
/// hide it — the same trap the #843 scan records having fallen into.
fn find_struct(name: &str) -> Option<Decl> {
    CORE_SOURCES
        .iter()
        .find_map(|(file, src)| scan(src, file, name))
}

/// The detector, over one source. Split out from [`find_struct`] so the controls can feed it a
/// fixture: every `pub struct` this crate declares is also published, so there is no crate-private
/// type in the tree to prove the "no reason recorded" answer with. A control that cannot be built
/// from the repository has to be built from a string.
fn scan(src: &str, file: &'static str, name: &str) -> Option<Decl> {
    {
        let lines: Vec<&str> = src.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let Some(at) = line.find(&format!("pub struct {name}")) else {
                continue;
            };
            // The character after the name must end it, or `Match` would match `MatchKind`.
            let after = line[at + format!("pub struct {name}").len()..]
                .chars()
                .next()
                .unwrap_or(' ');
            if after.is_alphanumeric() || after == '_' {
                continue;
            }
            // A declaration at the start of a line, or preceded only by attributes.
            let before = &line[..at];
            if !before.trim().is_empty() && !before.trim_start().starts_with("#[") {
                continue;
            }
            let attrs_above = lines[..i]
                .iter()
                .rev()
                .take_while(|l| l.trim_start().starts_with('#'));
            let attributed = line.contains("#[non_exhaustive]")
                || attrs_above.clone().any(|l| l.trim() == "#[non_exhaustive]");
            // The doc block sits above the attributes.
            let doc_start = lines[..i]
                .iter()
                .rev()
                .skip_while(|l| l.trim_start().starts_with('#'));
            let noted = doc_start
                .take_while(|l| l.trim_start().starts_with("///"))
                .any(|l| l.contains("#844"));
            return Some(Decl {
                file,
                attributed,
                noted,
            });
        }
    }
    None
}

#[test]
fn the_scan_can_tell_a_noted_declaration_from_an_unnoted_one() {
    // Controls, and they run first for the reason the #843 scan states: a scanner that answered
    // "fine" to everything would report a clean sweep, and "no violations" and "no eyes" are
    // otherwise the same result.
    const UNNOTED: &str = "/// Some ordinary type.\n#[derive(Debug)]\npub struct Widget {\n";
    const NOTED: &str = "/// Some ordinary type.\n///\n/// **No `#[non_exhaustive]` (#844).** \
                         Because.\n#[derive(Debug)]\npub struct Widget {\n";
    const ATTRIBUTED: &str = "/// Some ordinary type.\n#[derive(Debug)]\n#[non_exhaustive]\n\
                              pub struct Widget {\n";
    const ONE_LINE_ATTR: &str = "/// Some ordinary type.\n#[non_exhaustive] pub struct Widget {\n";

    let d = scan(UNNOTED, "fixture", "Widget").expect("the fixture declares Widget");
    assert!(!d.noted, "no #844 paragraph must read as unnoted");
    assert!(!d.attributed, "no attribute must read as unattributed");

    assert!(
        scan(NOTED, "fixture", "Widget").is_some_and(|d| d.noted && !d.attributed),
        "a #844 paragraph above the derive must be found"
    );
    assert!(
        scan(ATTRIBUTED, "fixture", "Widget").is_some_and(|d| d.attributed),
        "the attribute must be found through an intervening #[derive(..)]"
    );
    assert!(
        scan(ONE_LINE_ATTR, "fixture", "Widget").is_some_and(|d| d.attributed),
        "`#[non_exhaustive] pub struct X` on one line is legal Rust and must be found"
    );
    assert!(
        scan(UNNOTED, "fixture", "Widg").is_none(),
        "a prefix must not match: otherwise `Match` would resolve to `MatchKind`"
    );

    // And the detector must agree with the real tree on the one type #844 already decided.
    let frame = find_struct("Frame").expect("Frame is published and declared in serialize.rs");
    assert!(frame.noted, "Frame carries its #844 reason");
    assert!(
        !frame.attributed,
        "#844 decided AGAINST the attribute for Frame; if this flips, the reason recorded on it \
         is describing a different decision than the one in the code"
    );
}

#[test]
fn the_published_set_is_derived_and_not_empty() {
    let names = published_type_names(LIB);
    assert!(
        names.len() >= 30,
        "lib.rs publishes {} names, which is too few to be the real surface — the parser has \
         probably stopped matching `pub use`",
        names.len()
    );
    for expected in ["Frame", "Span", "KeyEvent", "Cell", "Row"] {
        assert!(
            names.contains(&expected.to_string()),
            "{expected} is published but the derived set does not contain it"
        );
    }
}

/// Whether a published name resolves to *any* declaration in the scanned sources.
///
/// This is the half that makes a missing source loud. A published name that is not a struct is
/// ordinary — #843 owns the enums, and there are consts and functions in the list too — so the
/// struct scan alone has to skip it, and a `continue` there is indistinguishable from a source
/// that fell out of `CORE_SOURCES`. Measured: dropping `selection.rs` from the list left this file
/// GREEN until this function existed, which is the #843 blind spot reproduced verbatim.
fn resolves_anywhere(name: &str) -> bool {
    CORE_SOURCES.iter().any(|(_, src)| {
        [
            "pub struct ",
            "pub enum ",
            "pub type ",
            "pub const ",
            "pub fn ",
            "pub trait ",
        ]
        .iter()
        .any(|kw| {
            src.lines().any(|l| {
                l.trim_start().strip_prefix(kw).is_some_and(|rest| {
                    rest.strip_prefix(name).is_some_and(|after| {
                        !after.starts_with(|c: char| c.is_alphanumeric() || c == '_')
                    })
                })
            })
        })
    })
}

#[test]
fn every_published_name_resolves_to_a_declaration_the_scan_can_see() {
    let unresolved: Vec<String> = published_type_names(LIB)
        .into_iter()
        .filter(|n| !resolves_anywhere(n))
        .collect();
    assert!(
        unresolved.is_empty(),
        "{} published name(s) resolve to no declaration in CORE_SOURCES. A source is missing from \
         that list, and every type in it is invisible to this scan with no entry left behind to go \
         missing: {unresolved:?}",
        unresolved.len()
    );
}

#[test]
fn every_published_struct_carries_an_attribute_or_a_reason() {
    let mut checked = 0;
    let mut missing = Vec::new();
    for name in published_type_names(LIB) {
        // Enums and type aliases resolve to no `pub struct`; #843 owns the enums. A name that
        // resolves to *nothing* is caught by `every_published_name_resolves_to_a_declaration...`,
        // which is what keeps this `continue` from swallowing a missing source.
        let Some(decl) = find_struct(&name) else {
            continue;
        };
        checked += 1;
        if !decl.attributed && !decl.noted {
            missing.push(format!("{name} ({})", decl.file));
        }
    }
    assert!(
        checked >= 25,
        "only {checked} published structs were resolved, which is fewer than this crate declares — \
         a source is missing from CORE_SOURCES and every type in it is invisible to this scan"
    );
    assert!(
        missing.is_empty(),
        "{} published struct(s) carry neither #[non_exhaustive] nor a recorded #844 reason, so \
         \"nobody looked\" and \"looked and declined\" leave the same trace: {missing:?}",
        missing.len()
    );
}
