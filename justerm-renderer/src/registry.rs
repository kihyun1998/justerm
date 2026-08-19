//! The grid registry — which terminal grids this renderer holds, and which of them are **drawn**
//! (#770, ADR-0021).
//!
//! Multi-viewport (#287) multiplies the per-grid tier and nothing else, so this module owns the
//! *multiplication* and stays ignorant of what a grid is: the payload is a type parameter, which is
//! what lets the whole registry be host-tested while `webgl.rs` — the only place that knows a grid
//! carries GPU state — stays `wasm32`-only (the crate's standing pure/glue split, #280).
//!
//! ## Registered is not drawn
//!
//! A registered grid that is not drawn is a **state**, not an absence. The consumer's adoption
//! design requires a hidden workspace's grid to stay registered with its viewport cleared
//! (penterm's `terminal-single-context-adoption` PRD, decision 3 — *"viewport-as-truth: a grid with
//! a viewport is drawn; hidden = no viewport, resources persist"*), because dropping and re-adding
//! it would reintroduce the re-attach cost Epic #287 exists to remove.
//!
//! So the drawn/not-drawn distinction is carried by `Option<Viewport>` and nothing else. Ghostty
//! converges on the state and diverges on its representation: it keeps a surface registered while
//! invisible and gates *both* the draw and the CPU cell rebuild on an explicit `flags.visible`
//! boolean, retaining the rect (`src/renderer/Thread.zig:110`, `:528-529`, `:646-648`). It can
//! retain the rect because a ghostty surface owns the OS window that produces it; here the rect's
//! producer is the consumer's DOM box, which is **unmeasurable while hidden** (`display:none` reads
//! back a zero rect), so a retained rect would be a copy that can be wrong on the way back. The
//! consumer re-supplies it on show — which it must anyway, since the layout it is coming back into
//! is why it was hidden.
//!
//! ## Identity
//!
//! A [`GridId`] is handed across the wasm boundary as a bare number, so the registry can never
//! reuse one: a stale handle in JS must fail loudly rather than silently address whichever grid
//! landed in the freed slot. Ghostty needs no ids at all — its registry is a list of pointers and
//! removal is a `swapRemove` (`src/App.zig:172`, `:200-202`) — which is a shape that does not cross
//! a language boundary, not a shape we chose against.

/// A handle to one registered grid. Opaque to the consumer, which holds it as a number.
///
/// Never reused: [`GridRegistry::register`] hands out monotonically increasing ids, so a handle to
/// a removed grid stays invalid for the renderer's whole life.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct GridId(u32);

impl GridId {
    /// The implicit grid every pre-multi-grid export acts on.
    ///
    /// **Scaffolding for the expand phase, retired by S5 (#773).** S2–S4 add the multi-grid form
    /// beside the single-grid one so `justerm-web` keeps working across every intermediate renderer
    /// release; the whole break lands in one release rather than in four. Until then this grid
    /// exists from construction to drop and [`GridRegistry::remove`] refuses it, because the
    /// legacy exports have nothing to act on without it.
    pub const DEFAULT: GridId = GridId(0);

    /// The number the consumer holds.
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Rebuild a handle from the number a consumer passed back. Unvalidated by construction — the
    /// registry lookup is what rejects an unknown or removed grid.
    pub fn from_raw(raw: u32) -> Self {
        GridId(raw)
    }
}

