//! Recorded speech as the Host's transcription takes it: mono signed 16-bit
//! PCM at [`SAMPLE_RATE`], little-endian. Apple platforms record in it; a
//! microphone that gives its own rate and samples (Windows' shared mode) is
//! converted here.

/// The rate transcription takes, in samples per second.
pub const SAMPLE_RATE: u32 = 24_000;

/// Mono `samples` in -1.0..=1.0 at `rate` per second, resampled to
/// [`SAMPLE_RATE`] and quantized; louder samples are clipped.
pub fn transcription_pcm(samples: Vec<f32>, rate: u32) -> Result<Vec<u8>, String> {
    use rubato::{Resampler, audioadapter_buffers::owned::InterleavedOwned};
    if samples.is_empty() {
        return Err("音声を録音できませんでした。".into());
    }
    let samples = if rate == SAMPLE_RATE {
        samples
    } else {
        let frames = samples.len();
        let input =
            InterleavedOwned::new_from(samples, 1, frames).map_err(|error| error.to_string())?;
        let parameters =
            rubato::SincInterpolationParameters::new(256, rubato::WindowFunction::BlackmanHarris2);
        let mut resampler = rubato::Async::<f32>::new_sinc(
            f64::from(SAMPLE_RATE) / f64::from(rate),
            1.0,
            &parameters,
            1024,
            1,
            rubato::FixedAsync::Input,
        )
        .map_err(|error| error.to_string())?;
        resampler
            .process_all(&input, frames, None)
            .map_err(|error| error.to_string())?
            .take_data()
    };
    Ok(samples
        .into_iter()
        .flat_map(|sample| ((sample.clamp(-1.0, 1.0) * 32768.0).round() as i16).to_le_bytes())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest::proptest! {
        #[test]
        fn the_transcription_rate_keeps_each_sample(samples in proptest::collection::vec(proptest::num::i16::ANY, 1..2048)) {
            let input = samples.iter().map(|sample| f32::from(*sample) / 32768.).collect();
            let expected: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
            proptest::prop_assert_eq!(transcription_pcm(input, SAMPLE_RATE).unwrap(), expected);
        }
    }

    #[test]
    fn samples_are_clipped_and_rounded_and_silence_is_refused() {
        let samples = vec![-2., -1., -0.5, -0.5 / 32768., 0., 0.5 / 32768., 0.5, 1., 2.];
        let expected = [-32768_i16, -32768, -16384, -1, 0, 1, 16384, 32767, 32767];
        let expected: Vec<u8> = expected.into_iter().flat_map(i16::to_le_bytes).collect();
        assert_eq!(transcription_pcm(samples, SAMPLE_RATE).unwrap(), expected);
        assert!(transcription_pcm(Vec::new(), SAMPLE_RATE).is_err());
    }

    #[test]
    fn other_rates_keep_the_duration_and_the_tone() {
        for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
            let output = transcription_pcm(vec![0.; 1013], rate).unwrap();
            assert_eq!(output.len() / 2, (1013_usize * 24_000).div_ceil(rate as usize));
            assert!(output.iter().all(|byte| *byte == 0));
            // A 1 kHz tone at 0.3 keeps its energy (0.3² / 2) and its cycles.
            let tone = |frame: usize| {
                0.3 * (std::f32::consts::TAU * 1000. * frame as f32 / rate as f32).sin()
            };
            let output = transcription_pcm((0..4093).map(tone).collect(), rate).unwrap();
            assert_eq!(output.len() / 2, (4093_usize * 24_000).div_ceil(rate as usize));
            let frames = output.len() / 2;
            let body: Vec<f32> = output
                .as_chunks::<2>()
                .0
                .iter()
                .skip(128)
                .take(frames - 256)
                .map(|bytes| f32::from(i16::from_le_bytes(*bytes)) / 32768.)
                .collect();
            let energy = body.iter().map(|sample| sample * sample).sum::<f32>() / body.len() as f32;
            assert!((energy - 0.045).abs() < 0.002, "rate={rate} energy={energy}");
            let cycles = body
                .windows(2)
                .filter(|pair| pair[0] <= 0. && pair[1] > 0.)
                .count();
            assert!(cycles.abs_diff(body.len() / 24) <= 1, "rate={rate} cycles={cycles}");
        }
    }
}
