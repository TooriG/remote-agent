//! How the composer's controls fit beside attaching and sending, as T3 Code's
//! footer decides it (`composerFooterLayout.ts`,
//! `resolveRestingComposerControlsLayout`): the blocks after the model control
//! (here the approval control) first show only their icons, from the last one,
//! then move into the "more" menu; the model's name then shortens down to
//! [`MODEL_MINIMUM_WIDTH`]; when not even that fits, the controls hide.

/// The narrowest the model control gets, its name shortened (T3's `min-w-13`).
pub const MODEL_MINIMUM_WIDTH: f32 = 52.;

/// Promotions wait for this much room, so that a width that hovers at a step's
/// edge does not flip between two layouts (T3's `RESTING_CONTROLS_SLACK_PX`).
const SLACK: f32 = 1.;

/// The controls at their natural size, in points, as T3 measures them.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlsMeasurement {
    /// Between the controls.
    pub gap: f32,
    /// The model control showing its whole name.
    pub model: f32,
    /// Each block after it, in order, with its label.
    pub blocks: Vec<f32>,
    /// Each block as its icon.
    pub icon_blocks: Vec<f32>,
    /// The "more" control.
    pub overflow: f32,
}

/// How the controls show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlsLayout {
    /// How many blocks, from the last, are in the "more" menu.
    pub hidden: usize,
    /// How many blocks, from the last, show only their icon.
    pub icon_only: usize,
    /// Whether the controls show at all.
    pub visible: bool,
}

impl Default for ControlsLayout {
    fn default() -> Self {
        Self {
            hidden: 0,
            icon_only: 0,
            visible: true,
        }
    }
}

impl ControlsLayout {
    /// Whether block `index` shows only its icon.
    pub fn icon_only_at(&self, index: usize, count: usize) -> bool {
        index + self.icon_only >= count
    }

    /// Whether block `index` is in the "more" menu.
    pub fn hidden_at(&self, index: usize, count: usize) -> bool {
        index + self.hidden >= count
    }
}

/// The controls' width with `hidden` blocks in the menu and `icon_only` ones
/// as icons, the model control `fixed` wide.
fn width(measurement: &ControlsMeasurement, fixed: f32, hidden: usize, icon_only: usize) -> f32 {
    let count = measurement.blocks.len();
    let visible = count - hidden.min(count);
    let blocks: f32 = (0..visible)
        .map(|index| {
            if index + icon_only >= count {
                measurement.icon_blocks[index]
            } else {
                measurement.blocks[index]
            }
        })
        .sum();
    let more = hidden > 0;
    fixed
        + blocks
        + if more { measurement.overflow } else { 0. }
        + measurement.gap * (visible + usize::from(more)) as f32
}

/// The first step whose controls fit `host` points: each step makes one more
/// block an icon, from the last, then moves one more into the menu. Going back
/// to an earlier step than `previous` waits for [`SLACK`] more room.
pub fn controls_layout(
    measurement: &ControlsMeasurement,
    host: f32,
    previous: Option<ControlsLayout>,
) -> ControlsLayout {
    let count = measurement.blocks.len();
    let previous_step = previous.map_or(0, |previous| {
        if previous.hidden > 0 {
            count + previous.hidden.min(count)
        } else {
            previous.icon_only.min(count)
        }
    });
    let at = |step: usize, fixed: f32| {
        width(
            measurement,
            fixed,
            step.saturating_sub(count),
            step.min(count),
        )
    };
    let mut step = 0;
    while step < 2 * count
        && at(step, measurement.model) > host - if step < previous_step { SLACK } else { 0. }
    {
        step += 1;
    }
    let minimum = at(step, MODEL_MINIMUM_WIDTH);
    let visible = if previous.is_some_and(|previous| !previous.visible) {
        minimum <= host - SLACK
    } else {
        minimum <= host
    };
    ControlsLayout {
        hidden: step.saturating_sub(count),
        icon_only: step.min(count),
        visible,
    }
}

