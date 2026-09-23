//! The configuration registry — which font configurations this renderer holds an atlas for, and
//! how many grids are standing on each (#772, ADR-0021 D2).
//!
//! Grids in the same font configuration share **one** glyph atlas. Generic over the payload, like
//! the [grid registry](crate::registry), so it is host-tested off `wasm32` (#280).
//!
//! Deliberately: the DPR is **not** in the key (one canvas has one density — it lives on the global
//! tier, and a density change rebuilds every entry in place), and a shared entry is **never mutated
//! in place** for one grid — a setting change joins a different entry via
//! [`ConfigRegistry::find`] + [`ConfigRegistry::insert`] + [`ConfigRegistry::release`]. Why, and the
//! ghostty comparison: `docs/map/territory/multi-viewport.md`.

use crate::css_font::FontWeight;

/// A font configuration: the seven per-grid selectors that decide which atlas serves a grid.
///
/// The `f32` selectors are stored as **bit patterns** so the key can be compared and hashed, with
/// `-0.0` normalised to `0.0` first (`docs/map/territory/multi-viewport.md`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ConfigKey {
    font_family: String,
    font_size: u32,
    font_weight: u32,
    font_weight_bold: u32,
    letter_spacing: u32,
    line_height: u32,
    subpixel: bool,
}

/// Normalise a selector to the bits that identify it. `-0.0` and `0.0` are the same configuration.
fn bits(v: f32) -> u32 {
    if v == 0.0 { 0.0f32 } else { v }.to_bits()
}

impl ConfigKey {
    /// The configuration a grid with these seven selectors stands on.
    pub fn new(
        font_family: &str,
        font_size: f32,
        font_weight: FontWeight,
        font_weight_bold: FontWeight,
        letter_spacing: f32,
        line_height: f32,
        subpixel: bool,
    ) -> Self {
        ConfigKey {
            font_family: font_family.to_string(),
            font_size: bits(font_size),
            font_weight: bits(font_weight.value()),
            font_weight_bold: bits(font_weight_bold.value()),
            letter_spacing: bits(letter_spacing),
            line_height: bits(line_height),
            subpixel,
        }
    }

    /// The CSS `font-family` this configuration rasterises through.
    pub fn font_family(&self) -> &str {
        &self.font_family
    }

    /// The font size in CSS px (#406).
    pub fn font_size(&self) -> f32 {
        f32::from_bits(self.font_size)
    }

    /// The weight regular text is drawn at (#928).
    pub fn font_weight(&self) -> FontWeight {
        FontWeight::from_value(f32::from_bits(self.font_weight))
    }

    /// The weight bold text is drawn at (#928).
    pub fn font_weight_bold(&self) -> FontWeight {
        FontWeight::from_value(f32::from_bits(self.font_weight_bold))
    }

    /// Extra space between columns in CSS px (#338, ADR-0023).
    pub fn letter_spacing(&self) -> f32 {
        f32::from_bits(self.letter_spacing)
    }

    /// The multiplier on the glyph height (#338).
    pub fn line_height(&self) -> f32 {
        f32::from_bits(self.line_height)
    }

    /// Whether text glyphs carry per-channel (LCD) coverage (#961).
    pub fn subpixel(&self) -> bool {
        self.subpixel
    }
}

/// A handle to one configuration entry.
///
/// Never reused, like a [`GridId`](crate::registry::GridId). Held only by a grid; it never crosses
/// the wasm boundary.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct ConfigId(u32);

struct Entry<T> {
    id: ConfigId,
    key: ConfigKey,
    /// How many grids select into this entry. Reaching zero destroys it immediately — deliberately
    /// not pooled.
    refs: u32,
    value: T,
}

/// Every font configuration this renderer holds a resource set for, and how many grids stand on each.
pub struct ConfigRegistry<T> {
    entries: Vec<Entry<T>>,
    next_id: u32,
}

impl<T> Default for ConfigRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ConfigRegistry<T> {
    /// Start an **empty** registry — a renderer holds no configuration until a grid asks for one
    /// (#773).
    pub fn new() -> Self {
        ConfigRegistry {
            entries: Vec::new(),
            next_id: 1,
        }
    }

    /// The entry serving `key`, if one already exists. `None` means the caller must build one —
    /// which is the expensive half, and the reason this lookup exists at all.
    pub fn find(&self, key: &ConfigKey) -> Option<ConfigId> {
        self.entries.iter().find(|e| &e.key == key).map(|e| e.id)
    }

    /// Add a freshly built entry with one reference — the grid that asked for it.
    pub fn insert(&mut self, key: ConfigKey, value: T) -> ConfigId {
        let id = ConfigId(self.next_id);
        self.next_id += 1;
        self.entries.push(Entry {
            id,
            key,
            refs: 1,
            value,
        });
        id
    }