/// Where a grid draws, in **device pixels** on the shared drawing buffer.
///
/// Top-left origin, as the consumer measures a DOM box. GL's bottom-origin flip belongs to the site
/// that issues `gl.viewport`/`gl.scissor` (#771), not to the state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Viewport {
    /// The same rect as `gl.viewport` / `gl.scissor` take it: y measured from the drawing buffer's
    /// **bottom** edge instead of its top (#771).
    ///
    /// The flip lives here rather than at the consumer, and that is a producer question rather than
    /// a convenience one. The rect is produced by a DOM measurement, and the DOM's origin is
    /// top-left; GL's is bottom-left. Whoever crosses between the two spaces has to know the
    /// buffer's height, and the consumer's copy of that is a `canvas.height` it does not own — this
    /// renderer sizes the buffer, and may be granted less than it asked for (#339). So the only site
    /// that can flip a rect correctly is the one holding the granted height.
    ///
    /// Note that three.js pushes this outward: `renderer.setViewport(x, y, w, h)` passes straight to
    /// `gl.viewport` and its multiple-views example computes `bottom` itself
    /// (`examples/webgl_multiple_views.html:265`). It can, because its caller supplies fractions of
    /// a canvas it also owns; ours supplies a measured box.
    pub fn gl_rect(&self, buffer_height: i32) -> (i32, i32, i32, i32) {
        (
            self.x,
            buffer_height - (self.y + self.height),
            self.width,
            self.height,
        )
    }
}

/// Why a registry lookup was refused. Every variant is a *caller* error arriving from JS, so each
/// is surfaced as a thrown error rather than a silent no-op (the `apply_damage` precedent, #355).
#[derive(Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// No grid with this id — never registered, or removed. The two are deliberately one error:
    /// a reused id is what would make them distinguishable, and ids are not reused.
    UnknownGrid(u32),
    /// The implicit default grid cannot be removed while the single-grid exports still act on it.
    DefaultNotRemovable,
    /// The implicit default grid's viewport is the drawing buffer, written by `resize` and by
    /// nothing else — so a consumer can neither place nor hide it.
    DefaultViewportIsTheBuffer,
}

impl RegistryError {
    /// The message the wasm boundary throws.
    pub fn message(&self) -> String {
        match self {
            RegistryError::UnknownGrid(id) => {
                format!("justerm-renderer: no grid with id {id}")
            }
            RegistryError::DefaultNotRemovable => {
                "justerm-renderer: the default grid cannot be removed".to_string()
            }
            RegistryError::DefaultViewportIsTheBuffer => {
                "justerm-renderer: the default grid's viewport is the drawing buffer — resize it                  instead of placing it"
                    .to_string()
            }
        }
    }
}

struct Entry<T> {
    id: GridId,
    /// `None` = registered but not drawn. See the module doc.
    viewport: Option<Viewport>,
    grid: T,
}

/// Every grid this renderer holds, in registration order.
///
/// Order is stable across removal (`Vec::remove`, not `swap_remove`) so the draw loop #771 adds
/// visits grids in a deterministic order — a proof that reads pixels cannot tell "grid B drew over
/// grid A" from "the order changed" if the order is free to move. `N` is a terminal count, so the
/// linear scan a stable order costs is not a scan worth indexing away.
pub struct GridRegistry<T> {
    entries: Vec<Entry<T>>,
    next_id: u32,
}

/// The slot the implicit default grid occupies, for as long as it exists.
///
/// Not a convention the draw loop has to remember: [`GridRegistry::new`] puts it there and
/// [`GridRegistry::remove`] refuses to take it out, so nothing can shift it — a later grid is
/// pushed after it, and removing one only shifts the grids behind it.
pub const DEFAULT_SLOT: usize = 0;

impl<T> GridRegistry<T> {
    /// Start a registry holding the implicit default grid, drawn over `viewport`.
    ///
    /// The default arrives drawn because that is what the single-grid renderer already is — one
    /// grid filling the drawing buffer. A grid registered *later* arrives **not drawn**: nobody has
    /// said where it goes yet.
    pub fn new(default_grid: T, viewport: Viewport) -> Self {
        GridRegistry {
            entries: vec![Entry {
                id: GridId::DEFAULT,
                viewport: Some(viewport),
                grid: default_grid,
            }],
            next_id: GridId::DEFAULT.0 + 1,
        }
    }

