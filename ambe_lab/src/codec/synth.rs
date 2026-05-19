//! MBE synthesis — port of mbelib's `mbe_synthesizeSpeechf` from
//! `mbelib.c`, faithful to the algorithm but adapted to Rust idioms.
//!
//! The synthesizer carries per-harmonic state across frames:
//!
//! - `PSI[l]` — accumulated cumulative phase per harmonic (eq 139).
//! - `PHI[l]` — current-frame phase: `PSI[l]` plus a random offset
//!              for high harmonics (eq 140) so they desynchronise.
//! - `Ml[l]`, `Vl[l]` — previous frame's magnitudes and V/UV flags
//!   so we can do per-harmonic overlap-add between frames.
//!
//! For each harmonic `l ∈ [1, max(L_cur, L_prev)]`, there are four
//! cases depending on the V/UV status of the previous and current
//! frame:
//!
//! | prev V/UV | cur V/UV | what we synthesise (per output sample)         |
//! |-----------|----------|-------------------------------------------------|
//! | V         | V        | C1 (prev voiced fade-out) + C2 (cur voiced fade-in) |
//! | UV        | UV       | C3 (prev multisine fade-out) + C4 (cur multisine fade-in) |
//! | V         | UV       | C1 (prev voiced fade-out) + C3 (cur multisine fade-in)    |
//! | UV        | V        | C2 (cur voiced fade-in) + C3-style (prev multisine fade-out) |
//!
//! All four blends are weighted by mbelib's `Ws` window so overlap-add
//! sums to a flat envelope.
//!
//! Unvoiced harmonics use **multisine synthesis**: `uvquality` (=3)
//! slightly detuned cosines per harmonic centered at `l·ω₀`, plus
//! injected white noise for harmonics above 2700 Hz.

use std::f32::consts::{E, PI, TAU};

use rand::{rngs::SmallRng, Rng, SeedableRng};

use crate::codec::spectral::Spectrum;
use crate::codec::tables::WS_WINDOW;
use crate::PCM_SAMPLES_PER_FRAME;

const MAX_HARMONICS: usize = 56;
/// mbelib's default uvquality — number of detuned cosines per
/// unvoiced harmonic in the multisine synthesis.
const UV_QUALITY: usize = 3;
/// Frame size matching mbelib's `N`.
const N: usize = PCM_SAMPLES_PER_FRAME;
/// Above this angular frequency (radians/sample), unvoiced harmonics
/// get additional white-noise injection. 2700 Hz mapped via mbelib's
/// `uvthresholdf * π / 4000` formula.
const UV_THRESHOLD_RAD: f32 = (2700.0 * PI) / 4000.0;
/// Scaling factors from mbelib's algorithm.
const UV_SINE: f32 = 1.3591409 * E;
const UV_RAND: f32 = 2.0;

/// MBE speech synthesiser. One instance is reused across all the
/// frames of a stream — phase + magnitude history persists between
/// `render` calls.
pub struct MbeSynth {
    /// Previous frame's `ω₀` (radians/sample). 0 on the very first call.
    prev_w0: f32,
    /// Previous frame's harmonic count L.
    prev_l: usize,
    /// Previous frame's spectral magnitudes, 1-indexed (`[0]` unused).
    prev_ml: [f32; MAX_HARMONICS + 1],
    /// Previous frame's per-band voicing (1-indexed, true=voiced).
    prev_vl: [bool; MAX_HARMONICS + 1],
    /// Previous frame's PHI (synthesis phase). 1-indexed.
    prev_phi: [f32; MAX_HARMONICS + 1],
    /// Previous frame's PSI (cumulative phase). 1-indexed.
    prev_psi: [f32; MAX_HARMONICS + 1],
    /// RNG used for multisine random phases and the high-frequency
    /// noise term.
    rng: SmallRng,
    /// Pre-computed `log(uvquality)/uvquality`, the q-factor that
    /// scales the multisine output.
    q_factor: f32,
    /// `1/uvquality` — frequency step between detuned subsines.
    uv_step: f32,
    /// Centring offset so detuned subsines are symmetric around `l`.
    uv_offset: f32,
}

impl Default for MbeSynth {
    fn default() -> Self {
        Self::new()
    }
}

impl MbeSynth {
    pub fn new() -> Self {
        // Mbelib: special case for uvquality=1 uses 1/e; for >1 uses log/N.
        let q_factor = if UV_QUALITY == 1 {
            1.0 / E
        } else {
            (UV_QUALITY as f32).ln() / UV_QUALITY as f32
        };
        let uv_step = 1.0 / UV_QUALITY as f32;
        let uv_offset = uv_step * (UV_QUALITY as f32 - 1.0) / 2.0;

        Self {
            prev_w0: 0.0,
            prev_l: 0,
            prev_ml: [0.0; MAX_HARMONICS + 1],
            // mbelib initialises Vl to 1 (voiced) for all harmonics so
            // the first frame's "prev frame" looks like benign silence.
            prev_vl: [true; MAX_HARMONICS + 1],
            prev_phi: [0.0; MAX_HARMONICS + 1],
            prev_psi: [0.0; MAX_HARMONICS + 1],
            rng: SmallRng::seed_from_u64(0x4D424553594E5448),
            q_factor,
            uv_step,
            uv_offset,
        }
    }