    /// One more grid selects into this entry.
    pub fn retain(&mut self, id: ConfigId) {
        let at = self.slot(id);
        self.entries[at].refs += 1;
    }

    /// One fewer grid selects into this entry. Hands the payload back — so the caller can release
    /// whatever GPU state it owns — exactly when the **last** grid leaves.
    pub fn release(&mut self, id: ConfigId) -> Option<T> {
        let at = self.slot(id);
        if self.entries[at].refs > 1 {
            self.entries[at].refs -= 1;
            return None;
        }
        Some(self.entries.remove(at).value)
    }

    /// The resources an entry holds.
    ///
    /// Infallible by construction, and that is a property of the caller rather than of this type: a
    /// `ConfigId` is only ever observable while a grid holds a reference to it, and a referenced
    /// entry is never removed. A panic here is a refcount bug, which is the failure it should be.
    pub fn get(&self, id: ConfigId) -> &T {
        &self.entries[self.slot(id)].value
    }

    /// Mutable form of [`get`](Self::get). In-place mutation is for changes true of every entry at
    /// once (a DPR change, a context restore); see the module doc on immutability.
    pub fn get_mut(&mut self, id: ConfigId) -> &mut T {
        let at = self.slot(id);
        &mut self.entries[at].value
    }

    /// The configuration an entry serves.
    pub fn key(&self, id: ConfigId) -> &ConfigKey {
        &self.entries[self.slot(id)].key
    }

    /// How many distinct configurations are live — i.e. how many atlases exist. Sharing is only a
    /// claim until something can count it.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// How many grids select into an entry.
    ///
    /// Test-only: the refcount is not a consumer question. What a consumer (and a browser proof)
    /// can see is [`len`](Self::len) — an entry that the last grid left is *gone*, so "the refcount
    /// reached zero" and "the atlas count went down" are the same observation from outside.
    #[cfg(test)]
    pub fn refs(&self, id: ConfigId) -> u32 {
        self.entries[self.slot(id)].refs
    }

    /// Every live entry, in creation order — for the walks that rebuild all of them at once. Owned
    /// rather than borrowed so the caller can rebuild each entry while holding the registry.
    pub fn ids(&self) -> Vec<ConfigId> {
        self.entries.iter().map(|e| e.id).collect()
    }

    /// The entries some grid will still hold **after** every grid has been placed on the entry its
    /// key asks for — the set a restore must re-bake (#788).
    ///
    /// **It is a prediction, and it has to be.** `restore` bakes at step 2 and reconciles at step 3,
    /// and that order is forced: the reconcile acquires entries, which needs the committed live
    /// context, so it cannot run first. Step 2 therefore has to answer a question about step 3's
    /// outcome. Matching on the **key alone** is what makes the answer right, because the key is
    /// what [`find`](Self::find) matches on — a grid whose selectors moved while the context was
    /// dead joins whichever entry carries its key, **including one no grid holds at this instant**.
    ///
    /// The predicate this replaced also required a current holder (`grid.config == id`), which is
    /// the set as of *now* rather than as of *after*. Two grids swapping configurations mid-loss
    /// then re-baked neither, and the reconcile moved one of them onto a texture that died with the
    /// context: no error, plausible glyphs, and no self-repair until the next loss.
    ///
    /// It lives here rather than at the call site for the reason ADR-0027 gives for putting
    /// `must_defer` on the state machine: `webgl.rs` is wasm32-only, so a predicate written there
    /// has no host test. The stronger half is that the old predicate is **unwritable** here — this
    /// type does not know which grid holds what, so the question it can ask is the one it should.
    ///
    /// Returned in creation order, deduplicated: an entry two grids want is baked once.
    pub fn ids_wanted_by(&self, keys: &[ConfigKey]) -> Vec<ConfigId> {
        self.entries
            .iter()
            .filter(|e| keys.contains(&e.key))
            .map(|e| e.id)
            .collect()
    }