/// How wide the model control is in `layout`: its whole name while that fits
/// beside the other controls, shortened down to [`MODEL_MINIMUM_WIDTH`] past
/// the last step, as a flexible item between those bounds shrinks.
pub fn model_width(measurement: &ControlsMeasurement, host: f32, layout: ControlsLayout) -> f32 {
    let others = width(measurement, 0., layout.hidden, layout.icon_only);
    (host - others).clamp(
        MODEL_MINIMUM_WIDTH,
        measurement.model.max(MODEL_MINIMUM_WIDTH),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measurement() -> ControlsMeasurement {
        ControlsMeasurement {
            gap: 2.,
            model: 100.,
            blocks: vec![150.],
            icon_blocks: vec![32.],
            overflow: 32.,
        }
    }

    fn layout(hidden: usize, icon_only: usize, visible: bool) -> ControlsLayout {
        ControlsLayout {
            hidden,
            icon_only,
            visible,
        }
    }

    #[test]
    fn a_block_shows_its_icon_then_moves_into_the_menu_then_the_controls_hide() {
        let m = measurement();
        // 100 + 150 + 2: everything.
        assert_eq!(controls_layout(&m, 252., None), layout(0, 0, true));
        // 100 + 32 + 2: the icon.
        assert_eq!(controls_layout(&m, 251.9, None), layout(0, 1, true));
        assert_eq!(controls_layout(&m, 134., None), layout(0, 1, true));
        // The menu is as wide as the icon: past it, the name shortens.
        assert_eq!(controls_layout(&m, 133.9, None), layout(1, 1, true));
        // 52 + 32 + 2 is the least that still shows.
        assert_eq!(controls_layout(&m, 86., None), layout(1, 1, true));
        assert_eq!(controls_layout(&m, 85.9, None), layout(1, 1, false));
    }

    #[test]
    fn the_name_shortens_only_past_the_last_step() {
        let m = measurement();
        assert_eq!(model_width(&m, 300., layout(0, 0, true)), 100.);
        assert_eq!(model_width(&m, 134., layout(0, 1, true)), 100.);
        assert_eq!(model_width(&m, 120., layout(1, 1, true)), 86.);
        assert_eq!(
            model_width(&m, 86., layout(1, 1, true)),
            MODEL_MINIMUM_WIDTH
        );
        assert_eq!(
            model_width(&m, 40., layout(1, 1, false)),
            MODEL_MINIMUM_WIDTH
        );
    }

    #[test]
    fn going_back_a_step_waits_for_a_point_more() {
        let m = measurement();
        let icon = layout(0, 1, true);
        assert_eq!(controls_layout(&m, 252., Some(icon)), icon);
        assert_eq!(controls_layout(&m, 253., Some(icon)), layout(0, 0, true));
        // Moving on to a later step does not wait.
        assert_eq!(controls_layout(&m, 251.9, Some(layout(0, 0, true))), icon);
        let hidden = layout(1, 1, false);
        assert_eq!(controls_layout(&m, 86., Some(hidden)), hidden);
        assert_eq!(controls_layout(&m, 87., Some(hidden)), layout(1, 1, true));
    }

    #[test]
    fn several_blocks_turn_to_icons_from_the_last() {
        let m = ControlsMeasurement {
            gap: 0.,
            model: 100.,
            blocks: vec![80., 60.],
            icon_blocks: vec![30., 20.],
            overflow: 10.,
        };
        assert_eq!(controls_layout(&m, 240., None), layout(0, 0, true));
        assert_eq!(controls_layout(&m, 200., None), layout(0, 1, true));
        assert!(layout(0, 1, true).icon_only_at(1, 2));
        assert!(!layout(0, 1, true).icon_only_at(0, 2));
        assert_eq!(controls_layout(&m, 150., None), layout(0, 2, true));
        assert_eq!(controls_layout(&m, 149., None), layout(1, 2, true));
        assert!(layout(1, 2, true).hidden_at(1, 2));
        assert!(!layout(1, 2, true).hidden_at(0, 2));
        assert_eq!(controls_layout(&m, 124., None), layout(2, 2, true));
    }
}
