//! MBE synthesis — turns per-harmonic spectral magnitudes `M_l` into
//! PCM samples.
//!
//! For each frame we render `FRAME_SAMPLES` (160) samples as:
//!
//! ```text
//! s[t] = Σ A_l(t) · cos(l · ω₀ · t + φ_l)
//!        l=1..L
//! ```
//!
//! where `ω₀ = 2π·f₀ / 8000` (radians/sample). Per-harmonic phase
//! `φ_l` is carried across frames so successive frames stitch
//! without clicks.
//!
//! ## Voiced vs unvoiced
//!
//! For **voiced** harmonics, phase is continuous across frames and
//! amplitudes are linearly interpolated between the previous frame's
//! and current frame's `M_l` over the 160 samples — this avoids the
//! step-edge clicks that come from hard frame boundaries.
//!
//! For **unvoiced** harmonics, we *don't* synthesise them as
//! individual cosines — that would produce a comb of clean
//! sinusoids (tonal) instead of noise. Instead we sum the unvoiced
//! `M_l` magnitudes into a per-frame envelope and multiply by
//! per-sample white noise, yielding a flat-spectrum noise band whose
//! amplitude tracks the unvoiced energy across the frame. This
//! loses the per-band spectral shape of the unvoiced part (a more
//! complete decoder would bandpass-filter the noise around each
//! unvoiced harmonic) but it's a substantial improvement over
//! comb-cosine synthesis and good enough for the audible milestone.

use std::f32::consts::{PI, TAU};

use rand::{rngs::SmallRng, Rng, SeedableRng};

use crate::codec::spectral::Spectrum;
use crate::PCM_SAMPLES_PER_FRAME;

const MAX_HARMONICS: usize = 64;

/// Stateful MBE synthesiser. Holds the per-harmonic phase angle and
/// previous-frame amplitudes across frame boundaries.
pub struct VoicedSynth {
    /// Cumulative phase per harmonic.
    phases: Vec<f32>,
    /// Previous frame's M_l per harmonic (already unvoiced-scaled).
    prev_ml: Vec<f32>,
    /// Whether harmonic l was voiced in the previous frame.
    prev_voiced: Vec<bool>,
    /// Previous frame's harmonic count.
    prev_l: usize,
    /// RNG for unvoiced phase randomisation.
    rng: SmallRng,
}

impl Default for VoicedSynth {
    fn default() -> Self {
        Self::new()
    }
}

impl VoicedSynth {
    pub fn new() -> Self {
        Self {
            phases: vec![0.0; MAX_HARMONICS],
            prev_ml: vec![0.0; MAX_HARMONICS],
            prev_voiced: vec![false; MAX_HARMONICS],
            prev_l: 0,
            rng: SmallRng::seed_from_u64(0x4D424553594E5448),
        }
    }

