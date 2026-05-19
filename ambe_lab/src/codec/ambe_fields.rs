//! Unpack the 49 voice bits of an AMBE+2 frame into the nine named
//! parameter fields b0..b8 used by the MBE decoder.
//!
//! The bit positions and field names below are a direct port of the
//! unpack code in mbelib's `ambe3600x2450.c`. Citing exactly:
//!
//! ```text
//! b0 = ambe_d[0]<<6 | [1]<<5 | [2]<<4 | [3]<<3 | [37]<<2 | [38]<<1 | [39]
//! b1 = ambe_d[4]<<4 | [5]<<3 | [6]<<2 | [7]<<1 | [35]
//! b2 = ambe_d[8]<<4 | [9]<<3 | [10]<<2 | [11]<<1 | [36]
//! b3 = ambe_d[12]<<8 | [13]<<7 | [14]<<6 | [15]<<5 | [16]<<4
//!    | [17]<<3 | [18]<<2 | [19]<<1 | [40]
//! b4 = ambe_d[20]<<6 | [21]<<5 | [22]<<4 | [23]<<3 | [41]<<2 | [42]<<1 | [43]
//! b5 = ambe_d[24]<<4 | [25]<<3 | [26]<<2 | [27]<<1 | [44]
//! b6 = ambe_d[28]<<3 | [29]<<2 | [30]<<1 | [45]
//! b7 = ambe_d[31]<<3 | [32]<<2 | [33]<<1 | [46]
//! b8 = ambe_d[34]<<2 | [47]<<1 | [48]
//! ```
//!
//! `ambe_d` is the 49-bit voice frame, MSB-first as md380-emu emits it
//! (which matches the firmware's own buffer at `0x20011c8e`).

use crate::ambe_frame::AmbeFrame;
use crate::codec::tables::{DG_TABLE, L_TABLE, SAMPLE_RATE_HZ, VUV_TABLE, W0_TABLE};

/// Decoded parameter indices for one AMBE+2 frame. Each field is the
/// index into its respective codebook in mbelib (`AmbeW0table`,
/// `AmbeVuv`, `AmbeDg`, `AmbePRBA24`, `AmbePRBA58`, `AmbeHOCb5..8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmbeFields {
    /// b0 — fundamental frequency (W0) index, 7 bits. Values 120..=127
    /// are reserved for non-voice frame markers — see [`FrameKind`].
    pub w0: u8,
    /// b1 — voicing decision (V/UV) pattern index, 5 bits.
    pub vuv: u8,
    /// b2 — gain (Δγ) index, 5 bits.
    pub gain: u8,
    /// b3 — PRBA24 spectral magnitude VQ index, 9 bits.
    pub prba24: u16,
    /// b4 — PRBA58 spectral magnitude VQ index, 7 bits.
    pub prba58: u8,
    /// b5 — HOC5 (higher-order coefficient block 5) index, 5 bits.
    pub hoc5: u8,
    /// b6 — HOC6 index, 4 bits.
    pub hoc6: u8,
    /// b7 — HOC7 index, 4 bits.
    pub hoc7: u8,
    /// b8 — HOC8 index, 3 bits.
    pub hoc8: u8,
}

/// Classification of a frame by its b0 (W0) value, per mbelib.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// Normal voiced/unvoiced speech frame. b0 in 0..=119.
    Voice,
    /// Encoder-marked silence. b0 in 124..=125.
    Silence,
    /// Encoder-marked single-tone frame. b0 in 126..=127.
    Tone,
    /// Erasure (corrupted / unrecoverable). b0 in 120..=123.
    Erasure,
}

