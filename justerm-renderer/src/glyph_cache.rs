//! Dynamic glyph atlas — slot map + LRU eviction. Pure, host `cargo test`-able (no GL).
//!
//! Mirrors beamterm `beamterm-core/src/gl/glyph_cache.rs`: two LRU regions (normal +
//! double-width), O(1) lookup/insert/evict, and a fast path that pre-allocates fixed
//! slots for normal-styled ASCII (no cache churn for the common case). The classification
//! (unicode-width / emoji detection) lives in the rasteriser layer (#264/#268); this cache
//! only allocates slots, taking a [`GlyphKind`] from the caller.
//!
//! ## Consumer contract (#264/#268)
//! - **Address the texture via [`GlyphSlot::slot_id`]**, never the raw `Emoji(v)` inner
//!   value — it carries [`EMOJI_FLAG`] (bit 15) which must be stripped before slot maths.
//! - A `Wide`/`Emoji` glyph occupies **two** physical atlas cells (`idx`, `idx+1`); on
//!   `is_new` the caller uploads both, and emits `idx+1` as the right (spacer) half.
//! - Derive [`GlyphKind::Wide`] from the **core frame's** width (justerm-core already marks
//!   wide/spacer cells) — do not re-run unicode-width in the renderer (two width tables can
//!   disagree). Emoji-ness is the one thing the rasteriser adds.
//! - Normalise a glyph's `text` to **NFC** and force emoji style to [`FontStyle::Normal`]
//!   before keying, so canonically-equal graphemes don't burn duplicate slots.

use lru::LruCache;

/// Font style — part of a glyph's atlas identity (bold `A` ≠ normal `A`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Normal,
    Bold,
    Italic,
    BoldItalic,
}

/// A glyph's identity in the atlas: its grapheme text + font style.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub text: String,
    pub style: FontStyle,
}

/// Which atlas region + addressing a glyph occupies. The inner value is the texture
/// slot index (emoji carries [`EMOJI_FLAG`] outside the slot bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphSlot {
    Normal(u16),
    Wide(u16),
    Emoji(u16),
}

impl GlyphSlot {
    /// The bare slot index (emoji flag stripped). This is the value a consumer addresses
    /// the texture with — the raw `Emoji(v)` inner value carries [`EMOJI_FLAG`] and must
    /// never be used as a slot address directly.
    pub fn slot_id(self) -> u16 {
        match self {
            GlyphSlot::Normal(i) | GlyphSlot::Wide(i) => i,
            GlyphSlot::Emoji(i) => i & !EMOJI_FLAG,
        }
    }
}

/// Glyphs per texture-array layer (a 1×32 vertical strip). Also `NUM_LAYERS = slots/32`.
pub const GLYPHS_PER_LAYER: u16 = 32;

/// Map a bare slot index to its `(layer, band)` in the texture array: 32 glyphs stack
/// vertically per layer, so `layer = slot / 32` and `band = slot % 32`. The upload path
/// and the fragment shader must agree on this (shader: `layer = idx >> 5`, `pos = idx & 31`).
pub fn slot_texcoord(slot: u16) -> (u16, u16) {
    (slot / GLYPHS_PER_LAYER, slot % GLYPHS_PER_LAYER)
}

/// Caller-supplied glyph classification (the cache does not run unicode-width/emoji
/// detection — that is the rasteriser's job).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphKind {
    Normal,
    Wide,
    Emoji,
    /// A **width-1 colour emoji** (#297 case 1): a glyph the font colour-draws but core marks
    /// single-width (`❤` without VS16). It lives in the **normal** region (so it never thrashes
    /// the wide LRU) yet allocates as [`GlyphSlot::Emoji`] carrying [`EMOJI_FLAG`], so the shader
    /// still colour-samples it — bit 15 is free for a normal slot (`≤ 0x7FF`), independent of the
    /// 13-bit slot address.
    EmojiNarrow,
}

impl GlyphKind {
    /// Whether this kind allocates in (and is looked up from) the **normal** region. Emoji-ness
    /// is orthogonal to region: [`GlyphKind::EmojiNarrow`] is a normal-region emoji.
    fn is_normal_region(self) -> bool {
        matches!(self, GlyphKind::Normal | GlyphKind::EmojiNarrow)
    }
}