    /// Register a grid. It is **not drawn** until [`set_viewport`](Self::set_viewport) places it.
    pub fn register(&mut self, grid: T) -> GridId {
        let id = GridId(self.next_id);
        self.next_id += 1;
        self.entries.push(Entry {
            id,
            viewport: None,
            grid,
        });
        id
    }

    /// Remove a grid and hand its state back, so the caller can release whatever the payload owns.
    ///
    /// Refuses [`GridId::DEFAULT`] — see its doc for why that special case exists and when it goes.
    pub fn remove(&mut self, id: GridId) -> Result<T, RegistryError> {
        if id == GridId::DEFAULT {
            return Err(RegistryError::DefaultNotRemovable);
        }
        let at = self.index_of(id)?;
        Ok(self.entries.remove(at).grid)
    }

    /// How many grids are registered, drawn or not.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The implicit grid the single-grid exports act on. Infallible: it is registered at
    /// construction and [`remove`](Self::remove) refuses it.
    pub fn default_grid(&self) -> &T {
        &self.entries[0].grid
    }

    /// Mutable form of [`default_grid`](Self::default_grid).
    pub fn default_grid_mut(&mut self) -> &mut T {
        &mut self.entries[0].grid
    }

    /// Place a grid — it is drawn from now on (#771 draws it; this slice only holds the state).
    ///
    /// Refuses [`GridId::DEFAULT`], whose rect is not the consumer's to write: see
    /// [`place_default`](Self::place_default).
    pub fn set_viewport(&mut self, id: GridId, viewport: Viewport) -> Result<(), RegistryError> {
        let at = self.index_of(self.consumer_placeable(id)?)?;
        self.entries[at].viewport = Some(viewport);
        Ok(())
    }

    /// Stop drawing a grid **without unregistering it**: every byte of its state stays resident, so
    /// coming back is a placement rather than a rebuild.
    ///
    /// Refuses [`GridId::DEFAULT`] for the same reason [`set_viewport`](Self::set_viewport) does —
    /// and it is the more important half. Letting a consumer clear the default's viewport would put
    /// the registry into a state the renderer contradicts: the default would report itself not
    /// drawn while still painting the whole canvas, because the single-grid draw path does not
    /// consult a viewport at all. At #771 it becomes worse than a wrong answer — a consumer looping
    /// over every id to hide everything would hide the one grid that must not be hidden, and stay
    /// hidden until something happened to resize.
    pub fn clear_viewport(&mut self, id: GridId) -> Result<(), RegistryError> {
        let at = self.index_of(self.consumer_placeable(id)?)?;
        self.entries[at].viewport = None;
        Ok(())
    }

    /// Re-place the default grid over the whole drawing buffer. The renderer's `resize` is the only
    /// caller, and that is the point of the method existing beside
    /// [`set_viewport`](Self::set_viewport) rather than as a special case inside it.
    ///
    /// **The default's rect has a different producer from every other grid's.** A registered grid is
    /// placed where the consumer measured its DOM box; the default is placed over the drawing
    /// buffer, which the renderer owns and derives from the grid it was sized to. A fact belongs to
    /// the site it is first true at, so the consumer cannot write this one and `resize` does not
    /// have to ask permission to.
    pub fn place_default(&mut self, viewport: Viewport) {
        self.entries[0].viewport = Some(viewport);
    }

    /// The default grid's viewport is the drawing buffer's, so the consumer may not write it.
    fn consumer_placeable(&self, id: GridId) -> Result<GridId, RegistryError> {
        if id == GridId::DEFAULT {
            return Err(RegistryError::DefaultViewportIsTheBuffer);
        }
        Ok(id)
    }

    /// Whether a grid currently has a viewport, i.e. whether it draws.
    pub fn is_drawn(&self, id: GridId) -> Result<bool, RegistryError> {
        Ok(self.entries[self.index_of(id)?].viewport.is_some())
    }

