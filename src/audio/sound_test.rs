/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Opt-in GTA short-effect comparisons; original PCM remains the default.

use crate::audio::openal::al_types::ALenum;
use crate::audio::openal::AL_FORMAT_MONO16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Original,
    SmoothEdges,
    LowerVolume,
}
impl Mode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "original" => Ok(Self::Original),
            "smooth-edges" => Ok(Self::SmoothEdges),
            "lower-volume" => Ok(Self::LowerVolume),
            _ => Err(format!("Unknown audio test {value:?}")),
        }
    }
}

/// Only touch short mono PCM effects, leaving stereo music, other sample
/// formats and long ambience/stream buffers unchanged. This is a comparison,
/// not a validated fix: smooth edges may soften the effect's original attack.
pub fn transform(mode: Mode, format: ALenum, rate: i32, data: &[u8]) -> Option<Vec<u8>> {
    if mode == Mode::Original
        || format != AL_FORMAT_MONO16
        || rate <= 0
        || !data.len().is_multiple_of(2)
    {
        return None;
    }
    let frames = data.len() / 2;
    let rate = rate as usize;
    // 10ms to 2s; tiny percussion clicks and longer audio are left intact.
    if frames < rate / 100 || frames > rate * 2 || frames < 4 {
        return None;
    }
    let ramp = (rate / 500).max(2).min(frames / 2);
    let mut result = data.to_vec();
    for (index, sample) in result.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let value = i16::from_le_bytes(*sample) as i32;
        let value = match mode {
            Mode::Original => unreachable!(),
            Mode::LowerVolume => value / 2,
            Mode::SmoothEdges => {
                let distance = index.min(frames - 1 - index);
                if distance >= ramp {
                    continue;
                }
                (value as i64 * distance as i64 / (ramp - 1) as i64) as i32
            }
        };
        sample.copy_from_slice(&(value as i16).to_le_bytes());
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_and_non_effect_audio_are_unchanged() {
        let data = vec![0; 1024];
        assert!(transform(Mode::Original, AL_FORMAT_MONO16, 22050, &data).is_none());
        assert!(transform(Mode::SmoothEdges, 0x1103, 22050, &data).is_none());
        assert!(transform(Mode::SmoothEdges, AL_FORMAT_MONO16, 0, &data).is_none());
        assert!(transform(Mode::SmoothEdges, AL_FORMAT_MONO16, 22050, &[1, 2, 3]).is_none());
        assert!(transform(Mode::SmoothEdges, AL_FORMAT_MONO16, 22050, &[1, 2]).is_none());
        assert!(Mode::parse("unknown").is_err());
    }
    #[test]
    fn ramps_zero_both_edges_without_changing_middle_length_or_sign() {
        for value in [i16::MAX, i16::MIN] {
            let data = value.to_le_bytes().repeat(1000);
            let result = transform(Mode::SmoothEdges, AL_FORMAT_MONO16, 22050, &data).unwrap();
            let samples: Vec<_> = result
                .as_chunks::<2>()
                .0
                .iter()
                .map(|s| i16::from_le_bytes(*s))
                .collect();
            assert_eq!(result.len(), data.len());
            assert_eq!(samples[0], 0);
            assert_eq!(samples[999], 0);
            assert_eq!(samples[500], value);
            assert!(samples[..44].windows(2).all(|s| if value > 0 {
                s[0] <= s[1]
            } else {
                s[0] >= s[1]
            }));
        }
    }
    #[test]
    fn half_volume_does_not_wrap_negative_full_scale() {
        let data = i16::MIN.to_le_bytes().repeat(1000);
        let result = transform(Mode::LowerVolume, AL_FORMAT_MONO16, 22050, &data).unwrap();
        assert!(result
            .as_chunks::<2>()
            .0
            .iter()
            .all(|s| i16::from_le_bytes(*s) == -16384));
    }
}
