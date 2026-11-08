//! Row-identity decisions, independent of native list runtimes.

/// Keep the mounted identity while a data splice precedes the runtime's
/// position update. Recycling to a different position binds the new row.
/// The caller resolves the retained key in the current conversation only.
///
/// A list that replaces a run of rows may reuse a cell at the position it
/// had: no position update follows, and keeping the old row would show it
/// twice. So a retention is kept for one answer of the runtime only:
/// `retained_before` says the cell already kept it for these same rows.
pub fn retained_row_index(
    previous_position: Option<usize>,
    position: usize,
    retained_index: Option<usize>,
    row_count: usize,
    retained_before: bool,
) -> Option<usize> {
    if previous_position == Some(position)
        && !retained_before
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
    fn splices_retain_the_mounted_message_until_the_runtime_updates_its_position() {
        // A progress row before the answer disappeared; its old position has
        // no data yet, but the answer remains mounted under its original key.
        assert_eq!(retained_row_index(Some(3), 3, Some(2), 3, false), Some(2));
        // Prepending a history page also precedes the runtime's index update.
        assert_eq!(
            retained_row_index(Some(3), 3, Some(23), 30, false),
            Some(23)
        );
        // The runtime then confirms that shifted position.
        assert_eq!(
            retained_row_index(Some(3), 23, Some(23), 30, false),
            Some(23)
        );
        // A cell recycled for another message must bind that new position.
        assert_eq!(retained_row_index(Some(3), 8, Some(3), 30, false), Some(8));
        // A removed key, new conversation or stale retained index cannot keep
        // the previous conversation's content alive.
        assert_eq!(retained_row_index(Some(3), 3, None, 4, false), Some(3));
        assert_eq!(retained_row_index(None, 3, Some(2), 4, false), Some(3));
        assert_eq!(retained_row_index(Some(3), 3, Some(4), 4, false), Some(3));
        assert_eq!(retained_row_index(Some(3), 3, None, 3, false), None);
        assert_eq!(retained_row_index(None, 0, None, 0, false), None);
    }

    #[test]
    fn a_cell_reused_at_its_position_shows_the_row_there_once_the_runtime_answered() {
        // Working, a commentary and the live row became the commentary, two
        // tools and the error: the list replaced them all and reused the
        // commentary's cell for the first tool, at the same position.
        assert_eq!(retained_row_index(Some(2), 2, Some(1), 5, false), Some(1));
        assert_eq!(retained_row_index(Some(2), 2, Some(1), 5, true), Some(2));
    }
}
