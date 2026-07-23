//! Pins the constants the PUBLISHED README quotes to the definitions they name.
//!
//! `README.md` is this package's npm front page — wasm-pack copies it verbatim into the tarball
//! (`readme = "README.md"` in Cargo.toml). Nothing else checks it: no compiler reads it, no test
//! imports it, and its numbers are prose. So its canonical usage snippet asserted
//! `wireVersion() === 2` against a shipped `WIRE_VERSION` of 13 — eleven wire bumps — and a reader who
//! pasted it got a failing assert on the package's first page.
//!
//! `include_str!` is load-bearing: a moved or renamed README fails to COMPILE here rather than
//! quietly dropping the check.

use justerm_core::WIRE_VERSION;

const README: &str = include_str!("../README.md");

/// The wire version the usage snippet tells a consumer to assert must be the one we encode.
#[test]
fn readme_quotes_the_shipped_wire_version() {
    const NEEDLE: &str = "wireVersion() === ";

    let quoted: Vec<u8> = README
        .match_indices(NEEDLE)
        .filter_map(|(at, _)| {
            let digits: String = README[at + NEEDLE.len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            digits.parse().ok()
        })
        .collect();

    // Without this the pin passes vacuously the moment someone rewords the snippet — the check
    // would still be green while checking nothing.
    assert!(
        !quoted.is_empty(),
        "README.md no longer quotes `wireVersion() === N`. Restore the assertion in the usage \
         snippet, or delete this pin deliberately — do not leave it passing on an empty set."
    );

    for v in quoted {
        assert_eq!(
            v, WIRE_VERSION,
            "README.md tells consumers to assert `wireVersion() === {v}`, but this crate ships \
             WIRE_VERSION = {WIRE_VERSION}. Bumping the wire means updating the README in the same \
             change (theflow Step 6 — the published README is a behavior surface)."
        );
    }
}

/// The engine crate is `justerm-core`; the bare `justerm` name on crates.io is the frozen v0.5.1
/// tombstone left by the ADR-0010 rename. Telling a consumer to pin "the `justerm` crate" sends
/// them to a package that will never move again.
#[test]
fn readme_does_not_name_the_tombstone_crate() {
    assert!(
        !README.contains("`justerm` crate"),
        "README.md calls the engine \"the `justerm` crate\" — since v0.6.0 (ADR-0010) that name is \
         a frozen tombstone and the engine is `justerm-core`. (The bare word \"justerm\" is fine \
         where it means the family.)"
    );
}
