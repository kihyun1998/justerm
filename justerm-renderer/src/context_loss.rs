//! WebGL context-loss state machine (#269) — pure, host-tested.
//!
//! A WebGL context can be lost at any time (GPU reset, tab backgrounded, driver eviction);
//! every GL object it owned is destroyed with it. The browser signals this with a
//! `webglcontextlost` event and *may* later fire `webglcontextrestored`. This module owns the
//! resulting state machine; the browser wiring (event `Closure`s, GL resource recreation) lives
//! in `webgl` (wasm32).

/// What the renderer must do with the current frame, given the context's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAction {
    /// The context is lost: do nothing at all this frame.
    Skip,
    /// The context is live but its GL objects were destroyed: recreate them, then draw.
    Rebuild,
    /// The context is live and its resources are intact: draw.
    Draw,
}

/// Tracks whether the WebGL context is usable and whether its GL objects need recreating.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContextState {
    is_lost: bool,
    pending_rebuild: bool,
}

impl ContextState {
    /// The browser fired `webglcontextlost`.
    pub fn on_lost(&mut self) {
        self.is_lost = true;
    }

    /// The browser fired `webglcontextrestored`: the context works again, but everything it
    /// owned must be recreated.
    pub fn on_restored(&mut self) {
        self.is_lost = false;
        self.pending_rebuild = true;
    }

    /// The GL resources were recreated **successfully**. Call this only after the rebuild is
    /// committed — a failed rebuild leaves the flag set so the next frame retries (self-healing).
    pub fn rebuilt(&mut self) {
        self.pending_rebuild = false;
    }

    /// Whether the context is currently lost. Exposed for observation (the consumer may want to
    /// grey out the terminal); the renderer itself branches on [`action`](Self::action).
    pub fn is_lost(&self) -> bool {
        self.is_lost
    }

    /// What to do with the current frame.
    pub fn action(&self) -> FrameAction {
        // A lost context beats a pending rebuild: GL objects created on a dead context come back
        // with their "invalidated" flag set (WebGL 1.0 spec, § Context Lost), so a rebuild there
        // produces a pipeline that cannot link and an atlas that holds nothing — it must wait for
        // the *next* `webglcontextrestored`. beamterm's `render_frame` checks these in the opposite
        // order (terminal.rs:334) and rebuilds on a dead context in the lost→restored→lost window,
        // where its `link_program` status check then fails every frame.
        if self.is_lost {
            return FrameAction::Skip;
        }
        if self.pending_rebuild {
            return FrameAction::Rebuild;
        }
        FrameAction::Draw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_context_draws() {
        // The default state is a healthy context: no loss has been signalled, nothing to rebuild.
        assert_eq!(ContextState::default().action(), FrameAction::Draw);
    }

    #[test]
    fn a_lost_context_skips_the_frame() {
        // Every GL object died with the context; a draw call would be a no-op at best.
        let mut state = ContextState::default();
        state.on_lost();
        assert_eq!(state.action(), FrameAction::Skip);
    }

    #[test]
    fn a_restored_context_rebuilds_before_it_draws() {
        // The context object is usable again, but the program/buffers/atlas it owned are gone.
        let mut state = ContextState::default();
        state.on_lost();
        state.on_restored();
        assert_eq!(state.action(), FrameAction::Rebuild);
    }

    #[test]
    fn a_successful_rebuild_returns_to_drawing() {
        let mut state = ContextState::default();
        state.on_lost();
        state.on_restored();
        state.rebuilt();
        assert_eq!(state.action(), FrameAction::Draw);
    }

    #[test]
    fn a_loss_during_a_pending_rebuild_skips_rather_than_rebuilding_on_a_dead_context() {
        // lost → restored → lost again, all before the renderer got a frame to rebuild in.
        // `pending_rebuild` is still set, but the context is dead again: GL objects created now are
        // born invalidated (WebGL 1.0 spec, § Context Lost), so `is_lost` MUST win. beamterm checks
        // `pending_rebuild` first (terminal.rs `render_frame`) and errors out of `restore_context`
        // here, then never clears the flag — so it re-errors every frame until the next restore.
        let mut state = ContextState::default();
        state.on_lost();
        state.on_restored();
        state.on_lost();
        assert_eq!(state.action(), FrameAction::Skip);

        // ...and the deferred rebuild is not forgotten: the next restore still rebuilds.
        state.on_restored();
        assert_eq!(state.action(), FrameAction::Rebuild);
    }

    #[test]
    fn a_failed_rebuild_is_retried_on_the_next_frame() {
        // The rebuild path only calls `rebuilt()` once the new resources are committed; a
        // rasterise/GL failure leaves the flag set so the next frame tries again (self-healing,
        // mirroring `set_device_pixel_ratio`'s leave-the-old-atlas-intact contract).
        let mut state = ContextState::default();
        state.on_lost();
        state.on_restored();
        assert_eq!(state.action(), FrameAction::Rebuild); // attempt 1 — fails, no `rebuilt()`
        assert_eq!(state.action(), FrameAction::Rebuild); // attempt 2
    }

    #[test]
    fn a_restore_without_a_preceding_loss_still_rebuilds() {
        // A spurious `webglcontextrestored` (or one whose `webglcontextlost` we never saw, e.g. a
        // listener attached mid-loss): rebuilding is idempotent, so honour it rather than draw
        // with resources that may already be dead.
        let mut state = ContextState::default();
        state.on_restored();
        assert_eq!(state.action(), FrameAction::Rebuild);
    }
}
