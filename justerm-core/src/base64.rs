//! RFC 4648 base64, engine-local (#828).
//!
//! `OSC 52` carries its clipboard payload base64-encoded, so the engine decodes
//! inbound and encodes the reply. That is mechanism under ADR-0017 — it depends
//! only on the byte stream — and pushing it outward would make every consumer
//! carry a base64 implementation to use an engine feature. alacritty draws the
//! same line, decoding in its terminal core
//! (`alacritty_terminal/src/term/mod.rs:1717`, `:1743`); ghostty draws it one
//! layer out, handing its apprt the payload still encoded
//! (`src/termio/stream_handler.zig:1009`).
//!
//! **Why this is not a dependency.** `base64` is not in this workspace's
//! `Cargo.lock` at all, so adding it would be a genuinely new supply-chain entry
//! for `justerm-core` and everything downstream of it. `CLAUDE.md` names the
//! short dependency list as deliberate, and the four crates on it each do
//! something hard — a Paul-Williams parser, a regex engine, two Unicode tables.
//! RFC 4648 is a fixed 64-entry alphabet and no tables, it is proven here
//! against the RFC's own §10 vectors rather than against our idea of it, and the
//! caller needs a decoder that *refuses* malformed input rather than one whose
//! strictness is a configuration.
//!
//! **What "malformed" means here, and where it is deliberately laxer than
//! alacritty.** A byte outside the alphabet, a length that cannot be a base64
//! encoding, padding anywhere but at the end, and non-zero bits in a final
//! partial group are all rejected: each of those makes the *decoded* bytes
//! something the sender did not unambiguously write, which is the one thing the
//! caller must never hand a consumer. Missing padding is not in that class —
//! `Zm9vYmE` and `Zm9vYmE=` denote the same five bytes — so both are accepted,
//! where alacritty's `STANDARD` engine requires canonical padding and would drop
//! the first. The reach of unpadded emitters is **unmeasured**; the asymmetry is
//! what decides it, since rejecting costs a silently dropped clipboard and
//! accepting costs nothing.

/// The standard alphabet (RFC 4648 §4). Not the URL-safe one: `-` and `_` are
/// refused rather than mapped, because a payload written in one alphabet and
/// read in the other decodes to different bytes with no error anywhere.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The 6-bit value a character stands for, or `None` if it stands for nothing.
const fn sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decode a standard-alphabet base64 payload, or `None` if it is not one.
///
/// See the module note for exactly which inputs are refused. The `None` is what
/// the `OSC 52` handler turns into *no event at all*.
pub(crate) fn decode(input: &[u8]) -> Option<Vec<u8>> {
    // Padding is a suffix and never more than two characters.
    let pad = input.iter().rev().take_while(|&&b| b == b'=').count();
    if pad > 2 {
        return None;
    }
    // Interior padding needs no guard of its own, and the first draft's — a
    // `contains(&b'=')` over everything before the suffix — was measured dead:
    // mutating it away left all nine tests green. An `=` that is not the suffix
    // lands inside a quantum or the remainder, where `sextet` has no value for
    // it; a stray one after real padding fails the quantum check below. Adding
    // the guard back buys nothing and costs a branch no input can reach.
    // A payload that pads at all pads to a whole four-character quantum, which
    // is what makes the pad count implied rather than a second thing to check.
    if pad != 0 && !input.len().is_multiple_of(4) {
        return None;
    }
    let data = &input[..input.len() - pad];
    let mut out = Vec::with_capacity(data.len() / 4 * 3 + 2);
    let mut quanta = data.chunks_exact(4);
    for q in &mut quanta {
        let n = (u32::from(sextet(q[0])?) << 18)
            | (u32::from(sextet(q[1])?) << 12)
            | (u32::from(sextet(q[2])?) << 6)
            | u32::from(sextet(q[3])?);
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
    }
    match quanta.remainder() {
        [] => {}
        // One leftover character carries six bits and no whole byte, so there is
        // no input it could be the encoding of.
        [_] => return None,
        [a, b] => {
            let (a, b) = (sextet(*a)?, sextet(*b)?);
            if b & 0b1111 != 0 {
                return None; // bits the sender wrote that no byte receives
            }
            out.push((a << 2) | (b >> 4));
        }
        [a, b, c] => {
            let (a, b, c) = (sextet(*a)?, sextet(*b)?, sextet(*c)?);
            if c & 0b11 != 0 {
                return None;
            }
            out.push((a << 2) | (b >> 4));
            out.push(((b & 0b1111) << 4) | (c >> 2));
        }
        _ => unreachable!("chunks_exact(4) leaves at most three characters"),
    }
    Some(out)
}