/// Pre-allocated slots for normal-styled ASCII glyphs (`0x20..=0x7E`).
pub const ASCII_SLOTS: u16 = 0x7E - 0x20 + 1; // 95
/// Normal region: slots `0..2048` (single-width glyphs).
pub const NORMAL_CAPACITY: u16 = 2048;
/// Wide region: 2048 double-width glyphs, 2 slots each → slots `2048..6144`.
pub const WIDE_CAPACITY: u16 = 2048;
/// First slot of the wide region.
pub const WIDE_BASE: u16 = NORMAL_CAPACITY;
/// Emoji flag (bit 15), outside the slot-address bits, distinguishing emoji from other
/// wide glyphs sharing the wide region.
pub const EMOJI_FLAG: u16 = 0x8000;

// The fragment shader addresses a slot with a 13-bit mask (`0x1FFF` = 8192 slots, 32 per
// texture layer); bit 13 is underline, bit 15 is the emoji flag. Guard both invariants at
// compile time so a future capacity bump can't silently overflow a slot index into the
// underline bit or straddle a wide glyph's two halves across texture layers.
const _: () = assert!(
    WIDE_BASE + WIDE_CAPACITY * 2 <= 0x2000,
    "wide region must fit the 13-bit (8192-slot) shader address space"
);
const _: () = assert!(
    WIDE_BASE & 31 == 0, // 32-aligned (32 glyphs/layer)
    "WIDE_BASE must be 32-aligned so a wide glyph's two halves share a texture layer"
);

/// The outcome of a slot request. For `Wide`/`Emoji`, `slot` is the base of a **two-cell**
/// span (`idx`, `idx+1`) — the caller uploads both halves on `is_new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    pub slot: GlyphSlot,
    /// The slot was freshly assigned — the caller must rasterise + upload the glyph.
    pub is_new: bool,
    /// If set, this slot was reused from a now-evicted glyph; invalidate its texture. The
    /// freed slot IS `slot` (the new glyph overwrites it), so no separate slot is returned.
    pub evicted: Option<GlyphKey>,
}

/// A partitioned, LRU-evicting glyph→slot map.
pub struct GlyphCache {
    normal: LruCache<GlyphKey, GlyphSlot>,
    wide: LruCache<GlyphKey, GlyphSlot>,
    normal_next: u16,
    wide_next: u16,
}

impl GlyphCache {
    pub fn new() -> Self {
        Self {
            normal: LruCache::unbounded(),
            wide: LruCache::unbounded(),
            normal_next: ASCII_SLOTS,
            wide_next: WIDE_BASE,
        }
    }

    /// Peek a glyph's slot **without allocating**: returns `Some` for a fast-path ASCII
    /// glyph or a resident cached glyph (marking it most-recently-used), `None` for a miss.
    /// Unlike [`get_or_insert`](Self::get_or_insert) a miss commits nothing — the caller can
    /// rasterise *before* committing (so a rasterise failure never strands a slot, #280).
    pub fn touch(&mut self, key: &GlyphKey, kind: GlyphKind) -> Option<GlyphSlot> {
        if kind.is_normal_region() {
            // The ASCII fast path is Normal-only: an emoji is never ASCII, and routing an
            // EmojiNarrow through it would drop the emoji flag.
            if kind == GlyphKind::Normal
                && let Some(slot) = ascii_fast_path(key)
            {
                return Some(slot);
            }
            self.normal.get(key).copied()
        } else {
            self.wide.get(key).copied()
        }
    }

    /// The glyph a *new* insertion of `kind` would evict right now, or `None` if the region
    /// still has a fresh slot (no eviction). Peeks without disturbing LRU order — the
    /// resolver uses this to reject a frame *before* committing, so an over-capacity frame
    /// never evicts a glyph it still references nor strands an uploaded-less slot (#280 P0).
    pub fn next_eviction(&self, kind: GlyphKind) -> Option<&GlyphKey> {
        if kind.is_normal_region() {
            if self.normal_next >= NORMAL_CAPACITY {
                self.normal.peek_lru().map(|(k, _)| k)
            } else {
                None
            }
        } else if self.wide_next >= WIDE_BASE + WIDE_CAPACITY * 2 {
            self.wide.peek_lru().map(|(k, _)| k)
        } else {
            None
        }
    }

    /// Get the slot for a glyph, allocating (and evicting the LRU glyph if the region is
    /// full) when it is new. Marks the glyph most-recently-used.
    /// Every resident dynamic `(grapheme, style) → slot` across both regions — so a DPR re-bake
    /// (#322) can re-rasterise each cached glyph into the *same* slot in the new atlas, keeping
    /// instances valid. Iterating does not disturb LRU order. The fixed ASCII fast-path slots are
    /// excluded (never cached; the re-bake re-prebakes them separately).
    pub fn entries(&self) -> impl Iterator<Item = (&GlyphKey, GlyphSlot)> {
        self.normal
            .iter()
            .chain(self.wide.iter())
            .map(|(k, &s)| (k, s))
    }

