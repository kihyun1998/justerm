#![no_main]
//! Fuzz the wire-format decoder. Sibling of the `decode_never_panics` proptest in
//! tests/robustness.rs — same entry point, but libFuzzer's `-timeout` also catches hangs and
//! coverage guidance reaches the length-driven span/side-table/link-table paths random bytes miss.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = justerm::decode(data);
});