/// Encode `input` as standard-alphabet base64, padded.
///
/// Padded because the reply is what a *client* parses, and every reference emits
/// the canonical form; the laxity above is for what we accept, never for what we
/// send.
pub(crate) fn encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut triples = input.chunks_exact(3);
    let glyph = |n: u32, shift: u32| ALPHABET[((n >> shift) & 0b11_1111) as usize] as char;
    for t in &mut triples {
        let n = (u32::from(t[0]) << 16) | (u32::from(t[1]) << 8) | u32::from(t[2]);
        out.extend([glyph(n, 18), glyph(n, 12), glyph(n, 6), glyph(n, 0)]);
    }
    match triples.remainder() {
        [] => {}
        [a] => {
            let n = u32::from(*a) << 16;
            out.extend([glyph(n, 18), glyph(n, 12), '=', '=']);
        }
        [a, b] => {
            let n = (u32::from(*a) << 16) | (u32::from(*b) << 8);
            out.extend([glyph(n, 18), glyph(n, 12), glyph(n, 6), '=']);
        }
        _ => unreachable!("chunks_exact(3) leaves at most two bytes"),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    /// RFC 4648 §10's vectors, which are the reason this module can be
    /// engine-local at all: it is checked against the specification rather than
    /// against a second implementation that could share its mistake.
    const RFC4648: [(&str, &str); 7] = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ];

    #[test]
    fn encodes_the_rfc4648_vectors() {
        for (plain, encoded) in RFC4648 {
            assert_eq!(encode(plain.as_bytes()), encoded, "encode({plain:?})");
        }
    }

    #[test]
    fn decodes_the_rfc4648_vectors() {
        for (plain, encoded) in RFC4648 {
            assert_eq!(
                decode(encoded.as_bytes()).as_deref(),
                Some(plain.as_bytes()),
                "decode({encoded:?})"
            );
        }
    }

    /// The whole 8-bit range, so the alphabet's `+` and `/` rows are exercised
    /// rather than assumed — an ASCII-only round trip never reaches them.
    #[test]
    fn round_trips_every_byte_value() {
        let all: Vec<u8> = (0..=255u8).collect();
        assert_eq!(decode(encode(&all).as_bytes()).as_deref(), Some(&all[..]));
    }

    #[test]
    fn refuses_a_byte_outside_the_alphabet() {
        // `-` and `_` are the URL-safe alphabet's substitutes; accepting them
        // would silently decode a payload the standard alphabet never wrote.
        for bad in ["Zm9v!!!!", "Zm-v", "Zm_v", "Zm9v Zm9v", "Zm9\u{f6}"] {
            assert_eq!(decode(bad.as_bytes()), None, "decode({bad:?})");
        }
    }

    /// A single leftover character encodes no whole byte, so there is no length
    /// it could be the encoding of.
    #[test]
    fn refuses_a_length_that_cannot_be_an_encoding() {
        for bad in ["Z", "Zm9vY", "Zm9vZm9vY"] {
            assert_eq!(decode(bad.as_bytes()), None, "decode({bad:?})");
        }
    }

    #[test]
    fn refuses_padding_that_is_not_at_the_end() {
        for bad in ["Zg==Zg==", "Z=g=", "=Zm9v", "Zm9v="] {
            assert_eq!(decode(bad.as_bytes()), None, "decode({bad:?})");
        }
    }

    /// `Zh==` and `Zg==` would decode to the same byte, so accepting the first
    /// means two payloads denote one answer and the engine has to guess which
    /// the sender wrote. Rejecting is what keeps decode injective.
    ///
    /// **`ZC==` and `ZmC=` are the cases that make this test able to fail for
    /// the right reason**, and they were added because it could not. Swapping
    /// the predicate for the plausible-but-wrong `& 0b1` — "is the last bit
    /// set" rather than "is any discarded bit set" — left the original three
    /// cases green, because each of them happens to have its lowest bit set as
    /// well. These two have a non-zero discarded nibble whose lowest bit is
    /// **zero**, which is the window the two predicates disagree in. The
    /// accepted neighbours below assert that window has a far side: without
    /// them a predicate that simply rejected everything would also pass.
    #[test]
    fn refuses_non_zero_bits_in_a_final_partial_group() {
        for bad in ["Zh==", "Zm9=", "Zm9vYmF=", "ZC==", "ZmC="] {
            assert_eq!(decode(bad.as_bytes()), None, "decode({bad:?})");
        }
        for good in ["Zg==", "ZA==", "Zm8=", "ZmA="] {
            assert!(decode(good.as_bytes()).is_some(), "decode({good:?})");
        }
    }

    /// The deliberate laxity, pinned so that tightening it later is a visible
    /// choice rather than an accident. See the module note.
    #[test]
    fn accepts_a_correct_payload_that_omits_its_padding() {
        assert_eq!(decode(b"Zg").as_deref(), Some(&b"f"[..]));
        assert_eq!(decode(b"Zm8").as_deref(), Some(&b"fo"[..]));
        assert_eq!(decode(b"Zm9vYmE").as_deref(), Some(&b"fooba"[..]));
    }

    /// The bytes tmux 3.2a was measured emitting on the RHEL 9 VM (#828, second
    /// comment) — the only payload this project has observed in the wild.
    #[test]
    fn decodes_the_payload_tmux_was_measured_emitting() {
        assert_eq!(
            decode(b"SEVMTE9KVVNURVJN").as_deref(),
            Some(&b"HELLOJUSTERM"[..])
        );
        assert_eq!(
            decode(b"SlVTVEVSTVBST0JF").as_deref(),
            Some(&b"JUSTERMPROBE"[..])
        );
    }
}
