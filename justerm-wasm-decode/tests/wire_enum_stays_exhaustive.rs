//! #843 — an enum this crate maps onto wire values must stay exhaustive in core.
//!
//! **The rule.** `#[non_exhaustive]` binds only across a crate boundary, so an enum
//! core marks and this crate matches would need a `_` arm here — and that arm turns a
//! future *compile error* into a silently wrong wire value. Four enums are in that
//! position (`CursorShape`, `FrameKind`, `MarkerKind`, `UnderlineStyle`); the rest are
//! free to take the attribute.
//!
//! **The fourth arrived after the scan was written, and the scan could not see it (#831).**
//! `UnderlineStyle` lives in `cell.rs`, which was not among the sources below, so the
//! roster had no chance of naming it — the class Control 1 exists for, one level up: a
//! *file* the scan cannot see hides every type in it, and unlike a lost type there is no
//! roster entry left behind to go missing. The repair is the source list, not an entry.
//!
//! **Why this is a test and not a paragraph.** The rule was written after
//! `cargo test --workspace` reddened on `MarkerKind`, and that gate is *not* a detector
//! for it: it fired because `MarkerKind`'s wire mapping happens to live one crate over.
//! `Color` is wire-carried too and its only exhaustive match is *inside* core, where the
//! attribute has no effect — so marking `Color` would have left the whole workspace
//! green. Until this file existed the rule was held by prose and by nothing executable.
//!
//! **`include_str!` is load-bearing**, as in `readme_pins.rs`: a moved or renamed source
//! fails to COMPILE here rather than quietly scanning nothing.
//!
//! **This is a source scan, so it is only as good as its own instrument** — which is why
//! the controls below run first. A scanner that silently matched nothing would report a
//! clean sweep, and "no violations" and "no eyes" are otherwise the same result.

/// The encoder — every `match` that turns a core enum into a wire number lives here.
const ENCODER: &str = include_str!("../src/lib.rs");

/// Core's public-enum sources, by the module each type lives in.
const CORE_SOURCES: &[(&str, &str)] = &[
    ("cell.rs", include_str!("../../justerm-core/src/cell.rs")),
    ("color.rs", include_str!("../../justerm-core/src/color.rs")),
    (
        "cursor.rs",
        include_str!("../../justerm-core/src/cursor.rs"),
    ),
    (
        "damage.rs",
        include_str!("../../justerm-core/src/damage.rs"),
    ),
    ("event.rs", include_str!("../../justerm-core/src/event.rs")),
    ("input.rs", include_str!("../../justerm-core/src/input.rs")),
    (
        "selection.rs",
        include_str!("../../justerm-core/src/selection.rs"),
    ),
    (
        "serialize.rs",
        include_str!("../../justerm-core/src/serialize.rs"),
    ),
];

/// Every `pub enum` core declares, paired with whether it is `#[non_exhaustive]`.
///
/// The attribute is read from the lines *above* the declaration — an intervening
/// `#[derive(..)]`, which every one of these has, must not hide it — **and from the
/// declaring line itself**, because `#[non_exhaustive] pub enum X {` is legal Rust.
/// The first draft anchored on `line.starts_with("pub enum ")` and a one-line
/// attribute made the whole enum vanish from the scan, which is worse than missing
/// the attribute: a type that is not in the list cannot violate anything. Found by
/// mutating this file's own subject, not by reading it.
fn public_enums() -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for (_, src) in CORE_SOURCES {
        let lines: Vec<&str> = src.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let Some(at) = line.find("pub enum ") else {
                continue;
            };
            // Only a declaration at the start of a line, or one preceded solely by
            // attributes — never `pub enum` inside a string or a doc-comment.
            let before = &line[..at];
            if !before.trim().is_empty() && !before.trim_start().starts_with("#[") {
                continue;
            }
            let name = line[at + "pub enum ".len()..]
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_end_matches('{')
                .to_string();
            let marked = line.contains("#[non_exhaustive]")
                || lines[..i]
                    .iter()
                    .rev()
                    .take_while(|l| l.trim_start().starts_with('#'))
                    .any(|l| l.trim() == "#[non_exhaustive]");
            out.push((name, marked));
        }
    }
    out
}