    /// Render one 20 ms frame (160 samples). Output is in mbelib's
    /// native range — caller scales to i16.
    pub fn render(&mut self, spectrum: &Spectrum) -> [f32; N] {
        let cw0 = TAU * spectrum.f0_hz / 8000.0;
        let pw0 = self.prev_w0;
        let l_cur = spectrum.l.min(MAX_HARMONICS);
        let l_prev = self.prev_l;
        let max_l = l_cur.max(l_prev);

        // Count current-frame unvoiced harmonics.
        let num_uv = spectrum.voiced.iter().take(l_cur).filter(|&&v| !v).count();

        // Compute fresh PSI[l] and PHI[l] for every harmonic up to 56.
        // PHI = PSI for low harmonics, PSI + random offset for high.
        let mut cur_psi = [0.0f32; MAX_HARMONICS + 1];
        let mut cur_phi = [0.0f32; MAX_HARMONICS + 1];
        let low_threshold = l_cur / 4;
        for l in 1..=MAX_HARMONICS {
            cur_psi[l] = self.prev_psi[l] + (pw0 + cw0) * (l as f32 * N as f32) * 0.5;
            if l <= low_threshold {
                cur_phi[l] = cur_psi[l];
            } else if l_cur > 0 {
                let jitter = rand_phase(&mut self.rng) * num_uv as f32 / l_cur as f32;
                cur_phi[l] = cur_psi[l] + jitter;
            } else {
                cur_phi[l] = cur_psi[l];
            }
        }

        let mut out = [0.0f32; N];

        // Iterate over every harmonic that's active in either frame.
        for l in 1..=max_l {
            let cw0l = cw0 * l as f32;
            let pw0l = pw0 * l as f32;

            // mbelib pads missing harmonics with M=0, V=voiced. We do
            // the same to keep the synthesis math symmetric.
            let cur_ml = if l <= l_cur { spectrum.ml[l - 1] } else { 0.0 };
            let cur_vl = if l <= l_cur { spectrum.voiced[l - 1] } else { true };
            let prev_ml = if l <= l_prev { self.prev_ml[l] } else { 0.0 };
            let prev_vl = if l <= l_prev { self.prev_vl[l] } else { true };

            // Random phases for the multisine paths. Mbelib generates
            // fresh ones per case; we generate up to two sets so all
            // four cases can pick what they need.
            let mut rphase_cur = [0.0f32; UV_QUALITY];
            let mut rphase_prev = [0.0f32; UV_QUALITY];
            for i in 0..UV_QUALITY {
                rphase_cur[i] = rand_phase(&mut self.rng);
                rphase_prev[i] = rand_phase(&mut self.rng);
            }

            for n in 0..N {
                let ws_n = WS_WINDOW[n];
                let ws_np = WS_WINDOW[n + N];

                // Voiced contributions: prev fading out, cur fading in.
                let c1 = ws_np * prev_ml * (pw0l * n as f32 + self.prev_phi[l]).cos();
                let c2 = ws_n * cur_ml * (cw0l * (n as f32 - N as f32) + cur_phi[l]).cos();

                // Unvoiced multisine contributions.
                let c3_uv = unvoiced_sample(
                    &mut self.rng,
                    cw0,
                    cw0l,
                    n,
                    l,
                    &rphase_cur,
                    self.uv_step,
                    self.uv_offset,
                );
                let c3 = c3_uv * UV_SINE * ws_n * cur_ml * self.q_factor;

                let c4_uv = unvoiced_sample(
                    &mut self.rng,
                    pw0,
                    pw0l,
                    n,
                    l,
                    &rphase_prev,
                    self.uv_step,
                    self.uv_offset,
                );
                let c4 = c4_uv * UV_SINE * ws_np * prev_ml * self.q_factor;

                // Pick the right blend per the V/UV transition table.
                let s = match (prev_vl, cur_vl) {
                    (true, true) => c1 + c2,
                    (false, false) => c3 + c4,
                    (true, false) => c1 + c3,
                    (false, true) => c2 + c4,
                };
                out[n] += s;
            }
        }

        // Persist state for next frame.
        self.prev_w0 = cw0;
        self.prev_l = l_cur;
        for l in 0..=MAX_HARMONICS {
            self.prev_psi[l] = cur_psi[l];
            self.prev_phi[l] = cur_phi[l];
        }
        for l in 1..=MAX_HARMONICS {
            let idx = l - 1;
            self.prev_ml[l] = if idx < l_cur { spectrum.ml[idx] } else { 0.0 };
            self.prev_vl[l] = if idx < l_cur { spectrum.voiced[idx] } else { true };
        }

        out
    }