    /// Where a grid draws, or `None` if it is registered and not drawn (#771's draw loop reads
    /// this, once per slot per frame).
    ///
    /// **Addressed by slot, not by id.** The loop visits `0..len()`, so an id lookup per grid per
    /// frame would be a linear scan inside a linear walk for nothing; a slot is stable for as long
    /// as the frame it is read in, because nothing removes a grid mid-frame. Every consumer-facing
    /// method still takes an id — a slot is an internal address and is never handed across the wasm
    /// boundary, where a stale one *would* silently retarget (which is why ids are not reused).
    pub fn viewport_at(&self, at: usize) -> Option<Viewport> {
        self.entries[at].viewport
    }

    /// The grid in a slot. See [`viewport_at`](Self::viewport_at) for why the draw loop addresses
    /// grids this way.
    pub fn grid_at(&self, at: usize) -> &T {
        &self.entries[at].grid
    }

    /// Mutable form of [`grid_at`](Self::grid_at).
    pub fn grid_at_mut(&mut self, at: usize) -> &mut T {
        &mut self.entries[at].grid
    }

    /// The slot a grid id addresses. The bridge from the consumer's handle to the draw loop's
    /// address, and the same lookup every id-taking method here already makes.
    pub fn index_of(&self, id: GridId) -> Result<usize, RegistryError> {
        self.entries
            .iter()
            .position(|e| e.id == id)
            .ok_or(RegistryError::UnknownGrid(id.raw()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VP: Viewport = Viewport {
        x: 0,
        y: 0,
        width: 640,
        height: 384,
    };

    /// A stand-in for `GridTier`: the registry must not know what a grid is.
    #[derive(Debug, PartialEq, Eq, Clone)]
    struct FakeGrid {
        cells: Vec<u32>,
    }

    impl FakeGrid {
        fn new(tag: u32) -> Self {
            FakeGrid {
                cells: vec![tag; 4],
            }
        }
    }

    fn registry() -> GridRegistry<FakeGrid> {
        GridRegistry::new(FakeGrid::new(0), VP)
    }

    #[test]
    fn the_default_grid_is_registered_and_drawn_from_construction() {
        let r = registry();
        assert_eq!(r.len(), 1);
        assert_eq!(r.is_drawn(GridId::DEFAULT), Ok(true));
        assert_eq!(r.default_grid().cells, vec![0; 4]);
    }

    #[test]
    fn n_registrations_hold_n_grids_each_with_its_own_state() {
        let mut r = registry();
        let ids: Vec<GridId> = (1..=4).map(|t| r.register(FakeGrid::new(t))).collect();

        // N per-grid records, plus the default. Distinct ids, distinct state.
        assert_eq!(r.len(), 5);
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            4
        );
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(r.grid_for_test(*id).cells, vec![i as u32 + 1; 4]);
        }
    }

    #[test]
    fn a_freshly_registered_grid_is_registered_but_not_drawn() {
        let mut r = registry();
        let id = r.register(FakeGrid::new(1));

        // Registered…
        assert_eq!(r.len(), 2);
        // …and NOT drawn: nobody has said where it goes. Absence of a viewport is the state.
        assert_eq!(r.is_drawn(id), Ok(false));
        // The default is unaffected by a sibling arriving.
        assert_eq!(r.is_drawn(GridId::DEFAULT), Ok(true));
    }

    #[test]
    fn hiding_and_restoring_a_grid_keeps_every_byte_of_its_state() {
        let mut r = registry();
        let id = r.register(FakeGrid::new(7));
        r.set_viewport(id, VP).unwrap();
        r.grid_for_test_mut(id).cells = vec![9, 9, 9, 9];
        let before = r.grid_for_test(id).clone();

        r.clear_viewport(id).unwrap();
        assert_eq!(r.is_drawn(id), Ok(false), "cleared viewport = not drawn");
        assert_eq!(r.len(), 2, "not drawn is not unregistered");
        assert_eq!(
            r.grid_for_test(id),
            &before,
            "state must survive being hidden — otherwise coming back is a rebuild"
        );

        r.set_viewport(id, Viewport { x: 10, ..VP }).unwrap();
        assert_eq!(r.is_drawn(id), Ok(true));
        assert_eq!(r.grid_for_test(id), &before, "and survive coming back");
    }

    #[test]
    fn removing_a_grid_hands_its_state_back_and_drops_the_entry() {
        let mut r = registry();
        let id = r.register(FakeGrid::new(3));

        let taken = r.remove(id).expect("removable");
        assert_eq!(
            taken,
            FakeGrid::new(3),
            "the payload comes back to be released"
        );
        assert_eq!(r.len(), 1);
        assert_eq!(r.is_drawn(id), Err(RegistryError::UnknownGrid(id.raw())));
    }

    #[test]
    fn an_unknown_grid_is_an_error_on_every_lookup() {
        let mut r = registry();
        let ghost = GridId::from_raw(99);

        assert_eq!(r.remove(ghost), Err(RegistryError::UnknownGrid(99)));
        assert_eq!(
            r.set_viewport(ghost, VP),
            Err(RegistryError::UnknownGrid(99))
        );
        assert_eq!(r.clear_viewport(ghost), Err(RegistryError::UnknownGrid(99)));
        assert_eq!(r.is_drawn(ghost), Err(RegistryError::UnknownGrid(99)));
    }

    #[test]
    fn the_default_grid_cannot_be_removed_while_the_single_grid_exports_need_it() {
        let mut r = registry();
        assert_eq!(
            r.remove(GridId::DEFAULT),
            Err(RegistryError::DefaultNotRemovable)
        );
        assert_eq!(r.len(), 1);
        assert_eq!(r.default_grid().cells, vec![0; 4]);
    }

    #[test]
    fn an_id_is_never_reused_so_a_stale_handle_cannot_address_a_new_grid() {
        let mut r = registry();
        let first = r.register(FakeGrid::new(1));
        r.remove(first).unwrap();
        let second = r.register(FakeGrid::new(2));

        assert_ne!(
            first, second,
            "a JS consumer holds a bare number; reusing it would silently retarget it"
        );
        assert_eq!(
            r.is_drawn(first),
            Err(RegistryError::UnknownGrid(first.raw()))
        );
        assert_eq!(r.is_drawn(second), Ok(false));
    }

    #[test]
    fn removal_keeps_the_remaining_grids_in_registration_order() {
        let mut r = registry();
        let a = r.register(FakeGrid::new(1));
        let b = r.register(FakeGrid::new(2));
        let c = r.register(FakeGrid::new(3));
        let d = r.register(FakeGrid::new(4));

        // Remove with TWO grids still behind it. Removing the second-to-last cannot tell
        // `Vec::remove` from `swap_remove` — the swapped-in element lands where it already was —
        // and the first version of this test did exactly that and stayed green under the mutation.
        r.remove(a).unwrap();

        // Stable order, not `swap_remove`'s — the draw loop (#771) must be deterministic.
        assert_eq!(
            r.ids_for_test(),
            vec![GridId::DEFAULT, b, c, d],
            "the survivors keep their order"
        );
    }

    #[test]
    fn the_consumer_can_neither_place_nor_hide_the_default_grid() {
        let mut r = registry();
        let moved = Viewport { x: 40, ..VP };

        // The default's rect is the drawing buffer's, and the buffer has one producer — `resize`.
        assert_eq!(
            r.set_viewport(GridId::DEFAULT, moved),
            Err(RegistryError::DefaultViewportIsTheBuffer)
        );
        // The important half: a cleared default would report itself not drawn while still painting
        // the whole canvas, because the single-grid draw path consults no viewport at all.
        assert_eq!(
            r.clear_viewport(GridId::DEFAULT),
            Err(RegistryError::DefaultViewportIsTheBuffer)
        );
        assert_eq!(
            r.is_drawn(GridId::DEFAULT),
            Ok(true),
            "and neither took effect"
        );
    }

    #[test]
    fn resize_re_places_the_default_through_its_own_door() {
        let mut r = registry();
        let grown = Viewport {
            width: 1280,
            height: 768,
            ..VP
        };

        r.place_default(grown);

        assert_eq!(r.is_drawn(GridId::DEFAULT), Ok(true));
        assert_eq!(r.viewport_for_test(GridId::DEFAULT), Some(grown));
        // …and it did not disturb anyone else's placement.
        let id = r.register(FakeGrid::new(1));
        assert_eq!(r.is_drawn(id), Ok(false));
    }

    #[test]
    fn an_error_says_which_grid_and_why() {
        assert_eq!(
            RegistryError::UnknownGrid(7).message(),
            "justerm-renderer: no grid with id 7"
        );
        assert_eq!(
            RegistryError::DefaultNotRemovable.message(),
            "justerm-renderer: the default grid cannot be removed"
        );
        assert!(
            RegistryError::DefaultViewportIsTheBuffer
                .message()
                .contains("the drawing buffer")
        );
    }

    // Test-only reach into a grid by id. The renderer itself only ever needs the default (every
    // pre-#773 export acts on it), so shipping a public `get`/`get_mut` would be dead code on
    // wasm32 — where dead-code analysis is the trustworthy one (lib.rs).
    impl<T> GridRegistry<T> {
        fn grid_for_test(&self, id: GridId) -> &T {
            &self.entries[self.index_of(id).unwrap()].grid
        }
        fn grid_for_test_mut(&mut self, id: GridId) -> &mut T {
            let at = self.index_of(id).unwrap();
            &mut self.entries[at].grid
        }
        fn ids_for_test(&self) -> Vec<GridId> {
            self.entries.iter().map(|e| e.id).collect()
        }
        fn viewport_for_test(&self, id: GridId) -> Option<Viewport> {
            self.entries[self.index_of(id).unwrap()].viewport
        }
    }
}

#[cfg(test)]
mod viewport_tests {
    use super::*;