/// The public enums core is known to declare in these files. A **named** roster
/// rather than a count: `>= 15` was the first draft's control, and it could not
/// notice one enum disappearing from the scan, because the files also declare two
/// crate-private ones (`MouseProtocol`, `MouseEncoding`) that pad the total.
const EXPECTED: &[&str] = &[
    "UnderlineStyle",
    "Color",
    "CursorShape",
    "TermDamage",
    "ClipboardTarget",
    "TermEvent",
    "Terminator",
    "KeypadKey",
    "Key",
    "KeyAction",
    "MouseButton",
    "MouseAction",
    "SelectionType",
    "Side",
    "FrameKind",
    "MarkerKind",
    "DecodeError",
];

/// Whether the encoder pattern-matches on this enum — `Name::Variant =>` on one line,
/// which is the shape every wire mapping in `lib.rs` has.
fn matched_in_encoder(name: &str) -> bool {
    let needle = format!("{name}::");
    ENCODER
        .lines()
        .any(|l| l.contains(&needle) && l.contains("=>"))
}

/// **Control 1: the scanner sees enums at all**, and sees both kinds. Without this a
/// broken parse reports zero violations and reads as a pass.
#[test]
fn the_scanner_finds_both_marked_and_unmarked_enums() {
    let enums = public_enums();
    let found: Vec<&str> = enums.iter().map(|(n, _)| n.as_str()).collect();
    let missing: Vec<&&str> = EXPECTED.iter().filter(|e| !found.contains(e)).collect();
    assert!(
        missing.is_empty(),
        "the scan lost {missing:?} — a type it cannot see cannot violate the invariant, \
         so this is the failure that matters most. Found: {found:?}"
    );
    assert!(
        enums.iter().any(|(_, marked)| *marked),
        "found no #[non_exhaustive] enum — the attribute detector is blind, so the \
         invariant below cannot fail for the right reason"
    );
    assert!(
        enums.iter().any(|(_, marked)| !*marked),
        "found no plain enum — the detector is reporting everything as marked"
    );
}

/// **Control 2: the encoder scan sees the three known wire mappings.** If `lib.rs` is
/// refactored so these stop matching this shape, the invariant test goes vacuous, and
/// this is what says so.
#[test]
fn the_scanner_finds_the_known_wire_mappings() {
    for name in ["CursorShape", "FrameKind", "MarkerKind", "UnderlineStyle"] {
        assert!(
            matched_in_encoder(name),
            "{name} is mapped onto a wire value in src/lib.rs, and the scan missed it — \
             the invariant test below is vacuous until this passes"
        );
    }
    assert!(
        !matched_in_encoder("Color"),
        "Color's wire mapping is `encode_color`, INSIDE justerm-core, which is exactly \
         why cargo test --workspace cannot police this rule — if it is matched here now, \
         the rule's own example has changed and the note in serialize.rs is stale"
    );
}

/// The invariant.
///
/// A failure is not necessarily a bug in *this* file: if a future change legitimately
/// matches a non-wire enum here behind a `_` arm, the honest repair is to narrow this
/// scan to the encode path rather than to delete the assertion.
#[test]
fn no_enum_this_crate_matches_is_non_exhaustive() {
    let offenders: Vec<String> = public_enums()
        .into_iter()
        .filter(|(name, marked)| *marked && matched_in_encoder(name))
        .map(|(name, _)| name)
        .collect();

    assert!(
        offenders.is_empty(),
        "these enums are #[non_exhaustive] in core AND matched in this crate's wire \
         encoder: {offenders:?}. Across a crate boundary that forces a `_` arm, which \
         turns the next added variant from a compile error into a silently wrong wire \
         value. See the rule on justerm_core::MarkerKind."
    );
}
