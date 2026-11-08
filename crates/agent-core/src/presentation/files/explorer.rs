//! The desktop client's file list widths beside an open file.
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explorer_preserves_content_space_and_keeps_the_preference_within_range() {
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
        assert_eq!(explorer_max_width(500.), 244.);
    }
}