    pub fn get_or_insert(&mut self, key: GlyphKey, kind: GlyphKind) -> Allocation {
        match kind {
            GlyphKind::Normal => {
                if let Some(slot) = ascii_fast_path(&key) {
                    return Allocation {
                        slot,
                        is_new: false,
                        evicted: None,
                    };
                }
                self.alloc_normal(key, false)
            }
            // A width-1 colour emoji (#297): normal region, but flagged Emoji.
            GlyphKind::EmojiNarrow => self.alloc_normal(key, true),
            GlyphKind::Wide => self.alloc_wide(key, false),
            GlyphKind::Emoji => self.alloc_wide(key, true),
        }
    }

    fn alloc_normal(&mut self, key: GlyphKey, is_emoji: bool) -> Allocation {
        if let Some(&slot) = self.normal.get(&key) {
            return Allocation {
                slot,
                is_new: false,
                evicted: None,
            };
        }
        // A normal glyph takes ONE slot (a wide/emoji-wide glyph takes two — that path is
        // alloc_wide). On eviction reuse the LRU's BARE index and rebuild the variant from
        // `is_emoji`, so an evicted Emoji slot never carries its stale flag into a plain Normal
        // (and vice versa) — mirrors alloc_wide's reconstruction.
        let (idx, evicted) = if self.normal_next < NORMAL_CAPACITY {
            let i = self.normal_next;
            self.normal_next += 1;
            (i, None)
        } else {
            let (ek, es) = self
                .normal
                .pop_lru()
                .expect("normal region non-empty when full");
            (es.slot_id(), Some(ek))
        };
        let slot = if is_emoji {
            GlyphSlot::Emoji(idx | EMOJI_FLAG)
        } else {
            GlyphSlot::Normal(idx)
        };
        self.normal.put(key, slot);
        Allocation {
            slot,
            is_new: true,
            evicted,
        }
    }

    fn alloc_wide(&mut self, key: GlyphKey, is_emoji: bool) -> Allocation {
        if let Some(&slot) = self.wide.get(&key) {
            return Allocation {
                slot,
                is_new: false,
                evicted: None,
            };
        }
        let wide_end = WIDE_BASE + WIDE_CAPACITY * 2; // 6144
        let (idx, evicted) = if self.wide_next < wide_end {
            let i = self.wide_next;
            self.wide_next += 2; // each wide glyph spans two slots
            (i, None)
        } else {
            let (ek, es) = self
                .wide
                .pop_lru()
                .expect("wide region non-empty when full");
            (es.slot_id(), Some(ek))
        };
        let slot = if is_emoji {
            GlyphSlot::Emoji(idx | EMOJI_FLAG)
        } else {
            GlyphSlot::Wide(idx)
        };
        self.wide.put(key, slot);
        Allocation {
            slot,
            is_new: true,
            evicted,
        }
    }

    /// Introspection used only by this module's tests (#465). The render path never asks the cache
    /// its size and never resets it, so these are `#[cfg(test)]` — which is what the wasm32 build
    /// (the one that compiles the whole crate) reported once the module stopped being `pub` and
    /// dead-code analysis switched back on.
    ///
    /// `clear`'s doc used to claim it was "needed when the atlas is rebuilt on a font-size / DPR
    /// change (#265)". That was **false**: all three rebuild paths — `set_device_pixel_ratio`,
    /// `set_font_size` and the context-loss `restore` — deliberately KEEP the cache and re-bake from
    /// it (`bake_all_glyphs` iterates `entries()`), so clearing first would leave nothing to bake.
    /// If a reset is ever genuinely wanted on a rebuild path, drop the `cfg` and re-read those three
    /// first.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.normal.len() + self.wide.len()
    }

    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.normal.clear();
        self.wide.clear();
        self.normal_next = ASCII_SLOTS;
        self.wide_next = WIDE_BASE;
    }
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new()
    }
}

