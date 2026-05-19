//! Canonical test signals for the AMBE+2 vector corpus.
//!
//! Each [`Stimulus`] variant deterministically produces a slice of
//! 8 kHz s16 mono PCM samples. Variants are kept simple on purpose —
//! the goal is reproducibility, not realism. White-noise / random
//! variants use a fixed RNG seed embedded in the variant so the same
//! `Stimulus` value always produces the same samples.

use std::f32::consts::{PI, TAU};

use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};

use crate::PCM_SAMPLE_RATE_HZ;

/// A single test signal in the corpus. `name()` produces a kebab-case
/// label suitable for filenames and manifest entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stimulus {
    Silence {
        duration_seconds: f32,
    },
    SineTone {
        frequency_hz: f32,
        amplitude: f32,
        duration_seconds: f32,
    },
    /// Linear frequency sweep from `start_hz` to `end_hz`.
    SineSweep {
        start_hz: f32,
        end_hz: f32,
        amplitude: f32,
        duration_seconds: f32,
    },
    WhiteNoise {
        amplitude: f32,
        duration_seconds: f32,
        seed: u64,
    },
    /// Single full-scale sample at `position_seconds`, silence elsewhere.
    Impulse {
        amplitude: f32,
        position_seconds: f32,
        total_seconds: f32,
    },
    /// Two simultaneous sine tones at equal amplitude — useful for
    /// observing multi-pitch behavior.
    DualTone {
        f1_hz: f32,
        f2_hz: f32,
        amplitude: f32,
        duration_seconds: f32,
    },
}

impl Stimulus {
    pub fn name(&self) -> String {
        match self {
            Stimulus::Silence { duration_seconds } => {
                format!("silence-{}ms", ms(*duration_seconds))
            }
            Stimulus::SineTone {
                frequency_hz,
                amplitude,
                duration_seconds,
            } => format!(
                "sine-{}hz-amp{:02}-{}ms",
                *frequency_hz as u32,
                (*amplitude * 100.0) as u32,
                ms(*duration_seconds)
            ),
            Stimulus::SineSweep {
                start_hz,
                end_hz,
                duration_seconds,
                ..
            } => format!(
                "sweep-{}-{}hz-{}ms",
                *start_hz as u32,
                *end_hz as u32,
                ms(*duration_seconds)
            ),
            Stimulus::WhiteNoise {
                amplitude,
                duration_seconds,
                seed,
            } => format!(
                "noise-amp{:02}-seed{}-{}ms",
                (*amplitude * 100.0) as u32,
                seed,
                ms(*duration_seconds)
            ),
            Stimulus::Impulse {
                position_seconds,
                total_seconds,
                ..
            } => format!(
                "impulse-at{}ms-of{}ms",
                ms(*position_seconds),
                ms(*total_seconds)
            ),
            Stimulus::DualTone {
                f1_hz,
                f2_hz,
                duration_seconds,
                ..
            } => format!(
                "dual-{}+{}hz-{}ms",
                *f1_hz as u32,
                *f2_hz as u32,
                ms(*duration_seconds)
            ),
        }
    }

    pub fn duration_seconds(&self) -> f32 {
        match self {
            Stimulus::Silence { duration_seconds }
            | Stimulus::SineTone {
                duration_seconds, ..
            }
            | Stimulus::SineSweep {
                duration_seconds, ..
            }
            | Stimulus::WhiteNoise {
                duration_seconds, ..
            }
            | Stimulus::DualTone {
                duration_seconds, ..
            } => *duration_seconds,
            Stimulus::Impulse { total_seconds, .. } => *total_seconds,
        }
    }

    pub fn generate(&self) -> Vec<i16> {
        match self {
            Stimulus::Silence { duration_seconds } => vec![0i16; samples(*duration_seconds)],
            Stimulus::SineTone {
                frequency_hz,
                amplitude,
                duration_seconds,
            } => sine_samples(*frequency_hz, *amplitude, *duration_seconds),
            Stimulus::SineSweep {
                start_hz,
                end_hz,
                amplitude,
                duration_seconds,
            } => sweep_samples(*start_hz, *end_hz, *amplitude, *duration_seconds),
            Stimulus::WhiteNoise {
                amplitude,
                duration_seconds,
                seed,
            } => noise_samples(*amplitude, *duration_seconds, *seed),
            Stimulus::Impulse {
                amplitude,
                position_seconds,
                total_seconds,
            } => impulse_samples(*amplitude, *position_seconds, *total_seconds),
            Stimulus::DualTone {
                f1_hz,
                f2_hz,
                amplitude,
                duration_seconds,
            } => dual_tone_samples(*f1_hz, *f2_hz, *amplitude, *duration_seconds),
        }
    }
}

