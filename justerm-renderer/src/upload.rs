//! Dirty-region upload planning (#263): decide the minimal GPU work to reconcile the
//! instance buffer between frames. justerm re-packs the whole grid each frame (from the
//! persistent `FrameGrid`), so instead of beamterm's mark-bitmask this diffs the freshly
//! packed instances against what was last uploaded and returns the contiguous ranges that
//! actually changed. Diffing (not marking) also catches a glyph-slot change on an
//! *undamaged* cell caused by atlas LRU eviction — a mark keyed off frame damage would
//! miss it. The GL layer (`webgl`, wasm32) executes the plan via `buffer_sub_data`.

/// The GPU upload needed to reconcile the instance buffer with a freshly packed frame.
#[derive(Debug, PartialEq)]
pub enum UploadPlan {
    /// The buffer size changed (first frame / resize) — reallocate and upload the whole buffer.
    Full,
    /// Upload these half-open `[start, end)` **instance-index** ranges via `buffer_sub_data`;
    /// an empty vec means nothing changed (no upload at all).
    Ranges(Vec<(usize, usize)>),
}

/// Plan the upload to turn the GPU's `prev` instance buffer into `curr`. `stride` is the
/// float count per instance (`INSTANCE_FLOATS`). A size change forces a `Full` reupload
/// (the buffer must be reallocated); otherwise the changed instances collapse into maximal
/// contiguous ranges.
pub fn plan_upload(prev: &[f32], curr: &[f32], stride: usize) -> UploadPlan {
    if prev.len() != curr.len() {
        return UploadPlan::Full;
    }
    let differs = |i: usize| {
        let lo = i * stride;
        prev[lo..lo + stride] != curr[lo..lo + stride]
    };
    let n = curr.len() / stride;
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < n {
        if differs(i) {
            let start = i;
            while i < n && differs(i) {
                i += 1;
            }
            ranges.push((start, i)); // maximal contiguous run of changed instances
        } else {
            i += 1;
        }
    }
    UploadPlan::Ranges(ranges)
}

/// Drop the upload baseline so the next [`plan_upload`] returns [`UploadPlan::Full`]. Call this
/// whenever the GPU instance buffer stops matching `uploaded` for a reason a *diff* cannot see —
/// i.e. the buffer itself was destroyed and reallocated, as a WebGL context loss does (#269).
/// This is the diff-based analogue of beamterm's `DirtyRegions::mark_all()` in
/// `recreate_resources` and of xterm.js's `_requestRedrawViewport()` on `webglcontextrestored`.
pub fn invalidate_baseline(uploaded: &mut Vec<f32>) {
    uploaded.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_changed_instance_yields_a_single_range() {
        // 3 instances, stride 2. Only instance 1 differs.
        let prev = vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0];
        let curr = vec![0.0, 0.0, 9.0, 1.0, 2.0, 2.0];
        assert_eq!(
            plan_upload(&prev, &curr, 2),
            UploadPlan::Ranges(vec![(1, 2)])
        );
    }

    #[test]
    fn an_identical_frame_plans_no_upload() {
        // The 0-upload-on-no-change contract (#263 AC): identical buffers → no ranges.
        let buf = vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(plan_upload(&buf, &buf, 2), UploadPlan::Ranges(vec![]));
    }

    #[test]
    fn disjoint_changes_yield_separate_ranges() {
        // 5 instances; instances 0 and 3 change, 1/2/4 unchanged → two ranges, the gap skipped.
        let prev = vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0];
        let curr = vec![8.0, 0.0, 1.0, 1.0, 2.0, 2.0, 8.0, 3.0, 4.0, 4.0];
        assert_eq!(
            plan_upload(&prev, &curr, 2),
            UploadPlan::Ranges(vec![(0, 1), (3, 4)])
        );
    }

    #[test]
    fn adjacent_changes_merge_into_one_range() {
        // Instances 1 and 2 both change → one contiguous [1, 3) upload, not two calls.
        let prev = vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0];
        let curr = vec![0.0, 0.0, 9.0, 1.0, 9.0, 2.0, 3.0, 3.0];
        assert_eq!(
            plan_upload(&prev, &curr, 2),
            UploadPlan::Ranges(vec![(1, 3)])
        );
    }

    #[test]
    fn a_restored_context_forces_a_full_reupload_of_an_identical_frame() {
        // #269: a context loss destroys `instance_vbo`; the restore path allocates a fresh, EMPTY
        // one. The baseline still mirrors the pre-loss frame, so an identical restored frame would
        // diff to zero ranges, skip the upload, and leave the fresh buffer empty — a blank render
        // that never self-heals. Invalidating the baseline is what makes the next plan `Full`.
        let instances = vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let mut uploaded = instances.clone();

        // While the context lives, re-packing the same frame is genuinely zero GPU work (#263).
        assert_eq!(
            plan_upload(&uploaded, &instances, 2),
            UploadPlan::Ranges(vec![])
        );

        invalidate_baseline(&mut uploaded);

        // After the restore, the very same frame must refill the whole buffer.
        assert_eq!(plan_upload(&uploaded, &instances, 2), UploadPlan::Full);
    }

    #[test]
    fn a_size_change_forces_a_full_reupload() {
        // First frame / resize: the GPU buffer must be reallocated, so no sub-range diff.
        let prev = vec![0.0, 0.0];
        let curr = vec![0.0, 0.0, 1.0, 1.0];
        assert_eq!(plan_upload(&prev, &curr, 2), UploadPlan::Full);
        assert_eq!(plan_upload(&[], &curr, 2), UploadPlan::Full); // empty prev (first frame)
    }
}
