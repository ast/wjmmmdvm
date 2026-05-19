//! Extract the three AMBE+2 voice frames from a 33-byte DMRD voice
//! payload.
//!
//! ## Burst bit layout
//!
//! The 33-byte DMRD voice payload carries 264 bits of one DMR
//! timeslot burst. Within those 264 bits the AMBE voice data is
//! transmitted alongside a SYNC / EMB field:
//!
//! ```text
//! bits   0..72   : AMBE frame 1 (36 dibits)
//! bits  72..108  : AMBE frame 2, dibits 0..17  (first half)
//! bits 108..156  : SYNC / EMB (48 bits) — skipped
//! bits 156..192  : AMBE frame 2, dibits 18..35 (second half)
//! bits 192..264  : AMBE frame 3 (36 dibits)
//! ```
//!
//! ## Per-frame dibit deinterleaver
//!
//! Each 72-bit AMBE+2 frame is **bit-interleaved** before
//! transmission — the wire bits are not in C0||C1||C2||C3 order.
//! Each pair of wire bits forms a dibit; the dibit's two bits are
//! distributed across the four `ambe_fr` rows so that any single
//! symbol error spans both a high-protection and a low-protection
//! position. The mapping is DSD's `rW`/`rX`/`rY`/`rZ` tables in
//! `include/dmr_const.h`; the tables produce mbelib's `[4][24]`
//! `AmbeFr` layout that [`crate::fec::strip_fec`] consumes.
//!
//! For dibit `i` (0..36), MSB first on the wire:
//!
//! ```text
//! bit 1 of dibit (MSB) → ambe_fr[RW[i]][RX[i]]
//! bit 0 of dibit (LSB) → ambe_fr[RY[i]][RZ[i]]
//! ```
//!
//! After applying the deinterleaver:
//! - `ambe_fr[0][0..=23]` holds C0 (Golay24, MSB at [23], extended
//!   parity at [0])
//! - `ambe_fr[1][0..=22]` holds C1 (Golay23, MSB at [22], [23]
//!   unused)
//! - `ambe_fr[2][0..=10]` holds C2 (11 uncoded bits, MSB at [10])
//! - `ambe_fr[3][0..=13]` holds C3 (14 uncoded bits, MSB at [13])

use crate::fec::ambe::AmbeFr;

// Per-AMBE-frame dibit deinterleaver. Verbatim from DSD's
// `include/dmr_const.h` (ISC license).
const RW: [usize; 36] = [
    0, 1, 0, 1, 0, 1,
    0, 1, 0, 1, 0, 1,
    0, 1, 0, 1, 0, 1,
    0, 1, 0, 1, 0, 2,
    0, 2, 0, 2, 0, 2,
    0, 2, 0, 2, 0, 2,
];
const RX: [usize; 36] = [
    23, 10, 22,  9, 21,  8,
    20,  7, 19,  6, 18,  5,
    17,  4, 16,  3, 15,  2,
    14,  1, 13,  0, 12, 10,
    11,  9, 10,  8,  9,  7,
     8,  6,  7,  5,  6,  4,
];
const RY: [usize; 36] = [
    0, 2, 0, 2, 0, 2,
    0, 2, 0, 3, 0, 3,
    1, 3, 1, 3, 1, 3,
    1, 3, 1, 3, 1, 3,
    1, 3, 1, 3, 1, 3,
    1, 3, 1, 3, 1, 3,
];
const RZ: [usize; 36] = [
     5,  3,  4,  2,  3,  1,
     2,  0,  1, 13,  0, 12,
    22, 11, 21, 10, 20,  9,
    19,  8, 18,  7, 17,  6,
    16,  5, 15,  4, 14,  3,
    13,  2, 12,  1, 11,  0,
];

