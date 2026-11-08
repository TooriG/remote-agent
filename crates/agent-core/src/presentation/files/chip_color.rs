//! Mention chip accents and the contrast solver from T3's themePalette.ts.
//! Surfaces are opaque sRGB.

/// T3 mobile's composerChipPresentation/themeAccentWithContrast. Callers supply
/// the opaque, same-text-shade message/composer/canvas surfaces of their theme.
#[cfg_attr(feature = "bindings", uniffi::export)]
pub fn mobile_file_chip_color(surfaces: Vec<u32>) -> u32 {
    let contrast = |color| {
        surfaces
            .iter()
            .map(|surface| contrast_ratio(color, *surface))
            .fold(f64::INFINITY, f64::min)
    };
    if contrast(0x0096af) >= 5. {
        return 0x0096af;
    }
    let [red, green, blue] = [0., 150f64 / 255., 175f64 / 255.].map(|channel| {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    });
    let l = (0.4122214708 * red + 0.5363325363 * green + 0.0514459929 * blue).cbrt();
    let m = (0.2119034982 * red + 0.6806995451 * green + 0.1073969566 * blue).cbrt();
    let s = (0.0883024619 * red + 0.2817188376 * green + 0.6299787005 * blue).cbrt();
    let mut lightness = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
    let a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
    let b = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
    let hue = b.atan2(a).to_degrees();
    let upper_chroma = a.hypot(b).max(0.2);
    let lighter = contrast(0xffffff) >= contrast(0);
    while (0. ..=1.).contains(&lightness) {
        let (mut low, mut high, mut chroma) = (0., upper_chroma, 0.);
        for _ in 0..20 {
            let mid = (low + high) / 2.;
            if linear_rgb(lightness, mid, hue)
                .iter()
                .all(|channel| (-0.0001..=1.0001).contains(channel))
            {
                low = mid;
                chroma = mid;
            } else {
                high = mid;
            }
        }
        let candidate = to_rgb(lightness, chroma, hue);
        if contrast(candidate) >= 5. {
            return candidate;
        }
        lightness += if lighter { 0.002 } else { -0.002 };
    }
    if lighter { 0xffffff } else { 0 }
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
    fn mobile_mention_accents_are_shared_across_the_theme_and_keep_contrast() {
        assert_eq!(mobile_file_chip_color(vec![]), 0x0096af);
        for surfaces in [vec![0xffffff, 0xf2f2f7], vec![0, 0x1c1c1e, 0x2c2c2e]] {
            let color = mobile_file_chip_color(surfaces.clone());
            for surface in surfaces {
                assert!(contrast_ratio(color, surface) >= 5.);
            }
        }
    }
}
