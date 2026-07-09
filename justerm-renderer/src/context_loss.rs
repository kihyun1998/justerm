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
    restore_overdue: bool,
    /// Bumped on every `webglcontextlost`. A restore deadline is armed for one specific loss and
    /// carries that loss's epoch, so a timer left over from an earlier loss — the browser has no
    /// obligation to have run it before the next one — identifies itself as stale and stays quiet
    /// instead of cutting the current loss's grace period short (#327).
    loss_epoch: u32,
}

/// How long to wait for `webglcontextrestored` before telling the consumer the context has not come
/// back (#327). Matches xterm.js (`addons/addon-webgl/src/WebglRenderer.ts:135`). It is a *policy*
/// default: the consumer overrides it, because only the consumer knows how long a blank terminal is
/// tolerable against how long its GPU takes to recover.
pub const DEFAULT_RESTORE_TIMEOUT_MS: i32 = 3000;

impl ContextState {
    /// The browser fired `webglcontextlost`. Opens a new loss episode: the caller arms a restore
    /// deadline stamped with the current [`loss_epoch`](Self::loss_epoch).
    pub fn on_lost(&mut self) {
        self.is_lost = true;
        self.restore_overdue = false;
        self.loss_epoch = self.loss_epoch.wrapping_add(1);
    }

    /// Identifies the current loss episode. A restore deadline must carry the epoch it was armed
    /// for and hand it back to [`on_restore_deadline`](Self::on_restore_deadline).
    pub fn loss_epoch(&self) -> u32 {
        self.loss_epoch
    }

    /// The browser fired `webglcontextrestored`: the context works again, but everything it
    /// owned must be recreated. Clears the overdue flag — however late it arrived, the context is
    /// back and the renderer heals (xterm.js's restored handler is likewise unguarded).
    pub fn on_restored(&mut self) {
        self.is_lost = false;
        self.pending_rebuild = true;
        self.restore_overdue = false;
    }

    /// Whether the restore deadline passed without the context coming back (#327). Advisory: a late
    /// `webglcontextrestored` clears it and the renderer recovers. Exposed for a consumer that
    /// registers its callback after the fact, or that polls instead of subscribing.
    pub fn restore_overdue(&self) -> bool {
        self.restore_overdue
    }

    /// The restore deadline armed for loss episode `epoch` expired. Returns whether the consumer
    /// should be notified **now** — true at most once per loss, because the notification is
    /// destructive (xterm.js's consumer tears the WebGL renderer down and falls back to a DOM one).
    ///
    /// Deadlines are never cancelled — the renderer cannot safely own the timer closure, see
    /// `webgl::arm_restore_deadline` — so a deadline can land with nothing to say. Three ways:
    /// - the context came back (`!is_lost`) — the deadline lost the race, say nothing;
    /// - we already notified for this loss (`restore_overdue`);
    /// - the deadline belongs to an older loss (`epoch` mismatch) — see [`loss_epoch`].
    ///
    /// [`loss_epoch`]: Self::loss_epoch
    pub fn on_restore_deadline(&mut self, epoch: u32) -> bool {
        if epoch != self.loss_epoch || !self.is_lost || self.restore_overdue {
            return false;
        }
        self.restore_overdue = true;
        true
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
    fn a_deadline_that_expires_while_still_lost_notifies_the_consumer() {
        // #327: the browser may never restore the context. Once the deadline passes, the consumer
        // is told, so it can apply its policy (VSCode swaps in its DOM renderer).
        let mut state = ContextState::default();
        state.on_lost();
        assert!(state.on_restore_deadline(state.loss_epoch()));
    }

    #[test]
    fn a_deadline_that_lands_after_the_restore_notifies_nobody() {
        // Our deadlines are never cancelled, so this one still lands after the context came back.
        // The context is alive; telling the consumer "it never came back" would be a lie.
        let mut state = ContextState::default();
        state.on_lost();
        state.on_restored();
        assert!(!state.on_restore_deadline(state.loss_epoch()));
    }

    #[test]
    fn one_loss_notifies_the_consumer_exactly_once() {
        // The consumer's handler is destructive — VSCode tears the renderer down — so it must not
        // run twice for a single loss, however many deadlines land.
        let mut state = ContextState::default();
        state.on_lost();
        assert!(state.on_restore_deadline(state.loss_epoch()));
        assert!(!state.on_restore_deadline(state.loss_epoch()));
    }

    #[test]
    fn a_restore_arriving_after_the_deadline_still_heals() {
        // The deadline is a hint to the consumer, NOT a verdict on the context. Chromium re-attempts
        // a real context restore every second indefinitely (webgl_rendering_context_base.cc:330
        // `kDurationBetweenRestoreAttempts`, retried at :9039), and a GPU-process restart easily
        // outlasts the 3 s xterm.js waits — so a restore arriving after we warned the consumer is a
        // normal event. Rebuild and draw. (xterm.js's restored handler is likewise unguarded; it is
        // its consumer, VSCode, that disposes the renderer.)
        let mut state = ContextState::default();
        state.on_lost();
        state.on_restore_deadline(state.loss_epoch());
        assert!(state.restore_overdue());

        state.on_restored();
        assert!(!state.restore_overdue());
        assert_eq!(state.action(), FrameAction::Rebuild);
    }

    #[test]
    fn an_overdue_context_is_still_skipped_not_drawn_on() {
        // Passing the deadline changes what we TELL the consumer, not what we do with the frame.
        let mut state = ContextState::default();
        state.on_lost();
        state.on_restore_deadline(state.loss_epoch());
        assert_eq!(state.action(), FrameAction::Skip);
    }

    #[test]
    fn a_second_loss_re_arms_the_notification() {
        // lost → notified → restored → lost again: the consumer must hear about the new loss too.
        let mut state = ContextState::default();
        state.on_lost();
        let first = state.loss_epoch();
        assert!(state.on_restore_deadline(first));
        state.on_restored();

        state.on_lost();
        assert!(state.on_restore_deadline(state.loss_epoch()));
    }

    #[test]
    fn a_deadline_left_over_from_a_previous_loss_never_notifies() {
        // t=0    lost         → deadline armed for loss #1
        // t=0.5  restored     → healed; loss #1's timer task is still pending in the browser
        // t=1.0  lost again   → deadline armed for loss #2, due much later
        // t=3.0  loss #1's timer fires. The context IS lost and we have NOT notified for loss #2,
        //        so every is_lost/overdue check says "notify" — yet loss #2's grace period has
        //        barely begun. Only the epoch tells the two deadlines apart.
        let mut state = ContextState::default();
        state.on_lost();
        let stale = state.loss_epoch();
        state.on_restored();
        state.on_lost();

        assert!(!state.on_restore_deadline(stale)); // loss #1's timer: not mine, stay quiet
        assert!(!state.restore_overdue());
        assert!(state.on_restore_deadline(state.loss_epoch())); // loss #2's own deadline, later
    }

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
