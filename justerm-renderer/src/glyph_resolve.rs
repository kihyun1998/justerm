//! Pure, host-testable frame → per-cell glyph-slot resolution (the hot loop lifted out of
//! the browser-only `webgl::apply_frame`, #280).
//!
//! Splitting this out of the GL layer lets `cargo test` gate the three correctness gaps the
//! #264 adversarial pass surfaced — none of which the `#[cfg(wasm32)]` `apply_frame` could
//! reach on the host:
//! - **P0** within-frame LRU eviction corrupting earlier cells,
//! - **P1** a rasterise failure stranding a committed-but-unuploaded slot,
//! - **S** control/combining codepoints burning a normal-region slot.
//!
//! Rasterisation and atlas upload are **injected seams** (`rasterize` / `upload` closures),
//! so a test drives the resolver with a fake bitmap tag + a recording upload — no GL, no
//! OffscreenCanvas. `webgl::apply_frame` becomes a thin wrapper passing the real closures.

use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;

use crate::attrs::{font_style, is_wide_lead, is_wide_spacer};
use crate::glyph_cache::{EMOJI_FLAG, FontStyle, GlyphCache, GlyphKey, GlyphKind, GlyphSlot};

/// A frame's per-cell glyph inputs (dense row-major). Grouped so the resolver stays within the
/// argument budget once grapheme clusters (#285) join codepoints + flags.
pub struct Cells<'a> {
    pub cols: u32,
    pub rows: u32,
    /// Per-cell **base** codepoint (used when the cell has no cluster override).
    pub codepoints: &'a [u32],
    pub flags: &'a [u16],
    /// Per-cell grapheme-cluster override (#285): a non-empty string is the full grapheme (ZWJ
    /// sequence, skin-tone, flag, combining marks) to rasterise instead of the base codepoint.
    /// Empty (or a short slice) means "use the codepoint". The renderer NFC-normalises it when
    /// keying so a decomposed cluster and its precomposed form share one slot.
    pub clusters: &'a [String],
}

/// Why a frame could not be resolved. `Rasterize` carries the injected rasteriser's error;
/// `FrameExceedsCapacity` fires when one frame references more distinct glyphs than a
/// region can hold (so pinning the working set is impossible — surfaced, never silently
/// corrupting an earlier cell, #280 P0).
#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError<E> {
    Rasterize(E),
    FrameExceedsCapacity,
}

/// Map a raw codepoint to the grapheme to rasterise. C0 (`0x00..=0x1F`), DEL + C1
/// (`0x7F..=0x9F`) controls have no printable glyph, so they fold to space rather than
/// rasterise and burn a normal-region slot (#280 S). (Lone combining marks belong with
/// grapheme-cluster segmentation in #285, not here.)
fn sanitize_codepoint(cp: u32) -> char {
    match cp {
        0x00..=0x1F | 0x7F..=0x9F => ' ',
        _ => char::from_u32(cp).unwrap_or(' '),
    }
}