    /// A 640x384 buffer, and a rect that fills it — the single-grid arrangement.
    #[test]
    fn a_rect_filling_the_buffer_is_unchanged_by_the_flip() {
        let vp = Viewport {
            x: 0,
            y: 0,
            width: 640,
            height: 384,
        };
        assert_eq!(vp.gl_rect(384), (0, 0, 640, 384));
    }

    #[test]
    fn a_rect_at_the_buffers_top_edge_lands_at_the_far_end_of_gls_y_axis() {
        let vp = Viewport {
            x: 10,
            y: 0,
            width: 100,
            height: 50,
        };
        // GL measures from the bottom, so a rect flush with the top sits at `H - height`.
        assert_eq!(vp.gl_rect(384), (10, 334, 100, 50));
    }

    #[test]
    fn a_rect_at_the_buffers_bottom_edge_lands_at_gl_y_zero() {
        let vp = Viewport {
            x: 10,
            y: 334,
            width: 100,
            height: 50,
        };
        assert_eq!(vp.gl_rect(384), (10, 0, 100, 50));
    }

    #[test]
    fn stacked_rects_stay_disjoint_and_reverse_order_under_the_flip() {
        let top = Viewport {
            x: 0,
            y: 0,
            width: 640,
            height: 192,
        };
        let bottom = Viewport {
            x: 0,
            y: 192,
            width: 640,
            height: 192,
        };
        let (_, top_y, _, top_h) = top.gl_rect(384);
        let (_, bottom_y, _, bottom_h) = bottom.gl_rect(384);

        // The one the consumer put on top is the one GL puts higher up its y axis...
        assert!(top_y > bottom_y);
        // ...and they still tile the buffer exactly, with no overlap and no gap.
        assert_eq!(bottom_y, 0);
        assert_eq!(bottom_y + bottom_h, top_y);
        assert_eq!(top_y + top_h, 384);
    }

