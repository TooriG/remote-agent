//! The image viewer, as the desktop client's: its magnification, from a
//! quarter to four times the size that fits the viewer, a quarter at a time,
//! its reset titled with the percentage; and its gallery of the conversation's
//! generated images.
use crate::client::SessionImage;

/// A generated image's encoded data as a source the image views load, as the
/// conversation shows it.
pub fn encoded_image_source(data: &str) -> String {
    format!("data:image/png;base64,{data}")
}

/// The gallery's images, by the sources the conversation shows them with.
pub fn gallery_sources(images: Vec<SessionImage>) -> Vec<String> {
    images
        .into_iter()
        .map(|image| {
            if image.encoded {
                encoded_image_source(&image.source)
            } else {
                image.source
            }
        })
        .collect()
}

/// The viewer's title: where `shown` is among the gallery's `sources`
/// (`2 / 3`), or `画像` for an image the gallery lacks.
pub fn gallery_title(sources: &[String], shown: &str) -> String {
    sources
        .iter()
        .position(|source| source == shown)
        .map_or_else(
            || "画像".into(),
            |index| format!("{} / {}", index + 1, sources.len()),
        )
}

/// How far the image viewer magnifies its image: `1` fits it to the viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageZoom(f32);

impl ImageZoom {
    pub const MIN: f32 = 0.25;
    pub const MAX: f32 = 4.0;
    const STEP: f32 = 0.25;
    /// The image fitted to the viewer, as it opens and after a reset.
    pub const FIT: Self = Self(1.0);

    /// `scale` within the viewer's range, as a pinch reports it.
    pub fn new(scale: f32) -> Self {
        if scale.is_nan() {
            Self::FIT
        } else {
            Self(scale.clamp(Self::MIN, Self::MAX))
        }
    }

    pub fn scale(self) -> f32 {
        self.0
    }

    /// A step larger; `None` at the largest, whose button is then disabled.
    pub fn zoomed_in(self) -> Option<Self> {
        (self.0 < Self::MAX).then(|| Self::new(self.0 + Self::STEP))
    }

    /// A step smaller; `None` at the smallest.
    pub fn zoomed_out(self) -> Option<Self> {
        (self.0 > Self::MIN).then(|| Self::new(self.0 - Self::STEP))
    }

    /// The reset's title: the magnification in whole percent.
    pub fn label(self) -> String {
        format!("{:.0}%", self.0 * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn steps_by_a_quarter_between_a_quarter_and_four_times() {
        assert_eq!(ImageZoom::FIT.label(), "100%");
        assert_eq!(
            ImageZoom::FIT.zoomed_in().map(ImageZoom::label),
            Some("125%".into())
        );
        assert_eq!(
            ImageZoom::FIT.zoomed_out().map(ImageZoom::label),
            Some("75%".into())
        );
        assert_eq!(ImageZoom::new(4.0).zoomed_in(), None);
        assert_eq!(ImageZoom::new(0.25).zoomed_out(), None);
        assert_eq!(ImageZoom::new(3.9).zoomed_in(), Some(ImageZoom::new(4.0)));
        assert_eq!(ImageZoom::new(0.3).zoomed_out(), Some(ImageZoom::new(0.25)));
        assert_eq!(ImageZoom::new(9.0).scale(), 4.0);
        assert_eq!(ImageZoom::new(0.0).scale(), 0.25);
        assert_eq!(ImageZoom::new(f32::NAN), ImageZoom::FIT);
        assert_eq!(ImageZoom::new(1.126).label(), "113%");
    }

    #[test]
    fn the_gallery_shows_encoded_images_as_the_conversation_does_and_titles_the_shown_one() {
        let sources = gallery_sources(vec![
            SessionImage {
                source: "/tmp/one.png".into(),
                encoded: false,
            },
            SessionImage {
                source: "aGk=".into(),
                encoded: true,
            },
        ]);
        assert_eq!(sources, ["/tmp/one.png", "data:image/png;base64,aGk="]);
        assert_eq!(
            crate::presentation::body::item_body(&crate::models::Item::new(
                "image".into(),
                crate::models::ItemStatus::Completed,
                crate::models::ItemBody::ImageGeneration {
                    saved_path: None,
                    data: Some("aGk=".into()),
                    revised_prompt: None,
                },
            ))
            .images,
            [sources[1].clone()],
            "a thumbnail's source is its image's in the conversation",
        );
        assert_eq!(
            gallery_title(&sources, "data:image/png;base64,aGk="),
            "2 / 2"
        );
        assert_eq!(gallery_title(&sources, "/tmp/one.png"), "1 / 2");
        assert_eq!(gallery_title(&sources, "/tmp/attached.png"), "画像");
        assert_eq!(gallery_title(&[], "/tmp/one.png"), "画像");
    }

    proptest! {
        #[test]
        fn stepping_stays_in_range_and_moves_toward_its_end(scale in -10.0f32..10.0) {
            let zoom = ImageZoom::new(scale);
            prop_assert!((ImageZoom::MIN..=ImageZoom::MAX).contains(&zoom.scale()));
            match zoom.zoomed_in() {
                Some(larger) => prop_assert!(larger.scale() > zoom.scale() && larger.scale() <= ImageZoom::MAX),
                None => prop_assert_eq!(zoom.scale(), ImageZoom::MAX),
            }
            match zoom.zoomed_out() {
                Some(smaller) => prop_assert!(smaller.scale() < zoom.scale() && smaller.scale() >= ImageZoom::MIN),
                None => prop_assert_eq!(zoom.scale(), ImageZoom::MIN),
            }
        }
    }
}
