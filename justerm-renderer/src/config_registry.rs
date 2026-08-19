//! The configuration registry — which font configurations this renderer holds an atlas for, and
//! how many grids are standing on each (#772, ADR-0021 D2).
//!
//! Six terminals in the same font hold **one** glyph atlas rather than six. That is the recurring
//! cost Epic #287 is justified on: it is paid at any terminal count, independent of the browser's
//! context ceiling.
//!
//! Like the [grid registry](crate::registry), this module owns the *multiplication* and stays
//! ignorant of what it multiplies — the payload is a type parameter, so `webgl.rs` remains the only
//! place that knows a configuration owns an atlas texture and a rasteriser, and the whole of this is
//! host-testable off `wasm32` (the crate's standing pure/glue split, #280).
//!
//! ## What is in the key, and what is deliberately not
//!
//! ADR-0021 describes the key as *"(font family, size, spacing, DPR)"*. Three of those four are
//! here; **DPR is not**, and the omission follows from the record's own sentences rather than
//! disagreeing with them. One canvas means one drawing buffer and one `devicePixelRatio`, so the DPR
//! is *globally constant across the registry at any instant* — no two live entries can differ in it.
//! A component every key shares cannot separate two keys, so putting it in would buy nothing and
//! cost something real: a DPR change would have to rewrite **every** key, and until it did, every
//! entry's key would be a lie. What a DPR change actually needs is what ADR-0021 already says it is
//! — *"re-keying one entry and rebuilding all of them are separate paths"* — so the DPR lives on the
//! global tier and the registry is rebuilt in place against it.
//!
//! Ghostty **does** hash the DPI, in `DesiredSize.xdpi`/`ydpi` (`src/font/face.zig:46-52`, keyed at
//! `SharedGridSet.zig:566`), and that does not transfer for a reason already recorded in
//! `docs/map/territory/multi-viewport.md`: a ghostty `Surface` is an OS window that can be dragged
//! onto a monitor of its own density, so its DPI genuinely *is* per-surface. N viewports on one
//! canvas have one density between them.
//!
//! ## Immutability, and what it is actually protecting
//!
//! A shared entry is **never mutated in place** to serve one grid's changed setting — ghostty states
//! the reason in one line: *"increasing the font size in one would increase it in all"*
//! (`src/font/SharedGrid.zig:1-22`). A configuration change means *joining a different entry*, which
//! is what [`ConfigRegistry::find`] + [`ConfigRegistry::insert`] + [`ConfigRegistry::release`] are
//! for. In-place mutation is reserved for changes that are true of every entry at once — a DPR
//! change, a context restore — where "it would change in all" is the correct outcome rather than the
//! bug.

/// A font configuration: the four per-grid selectors that decide which atlas serves a grid.
///
/// The three `f32` selectors are stored as **bit patterns** so the key can be compared and hashed.
/// A `-0.0` is normalised to `0.0` first: the two compare equal as floats and would otherwise key
/// two byte-identical atlases, which is exactly the duplication this registry exists to remove.
///
/// A non-finite selector cannot arrive — `set_font_size` refuses one, `set_letter_spacing` maps it
/// to `0.0`, `set_line_height` clamps to `>= 1` — and if one ever did it would key an entry of its
/// own rather than corrupt a shared one, since the bits are compared and never the floats.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ConfigKey {
    font_family: String,
    font_size: u32,
    letter_spacing: u32,
    line_height: u32,
}

/// Normalise a selector to the bits that identify it. `-0.0` and `0.0` are the same configuration.
fn bits(v: f32) -> u32 {
    if v == 0.0 { 0.0f32 } else { v }.to_bits()
}

impl ConfigKey {
    /// The configuration a grid with these four selectors stands on.
    pub fn new(font_family: &str, font_size: f32, letter_spacing: f32, line_height: f32) -> Self {
        ConfigKey {
            font_family: font_family.to_string(),
            font_size: bits(font_size),
            letter_spacing: bits(letter_spacing),
            line_height: bits(line_height),
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

    /// Extra space between columns in CSS px (#338, ADR-0023).
    pub fn letter_spacing(&self) -> f32 {
        f32::from_bits(self.letter_spacing)
    }

    /// The multiplier on the glyph height (#338).
    pub fn line_height(&self) -> f32 {
        f32::from_bits(self.line_height)
    }
}

/// A handle to one configuration entry.
///
/// Never reused, for the same reason a [`GridId`](crate::registry::GridId) is not: a stale handle
/// must be unable to address whichever entry landed in a freed slot. Unlike a grid id this one never
/// crosses the wasm boundary — a grid holds it, and a grid is the only thing that can.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct ConfigId(u32);

struct Entry<T> {
    id: ConfigId,
    key: ConfigKey,
    /// How many grids select into this entry. Reaching zero destroys it — immediately, as ghostty's
    /// `deref` does (`src/font/SharedGridSet.zig:395-413`), rather than parking it in a free pool: a
    /// terminal that closes should not hold an atlas open against the next font change.
    refs: u32,
    value: T,
}

/// Every font configuration this renderer holds a resource set for, and how many grids stand on each.
pub struct ConfigRegistry<T> {
    entries: Vec<Entry<T>>,
    next_id: u32,
}

impl<T> ConfigRegistry<T> {
    /// Start a registry holding one entry — the configuration the implicit default grid is born
    /// into — with a single reference, which is that grid's.
    pub fn new(key: ConfigKey, value: T) -> (Self, ConfigId) {
        let id = ConfigId(0);
        (
            ConfigRegistry {
                entries: vec![Entry {
                    id,
                    key,
                    refs: 1,
                    value,
                }],
                next_id: 1,
            },
            id,
        )
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

    /// How many distinct configurations are live — i.e. how many atlases exist. Ghostty exposes the
    /// same number for the same reason (`SharedGridSet.count`, `src/font/SharedGridSet.zig:80-84`):
    /// sharing is only a claim until something can count it.
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

    fn key(family: &str, size: f32) -> ConfigKey {
        ConfigKey::new(family, size, 0.0, 1.0)
    }

    fn start() -> (ConfigRegistry<FakeAtlas>, ConfigId) {
        ConfigRegistry::new(key("monospace", 15.0), FakeAtlas(1))
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
            ConfigKey::new("Fira Code", 15.0, 0.0, 1.0),
            ConfigKey::new("monospace", 16.0, 0.0, 1.0),
            ConfigKey::new("monospace", 15.0, 1.0, 1.0),
            ConfigKey::new("monospace", 15.0, 0.0, 1.5),
        ] {
            assert_eq!(reg.find(&other), None, "{other:?} must not share");
        }
    }

    #[test]
    fn negative_zero_spacing_is_the_same_configuration_as_zero() {
        let (reg, first) = start();
        let neg = ConfigKey::new("monospace", 15.0, -0.0, 1.0);
        assert_eq!(reg.find(&neg), Some(first));
    }

    #[test]
    fn a_key_hands_its_selectors_back() {
        let (reg, first) = start();
        let k = reg.key(first);
        assert_eq!(k.font_family(), "monospace");
        assert_eq!(k.font_size(), 15.0);
        assert_eq!(k.letter_spacing(), 0.0);
        assert_eq!(k.line_height(), 1.0);
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
}