/// Pull 3 AMBE frames out of a 33-byte DMRD voice payload, each in
/// the [`AmbeFr`] layout that [`crate::fec::strip_fec`] expects.
pub fn extract_voice_frames(payload: &[u8; 33]) -> [AmbeFr; 3] {
    // Convert the 264-bit payload to single bits, MSB-first per byte.
    let mut bits = [0u8; 264];
    for (byte_idx, byte) in payload.iter().enumerate() {
        for bit in 0..8 {
            bits[byte_idx * 8 + bit] = (byte >> (7 - bit)) & 1;
        }
    }

    // Frame 1: dibits 0..35 from burst bits 0..71.
    let mut f1 = [0u8; 36];
    for i in 0..36 {
        f1[i] = (bits[2 * i] << 1) | bits[2 * i + 1];
    }

    // Frame 2: dibits 0..17 from burst bits 72..107, then dibits 18..35
    // from burst bits 156..191 (skipping the 48-bit sync/EMB block).
    let mut f2 = [0u8; 36];
    for i in 0..18 {
        f2[i] = (bits[72 + 2 * i] << 1) | bits[72 + 2 * i + 1];
    }
    for i in 0..18 {
        f2[18 + i] = (bits[156 + 2 * i] << 1) | bits[156 + 2 * i + 1];
    }

    // Frame 3: dibits 0..35 from burst bits 192..263.
    let mut f3 = [0u8; 36];
    for i in 0..36 {
        f3[i] = (bits[192 + 2 * i] << 1) | bits[192 + 2 * i + 1];
    }

    [deinterleave(&f1), deinterleave(&f2), deinterleave(&f3)]
}

/// Apply the per-AMBE-frame dibit deinterleaver to one 36-dibit
/// frame, producing an mbelib-format `AmbeFr`.
fn deinterleave(dibits: &[u8; 36]) -> AmbeFr {
    let mut fr: AmbeFr = [[0u8; 24]; 4];
    for i in 0..36 {
        let d = dibits[i];
        fr[RW[i]][RX[i]] = (d >> 1) & 1;
        fr[RY[i]][RZ[i]] = d & 1;
    }
    fr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_zero_payload_yields_all_zero_frames() {
        let payload = [0u8; 33];
        let frames = extract_voice_frames(&payload);
        for fr in &frames {
            for row in fr {
                for bit in row {
                    assert_eq!(*bit, 0);
                }
            }
        }
    }

    #[test]
    fn first_wire_bit_of_each_frame_lands_at_c0_msb() {
        // Per RW[0]=0, RX[0]=23: dibit-0 MSB of any frame ends up at
        // ambe_fr[0][23] (= the MSB of the C0 codeword).
        let mut payload = [0u8; 33];
        payload[0] = 0x80; // frame 1, wire bit 0
        let f = extract_voice_frames(&payload);
        assert_eq!(f[0][0][23], 1);

        let mut payload = [0u8; 33];
        payload[9] = 0x80; // frame 2, wire bit 0 (burst bit 72)
        let f = extract_voice_frames(&payload);
        assert_eq!(f[1][0][23], 1);

        let mut payload = [0u8; 33];
        payload[24] = 0x80; // frame 3, wire bit 0 (burst bit 192)
        let f = extract_voice_frames(&payload);
        assert_eq!(f[2][0][23], 1);
    }

    #[test]
    fn frame2_second_half_uses_dibits_18_onwards() {
        // Burst bit 156 is the MSB of dibit 18 of frame 2. Per
        // RW[18]=0, RX[18]=14 the bit lands at ambe_fr[0][14].
        let mut payload = [0u8; 33];
        payload[19] = 0x08; // bit 156 (byte 19 has bit-from-MSB position 4)
        let frames = extract_voice_frames(&payload);
        assert_eq!(frames[1][0][14], 1);
        // Nothing else should be set anywhere.
        for (fi, fr) in frames.iter().enumerate() {
            for (r, row) in fr.iter().enumerate() {
                for (c, bit) in row.iter().enumerate() {
                    if !(fi == 1 && r == 0 && c == 14) {
                        assert_eq!(*bit, 0, "frame {fi} fr[{r}][{c}]");
                    }
                }
            }
        }
    }
}
