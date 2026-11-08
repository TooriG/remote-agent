//! How the composer's row of controls fits its width, as T3 Code's footer does
//! (`composerFooterLayout.ts`): the approval control drops its label first,
//! while attaching, the model, dictation and sending keep their place. T3's
//! next step moves the control into a "more" menu, but here that menu would
//! hold only this control and be as wide as its icon, so the window instead
//! keeps room for the narrowest row and no control is ever cut off.

/// How the approval control shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPlacement {
    /// Its icon and the current mode.
    Labelled,
    /// Its icon alone.
    Icon,
}

/// The widths of the row and of its controls at their natural size, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FooterWidths {
    /// The row's.
    pub row: f32,
    /// The controls that always show whole: attaching, the model, dictation, sending.
    pub fixed: f32,
    /// The approval control with its label.
    pub approval: f32,
    /// The approval control as its icon.
    pub approval_icon: f32,
}

/// With its label while the row has room for it.
pub fn approval_placement(widths: FooterWidths) -> ApprovalPlacement {
    if widths.fixed + widths.approval <= widths.row {
        ApprovalPlacement::Labelled
    } else {
        ApprovalPlacement::Icon
    }
}

/// The narrowest row that still shows every control whole.
pub fn minimum_row_width(widths: FooterWidths) -> f32 {
    widths.fixed + widths.approval_icon
}

/// The narrowest window that keeps the row at least `minimum` wide, as the
/// window is laid out now: narrowing the window narrows the row by as much at
/// most, so this never lets the row get too narrow. It is wider than the window
/// when the row already lacks room (a panel opened beside it).
pub fn minimum_window_width(window: f32, row: f32, minimum: f32) -> f32 {
    window - (row - minimum)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(row: f32) -> FooterWidths {
        FooterWidths {
            row,
            fixed: 220.,
            approval: 150.,
            approval_icon: 32.,
        }
    }

    #[test]
    fn the_approval_control_keeps_its_label_while_the_row_has_room() {
        let placement = |row| approval_placement(widths(row));
        assert_eq!(placement(370.), ApprovalPlacement::Labelled);
        assert_eq!(placement(369.9), ApprovalPlacement::Icon);
        assert_eq!(placement(0.), ApprovalPlacement::Icon);
    }

    #[test]
    fn the_narrowest_row_keeps_every_control_whole() {
        assert_eq!(minimum_row_width(widths(0.)), 252.);
    }

    #[test]
    fn the_window_keeps_room_for_the_narrowest_row_and_grows_when_it_lacks_it() {
        assert_eq!(minimum_window_width(1000., 500., 252.), 752.);
        assert_eq!(minimum_window_width(752., 252., 252.), 752.);
        assert_eq!(minimum_window_width(700., 200., 252.), 752.);
    }
}