    /// The entry an id addresses.
    ///
    /// The `expect` cannot fire, and it rests on three properties of the *caller* rather than of this
    /// type — stated because breaking any one of them turns a refcount bug into a panic across the
    /// wasm boundary, which is a worse failure than the `Err` this crate hands back everywhere else:
    ///
    /// 1. a configuration change **acquires before it releases**, so re-selecting the same key cannot
    ///    free the entry in between;
    /// 2. the only site that drops a grid releases its configuration unconditionally on the success
    ///    arm;
    /// 3. nothing fallible sits between registering a grid and the `retain` that pays for its
    ///    reference.
    fn slot(&self, id: ConfigId) -> usize {
        self.entries
            .iter()
            .position(|e| e.id == id)
            .expect("justerm-renderer: a config id outlived the last grid holding it")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for `ConfigTier`: the registry must not know what a configuration owns.
    #[derive(Debug, PartialEq, Eq, Clone)]
    struct FakeAtlas(u32);

    const W: FontWeight = FontWeight::NORMAL;
    const B: FontWeight = FontWeight::BOLD;

    fn key(family: &str, size: f32) -> ConfigKey {
        ConfigKey::new(family, size, W, B, 0.0, 1.0, false)
    }

    fn weight(v: f64) -> FontWeight {
        FontWeight::from_number(v).unwrap()
    }

    fn start() -> (ConfigRegistry<FakeAtlas>, ConfigId) {
        let mut reg = ConfigRegistry::new();
        let id = reg.insert(key("monospace", 15.0), FakeAtlas(1));
        (reg, id)
    }

    #[test]
    fn a_new_registry_holds_no_configuration() {
        let reg: ConfigRegistry<FakeAtlas> = ConfigRegistry::new();
        assert_eq!(reg.len(), 0);
        assert_eq!(
            reg.find(&key("monospace", 15.0)),
            None,
            "nothing to join until a grid asks"
        );
    }

    #[test]
    fn two_grids_in_the_same_configuration_find_one_entry() {
        let (mut reg, first) = start();
        let found = reg.find(&key("monospace", 15.0)).expect("same key");
        assert_eq!(found, first);
        reg.retain(found);
        assert_eq!(reg.len(), 1, "sharing must not add an entry");
        assert_eq!(reg.refs(first), 2);
    }

    #[test]
    fn each_selector_separates_a_configuration() {
        let (reg, _) = start();
        for other in [
            ConfigKey::new("Fira Code", 15.0, W, B, 0.0, 1.0, false),
            ConfigKey::new("monospace", 16.0, W, B, 0.0, 1.0, false),
            ConfigKey::new("monospace", 15.0, weight(300.0), B, 0.0, 1.0, false),
            ConfigKey::new("monospace", 15.0, W, weight(900.0), 0.0, 1.0, false),
            ConfigKey::new("monospace", 15.0, W, B, 1.0, 1.0, false),
            ConfigKey::new("monospace", 15.0, W, B, 0.0, 1.5, false),
            ConfigKey::new("monospace", 15.0, W, B, 0.0, 1.0, true),
        ] {
            assert_eq!(reg.find(&other), None, "{other:?} must not share");
        }
    }

    #[test]
    fn negative_zero_spacing_is_the_same_configuration_as_zero() {
        let (reg, first) = start();
        let neg = ConfigKey::new("monospace", 15.0, W, B, -0.0, 1.0, false);
        assert_eq!(reg.find(&neg), Some(first));
    }

    #[test]
    fn a_weight_given_as_its_number_is_the_same_configuration_as_its_keyword() {
        let (reg, first) = start();
        let numeric = ConfigKey::new(
            "monospace",
            15.0,
            weight(400.0),
            weight(700.0),
            0.0,
            1.0,
            false,
        );
        assert_eq!(reg.find(&numeric), Some(first));
    }

    #[test]
    fn a_key_hands_its_selectors_back() {
        let (reg, first) = start();
        let k = reg.key(first);
        assert_eq!(k.font_family(), "monospace");
        assert_eq!(k.font_size(), 15.0);
        assert_eq!(k.font_weight(), W);
        assert_eq!(k.font_weight_bold(), B);
        let other = ConfigKey::new(
            "monospace",
            15.0,
            weight(350.5),
            weight(1000.0),
            0.0,
            1.0,
            false,
        );
        assert_eq!(other.font_weight().value(), 350.5);
        assert_eq!(other.font_weight_bold().value(), 1000.0);
        assert_eq!(k.letter_spacing(), 0.0);
        assert_eq!(k.line_height(), 1.0);
        assert!(!k.subpixel());
        assert!(ConfigKey::new("monospace", 15.0, W, B, 0.0, 1.0, true).subpixel());
    }

    #[test]
    fn the_last_grid_to_leave_releases_the_entry() {
        let (mut reg, first) = start();
        reg.retain(first);
        assert_eq!(reg.release(first), None, "a shared entry survives");
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.refs(first), 1);
        assert_eq!(
            reg.release(first),
            Some(FakeAtlas(1)),
            "the last one frees it"
        );
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn a_second_configuration_is_a_second_entry_and_leaves_the_first_alone() {
        let (mut reg, first) = start();
        let second = reg.insert(key("monospace", 30.0), FakeAtlas(2));
        assert_ne!(second, first);
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.get(first), &FakeAtlas(1));
        assert_eq!(reg.get(second), &FakeAtlas(2));
        assert_eq!(reg.refs(first), 1, "inserting must not touch a sibling");
    }