    #[test]
    fn doubling_the_device_pixel_ratio_doubles_the_flipped_rect() {
        // The same CSS layout measured at dpr 1 and at dpr 2: every device-px number doubles,
        // buffer height included. The flip must be linear in that, or a grid drawn correctly at
        // dpr 1 slides off its box at dpr 2 — the failure #331 is named for.
        let at_1 = Viewport {
            x: 5,
            y: 7,
            width: 100,
            height: 50,
        };
        let at_2 = Viewport {
            x: 10,
            y: 14,
            width: 200,
            height: 100,
        };
        let (x1, y1, w1, h1) = at_1.gl_rect(384);
        assert_eq!(at_2.gl_rect(768), (x1 * 2, y1 * 2, w1 * 2, h1 * 2));
    }
}

#[cfg(test)]
mod draw_order_tests {
    use super::*;

    const VP: Viewport = Viewport {
        x: 0,
        y: 0,
        width: 640,
        height: 384,
    };

    #[derive(Debug, PartialEq, Eq)]
    struct FakeGrid(u32);

    fn registry() -> GridRegistry<FakeGrid> {
        GridRegistry::new(FakeGrid(0), VP)
    }

    fn placed(x: i32) -> Viewport {
        Viewport {
            x,
            y: 0,
            width: 100,
            height: 50,
        }
    }

