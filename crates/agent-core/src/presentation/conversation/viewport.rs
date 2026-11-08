//! Viewport and row-identity decisions, independent of native list runtimes.

/// While reading back, fetch the next page with two viewports still available.
/// Failed reads remain manual retries; opening/following the latest answer must
/// not walk the entire archive merely because initial sizes are still estimates.
pub fn should_prefetch_history(
    has_more: bool,
    loading: bool,
    failed: bool,
    following_latest: bool,
    distance_to_oldest: f32,
    viewport_height: f32,
) -> bool {
    has_more
        && !loading
        && !failed
        && !following_latest
        && distance_to_oldest.is_finite()
        && viewport_height.is_finite()
        && viewport_height > 0.0
        && distance_to_oldest <= 2.0 * viewport_height
}

/// Keep the mounted identity while a data splice precedes the runtime's
/// position update. Recycling to a different position binds the new row.
/// The caller resolves the retained key in the current conversation only.
pub fn retained_row_index(
    previous_position: Option<usize>,
    position: usize,
    retained_index: Option<usize>,
    row_count: usize,
) -> Option<usize> {
    if previous_position == Some(position)
        && let Some(index) = retained_index.filter(|&index| index < row_count)
    {
        return Some(index);
    }
    (position < row_count).then_some(position)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_prefetch_leaves_time_to_render_and_does_not_retry_failures() {
        for distance in [-10.0, 0.0, 300.0, 600.0] {
            assert!(should_prefetch_history(
                true, false, false, false, distance, 300.0
            ));
        }
        for distance in [600.1, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert!(!should_prefetch_history(
                true, false, false, false, distance, 300.0
            ));
        }
        for (more, loading, failed, following) in [
            (false, false, false, false),
            (true, true, false, false),
            (true, false, true, false),
            (true, false, false, true),
        ] {
            assert!(!should_prefetch_history(
                more, loading, failed, following, 0.0, 300.0
            ));
        }
        for viewport in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(!should_prefetch_history(
                true, false, false, false, 0.0, viewport
            ));
        }
    }

    #[test]
    fn splices_retain_the_mounted_message_until_the_runtime_updates_its_position() {
        // A progress row before the answer disappeared; its old position has
        // no data yet, but the answer remains mounted under its original key.
        assert_eq!(retained_row_index(Some(3), 3, Some(2), 3), Some(2));
        // Prepending a history page also precedes the runtime's index update.
        assert_eq!(retained_row_index(Some(3), 3, Some(23), 30), Some(23));
        // The runtime then confirms that shifted position.
        assert_eq!(retained_row_index(Some(3), 23, Some(23), 30), Some(23));
        // A cell recycled for another message must bind that new position.
        assert_eq!(retained_row_index(Some(3), 8, Some(3), 30), Some(8));
        // A removed key, new conversation or stale retained index cannot keep
        // the previous conversation's content alive.
        assert_eq!(retained_row_index(Some(3), 3, None, 4), Some(3));
        assert_eq!(retained_row_index(None, 3, Some(2), 4), Some(3));
        assert_eq!(retained_row_index(Some(3), 3, Some(4), 4), Some(3));
        assert_eq!(retained_row_index(Some(3), 3, None, 3), None);
        assert_eq!(retained_row_index(None, 0, None, 0), None);
    }
}
