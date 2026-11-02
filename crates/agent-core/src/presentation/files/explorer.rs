//! FileExplorerPane/useResizableWidth dimensions and keyboard controls in T3.
pub const EXPLORER_DEFAULT_WIDTH: f32 = 256.;
pub const EXPLORER_MIN_WIDTH: f32 = 160.;
pub const EXPLORER_CONTENT_MIN_WIDTH: f32 = 256.;
const EXPLORER_MAX_WIDTH: f32 = 560.;

pub fn explorer_max_width(available: f32) -> f32 {
    (available - EXPLORER_CONTENT_MIN_WIDTH).clamp(EXPLORER_MIN_WIDTH, EXPLORER_MAX_WIDTH)
}
pub fn explorer_width(preferred: f32, available: f32) -> f32 {
    if !preferred.is_finite() {
        return EXPLORER_DEFAULT_WIDTH;
    }
    preferred.clamp(EXPLORER_MIN_WIDTH, explorer_max_width(available))
}
pub fn explorer_key_width(key: &str, current: f32, available: f32) -> Option<f32> {
    let width = match key {
        "left" => current + 16.,
        "right" => current - 16.,
        "home" => EXPLORER_MIN_WIDTH,
        "end" => explorer_max_width(available),
        _ => return None,
    };
    Some(explorer_width(width, available))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explorer_preserves_content_space_and_clamps_keyboard_steps() {
        assert_eq!(explorer_width(320., 1000.), 320.);
        assert_eq!(explorer_width(320., 500.), 244.);
        assert_eq!(
            explorer_width(320., 1000.),
            320.,
            "narrowing must not overwrite the preference"
        );
        assert_eq!(explorer_width(900., 1000.), 560.);
        assert_eq!(explorer_width(-1., 1000.), 160.);
        assert_eq!(explorer_width(f32::NAN, 1000.), 256.);
        assert_eq!(explorer_max_width(300.), 160.);
        assert_eq!(explorer_key_width("left", 300., 1000.), Some(316.));
        assert_eq!(explorer_key_width("right", 300., 1000.), Some(284.));
        assert_eq!(explorer_key_width("left", 550., 1000.), Some(560.));
        assert_eq!(explorer_key_width("right", 164., 1000.), Some(160.));
        assert_eq!(explorer_key_width("home", 300., 1000.), Some(160.));
        assert_eq!(explorer_key_width("end", 300., 500.), Some(244.));
        assert_eq!(explorer_key_width("down", 300., 500.), None);
    }
}