fn samples(seconds: f32) -> usize {
    (seconds * PCM_SAMPLE_RATE_HZ as f32) as usize
}

fn ms(seconds: f32) -> u32 {
    (seconds * 1000.0) as u32
}

fn sine_samples(frequency_hz: f32, amplitude: f32, duration_seconds: f32) -> Vec<i16> {
    let amp = amplitude.clamp(0.0, 1.0) * i16::MAX as f32;
    let phase_step = TAU * frequency_hz / PCM_SAMPLE_RATE_HZ as f32;
    let mut out = Vec::with_capacity(samples(duration_seconds));
    let mut phase = 0.0f32;
    for _ in 0..samples(duration_seconds) {
        out.push((phase.sin() * amp) as i16);
        phase = (phase + phase_step) % TAU;
    }
    out
}

fn sweep_samples(start_hz: f32, end_hz: f32, amplitude: f32, duration_seconds: f32) -> Vec<i16> {
    let amp = amplitude.clamp(0.0, 1.0) * i16::MAX as f32;
    let n = samples(duration_seconds);
    let mut out = Vec::with_capacity(n);
    let mut phase = 0.0f32;
    for i in 0..n {
        let t = i as f32 / PCM_SAMPLE_RATE_HZ as f32;
        let f = start_hz + (end_hz - start_hz) * (t / duration_seconds);
        let phase_step = TAU * f / PCM_SAMPLE_RATE_HZ as f32;
        out.push((phase.sin() * amp) as i16);
        phase = (phase + phase_step) % TAU;
    }
    out
}

fn noise_samples(amplitude: f32, duration_seconds: f32, seed: u64) -> Vec<i16> {
    let amp = amplitude.clamp(0.0, 1.0) * i16::MAX as f32;
    let mut rng = StdRng::seed_from_u64(seed);
    (0..samples(duration_seconds))
        .map(|_| {
            let r: f32 = rng.gen_range(-1.0..1.0);
            (r * amp) as i16
        })
        .collect()
}

fn impulse_samples(amplitude: f32, position_seconds: f32, total_seconds: f32) -> Vec<i16> {
    let n = samples(total_seconds);
    let mut out = vec![0i16; n];
    let pos = samples(position_seconds).min(n.saturating_sub(1));
    out[pos] = (amplitude.clamp(0.0, 1.0) * i16::MAX as f32) as i16;
    out
}

fn dual_tone_samples(f1_hz: f32, f2_hz: f32, amplitude: f32, duration_seconds: f32) -> Vec<i16> {
    // Split amplitude across both tones so the combined signal stays
    // within full-scale.
    let amp = amplitude.clamp(0.0, 1.0) * i16::MAX as f32 * 0.5;
    let step1 = TAU * f1_hz / PCM_SAMPLE_RATE_HZ as f32;
    let step2 = TAU * f2_hz / PCM_SAMPLE_RATE_HZ as f32;
    let mut phase1 = 0.0f32;
    let mut phase2 = PI * 0.25; // small offset so they don't start in phase
    let n = samples(duration_seconds);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let v = (phase1.sin() + phase2.sin()) * amp;
        out.push(v as i16);
        phase1 = (phase1 + step1) % TAU;
        phase2 = (phase2 + step2) % TAU;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_zero_filled() {
        let s = Stimulus::Silence {
            duration_seconds: 0.25,
        };
        let pcm = s.generate();
        assert_eq!(pcm.len(), 2000);
        assert!(pcm.iter().all(|&x| x == 0));
    }

    #[test]
    fn names_are_filename_safe() {
        for s in [
            Stimulus::Silence {
                duration_seconds: 1.0,
            },
            Stimulus::SineTone {
                frequency_hz: 1000.0,
                amplitude: 0.5,
                duration_seconds: 2.0,
            },
            Stimulus::WhiteNoise {
                amplitude: 0.3,
                duration_seconds: 1.0,
                seed: 42,
            },
        ] {
            let name = s.name();
            assert!(!name.contains(' '));
            assert!(!name.contains('/'));
            assert!(!name.contains('\\'));
        }
    }

    #[test]
    fn noise_is_deterministic_for_same_seed() {
        let s1 = Stimulus::WhiteNoise {
            amplitude: 0.5,
            duration_seconds: 0.1,
            seed: 42,
        };
        let s2 = Stimulus::WhiteNoise {
            amplitude: 0.5,
            duration_seconds: 0.1,
            seed: 42,
        };
        assert_eq!(s1.generate(), s2.generate());
    }
}