impl AmbeFields {
    pub fn from_frame(frame: &AmbeFrame) -> Self {
        let b = frame.voice_bits();
        Self {
            w0: pack(&b, &[0, 1, 2, 3, 37, 38, 39]) as u8,
            vuv: pack(&b, &[4, 5, 6, 7, 35]) as u8,
            gain: pack(&b, &[8, 9, 10, 11, 36]) as u8,
            prba24: pack(&b, &[12, 13, 14, 15, 16, 17, 18, 19, 40]) as u16,
            prba58: pack(&b, &[20, 21, 22, 23, 41, 42, 43]) as u8,
            hoc5: pack(&b, &[24, 25, 26, 27, 44]) as u8,
            hoc6: pack(&b, &[28, 29, 30, 45]) as u8,
            hoc7: pack(&b, &[31, 32, 33, 46]) as u8,
            hoc8: pack(&b, &[34, 47, 48]) as u8,
        }
    }

    pub fn kind(&self) -> FrameKind {
        match self.w0 {
            120..=123 => FrameKind::Erasure,
            124..=125 => FrameKind::Silence,
            126..=127 => FrameKind::Tone,
            _ => FrameKind::Voice,
        }
    }

    /// Fundamental pitch frequency in Hz for normal voice frames.
    /// `None` for silence / tone / erasure frames (b0 ≥ 120) since
    /// those use reserved W0 codepoints without a table entry.
    pub fn pitch_hz(&self) -> Option<f32> {
        (self.w0 < 120).then(|| W0_TABLE[self.w0 as usize] * SAMPLE_RATE_HZ)
    }

    /// Number of harmonics `L` covering 0–4 kHz at the decoded pitch.
    /// `None` for non-voice frames.
    pub fn harmonic_count(&self) -> Option<u8> {
        (self.w0 < 120).then(|| L_TABLE[self.w0 as usize])
    }

    /// Quantized gain change `Δγ` from mbelib's `AmbeDg` table. This
    /// is **not** absolute dB — the codec computes the absolute log
    /// magnitude as `γ = Δγ + 0.5 · γ_prev`, so the per-frame value
    /// has to be combined with prior-frame state to recover real
    /// dB-equivalent loudness. `None` for non-voice frames.
    pub fn gain_delta_log_mag(&self) -> Option<f32> {
        (self.w0 < 120).then(|| DG_TABLE[self.gain as usize])
    }

    /// V/UV (voiced/unvoiced) decision per band — 8 entries, each
    /// `true` if that frequency band is voiced (harmonic) or `false`
    /// if unvoiced (noise-like).
    ///
    /// The pattern is selected by `b1` ∈ [0, 31] from mbelib's
    /// `AmbeVuv` table. `None` for non-voice frames (silence forces
    /// all bands to unvoiced anyway, per mbelib).
    pub fn voicing_pattern(&self) -> Option<[bool; 8]> {
        if self.w0 >= 120 {
            return None;
        }
        let row = &VUV_TABLE[self.vuv as usize];
        Some([
            row[0] == 1, row[1] == 1, row[2] == 1, row[3] == 1,
            row[4] == 1, row[5] == 1, row[6] == 1, row[7] == 1,
        ])
    }
}