/// A single normal-styled ASCII grapheme (`0x20..=0x7E`) maps to a fixed pre-allocated
/// slot (`codepoint - 0x20`) with no cache entry. Anything else returns `None`.
fn ascii_fast_path(key: &GlyphKey) -> Option<GlyphSlot> {
    if key.style != FontStyle::Normal {
        return None;
    }
    let mut chars = key.text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None; // more than one grapheme
    }
    if ('\u{20}'..='\u{7E}').contains(&c) {
        Some(GlyphSlot::Normal(c as u16 - 0x20))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str, style: FontStyle) -> GlyphKey {
        GlyphKey {
            text: text.to_string(),
            style,
        }
    }

    #[test]
    fn entries_lists_resident_dynamic_glyphs_with_their_slots() {
        // #322: a DPR re-bake re-rasterises the resident glyphs into the same slots, so the cache
        // must expose (grapheme, style) → slot for both regions. ASCII fast-path slots are NOT
        // enumerated (the re-bake re-prebakes those separately).
        let mut c = GlyphCache::new();
        c.get_or_insert(key("A", FontStyle::Normal), GlyphKind::Normal); // ASCII fast path
        let star = c.get_or_insert(key("★", FontStyle::Bold), GlyphKind::Normal);
        let wide = c.get_or_insert(key("한", FontStyle::Normal), GlyphKind::Wide);
        let emoji = c.get_or_insert(key("😀", FontStyle::Normal), GlyphKind::Emoji); // wide colour emoji

        let got: Vec<(String, FontStyle, u16)> = c
            .entries()
            .map(|(k, s)| (k.text.clone(), k.style, s.slot_id()))
            .collect();

        assert_eq!(
            got.len(),
            3,
            "the three dynamic glyphs, not the ASCII fast path"
        );
        assert!(got.contains(&("★".to_string(), FontStyle::Bold, star.slot.slot_id())));
        assert!(got.contains(&("한".to_string(), FontStyle::Normal, wide.slot.slot_id())));
        // A *wide* colour emoji is a `GlyphSlot::Emoji` in the WIDE region, so its enumerated
        // slot_id >= WIDE_BASE — the signal the #322 re-bake keys wide-ness off (a `matches!(Wide)`
        // check would misread it as a `Normal`/narrow slot and re-rasterise only half of it).
        let emoji_slot = got.iter().find(|(t, _, _)| t == "😀").unwrap().2;
        assert_eq!(emoji_slot, emoji.slot.slot_id());
        assert!(
            emoji_slot >= WIDE_BASE,
            "a wide emoji lives in the wide region"
        );
    }

    #[test]
    fn slot_texcoord_splits_into_layer_and_band() {
        // 32 glyphs per layer. Worked by hand: layer = slot/32, band = slot%32.
        assert_eq!(slot_texcoord(0), (0, 0));
        assert_eq!(slot_texcoord(31), (0, 31));
        assert_eq!(slot_texcoord(32), (1, 0));
        assert_eq!(slot_texcoord(95), (2, 31)); // last ASCII slot
        assert_eq!(slot_texcoord(WIDE_BASE), (64, 0)); // 2048/32
    }

    #[test]
    fn ascii_normal_uses_fixed_fast_path_slot() {
        let mut c = GlyphCache::new();
        // Fast path: slot = codepoint - 0x20, never cached.
        //   ' ' 0x20 -> 0, 'A' 0x41 -> 33, '~' 0x7E -> 94.
        for (ch, want) in [(" ", 0u16), ("A", 33), ("~", 94)] {
            let a = c.get_or_insert(key(ch, FontStyle::Normal), GlyphKind::Normal);
            assert_eq!(a.slot, GlyphSlot::Normal(want), "char {ch:?}");
            assert!(!a.is_new, "fast-path glyphs are pre-baked, not new");
        }
        assert_eq!(c.len(), 0, "fast-path glyphs never enter the cache");
    }

    #[test]
    fn non_ascii_normal_allocates_after_the_reserved_ascii_slots() {
        let mut c = GlyphCache::new();
        // First cached normal glyph lands at ASCII_SLOTS (95), the next at 96.
        let a = c.get_or_insert(key("\u{2192}", FontStyle::Normal), GlyphKind::Normal);
        assert_eq!(a.slot, GlyphSlot::Normal(ASCII_SLOTS));
        assert!(a.is_new && a.evicted.is_none());

        let b = c.get_or_insert(key("\u{2190}", FontStyle::Normal), GlyphKind::Normal);
        assert_eq!(b.slot, GlyphSlot::Normal(ASCII_SLOTS + 1));

        // Re-request the first: same slot, not new, nothing evicted.
        let again = c.get_or_insert(key("\u{2192}", FontStyle::Normal), GlyphKind::Normal);
        assert_eq!(again.slot, GlyphSlot::Normal(ASCII_SLOTS));
        assert!(!again.is_new && again.evicted.is_none());
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn bold_ascii_bypasses_the_normal_fast_path_and_is_cached() {
        let mut c = GlyphCache::new();
        // 'A' Normal is the fast path (33); 'A' Bold is a distinct glyph → cached at 95.
        assert_eq!(
            c.get_or_insert(key("A", FontStyle::Normal), GlyphKind::Normal)
                .slot,
            GlyphSlot::Normal(33)
        );
        let bold = c.get_or_insert(key("A", FontStyle::Bold), GlyphKind::Normal);
        assert_eq!(bold.slot, GlyphSlot::Normal(ASCII_SLOTS));
        assert!(bold.is_new);
        assert_eq!(c.len(), 1, "only the bold 'A' is cached");
    }

    #[test]
    fn wide_and_emoji_share_the_wide_region_two_slots_each() {
        let mut c = GlyphCache::new();
        // CJK (wide, no emoji flag): 2048, then 2050.
        assert_eq!(
            c.get_or_insert(key("\u{4E2D}", FontStyle::Normal), GlyphKind::Wide)
                .slot,
            GlyphSlot::Wide(WIDE_BASE)
        );
        assert_eq!(
            c.get_or_insert(key("\u{6587}", FontStyle::Normal), GlyphKind::Wide)
                .slot,
            GlyphSlot::Wide(WIDE_BASE + 2)
        );
        // Emoji: same region, carries EMOJI_FLAG, still steps by 2.
        let e = c.get_or_insert(key("\u{1F680}", FontStyle::Normal), GlyphKind::Emoji);
        assert_eq!(e.slot, GlyphSlot::Emoji((WIDE_BASE + 4) | EMOJI_FLAG));
        assert_eq!(
            e.slot.slot_id(),
            WIDE_BASE + 4,
            "slot_id strips the emoji flag"
        );
    }

    #[test]
    fn emoji_narrow_allocates_in_the_normal_region_carrying_the_emoji_flag() {
        let mut c = GlyphCache::new();
        // #297 case 1: a width-1 colour glyph (❤ U+2764) allocates in the NORMAL region — its
        // slot address is below WIDE_BASE — yet it is an Emoji slot carrying EMOJI_FLAG so the
        // shader colour-samples it. bit 15 never collides with the ≤ 0x7FF normal slot address.
        let a = c.get_or_insert(key("\u{2764}", FontStyle::Normal), GlyphKind::EmojiNarrow);
        assert!(a.is_new);
        assert_eq!(a.slot, GlyphSlot::Emoji(ASCII_SLOTS | EMOJI_FLAG));
        assert_eq!(
            a.slot.slot_id(),
            ASCII_SLOTS,
            "bare slot addresses the normal region"
        );
        assert!(a.slot.slot_id() < WIDE_BASE, "not in the wide region");

        // Re-request: a cache hit, no new alloc — the glyph is resident across frames (no
        // re-rasterise thrash, the whole point of the normal-region path).
        let b = c.get_or_insert(key("\u{2764}", FontStyle::Normal), GlyphKind::EmojiNarrow);
        assert!(!b.is_new);
        assert_eq!(b.slot, a.slot);

        // The resolver looks a narrow glyph up as GlyphKind::Normal (width is known before the
        // rasteriser reports emoji-ness). EmojiNarrow shares the normal region, so that touch hits.
        assert_eq!(
            c.touch(&key("\u{2764}", FontStyle::Normal), GlyphKind::Normal),
            Some(a.slot),
            "reachable via a Normal-kind touch — same region"
        );
        assert_eq!(c.len(), 1, "one cached glyph");
    }

    #[test]
    fn full_normal_region_evicts_lru_and_reuses_its_slot() {
        let mut c = GlyphCache::new();
        // Fill the whole normal region: slots ASCII_SLOTS..NORMAL_CAPACITY.
        let fill = NORMAL_CAPACITY - ASCII_SLOTS; // 1953
        let first = '\u{2200}';
        for i in 0..fill {
            let ch = char::from_u32(0x2200 + i as u32).unwrap();
            let a = c.get_or_insert(key(&ch.to_string(), FontStyle::Normal), GlyphKind::Normal);
            assert!(a.is_new && a.evicted.is_none());
        }
        // Region full; the first-inserted glyph is the LRU (never re-touched).
        let overflow = char::from_u32(0x2200 + fill as u32).unwrap();
        let a = c.get_or_insert(
            key(&overflow.to_string(), FontStyle::Normal),
            GlyphKind::Normal,
        );
        assert!(a.is_new);
        assert_eq!(
            a.slot,
            GlyphSlot::Normal(ASCII_SLOTS),
            "reuses the evicted LRU slot"
        );
        assert_eq!(
            a.evicted,
            Some(key(&first.to_string(), FontStyle::Normal)),
            "the LRU (first-inserted) glyph is evicted"
        );
        assert_eq!(
            c.len(),
            fill as usize,
            "count is unchanged after an eviction"
        );
    }

    #[test]
    fn full_normal_region_evicting_an_emoji_reuses_the_bare_slot_without_the_flag() {
        let mut c = GlyphCache::new();
        // Fill the whole normal region with narrow colour emoji (EmojiNarrow, 1 slot each).
        let fill = NORMAL_CAPACITY - ASCII_SLOTS; // 1953
        let first = '\u{2764}';
        for i in 0..fill {
            let ch = char::from_u32(0x2764 + i as u32).unwrap();
            let a = c.get_or_insert(
                key(&ch.to_string(), FontStyle::Normal),
                GlyphKind::EmojiNarrow,
            );
            assert!(a.is_new && a.evicted.is_none());
            assert!(
                matches!(a.slot, GlyphSlot::Emoji(_)),
                "each is a normal-region emoji"
            );
        }
        // Overflow with a plain Normal glyph: it evicts the LRU emoji and reuses its BARE index
        // as a Normal — the stale EMOJI_FLAG must NOT survive, else the shader would wrongly
        // colour-sample a text glyph. (The normal-region mirror of the wide-region guard below.)
        let a = c.get_or_insert(key("\u{2192}", FontStyle::Normal), GlyphKind::Normal);
        assert!(a.is_new);
        assert_eq!(
            a.slot,
            GlyphSlot::Normal(ASCII_SLOTS),
            "reuses the evicted emoji's bare slot as a plain Normal (no emoji flag)"
        );
        assert_eq!(a.evicted, Some(key(&first.to_string(), FontStyle::Normal)));
    }

    #[test]
    fn full_wide_region_evicts_lru_and_reuses_the_bare_slot() {
        let mut c = GlyphCache::new();
        // Fill the wide region with emoji (2048 glyphs × 2 slots → 2048..6144).
        let first = '\u{1F600}';
        for i in 0..WIDE_CAPACITY {
            let ch = char::from_u32(0x1F600 + i as u32).unwrap();
            let a = c.get_or_insert(key(&ch.to_string(), FontStyle::Normal), GlyphKind::Emoji);
            assert!(a.is_new && a.evicted.is_none());
            assert_eq!(a.slot, GlyphSlot::Emoji((WIDE_BASE + i * 2) | EMOJI_FLAG));
        }
        // Overflow with a CJK wide glyph: evicts the LRU emoji and reuses its BARE index
        // (emoji flag stripped) as a plain Wide — no stray bit 15 carried over.
        let a = c.get_or_insert(key("\u{4E2D}", FontStyle::Normal), GlyphKind::Wide);
        assert!(a.is_new);
        assert_eq!(
            a.slot,
            GlyphSlot::Wide(WIDE_BASE),
            "reuses the evicted emoji's bare slot as a Wide (no emoji flag)"
        );
        assert_eq!(a.evicted, Some(key(&first.to_string(), FontStyle::Normal)));
    }

    #[test]
    fn clear_frees_every_slot_and_resets_the_allocator() {
        let mut c = GlyphCache::new();
        c.get_or_insert(key("\u{2192}", FontStyle::Normal), GlyphKind::Normal); // Normal(95)
        c.get_or_insert(key("\u{4E2D}", FontStyle::Normal), GlyphKind::Wide); // Wide(2048)
        assert_eq!(c.len(), 2);

        c.clear();

        assert_eq!(c.len(), 0);
        // Allocation restarts from the reserved boundaries after a reset.
        assert_eq!(
            c.get_or_insert(key("\u{2192}", FontStyle::Normal), GlyphKind::Normal)
                .slot,
            GlyphSlot::Normal(ASCII_SLOTS)
        );
        assert_eq!(
            c.get_or_insert(key("\u{4E2D}", FontStyle::Normal), GlyphKind::Wide)
                .slot,
            GlyphSlot::Wide(WIDE_BASE)
        );
    }
}