    /// Reset all state to the defaults used on construction. Useful
    /// when crossing a non-voice gap (silence / tone / erasure) so we
    /// don't drag stale phase into the next voice frame.
    pub fn reset(&mut self) {
        self.prev_w0 = 0.0;
        self.prev_l = 0;
        for v in self.prev_ml.iter_mut() {
            *v = 0.0;
        }
        for v in self.prev_vl.iter_mut() {
            *v = true;
        }
        for v in self.prev_phi.iter_mut() {
            *v = 0.0;
        }
        for v in self.prev_psi.iter_mut() {
            *v = 0.0;
        }
    }
}

/// One sample of the unvoiced multisine path for harmonic `l` at
/// time index `n`, using the given fundamental `w0` (radians/sample),
/// pre-multiplied `w0*l` for the noise threshold check, and the
/// pre-randomised phases. Mbelib's eq 131 inner formula.
fn unvoiced_sample(
    rng: &mut SmallRng,
    w0: f32,
    w0l: f32,
    n: usize,
    l: usize,
    rphase: &[f32; UV_QUALITY],
    uv_step: f32,
    uv_offset: f32,
) -> f32 {
    let n_f = n as f32;
    let l_f = l as f32;
    let mut acc = 0.0f32;
    for i in 0..UV_QUALITY {
        let detune = l_f + (i as f32 * uv_step) - uv_offset;
        acc += (w0 * n_f * detune + rphase[i]).cos();
        if w0l > UV_THRESHOLD_RAD {
            // White noise scaled by how far above the threshold.
            let r: f32 = rng.gen_range(0.0..1.0);
            acc += (w0l - UV_THRESHOLD_RAD) * UV_RAND * r;
        }
    }
    acc
}

fn rand_phase(rng: &mut SmallRng) -> f32 {
    let r: f32 = rng.gen_range(0.0..1.0);
    r * TAU - PI
}

/// Scale a frame of f32 samples to i16-LE bytes. `peak_seen` tracks
/// the running peak amplitude across the stream so consecutive frames
/// share a consistent gain. `headroom` (0.0..=1.0) is the safety
/// margin against clipping; 0.7 is a sensible default.
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

    fn unit_voiced_frame(f0_hz: f32) -> Spectrum {
        Spectrum {
            gamma: 0.0,
            l: 1,
            f0_hz,
            ml: vec![1.0],
            log2_ml: vec![0.0],
            voiced: vec![true],
        }
    }

    #[test]
    fn voiced_frame_produces_audible_output() {
        let spec = unit_voiced_frame(1000.0);
        let mut synth = MbeSynth::new();
        let frame = synth.render(&spec);
        let max_abs = frame.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!(max_abs > 0.1, "voiced output should be audible, got max={max_abs}");
    }

    #[test]
    fn psi_carries_across_frames() {
        // After two frames at constant pitch the PSI must have
        // advanced by exactly cw0 * N for harmonic 1.
        let spec = unit_voiced_frame(1000.0);
        let mut synth = MbeSynth::new();
        synth.render(&spec);
        let psi1 = synth.prev_psi[1];
        synth.render(&spec);
        let psi2 = synth.prev_psi[1];
        let cw0 = TAU * 1000.0 / 8000.0;
        let delta = psi2 - psi1;
        let expected = cw0 * N as f32;
        assert!((delta - expected).abs() < 1e-3, "delta={delta}, expected={expected}");
    }

    #[test]
    fn unvoiced_frame_decorrelated() {
        // All-unvoiced spectrum should produce per-sample-varying
        // output (multisine + noise is decorrelated).
        let spec = Spectrum {
            gamma: 0.0,
            l: 8,
            f0_hz: 500.0,
            ml: vec![1.0; 8],
            log2_ml: vec![0.0; 8],
            voiced: vec![false; 8],
        };
        let mut synth = MbeSynth::new();
        let a = synth.render(&spec);
        let b = synth.render(&spec);
        let differing = (0..N).filter(|&i| (a[i] - b[i]).abs() > 1e-3).count();
        assert!(differing > N * 3 / 4, "expected most samples to differ, got {differing}");
    }

    #[test]
    fn reset_zeroes_state() {
        let mut synth = MbeSynth::new();
        synth.render(&unit_voiced_frame(800.0));
        assert_ne!(synth.prev_w0, 0.0);
        synth.reset();
        assert_eq!(synth.prev_w0, 0.0);
        assert_eq!(synth.prev_l, 0);
    }

    #[test]
    fn frame_to_i16_scales_within_range() {
        let frame = [10.0f32; N];
        let mut peak = 0.0f32;
        let bytes = frame_to_i16_le(&frame, &mut peak, 0.5);
        let first = i16::from_le_bytes([bytes[0], bytes[1]]);
        // 10.0 mapped to peak, then scaled by 0.5 = ~16383.
        assert!((first as i32 - 16383).abs() < 2, "first = {first}");
    }
}

/// Backwards-compatible alias so existing callers (decode-rust)
/// don't need to rename.
pub type VoicedSynth = MbeSynth;