/// Combine selected bits of `bits` (1 byte each, 0 or 1) into a packed
/// integer. The first position in `positions` becomes the MSB.
fn pack(bits: &[u8; 49], positions: &[usize]) -> u32 {
    let mut value = 0u32;
    let n = positions.len();
    for (i, &pos) in positions.iter().enumerate() {
        let shift = n - 1 - i;
        value |= (bits[pos] as u32) << shift;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AMBE_BYTES_PER_FRAME;

    /// The canonical md380-emu silence frame we captured in the
    /// corpus run on this branch. mbelib marks W0 values 124–125 as
    /// silence; this frame should decode that way.
    const SILENCE_FRAME: [u8; AMBE_BYTES_PER_FRAME] =
        [0x00, 0xf8, 0x01, 0xa0, 0x9f, 0x8c, 0x40, 0x01];

    #[test]
    fn silence_frame_classifies_as_silence() {
        let f = AmbeFrame::from_bytes(SILENCE_FRAME);
        let fields = AmbeFields::from_frame(&f);
        assert!(matches!(fields.kind(), FrameKind::Silence));
        // Per mbelib, silence is b0 in {124, 125}.
        assert!(fields.w0 == 124 || fields.w0 == 125);
    }

    #[test]
    fn gain_extracts_high_4_bits_from_byte_2() {
        // byte[2] = 0xb2 → bits 8..15 = 10110010. The gain field b2
        // takes bits [8,9,10,11,36]. We set byte[5] bit 3 to control
        // bit 36. With byte[5] = 0x00, bit 36 = 0, so gain index is
        // (1011)<<1 | 0 = 22.
        let raw = [0x00, 0x00, 0xb2, 0x00, 0x00, 0x00, 0x00, 0x00];
        let f = AmbeFrame::from_bytes(raw);
        let fields = AmbeFields::from_frame(&f);
        assert_eq!(fields.gain, 22);
    }

    #[test]
    fn gain_picks_up_bit_36_as_lsb() {
        // Same byte[2] = 0xb2 as above, but now byte[5] = 0x08 sets
        // bit 36 = 1. Gain index becomes (1011)<<1 | 1 = 23.
        let raw = [0x00, 0x00, 0xb2, 0x00, 0x00, 0x08, 0x00, 0x00];
        let f = AmbeFrame::from_bytes(raw);
        let fields = AmbeFields::from_frame(&f);
        assert_eq!(fields.gain, 23);
    }

    #[test]
    fn pack_msb_first() {
        let mut b = [0u8; 49];
        // Set positions [0, 5] to 1 and pack from [0, 5]. With 2
        // positions, position 0 becomes the MSB (bit 1, value 2) and
        // position 5 becomes the LSB (bit 0, value 1). Result = 3.
        b[0] = 1;
        b[5] = 1;
        assert_eq!(pack(&b, &[0, 5]), 3);
    }

    #[test]
    fn accessors_return_none_for_non_voice() {
        let f = AmbeFrame::from_bytes(SILENCE_FRAME);
        let fields = AmbeFields::from_frame(&f);
        assert!(fields.pitch_hz().is_none());
        assert!(fields.harmonic_count().is_none());
        assert!(fields.gain_delta_log_mag().is_none());
        assert!(fields.voicing_pattern().is_none());
    }

    #[test]
    fn voice_frame_accessors_return_sane_values() {
        // Construct a synthetic frame with w0 = 0 (highest pitch in
        // the voice range; ~399.8 Hz at the sample rate of 8 kHz).
        // All other bits zeroed.
        let raw = [0; AMBE_BYTES_PER_FRAME];
        let f = AmbeFrame::from_bytes(raw);
        let fields = AmbeFields::from_frame(&f);
        assert_eq!(fields.w0, 0);
        let pitch = fields.pitch_hz().expect("voice frame should have pitch");
        // 0.049971 cyc/sample × 8000 Hz ≈ 399.77 Hz
        assert!((pitch - 399.77).abs() < 1.0, "pitch_hz was {pitch}");
        assert_eq!(fields.harmonic_count(), Some(9));
        assert_eq!(fields.gain_delta_log_mag(), Some(-2.0));
        assert_eq!(fields.voicing_pattern(), Some([true; 8]));
    }

    #[test]
    fn w0_low_bits_come_from_byte_5_lsb() {
        // b0 uses bits [0,1,2,3, 37,38,39]. Bits 37,38,39 live in
        // byte 5 at positions 2,1,0 from MSB. With byte[5] = 0x07,
        // those three bits are 1,1,1, giving b0 a lower nibble of
        // 0b111 = 7 even when bits 0-3 are zero.
        let raw = [0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00];
        let f = AmbeFrame::from_bytes(raw);
        let fields = AmbeFields::from_frame(&f);
        assert_eq!(fields.w0, 7);
    }
}