/// Resolve a frame ([`Cells`], dense row-major) to one atlas slot per cell.
pub fn resolve_frame<B, E>(
    cells: &Cells,
    cache: &mut GlyphCache,
    // Rasterise a grapheme and report whether the bitmap is a **colour emoji** (`is_color_bitmap`)
    // — the caller inspects the pixels it just drew (#284). An emoji is allocated in the wide
    // region with `EMOJI_FLAG`, so its glyph field tells the shader to sample the texture colour.
    mut rasterize: impl FnMut(&str, FontStyle, bool) -> Result<(B, bool), E>,
    mut upload: impl FnMut(u16, bool, B),
) -> Result<Vec<u16>, ResolveError<E>> {
    let Cells {
        cols,
        rows,
        codepoints,
        flags,
        clusters,
    } = *cells;
    let count = (cols * rows) as usize;
    let mut slots = Vec::with_capacity(count);
    // Glyphs already resolved in THIS frame — evicting one would corrupt an earlier cell.
    // Tracked per region (the normal and wide LRUs are independent, `glyph_cache`), so a key
    // referenced in one region can't spuriously protect an identical key in the other.
    let mut normal_frame: HashSet<GlyphKey> = HashSet::new();
    let mut wide_frame: HashSet<GlyphKey> = HashSet::new();
    // A wide lead assigns its right half to the following (spacer) cell.
    let mut pending_right: Option<u16> = None;
    for idx in 0..count {
        // A wide glyph never spans rows (core wraps a lead off the last column), so a
        // pending right-half must not leak across a row boundary — reset at col 0.
        if (idx as u32).is_multiple_of(cols) {
            pending_right = None;
        }
        let cell_flags = flags.get(idx).copied().unwrap_or(0);

        // A spacer draws the lead glyph's right-half slot (no glyph of its own).
        if is_wide_spacer(cell_flags) {
            slots.push(pending_right.take().unwrap_or(0));
            continue;
        }
        pending_right = None;

        let wide = is_wide_lead(cell_flags);
        let kind = if wide {
            GlyphKind::Wide
        } else {
            GlyphKind::Normal
        };
        // A cell with a grapheme-cluster override (#285) keys the whole cluster — NFC-normalised
        // so a decomposed form and its precomposed equal share one slot — else its base codepoint
        // (controls fold to space, #280 S).
        let text = match clusters.get(idx) {
            Some(cluster) if !cluster.is_empty() => cluster.nfc().collect::<String>(),
            _ => {
                let cp = codepoints.get(idx).copied().unwrap_or(0x20);
                sanitize_codepoint(cp).to_string()
            }
        };
        let key = GlyphKey {
            text,
            style: font_style(cell_flags),
        };

        let region = if kind == GlyphKind::Normal {
            &mut normal_frame
        } else {
            &mut wide_frame
        };

        let slot = match cache.touch(&key, kind) {
            Some(slot) => slot,
            None => {
                // Reject the frame BEFORE any mutation if the next eviction would drop a
                // glyph this frame still references (#280 P0): more distinct glyphs than the
                // region can hold is impossible to pack, so surface it rather than silently
                // corrupt the earlier cell — and don't strand a committed-but-unuploaded slot.
                // (Emoji share the wide region, so the `kind` used here is region-correct.)
                if let Some(victim) = cache.next_eviction(kind)
                    && region.contains(victim)
                {
                    return Err(ResolveError::FrameExceedsCapacity);
                }
                // Rasterise BEFORE committing the cache entry: a failure returns here with
                // the cache untouched, so no slot is left "resident but never uploaded"
                // (#280 P1). The rasteriser reports colour-emoji-ness from the pixels it drew;
                // an emoji is committed as `Emoji` (wide region + `EMOJI_FLAG`). Only on
                // success do we commit + upload.
                let (bitmap, is_emoji) =
                    rasterize(&key.text, key.style, wide).map_err(ResolveError::Rasterize)?;
                // Emoji live in the wide region (EMOJI_FLAG + a 2-slot span), so only a *wide*
                // colour glyph is upgraded to `Emoji`. A width-1 colour glyph (is_emoji && !wide)
                // stays `Normal` — routing it to the wide region would store it in the wide LRU
                // but look it up in the normal LRU (via `kind` above), re-rasterising it every
                // frame. It renders monochrome instead; colour width-1 glyphs are tracked (#297).
                let alloc_kind = if is_emoji && wide {
                    GlyphKind::Emoji
                } else {
                    kind
                };
                let alloc = cache.get_or_insert(key.clone(), alloc_kind);
                upload(alloc.slot.slot_id(), wide, bitmap);
                alloc.slot
            }
        };
        region.insert(key);
        // The glyph field carries the bare slot for a text/CJK glyph, or the slot | EMOJI_FLAG
        // (bit 15) for a colour emoji so the shader samples the atlas colour. The wide spacer
        // (right half) carries the same emoji bit.
        let base = slot.slot_id();
        let is_emoji = matches!(slot, GlyphSlot::Emoji(_));
        let field = if is_emoji { base | EMOJI_FLAG } else { base };
        slots.push(field);
        if wide {
            pending_right = Some(if is_emoji {
                (base + 1) | EMOJI_FLAG
            } else {
                base + 1
            });
        }
    }
    Ok(slots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyph_cache::{ASCII_SLOTS, NORMAL_CAPACITY};

    /// A rasterise/upload seam that panics if touched — for frames that must resolve
    /// entirely from pre-baked/resident slots.
    fn no_raster(_: &str, _: FontStyle, _: bool) -> Result<(String, bool), ()> {
        panic!("rasterize must not be called for this frame");
    }
    fn no_upload(_: u16, _: bool, _: String) {
        panic!("upload must not be called for this frame");
    }

    /// Build a cluster-free [`Cells`] (the common case — one codepoint per cell).
    fn cells<'a>(cols: u32, rows: u32, codepoints: &'a [u32], flags: &'a [u16]) -> Cells<'a> {
        Cells {
            cols,
            rows,
            codepoints,
            flags,
            clusters: &[],
        }
    }

    #[test]
    fn all_ascii_frame_resolves_to_fast_path_slots_without_rasterising() {
        // "AB" on one row. Fast-path slot = codepoint - 0x20 (worked by hand from the
        // ASCII fast-path spec): 'A' 0x41 -> 33, 'B' 0x42 -> 34. No glyph is new, so neither
        // the rasterize nor the upload seam is touched.
        let mut cache = GlyphCache::new();
        let slots = resolve_frame(
            &cells(2, 1, &[0x41, 0x42], &[0, 0]),
            &mut cache,
            no_raster,
            no_upload,
        )
        .expect("frame resolves");
        assert_eq!(slots, vec![33, 34]);
    }

    #[test]
    fn a_new_normal_glyph_is_rasterised_then_uploaded_to_its_allocated_slot() {
        // '→' U+2192 is not ASCII, so it lands at the first cached normal slot, ASCII_SLOTS
        // = 95 (worked by hand from the reserved-ASCII spec). It is new → rasterised once and
        // uploaded once, to slot 95, non-wide.
        let mut cache = GlyphCache::new();
        let mut rasterized: Vec<(String, FontStyle, bool)> = Vec::new();
        let mut uploaded: Vec<(u16, bool, String)> = Vec::new();
        let slots = resolve_frame(
            &cells(1, 1, &[0x2192], &[0]),
            &mut cache,
            |t, s, w| {
                rasterized.push((t.to_string(), s, w));
                Ok::<(String, bool), ()>((t.to_string(), false))
            },
            |slot, w, b| uploaded.push((slot, w, b)),
        )
        .expect("frame resolves");
        assert_eq!(slots, vec![95]);
        assert_eq!(
            rasterized,
            vec![("→".to_string(), FontStyle::Normal, false)]
        );
        assert_eq!(uploaded, vec![(95, false, "→".to_string())]);
    }

    #[test]
    fn a_rasterise_failure_commits_nothing_and_uploads_nothing() {
        // #280 P1: the buggy order committed the cache entry, then `?`-returned on a
        // rasterise failure, stranding a "resident but never uploaded" slot forever. The
        // resolver rasterises first, so a failure leaves the cache empty and the atlas
        // untouched — the frame is rejected coherently.
        let mut cache = GlyphCache::new();
        let mut uploads = 0;
        let err = resolve_frame(
            &cells(1, 1, &[0x2192], &[0]),
            &mut cache,
            |_, _, _| Err::<(String, bool), &str>("boom"),
            |_, _, _| uploads += 1,
        )
        .unwrap_err();
        assert_eq!(err, ResolveError::Rasterize("boom"));
        assert_eq!(cache.len(), 0, "a rasterise failure commits no cache entry");
        assert_eq!(uploads, 0, "nothing is uploaded on failure");
    }

    #[test]
    fn a_frame_with_more_distinct_glyphs_than_capacity_is_surfaced_not_corrupted() {
        // #280 P0: the normal region has NORMAL_CAPACITY - ASCII_SLOTS = 1953 free slots. A
        // single frame of 1954 distinct non-ASCII glyphs cannot all stay resident, so packing
        // the 1954th would evict a glyph an earlier cell still points at (silent corruption).
        // The resolver surfaces this as FrameExceedsCapacity instead of emitting a bad frame.
        let free = (NORMAL_CAPACITY - ASCII_SLOTS) as u32; // 1953
        let n = free + 1; // 1954 distinct glyphs
        // Distinct non-ASCII codepoints from the Supplementary-Multilingual-agnostic BMP
        // range 0x2200.. (all single-width symbols, none in 0x20..=0x7E).
        let cps: Vec<u32> = (0..n).map(|i| 0x2200 + i).collect();
        let flags = vec![0u16; n as usize];
        let mut cache = GlyphCache::new();
        let err = resolve_frame(
            &cells(n, 1, &cps, &flags),
            &mut cache,
            |t, _, _| Ok::<(String, bool), ()>((t.to_string(), false)),
            |_, _, _| {},
        )
        .unwrap_err();
        assert_eq!(err, ResolveError::FrameExceedsCapacity);
    }

    #[test]
    fn a_wide_frame_over_capacity_is_surfaced_like_the_normal_region() {
        use crate::attrs::WIDE_CHAR;
        use crate::glyph_cache::WIDE_CAPACITY;
        // The wide region holds WIDE_CAPACITY = 2048 double-width glyphs. A frame of 2049
        // distinct wide (CJK) glyphs overflows it, and the guard must fire for the wide LRU
        // too — not only the normal region (the wide branch of next_eviction / the wide
        // frame set). Distinct CJK codepoints from 0x4E00, each flagged WIDE_CHAR.
        let n = WIDE_CAPACITY as u32 + 1; // 2049
        let cps: Vec<u32> = (0..n).map(|i| 0x4E00 + i).collect();
        let flags = vec![WIDE_CHAR; n as usize];
        let mut cache = GlyphCache::new();
        let err = resolve_frame(
            &cells(n, 1, &cps, &flags),
            &mut cache,
            |t, _, _| Ok::<(String, bool), ()>((t.to_string(), false)),
            |_, _, _| {},
        )
        .unwrap_err();
        assert_eq!(err, ResolveError::FrameExceedsCapacity);
    }

    #[test]
    fn a_glyph_referenced_twice_within_capacity_keeps_one_stable_slot() {
        // The common case: an early glyph re-referenced later must not be evicted mid-frame.
        // Two distinct glyphs (well within capacity), pattern [X, Y, X] → [95, 96, 95].
        let mut cache = GlyphCache::new();
        let slots = resolve_frame(
            &cells(3, 1, &[0x2192, 0x2190, 0x2192], &[0, 0, 0]),
            &mut cache,
            |t, _, _| Ok::<(String, bool), ()>((t.to_string(), false)),
            |_, _, _| {},
        )
        .expect("frame within capacity resolves");
        assert_eq!(slots, vec![95, 96, 95]);
    }

    #[test]
    fn a_wide_lead_takes_two_slots_and_its_spacer_draws_the_right_half() {
        use crate::attrs::{WIDE_CHAR, WIDE_CHAR_SPACER};
        // "中" (wide) at col 0, its spacer at col 1, 'A' at col 2. The wide region starts at
        // WIDE_BASE = 2048; the lead takes 2048 and its spacer draws the right half 2049.
        // 'A' is the ASCII fast path (33). The wide glyph is rasterised once (wide=true) and
        // uploaded once at its base; the spacer rasterises nothing.
        let mut cache = GlyphCache::new();
        let mut rasterized: Vec<(String, FontStyle, bool)> = Vec::new();
        let mut uploaded: Vec<(u16, bool, String)> = Vec::new();
        let slots = resolve_frame(
            &cells(
                3,
                1,
                &[0x4E2D, 0x0, 0x41],
                &[WIDE_CHAR, WIDE_CHAR_SPACER, 0],
            ),
            &mut cache,
            |t, s, w| {
                rasterized.push((t.to_string(), s, w));
                Ok::<(String, bool), ()>((t.to_string(), false))
            },
            |slot, w, b| uploaded.push((slot, w, b)),
        )
        .expect("frame resolves");
        assert_eq!(slots, vec![2048, 2049, 33]);
        assert_eq!(
            rasterized,
            vec![("中".to_string(), FontStyle::Normal, true)]
        );
        assert_eq!(uploaded, vec![(2048, true, "中".to_string())]);
    }

    #[test]
    fn a_colour_emoji_is_flagged_in_the_wide_region_lead_and_spacer() {
        use crate::attrs::{WIDE_CHAR, WIDE_CHAR_SPACER};
        use crate::glyph_cache::WIDE_BASE;
        // A wide emoji lead at col 0 + its spacer at col 1. The rasteriser reports is_emoji=true,
        // so the glyph allocates in the wide region (base WIDE_BASE) AND its field carries
        // EMOJI_FLAG (bit 15) — the shader's colour-sample signal. The spacer draws the right half
        // (base+1) with the same emoji bit. Upload addresses the texture with the BARE slot.
        let mut cache = GlyphCache::new();
        let mut uploaded: Vec<(u16, bool)> = Vec::new();
        let slots = resolve_frame(
            &cells(2, 1, &[0x1F680, 0x0], &[WIDE_CHAR, WIDE_CHAR_SPACER]),
            &mut cache,
            |_t, _s, _w| Ok::<(String, bool), ()>(("rocket".to_string(), true)),
            |slot, w, _b| uploaded.push((slot, w)),
        )
        .expect("frame resolves");
        assert_eq!(
            slots,
            vec![WIDE_BASE | EMOJI_FLAG, (WIDE_BASE + 1) | EMOJI_FLAG]
        );
        assert_eq!(
            slots[0] & !EMOJI_FLAG,
            WIDE_BASE,
            "bare slot addresses the texture"
        );
        assert_eq!(
            uploaded,
            vec![(WIDE_BASE, true)],
            "upload uses the bare (flag-free) slot"
        );
    }

    #[test]
    fn a_narrow_colour_glyph_stays_normal_and_caches_without_thrash() {
        use crate::glyph_cache::NORMAL_CAPACITY;
        // A width-1 glyph (no WIDE_CHAR) the font colour-draws reports is_emoji=true but is NOT
        // wide. Emoji occupy the wide region, so upgrading it there would store it in the wide
        // LRU while `touch` looks in the normal LRU → a re-rasterise every frame. Gating the
        // upgrade on `wide` keeps it Normal (rendered monochrome), region-consistent + cached.
        let mut cache = GlyphCache::new();
        let mut raster_calls = 0;
        let f1 = resolve_frame(
            &cells(1, 1, &[0x2764], &[0]), // ❤ codepoint, flags have no WIDE_CHAR
            &mut cache,
            |_t, _s, _w| {
                raster_calls += 1;
                Ok::<(String, bool), ()>(("x".to_string(), true)) // font drew it in colour
            },
            |_s, _w, _b| {},
        )
        .expect("resolves")[0];
        assert_eq!(
            f1 & EMOJI_FLAG,
            0,
            "narrow colour glyph is not emoji-flagged"
        );
        assert!(f1 < NORMAL_CAPACITY, "stays in the normal region");
        // Second frame: cached in the normal region → touch hits, no re-rasterise (no thrash).
        let f2 = resolve_frame(
            &cells(1, 1, &[0x2764], &[0]),
            &mut cache,
            |_t, _s, _w| {
                raster_calls += 1;
                Ok::<(String, bool), ()>(("x".to_string(), true))
            },
            |_s, _w, _b| {},
        )
        .expect("resolves")[0];
        assert_eq!(f2, f1, "same slot, region-consistent");
        assert_eq!(
            raster_calls, 1,
            "rasterised once, not re-rasterised each frame"
        );
    }

    #[test]
    fn control_codepoints_map_to_space_and_burn_no_slot() {
        // #280 S: C0 (0x00..=0x1F), DEL (0x7F) and C1 (0x80..=0x9F) controls have no printable
        // glyph — they must map to space (fast-path slot 0 = 0x20 - 0x20) rather than
        // rasterise and consume a normal-region slot. No rasterise/upload happens.
        let mut cache = GlyphCache::new();
        let slots = resolve_frame(
            &cells(5, 1, &[0x01, 0x1F, 0x7F, 0x80, 0x9F], &[0, 0, 0, 0, 0]),
            &mut cache,
            no_raster,
            no_upload,
        )
        .expect("frame resolves");
        assert_eq!(
            slots,
            vec![0, 0, 0, 0, 0],
            "every control resolves to the space slot"
        );
        assert_eq!(cache.len(), 0, "controls burn no cache slot");
    }

    #[test]
    fn a_cluster_override_rasterises_the_whole_grapheme_not_the_base_codepoint() {
        // #285: a ZWJ sequence 👨‍👩‍👧 rides the cluster column; its cell's base codepoint is only
        // the first scalar (👨). The resolver must rasterise the FULL cluster string, keyed once.
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        let mut cache = GlyphCache::new();
        let mut rasterized: Vec<String> = Vec::new();
        let slots = resolve_frame(
            &Cells {
                cols: 1,
                rows: 1,
                codepoints: &[0x1F468], // base scalar only
                flags: &[0],
                clusters: &[family.to_string()],
            },
            &mut cache,
            |t, _, _| {
                rasterized.push(t.to_string());
                Ok::<(String, bool), ()>((t.to_string(), false))
            },
            |_, _, _| {},
        )
        .expect("frame resolves");
        assert_eq!(
            slots,
            vec![ASCII_SLOTS],
            "cluster lands at the first cached slot"
        );
        assert_eq!(
            rasterized,
            vec![family],
            "the whole cluster is rasterised, not just 👨"
        );
    }

    #[test]
    fn a_decomposed_cluster_is_nfc_keyed_to_share_its_precomposed_slot() {
        // "é" as base 'e' + combining acute (decomposed) must NFC-normalise to U+00E9 and share
        // one slot with a precomposed "é" — no duplicate glyph burned. Two cells: [decomposed
        // cluster, precomposed base] → both resolve to the same single new slot, rasterised once.
        let mut cache = GlyphCache::new();
        let mut rasterized: Vec<String> = Vec::new();
        let slots = resolve_frame(
            &Cells {
                cols: 2,
                rows: 1,
                codepoints: &[0x65, 0x00E9], // 'e' (base of the decomposed cell), precomposed 'é'
                flags: &[0, 0],
                clusters: &["e\u{0301}".to_string(), String::new()], // cell0 = decomposed cluster
            },
            &mut cache,
            |t, _, _| {
                rasterized.push(t.to_string());
                Ok::<(String, bool), ()>((t.to_string(), false))
            },
            |_, _, _| {},
        )
        .expect("frame resolves");
        assert_eq!(
            slots,
            vec![ASCII_SLOTS, ASCII_SLOTS],
            "both cells share one slot"
        );
        assert_eq!(
            rasterized,
            vec!["é".to_string()],
            "keyed once as NFC 'é' (U+00E9)"
        );
        assert_eq!(cache.len(), 1, "no duplicate slot burned");
    }
}
