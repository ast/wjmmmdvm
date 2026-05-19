use std::f32::consts::TAU;

use crate::PCM_SAMPLE_RATE_HZ;

/// Sine-wave generator producing s16 mono PCM samples at 8 kHz.
pub struct ToneGen {
    frequency_hz: f32,
    amplitude: f32,
    phase: f32,
    phase_step: f32,
}

impl ToneGen {
    /// Create a new generator. `amplitude` is 0.0–1.0 of full-scale i16.
    pub fn new(frequency_hz: f32, amplitude: f32) -> Self {
        let amplitude = amplitude.clamp(0.0, 1.0);
        let phase_step = TAU * frequency_hz / PCM_SAMPLE_RATE_HZ as f32;
        Self {
            frequency_hz,
            amplitude,
            phase: 0.0,
            phase_step,
        }
    }

    pub fn frequency_hz(&self) -> f32 {
        self.frequency_hz
    }

    pub fn next_sample(&mut self) -> i16 {
        let value = self.phase.sin() * self.amplitude * i16::MAX as f32;
        self.phase = (self.phase + self.phase_step) % TAU;
        value as i16
    }

    /// Fill a slice with the next `out.len()` samples.
    pub fn fill(&mut self, out: &mut [i16]) {
        for s in out.iter_mut() {
            *s = self.next_sample();
        }
    }

    /// Generate exactly `seconds * 8000` samples.
    pub fn generate_seconds(&mut self, seconds: f32) -> Vec<i16> {
        let n = (seconds * PCM_SAMPLE_RATE_HZ as f32) as usize;
        let mut buf = vec![0i16; n];
        self.fill(&mut buf);
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_expected_sample_count() {
        let mut tone = ToneGen::new(1000.0, 0.5);
        let samples = tone.generate_seconds(0.5);
        assert_eq!(samples.len(), 4000);
    }

    #[test]
    fn one_khz_tone_period_matches_eight_samples() {
        // At 1 kHz with 8 kHz sample rate, a period is 8 samples.
        // After 8 samples the phase should be back to ~0.
        let mut tone = ToneGen::new(1000.0, 1.0);
        let first = tone.next_sample();
        for _ in 0..7 {
            tone.next_sample();
        }
        let after_one_period = tone.next_sample();
        assert!(
            (first - after_one_period).abs() < 200,
            "first={first} after_one_period={after_one_period}"
        );
    }
}
