//! The real Messages sounds for the onboarding chat, read at run time from
//! macOS's own files. They are Apple's, so Hex never bundles them: anywhere
//! they're missing, and off macOS, the bundled stand-in plays instead.

use std::num::NonZero;

use rodio::buffer::SamplesBuffer;

use super::Tapback;

/// A message arriving in the open conversation.
pub(super) const RECEIVED: &str = "/System/Library/PrivateFrameworks/ToneLibrary.framework/Versions/A/Resources/AlertTones/ReceivedMessage.caf";
/// Your message leaving: the swoosh.
pub(super) const SENT: &str = "/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/SentMessage.caf";

/// Each tapback's own sound.
pub(super) fn tapback(kind: Tapback) -> &'static str {
    match kind {
        Tapback::Heart => {
            "/System/Library/PrivateFrameworks/ToneLibrary.framework/Versions/A/Resources/AlertTones/Text-Message-Acknowledgement-Heart.caf"
        }
        Tapback::ThumbsUp => {
            "/System/Library/PrivateFrameworks/ToneLibrary.framework/Versions/A/Resources/AlertTones/Text-Message-Acknowledgement-ThumbsUp.caf"
        }
        Tapback::ThumbsDown => {
            "/System/Library/PrivateFrameworks/ToneLibrary.framework/Versions/A/Resources/AlertTones/Text-Message-Acknowledgement-ThumbsDown.caf"
        }
        Tapback::Haha => {
            "/System/Library/PrivateFrameworks/ToneLibrary.framework/Versions/A/Resources/AlertTones/Text-Message-Acknowledgement-HaHa.caf"
        }
        Tapback::Exclamation => {
            "/System/Library/PrivateFrameworks/ToneLibrary.framework/Versions/A/Resources/AlertTones/Text-Message-Acknowledgement-Exclamation.caf"
        }
        Tapback::Question => {
            "/System/Library/PrivateFrameworks/ToneLibrary.framework/Versions/A/Resources/AlertTones/Text-Message-Acknowledgement-QuestionMark.caf"
        }
    }
}

/// The macOS sound at `path`, brought to `bundled`'s peak so it sits where
/// the stand-in did in the mix; `bundled` itself when there is no such file.
pub(super) fn or_bundled(path: &str, bundled: &SamplesBuffer) -> SamplesBuffer {
    if !cfg!(target_os = "macos") {
        return bundled.clone();
    }
    let Some((channels, rate, mut samples)) = std::fs::read(path).ok().and_then(|b| caf(&b)) else {
        tracing::debug!(path, "no macOS sound; playing Hex's own");
        return bundled.clone();
    };
    let peak = |s: &[f32]| s.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
    let have = peak(&samples);
    if have > 0.0 {
        let gain = peak(&bundled.clone().collect::<Vec<_>>()) / have;
        samples.iter_mut().for_each(|x| *x *= gain);
    }
    // The near-silent lead (the swoosh has 64 ms) would land it late on
    // its bubble.
    let floor = peak(&samples) / 100.0;
    let lead = samples.iter().position(|x| x.abs() >= floor).unwrap_or(0);
    samples.drain(..lead - lead % usize::from(channels.get()));
    SamplesBuffer::new(channels, rate, samples)
}

/// 16-bit linear PCM from a Core Audio Format file, the only kind the
/// Messages sounds use.
fn caf(bytes: &[u8]) -> Option<(NonZero<u16>, NonZero<u32>, Vec<f32>)> {
    let mut rest = bytes.strip_prefix(b"caff")?.get(4..)?;
    let (mut format, mut data) = (None, None);
    while rest.len() >= 12 {
        let size = i64::from_be_bytes(rest[4..12].try_into().ok()?);
        let body = &rest[12..];
        // A data chunk of unknown size runs to the end.
        let len = usize::try_from(size).map_or(body.len(), |n| n.min(body.len()));
        match &rest[..4] {
            b"desc" => {
                let d = body.get(..32)?;
                let word = |i: usize| u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]);
                let float = word(12) & 1 != 0;
                if &d[8..12] != b"lpcm" || float || word(28) != 16 {
                    return None;
                }
                let rate = f64::from_be_bytes(d[..8].try_into().ok()?);
                format = Some((rate, word(24), word(12) & 2 != 0));
            }
            // After the edit count.
            b"data" => data = body.get(4..len),
            _ => {}
        }
        rest = &body[len..];
    }
    let (rate, channels, little) = format?;
    let samples = data?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| {
            let v = if little {
                i16::from_le_bytes(b)
            } else {
                i16::from_be_bytes(b)
            };
            f32::from(v) / f32::from(i16::MAX)
        })
        .collect();
    Some((
        NonZero::new(u16::try_from(channels).ok()?)?,
        NonZero::new(rate.round() as u32)?,
        samples,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(little: bool, samples: &[i16]) -> Vec<u8> {
        let mut f = b"caff\0\x01\0\0".to_vec();
        f.extend(b"desc");
        f.extend(32_i64.to_be_bytes());
        f.extend(44_100.0_f64.to_be_bytes());
        f.extend(b"lpcm");
        for word in [if little { 2 } else { 0 }, 4, 1, 2, 16_u32] {
            f.extend(word.to_be_bytes());
        }
        f.extend(b"data");
        f.extend((-1_i64).to_be_bytes());
        f.extend(0_u32.to_be_bytes());
        for s in samples {
            f.extend(if little {
                s.to_le_bytes()
            } else {
                s.to_be_bytes()
            });
        }
        f
    }

    #[test]
    fn reads_16_bit_pcm_of_either_byte_order() {
        for little in [true, false] {
            let (channels, rate, samples) = caf(&file(little, &[i16::MAX, 0, -16_384, 1])).unwrap();
            assert_eq!((channels.get(), rate.get()), (2, 44_100));
            assert_eq!(samples.len(), 4);
            assert!((samples[0] - 1.0).abs() < 1e-6 && samples[1] == 0.0);
            assert!((samples[2] + 0.5).abs() < 1e-3);
        }
        assert!(caf(b"RIFF....").is_none());
    }

    #[test]
    fn a_missing_file_plays_the_bundled_cue() {
        let bundled = SamplesBuffer::new(
            NonZero::new(1).unwrap(),
            NonZero::new(48_000).unwrap(),
            vec![0.0, 0.2, -0.1],
        );
        let got = or_bundled("/nonexistent/ReceivedMessage.caf", &bundled);
        assert_eq!(got.collect::<Vec<_>>(), vec![0.0, 0.2, -0.1]);
    }

    /// On a Mac, every Messages sound is there and lands at its stand-in's
    /// peak.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_macos_sounds_load_at_the_bundled_level() {
        let bundled = SamplesBuffer::new(
            NonZero::new(1).unwrap(),
            NonZero::new(48_000).unwrap(),
            vec![0.0, 0.2, -0.1],
        );
        let paths = Tapback::ALL.map(tapback);
        for path in [RECEIVED, SENT].iter().chain(&paths) {
            let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert!(caf(&bytes).is_some(), "{path}");
            let got: Vec<f32> = or_bundled(path, &bundled).collect();
            let peak = got.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
            assert!((peak - 0.2).abs() < 1e-4, "{path}: {peak}");
            assert!(
                got[..2].iter().any(|x| x.abs() >= 0.002),
                "{path} starts silent"
            );
        }
    }
}