    #[test]
    fn an_id_is_never_reused_after_its_entry_is_released() {
        let (mut reg, first) = start();
        let second = reg.insert(key("monospace", 30.0), FakeAtlas(2));
        assert_eq!(reg.release(second), Some(FakeAtlas(2)));
        let third = reg.insert(key("monospace", 45.0), FakeAtlas(3));
        assert_ne!(third, second, "a freed slot must not hand its id back");
        assert_ne!(third, first);
        assert_eq!(reg.get(third), &FakeAtlas(3));
    }

    #[test]
    fn releasing_a_middle_entry_leaves_the_later_ones_reachable_by_id() {
        let (mut reg, first) = start();
        let second = reg.insert(key("monospace", 30.0), FakeAtlas(2));
        let third = reg.insert(key("monospace", 45.0), FakeAtlas(3));
        assert_eq!(reg.release(second), Some(FakeAtlas(2)));
        // `Vec::remove` shifted `third` down a slot; an id must not follow the slot.
        assert_eq!(reg.get(third), &FakeAtlas(3));
        assert_eq!(reg.get(first), &FakeAtlas(1));
        assert_eq!(reg.ids(), vec![first, third]);
    }

    #[test]
    fn an_entry_can_be_rebuilt_in_place_without_disturbing_its_refcount() {
        // The DPR / context-restore path: every entry changes, nobody joins or leaves.
        let (mut reg, first) = start();
        reg.retain(first);
        let second = reg.insert(key("monospace", 30.0), FakeAtlas(2));
        for id in reg.ids() {
            *reg.get_mut(id) = FakeAtlas(9);
        }
        assert_eq!(reg.get(first), &FakeAtlas(9));
        assert_eq!(reg.get(second), &FakeAtlas(9));
        assert_eq!(reg.refs(first), 2);
        assert_eq!(reg.refs(second), 1);
        assert_eq!(reg.len(), 2);
    }

    // ── `ids_wanted_by` — which entries survive a re-key (#788) ─────────────────────────────────
    //
    // `restore` bakes BEFORE it reconciles, because the reconcile needs the committed live context
    // and cannot run first. So the bake step has to *predict* the reconcile's outcome, and the
    // prediction is by KEY alone — that is what `find` matches on. The bug this replaces asked
    // "who holds this entry now", which is a different set and the reason a grid could be
    // reconciled onto an entry nothing re-baked.

    #[test]
    fn an_entry_nobody_asks_for_is_not_wanted() {
        // The release case, and the one the old predicate also got right.
        let (reg, first) = start();
        assert_eq!(reg.ids_wanted_by(&[key("monospace", 30.0)]), vec![]);
        assert_eq!(reg.ids_wanted_by(&[key("monospace", 15.0)]), vec![first]);
    }

    #[test]
    fn an_entry_with_no_current_holder_is_wanted_when_a_key_asks_for_it() {
        // **The #788 case.** Two entries, and the two grids swap: each asks for the key the OTHER
        // one is standing on. Every entry survives the reconcile, so every entry must be re-baked —
        // and a predicate that also required a *current* holder would return neither.
        let mut reg = ConfigRegistry::new();
        let small = reg.insert(key("monospace", 15.0), FakeAtlas(1));
        let big = reg.insert(key("monospace", 30.0), FakeAtlas(2));
        let wanted = reg.ids_wanted_by(&[key("monospace", 30.0), key("monospace", 15.0)]);
        assert_eq!(wanted, vec![small, big]);
    }

    #[test]
    fn a_key_no_entry_serves_adds_nothing() {
        // The grid that will force a fresh bake in the reconcile contributes no id here — it has no
        // entry to re-bake yet, which is exactly why it is the reconcile's job and not step 2's.
        let (reg, first) = start();
        let wanted = reg.ids_wanted_by(&[key("monospace", 15.0), key("monospace", 45.0)]);
        assert_eq!(wanted, vec![first]);
    }

    #[test]
    fn an_entry_wanted_by_two_grids_is_named_once_and_order_follows_creation() {
        // Baked once, not once per holder — and in `ids()` order, so the caller can zip the result
        // against what it builds.
        let mut reg = ConfigRegistry::new();
        let small = reg.insert(key("monospace", 15.0), FakeAtlas(1));
        let big = reg.insert(key("monospace", 30.0), FakeAtlas(2));
        let k = [
            key("monospace", 30.0),
            key("monospace", 15.0),
            key("monospace", 30.0),
        ];
        assert_eq!(reg.ids_wanted_by(&k), vec![small, big]);
    }

    #[test]
    fn no_keys_wants_nothing() {
        // A renderer whose last grid left mid-loss: nothing to re-bake, and no panic on the way.
        let (reg, _) = start();
        assert_eq!(reg.ids_wanted_by(&[]), vec![]);
    }
}