    /// What the draw loop walks: every slot, with the rect if the grid has one. The default is
    /// slot 0 and is drawn from construction, so a single-grid consumer's loop is one iteration.
    #[test]
    fn a_registered_grid_occupies_a_slot_before_anything_places_it() {
        let mut r = registry();
        r.register(FakeGrid(1));

        assert_eq!(r.len(), 2);
        assert_eq!(r.viewport_at(0), Some(VP));
        assert_eq!(r.viewport_at(1), None);
    }

    #[test]
    fn the_draw_order_is_registration_order_and_placing_does_not_reorder_it() {
        let mut r = registry();
        let a = r.register(FakeGrid(1));
        let b = r.register(FakeGrid(2));
        // Placed youngest-first: the order the draw loop sees must not follow it.
        r.set_viewport(b, placed(200)).unwrap();
        r.set_viewport(a, placed(100)).unwrap();

        let order: Vec<(u32, Option<i32>)> = (0..r.len())
            .map(|at| (r.grid_at(at).0, r.viewport_at(at).map(|v| v.x)))
            .collect();
        assert_eq!(order, vec![(0, Some(0)), (1, Some(100)), (2, Some(200))]);
    }

    #[test]
    fn hiding_a_grid_takes_it_out_of_the_draw_order_and_leaves_its_neighbours_in_place() {
        let mut r = registry();
        let a = r.register(FakeGrid(1));
        let b = r.register(FakeGrid(2));
        r.set_viewport(a, placed(100)).unwrap();
        r.set_viewport(b, placed(200)).unwrap();

        r.clear_viewport(a).unwrap();

        assert_eq!(r.viewport_at(1), None, "the hidden grid draws nowhere");
        assert_eq!(r.grid_at(1), &FakeGrid(1), "and is still in its slot");
        assert_eq!(
            r.viewport_at(2).map(|v| v.x),
            Some(200),
            "the grid after it did not shift"
        );
    }

    /// The draw loop needs to reach a grid by slot; the consumer's exports reach it by id. The
    /// bridge is one lookup, and it is the same one every id-taking method already makes.
    #[test]
    fn a_slot_can_be_reached_by_id_and_mutated_there() {
        let mut r = registry();
        let a = r.register(FakeGrid(1));

        let at = r.index_of(a).unwrap();
        r.grid_at_mut(at).0 = 42;

        assert_eq!(r.grid_at(at), &FakeGrid(42));
        assert_eq!(
            r.index_of(GridId::from_raw(99)),
            Err(RegistryError::UnknownGrid(99))
        );
    }
}
