//! Spectral magnitude reconstruction — port of mbelib's
//! `mbe_processAmbe2450Frame` math, stripped to the magnitude path.
//!
//! Given a [`AmbeFields`] for one voice frame plus inter-frame
//! [`SpectralState`], computes the per-harmonic spectral magnitudes
//! `M_l` that downstream synthesis needs to render PCM.
//!
//! ## The pipeline
//!
//! ```text
//! (b3, b4) ──► Gm[2..8] = PRBA24/PRBA58 lookup
//!              │
//!              ▼
//!         Ri[1..8] = IDCT-like sum over Gm
//!              │
//!              ▼
//!         Cik[1..4][1..2] = ½·(Ri sums/diffs)
//!         Cik[1..4][3..6] = HOCb5/6/7/8 lookups (b5..b8)  if Ji[band] > 2
//!              │
//!              ▼
//!         Tl[1..L] = IDCT each Cik row
//!              │
//!              ▼
//!         log2_Ml[l] = Tl[l] + 0.65·(prev-frame interpolation) - Sum43 + BigGamma
//!              │
//!              ▼
//!         M_l = 2^log2_Ml[l]              (voiced harmonic)
//!         M_l = unvc · 2^log2_Ml[l]       (unvoiced — scaled down by 0.2046/√w0)
//! ```
//!
//! `BigGamma = γ - ½·log2(L) - mean(Tl)` where γ is the absolute log
//! magnitude built up across frames as `γ = Δγ + 0.5·γ_prev`.
//!
//! See mbelib's `ambe3600x2450.c` lines ~260–550 for the original.

use std::f32::consts::PI;

use crate::codec::ambe_fields::{AmbeFields, FrameKind};
use crate::codec::tables::{
    DG_TABLE, HOCB5_TABLE, HOCB6_TABLE, HOCB7_TABLE, HOCB8_TABLE, LMPRBL_TABLE, L_TABLE,
    PRBA24_TABLE, PRBA58_TABLE, W0_TABLE,
};

/// Inter-frame state carried across calls to [`reconstruct`]. The
/// codec is predictive — log-magnitude prediction reuses the previous
/// frame's `log2_Ml` and the running gain `γ`.
#[derive(Debug, Clone)]
pub struct SpectralState {
    /// Running absolute log magnitude gain γ (mbelib's `gamma`).
    pub gamma: f32,
    /// Previous frame's harmonic count L. mbelib initialises to 14.
    pub l: usize,
    /// `log2_Ml[0..=L]`. `[0]` is a duplicate of `[1]` per mbelib.
    pub log2_ml: Vec<f32>,
}

impl Default for SpectralState {
    fn default() -> Self {
        Self {
            gamma: 0.0,
            l: 14,
            log2_ml: vec![0.0; 15],
        }
    }
}

/// Output of one frame's spectral magnitude decode.
#[derive(Debug, Clone)]
pub struct Spectrum {
    /// Running absolute log magnitude γ for this frame.
    pub gamma: f32,
    /// Number of harmonics L (= bands × harmonics, in [9, 56]).
    pub l: usize,
    /// Fundamental frequency in Hz (cached so synthesis doesn't need
    /// to redo the W0 table lookup).
    pub f0_hz: f32,
    /// `M_l` per harmonic, indexed `[0..L]` (0-indexed for caller
    /// convenience — `[0]` is the 1st harmonic). Unvoiced harmonics
    /// already have the `unvc` scale factor applied.
    pub ml: Vec<f32>,
    /// `log2 M_l` per harmonic, same indexing as `ml`. Useful for
    /// inspection / debugging.
    pub log2_ml: Vec<f32>,
    /// Per-harmonic voicing decision (true = voiced, false = unvoiced).
    /// Length = L. Comes from the V/UV pattern at band index
    /// `jl = floor(l · 16 · f0)`.
    pub voiced: Vec<bool>,
}