    /// Render one frame of audio from the supplied spectrum.
    /// Returns floating-point samples in arbitrary range; caller
    /// scales to i16.
    pub fn render(&mut self, spectrum: &Spectrum) -> [f32; PCM_SAMPLES_PER_FRAME] {
        let omega0 = TAU * spectrum.f0_hz / 8000.0;
        let mut out = [0.0f32; PCM_SAMPLES_PER_FRAME];
        let l = spectrum.l.min(MAX_HARMONICS);

        // 1. Voiced harmonics: sum continuous-phase cosines with
        //    amplitude linearly interpolated across the frame.
        let inv_n = 1.0 / PCM_SAMPLES_PER_FRAME as f32;
        for ll in 0..l {
            if !spectrum.voiced[ll] {
                continue;
            }
            let l_plus_one = (ll + 1) as f32;
            let step = l_plus_one * omega0;
            let cur_mag = spectrum.ml[ll];
            let prev_mag = if ll < self.prev_l && self.prev_voiced[ll] {
                self.prev_ml[ll]
            } else {
                // No prior voiced presence for this harmonic — start
                // at the current magnitude (no fade-in).
                cur_mag
            };
            let mut phase = self.phases[ll];
            for t in 0..PCM_SAMPLES_PER_FRAME {
                let alpha = t as f32 * inv_n;
                let mag = prev_mag + (cur_mag - prev_mag) * alpha;
                out[t] += mag * phase.cos();
                phase += step;
            }
            self.phases[ll] = wrap_pi(phase);
        }

        // 2. Unvoiced harmonics: collapse into a single broadband
        //    noise contribution scaled by their total interpolated
        //    magnitude. This gives flat-spectrum noise instead of a
        //    comb of cosines.
        let cur_unvoiced_amp: f32 = (0..l)
            .filter(|&ll| !spectrum.voiced[ll])
            .map(|ll| spectrum.ml[ll])
            .sum();
        let prev_unvoiced_amp: f32 = (0..l.min(self.prev_l))
            .filter(|&ll| !self.prev_voiced[ll])
            .map(|ll| self.prev_ml[ll])
            .sum();
        // If the previous frame had no unvoiced content here, start
        // at the current level (no fade-in pop).
        let prev_unvoiced_amp = if prev_unvoiced_amp == 0.0 && cur_unvoiced_amp > 0.0 {
            cur_unvoiced_amp
        } else {
            prev_unvoiced_amp
        };

        if cur_unvoiced_amp > 0.0 || prev_unvoiced_amp > 0.0 {
            for t in 0..PCM_SAMPLES_PER_FRAME {
                let alpha = t as f32 * inv_n;
                let amp = prev_unvoiced_amp + (cur_unvoiced_amp - prev_unvoiced_amp) * alpha;
                let noise: f32 = self.rng.gen_range(-1.0..1.0);
                out[t] += amp * noise;
            }
        }

        // 3. Persist for next frame.
        for ll in 0..l {
            self.prev_ml[ll] = spectrum.ml[ll];
            self.prev_voiced[ll] = spectrum.voiced[ll];
        }
        for ll in l..self.prev_l {
            self.prev_ml[ll] = 0.0;
            self.prev_voiced[ll] = false;
        }
        self.prev_l = l;

        out
    }

    /// Reset all state. Useful when transitioning from a non-voice
    /// stretch (silence, tone) back into a voice frame so we don't
    /// pop into the old harmonic phases.
    pub fn reset(&mut self) {
        for p in self.phases.iter_mut() {
            *p = 0.0;
        }
        for m in self.prev_ml.iter_mut() {
            *m = 0.0;
        }
        for v in self.prev_voiced.iter_mut() {
            *v = false;
        }
        self.prev_l = 0;
    }
}

/// Wrap an angle into [-π, π].
fn wrap_pi(theta: f32) -> f32 {
    let twopi = TAU;
    let mut a = theta % twopi;
    if a > PI {
        a -= twopi;
    } else if a < -PI {
        a += twopi;
    }
    a
}

