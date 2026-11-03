//! Chat ContextChip accents and the contrast solver from T3's
//! lib/contextChipAccents.ts and themePalette.ts. Surfaces are opaque sRGB.
use super::desktop_file_icon;

pub fn desktop_file_chip_color(path: &str, surfaces: &[u32]) -> u32 {
    let (chroma, hue) = match desktop_file_icon(path) {
        "t3-file-icon-pdf" => (0.16, 24.),
        "t3-file-icon-word" => (0.136, 248.),
        "t3-file-icon-spreadsheet" => (0.134, 148.),
        "t3-file-icon-slides" => (0.134, 56.),
        "t3-file-icon-epub" => (0.16, 291.),
        _ => (0.11, 215.),
    };
    let contrast = |color| {
        surfaces
            .iter()
            .map(|surface| contrast_ratio(color, *surface))
            .fold(f64::INFINITY, f64::min)
    };
    let black = contrast(0);
    let white = contrast(0xffffff);
    let lighter = white > black;
    let minimum = 5f64.min(black.max(white));
    if contrast(to_rgb(0.62, chroma, hue)) >= minimum {
        return to_rgb(0.62, chroma, hue);
    }
    let (mut low, mut high) = if lighter { (0.62, 1.) } else { (0., 0.62) };
    for _ in 0..18 {
        let mid = (low + high) / 2.;
        if (contrast(to_rgb(mid, chroma, hue)) >= minimum) == lighter {
            high = mid;
        } else {
            low = mid;
        }
    }
    to_rgb(if lighter { high } else { low }, chroma, hue)
}

fn linear_rgb(lightness: f64, chroma: f64, hue: f64) -> [f64; 3] {
    let a = chroma * hue.to_radians().cos();
    let b = chroma * hue.to_radians().sin();
    let l = (lightness + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (lightness - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (lightness - 0.0894841775 * a - 1.291485548 * b).powi(3);
    [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
    ]
}

fn to_rgb(lightness: f64, chroma: f64, hue: f64) -> u32 {
    let in_gamut = |chroma| {
        linear_rgb(lightness, chroma, hue)
            .iter()
            .all(|channel| (-0.0001..=1.0001).contains(channel))
    };
    let mut mapped = chroma;
    if !in_gamut(chroma) {
        let (mut low, mut high) = (0., chroma);
        let steps = (chroma.max(0.000001).log2() - 0.000001f64.log2())
            .ceil()
            .max(1.) as u32;
        for _ in 0..steps {
            let mid = (low + high) / 2.;
            if in_gamut(mid) {
                low = mid;
            } else {
                high = mid;
            }
        }
        mapped = low;
    }
    linear_rgb(lightness, mapped, hue)
        .into_iter()
        .fold(0, |rgb, channel| {
            let channel = if channel <= 0.0031308 {
                channel * 12.92
            } else {
                1.055 * channel.powf(1. / 2.4) - 0.055
            };
            (rgb << 8) | (channel.clamp(0., 1.) * 255.).round() as u32
        })
}

fn contrast_ratio(a: u32, b: u32) -> f64 {
    let luminance = |rgb: u32| {
        [16, 8, 0]
            .into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(shift, weight)| {
                let channel = ((rgb >> shift) & 255) as f64 / 255.;
                weight
                    * if channel <= 0.03928 {
                        channel / 12.92
                    } else {
                        ((channel + 0.055) / 1.055).powf(2.4)
                    }
            })
            .sum::<f64>()
    };
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accents_remain_readable_on_every_surface_and_share_document_families() {
        for surfaces in [
            &[0xffffff, 0xfafafa, 0xf4f4f5][..],
            &[0x080808, 0x0f0f0f, 0x141414][..],
        ] {
            let mut colors = std::collections::HashSet::new();
            for name in ["a.rs", "a.pdf", "a.docx", "a.xlsx", "a.pptx", "a.epub"] {
                let color = desktop_file_chip_color(name, surfaces);
                assert!(
                    surfaces
                        .iter()
                        .all(|surface| contrast_ratio(color, *surface) >= 5.),
                    "{name}: {color:06x}"
                );
                assert!(colors.insert(color));
            }
            assert_eq!(
                desktop_file_chip_color("a.doc", surfaces),
                desktop_file_chip_color("a.docx", surfaces)
            );
            assert_eq!(
                desktop_file_chip_color("a.rs", surfaces),
                desktop_file_chip_color("a.png", surfaces)
            );
        }
    }
}