/// Reconstruct the spectral magnitudes for one voice frame. Updates
/// `state` in place. Returns `None` for silence / tone / erasure
/// frames (whose magnitudes are not produced by this path).
pub fn reconstruct(fields: &AmbeFields, state: &mut SpectralState) -> Option<Spectrum> {
    if !matches!(fields.kind(), FrameKind::Voice) {
        return None;
    }

    let w0_idx = fields.w0 as usize;
    let l = L_TABLE[w0_idx] as usize;
    let f0_cyc = W0_TABLE[w0_idx]; // cycles/sample
    let w0_rad = f0_cyc * 2.0 * PI; // radians/sample (mbelib's w0)
    let unvc = 0.2046 / w0_rad.sqrt();

    // 1. Gain integrator.
    let delta_gamma = DG_TABLE[fields.gain as usize];
    let gamma = delta_gamma + 0.5 * state.gamma;

    // 2. PRBA codebook lookups — Gm[2..=8]. Index 1 is implicit zero.
    let prba24 = PRBA24_TABLE[fields.prba24 as usize];
    let prba58 = PRBA58_TABLE[fields.prba58 as usize];
    let mut gm = [0.0f32; 9]; // 1-indexed; gm[0] unused
    gm[2] = prba24[0];
    gm[3] = prba24[1];
    gm[4] = prba24[2];
    gm[5] = prba58[0];
    gm[6] = prba58[1];
    gm[7] = prba58[2];
    gm[8] = prba58[3];

    // 3. Compute Ri[1..=8] via the inverse DCT-like sum of Gm.
    let mut ri = [0.0f32; 9];
    for i in 1..=8 {
        let mut sum = 0.0f32;
        for m in 1..=8 {
            let am = if m == 1 { 1.0 } else { 2.0 };
            let theta = PI * (m as f32 - 1.0) * (i as f32 - 0.5) / 8.0;
            sum += am * gm[m] * theta.cos();
        }
        ri[i] = sum;
    }

    // 4. Generate Cik first two columns from Ri.
    let rconst = 1.0 / (2.0 * std::f32::consts::SQRT_2);
    let mut cik = [[0.0f32; 7]; 5]; // [1..=4][1..=6]
    cik[1][1] = 0.5 * (ri[1] + ri[2]);
    cik[1][2] = rconst * (ri[1] - ri[2]);
    cik[2][1] = 0.5 * (ri[3] + ri[4]);
    cik[2][2] = rconst * (ri[3] - ri[4]);
    cik[3][1] = 0.5 * (ri[5] + ri[6]);
    cik[3][2] = rconst * (ri[5] - ri[6]);
    cik[4][1] = 0.5 * (ri[7] + ri[8]);
    cik[4][2] = rconst * (ri[7] - ri[8]);

    // 5. Look up Ji[1..=4] — per-band block lengths for the L row.
    let ji = LMPRBL_TABLE[l];

    // 6. Fill Cik[band][3..=Ji[band]] from HOC codebooks, clipped at k=6.
    let hocs: [&[f32; 4]; 4] = [
        &HOCB5_TABLE[fields.hoc5 as usize],
        &HOCB6_TABLE[fields.hoc6 as usize],
        &HOCB7_TABLE[fields.hoc7 as usize],
        &HOCB8_TABLE[fields.hoc8 as usize],
    ];
    for band in 1..=4 {
        let ji_band = ji[band - 1] as usize;
        for k in 3..=ji_band {
            if k > 6 {
                cik[band][k.min(6)] = 0.0;
            } else {
                cik[band][k] = hocs[band - 1][k - 3];
            }
        }
    }

    // 7. Inverse DCT each Cik row to give Tl per harmonic.
    let mut tl = vec![0.0f32; l + 1]; // 1-indexed; tl[0] unused
    let mut tl_idx = 1usize;
    for band in 1..=4 {
        let ji_band = ji[band - 1] as usize;
        if ji_band == 0 {
            continue;
        }
        for j in 1..=ji_band {
            let mut sum = 0.0f32;
            for k in 1..=ji_band {
                let ak = if k == 1 { 1.0 } else { 2.0 };
                let theta = PI * (k as f32 - 1.0) * (j as f32 - 0.5) / ji_band as f32;
                sum += ak * cik[band][k.min(6)] * theta.cos();
            }
            if tl_idx <= l {
                tl[tl_idx] = sum;
            }
            tl_idx += 1;
        }
    }

    // 8. Spectral prediction from previous frame.
    let prev_l = state.l;
    // Extend prev_log2_ml if cur L > prev L (mbelib's "fix" block).
    let mut prev_log2_ml = state.log2_ml.clone();
    if l > prev_l && prev_l > 0 {
        let last = prev_log2_ml[prev_l.min(prev_log2_ml.len() - 1)];
        while prev_log2_ml.len() <= l {
            prev_log2_ml.push(last);
        }
    }
    if !prev_log2_ml.is_empty() {
        prev_log2_ml[0] = prev_log2_ml.get(1).copied().unwrap_or(0.0);
    }

    let mut flokl = vec![0.0f32; l + 1];
    let mut intkl = vec![0usize; l + 1];
    let mut deltal = vec![0.0f32; l + 1];
    let prev_l_over_l = if l > 0 { prev_l as f32 / l as f32 } else { 0.0 };
    let mut sum43 = 0.0f32;
    for ll in 1..=l {
        flokl[ll] = prev_l_over_l * ll as f32;
        intkl[ll] = flokl[ll] as usize;
        deltal[ll] = flokl[ll] - intkl[ll] as f32;
        let idx_a = intkl[ll];
        let idx_b = intkl[ll] + 1;
        let a = prev_log2_ml.get(idx_a).copied().unwrap_or(0.0);
        let b = prev_log2_ml.get(idx_b).copied().unwrap_or(0.0);
        sum43 += (1.0 - deltal[ll]) * a + deltal[ll] * b;
    }
    sum43 = (0.65 / l as f32) * sum43;

    let sum42: f32 = (1..=l).map(|ll| tl[ll]).sum::<f32>() / l as f32;
    let big_gamma = gamma - 0.5 * (l as f32).log2() - sum42;

    // 9. Reconstruct log2_Ml and Ml. Voicing per harmonic comes from
    // the V/UV pattern at band index jl = floor(l * 16 * f0).
    let voicing = fields.voicing_pattern().unwrap_or([false; 8]);
    let mut log2_ml_new = vec![0.0f32; l];
    let mut ml = vec![0.0f32; l];
    let mut voiced_per_harmonic = vec![false; l];

    for ll in 1..=l {
        let idx_a = intkl[ll];
        let idx_b = intkl[ll] + 1;
        let a = prev_log2_ml.get(idx_a).copied().unwrap_or(0.0);
        let b = prev_log2_ml.get(idx_b).copied().unwrap_or(0.0);
        let c1 = 0.65 * (1.0 - deltal[ll]) * a;
        let c2 = 0.65 * deltal[ll] * b;
        let log2 = tl[ll] + c1 + c2 - sum43 + big_gamma;
        log2_ml_new[ll - 1] = log2;

        let jl = (ll as f32 * 16.0 * f0_cyc) as usize;
        let voiced = voicing.get(jl).copied().unwrap_or(false);
        voiced_per_harmonic[ll - 1] = voiced;

        // mbelib uses exp(0.693 * log2). 0.693 ≈ ln(2), so this is
        // equivalent to 2^log2. Keeping mbelib's exact constant.
        let amplitude = (0.693 * log2).exp();
        ml[ll - 1] = if voiced { amplitude } else { unvc * amplitude };
    }

    // 10. Persist state for the next frame.
    state.gamma = gamma;
    state.l = l;
    state.log2_ml = std::iter::once(log2_ml_new[0])
        .chain(log2_ml_new.iter().copied())
        .collect();

    Some(Spectrum {
        gamma,
        l,
        f0_hz: f0_cyc * 8000.0,
        ml,
        log2_ml: log2_ml_new,
        voiced: voiced_per_harmonic,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ambe_frame::AmbeFrame;

    #[test]
    fn non_voice_frame_returns_none() {
        // Silence frame from our captured corpus — b0 = 124.
        let raw = [0x00, 0xf8, 0x01, 0xa0, 0x9f, 0x8c, 0x40, 0x01];
        let f = AmbeFields::from_frame(&AmbeFrame::from_bytes(raw));
        let mut state = SpectralState::default();
        assert!(reconstruct(&f, &mut state).is_none());
    }

    #[test]
    fn voice_frame_produces_l_magnitudes() {
        // Synthetic frame with w0_idx = 0 → L = 9 harmonics.
        let raw = [0; 8];
        let f = AmbeFields::from_frame(&AmbeFrame::from_bytes(raw));
        let mut state = SpectralState::default();
        let spec = reconstruct(&f, &mut state).expect("voice frame");
        assert_eq!(spec.l, 9);
        assert_eq!(spec.ml.len(), 9);
        // All magnitudes should be finite.
        for m in &spec.ml {
            assert!(m.is_finite(), "non-finite magnitude {m}");
        }
        // State should have been updated.
        assert_eq!(state.l, 9);
    }

    #[test]
    fn gain_accumulates_across_frames() {
        let raw = [0; 8];
        let f = AmbeFields::from_frame(&AmbeFrame::from_bytes(raw));
        let mut state = SpectralState {
            gamma: 4.0,
            ..Default::default()
        };
        let s = reconstruct(&f, &mut state).unwrap();
        // delta_gamma = DG_TABLE[0] = -2.0, gamma = -2.0 + 0.5*4.0 = 0.0
        assert!((s.gamma - 0.0).abs() < 1e-6, "gamma was {}", s.gamma);
        assert!((state.gamma - 0.0).abs() < 1e-6);
    }
}