/// Scale a frame of f32 samples to i16-LE bytes. `peak_seen` is
/// updated upward as needed so consecutive frames share a consistent
/// gain. `headroom` controls the safety margin against clipping
/// (e.g., 0.95 = 5% headroom, 0.5 = 6 dB).
pub fn frame_to_i16_le(
    frame: &[f32; PCM_SAMPLES_PER_FRAME],
    peak_seen: &mut f32,
    headroom: f32,
) -> [u8; PCM_SAMPLES_PER_FRAME * 2] {
    for &s in frame {
        let abs = s.abs();
        if abs > *peak_seen {
            *peak_seen = abs;
        }
    }
    let scale = if *peak_seen > 0.0 {
        (i16::MAX as f32 * headroom) / *peak_seen
    } else {
        0.0
    };
    let mut bytes = [0u8; PCM_SAMPLES_PER_FRAME * 2];
    for (i, &s) in frame.iter().enumerate() {
        let v = (s * scale).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        let le = v.to_le_bytes();
        bytes[i * 2] = le[0];
        bytes[i * 2 + 1] = le[1];
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_harmonic_produces_sinusoid() {
        let spec = Spectrum {
            gamma: 0.0,
            l: 1,
            f0_hz: 1000.0,
            ml: vec![1.0],
            log2_ml: vec![0.0],
            voiced: vec![true],
        };
        let mut synth = VoicedSynth::new();
        let frame = synth.render(&spec);
        // First sample with zero phase is cos(0) = 1.0.
        assert!((frame[0] - 1.0).abs() < 1e-5, "first sample = {}", frame[0]);
        // At 1 kHz / 8 kHz = 0.125 cyc/sample, one full period = 8
        // samples. After 8 samples we should be back near 1.0.
        assert!((frame[8] - frame[0]).abs() < 1e-3, "frame[8] = {}", frame[8]);
    }

    #[test]
    fn phase_carries_across_voiced_frames() {
        let spec = Spectrum {
            gamma: 0.0,
            l: 1,
            f0_hz: 137.0,
            ml: vec![1.0],
            log2_ml: vec![0.0],
            voiced: vec![true],
        };
        let mut synth = VoicedSynth::new();
        let a = synth.render(&spec);
        let b = synth.render(&spec);
        let omega0 = TAU * 137.0 / 8000.0;
        let predicted = (omega0 * 160.0).cos();
        assert!(
            (b[0] - predicted).abs() < 1e-3,
            "phase discontinuity: b[0]={}, predicted={}, a[last]={}",
            b[0], predicted, a[PCM_SAMPLES_PER_FRAME - 1]
        );
    }

    #[test]
    fn unvoiced_output_is_decorrelated() {
        // All-unvoiced spectrum should produce per-sample white noise
        // rather than a clean cosine. Two consecutive frames must
        // differ at every sample, AND consecutive samples within a
        // frame must not be strongly correlated.
        let spec = Spectrum {
            gamma: 0.0,
            l: 1,
            f0_hz: 500.0,
            ml: vec![1.0],
            log2_ml: vec![0.0],
            voiced: vec![false],
        };
        let mut synth = VoicedSynth::new();
        let a = synth.render(&spec);
        let b = synth.render(&spec);
        // Frame-to-frame: identical spectra must produce different
        // sample sequences (noise is per-sample).
        let differing = (0..PCM_SAMPLES_PER_FRAME).filter(|&i| (a[i] - b[i]).abs() > 1e-6).count();
        assert!(differing > 140, "expected most samples to differ, got {differing}");
        // Within-frame: adjacent samples shouldn't be highly
        // correlated. Compute zero-crossing rate as a quick proxy —
        // for white noise it averages ~half the samples.
        let zc = (1..PCM_SAMPLES_PER_FRAME)
            .filter(|&i| (a[i - 1] >= 0.0) != (a[i] >= 0.0))
            .count();
        assert!(zc > 50, "expected ~half zero-crossings, got {zc}");
    }

    #[test]
    fn amplitude_interpolates_across_frames() {
        // Frame 1: M_l = 1.0; frame 2: M_l = 0.5. Sample 0 of frame
        // 2 should still see ~1.0 (alpha=0); sample at end approaches
        // 0.5 from the previous-frame baseline.
        let mut synth = VoicedSynth::new();
        let s1 = Spectrum {
            gamma: 0.0, l: 1, f0_hz: 1000.0,
            ml: vec![1.0], log2_ml: vec![0.0], voiced: vec![true],
        };
        let s2 = Spectrum {
            gamma: 0.0, l: 1, f0_hz: 1000.0,
            ml: vec![0.5], log2_ml: vec![0.0], voiced: vec![true],
        };
        synth.render(&s1);
        let f2 = synth.render(&s2);
        // The sample value depends on both amplitude and phase, but
        // peaks (where cos≈1) should walk from ~1.0 at frame start
        // to ~0.5 at frame end.
        // At sample 0: amplitude = 1.0 (alpha=0)
        // At sample 159: amplitude ≈ 0.5 (alpha≈1)
        let max_early: f32 = f2[..20].iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        let max_late: f32 = f2[140..].iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(max_early > max_late, "expected fade {max_early} → {max_late}");
    }
}
