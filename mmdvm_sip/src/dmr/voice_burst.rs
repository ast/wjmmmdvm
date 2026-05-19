//! Extract the three AMBE+2 voice frames from a 33-byte DMRD voice
//! payload.
//!
//! ## Burst bit layout
//!
//! The 33-byte DMRD voice payload carries 264 bits of one DMR
//! timeslot burst (60 ms). Within those 264 bits the AMBE voice
//! data is interleaved with the burst SYNC / EMB field:
//!
//! ```text
//! bits   0..72   : AMBE frame 1 (72 bits, full)
//! bits  72..108  : AMBE frame 2 first half (36 bits)
//! bits 108..156  : SYNC / EMB (48 bits) — skipped
//! bits 156..192  : AMBE frame 2 second half (36 bits)
//! bits 192..264  : AMBE frame 3 (72 bits, full)
//! ```
//!
//! We extract each frame's 72 bits into a flat array (bit 0 = first
//! received bit) and then re-arrange them into mbelib's `[4][24]`
//! `AmbeFr` matrix for the FEC strip.
//!
//! ## AMBE frame → AmbeFr mapping
//!
//! Following mbelib's convention (see `mbe_processAmbe2450Data` and
//! `mbe_eccAmbe3600x2450C0`), the 72 received bits of one AMBE+2
//! frame split into four protected/unprotected blocks:
//!
//! | Wire bits | Block | Row | Indices used                 |
//! |-----------|-------|-----|------------------------------|
//! | 0..24     | C0    |  0  | [0]=extended parity, [1..=23] |
//! | 24..47    | C1    |  1  | [0]=unused, [1..=23]          |
//! | 47..58    | C2    |  2  | [0..=10]                      |
//! | 58..72    | C3    |  3  | [0..=13]                      |
//!
//! Within each row, mbelib treats the **highest used index** as the
//! MSB of the codeword. So the first wire bit of C0 lands at
//! `fr[0][23]`, the last at `fr[0][0]` (= the extended parity).

use crate::fec::ambe::AmbeFr;

/// Pull 3 AMBE frames out of a 33-byte DMRD voice payload, each in
/// the [`AmbeFr`] layout that [`crate::fec::strip_fec`] expects.
pub fn extract_voice_frames(payload: &[u8; 33]) -> [AmbeFr; 3] {
    // Build a 264-bit array (one u8 per bit, MSB-first per byte) so
    // we can index into burst bit positions cleanly.
    let mut bits = [0u8; 264];
    for (byte_idx, byte) in payload.iter().enumerate() {
        for bit in 0..8 {
            bits[byte_idx * 8 + bit] = (byte >> (7 - bit)) & 1;
        }
    }

    // Voice frame 1: bits 0..72 (contiguous)
    let frame1 = pack_into_ambe_fr(&bits[0..72]);

    // Voice frame 2: bits 72..108 + bits 156..192
    let mut frame2_bits = [0u8; 72];
    frame2_bits[..36].copy_from_slice(&bits[72..108]);
    frame2_bits[36..].copy_from_slice(&bits[156..192]);
    let frame2 = pack_into_ambe_fr(&frame2_bits);

    // Voice frame 3: bits 192..264 (contiguous)
    let frame3 = pack_into_ambe_fr(&bits[192..264]);

    [frame1, frame2, frame3]
}

/// Re-arrange one 72-bit AMBE frame from wire order (`bits[0]` =
/// first received bit) into mbelib's `[4][24]` matrix.
fn pack_into_ambe_fr(bits: &[u8]) -> AmbeFr {
    debug_assert_eq!(bits.len(), 72);
    let mut fr: AmbeFr = [[0u8; 24]; 4];

    // C0: wire bits 0..24 → fr[0]. First wire bit = fr[0][23].
    for i in 0..24 {
        fr[0][23 - i] = bits[i];
    }
    // C1: wire bits 24..47 (23 bits) → fr[1][23..=1]. fr[1][0] unused.
    for i in 0..23 {
        fr[1][23 - i] = bits[24 + i];
    }
    // C2: wire bits 47..58 (11 bits) → fr[2][10..=0].
    for i in 0..11 {
        fr[2][10 - i] = bits[47 + i];
    }
    // C3: wire bits 58..72 (14 bits) → fr[3][13..=0].
    for i in 0..14 {
        fr[3][13 - i] = bits[58 + i];
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
    fn first_bit_of_frame1_lands_at_fr_0_23() {
        let mut payload = [0u8; 33];
        payload[0] = 0x80; // MSB of byte 0
        let frames = extract_voice_frames(&payload);
        assert_eq!(frames[0][0][23], 1);
        // Nothing else should be set.
        for r in 0..4 {
            for c in 0..24 {
                if !(r == 0 && c == 23) {
                    assert_eq!(frames[0][r][c], 0, "frame[0][{r}][{c}]");
                }
            }
        }
    }

    #[test]
    fn voice_frame_2_is_stitched_across_sync() {
        // Place a marker at bit 72 (first half of frame 2) and bit
        // 156 (start of second half). Both should land in frame 2.
        let mut payload = [0u8; 33];
        // Bit 72 is byte 9 bit 7 (MSB).
        payload[9] = 0x80;
        // Bit 156 is byte 19 bit 4 (counting from MSB = position 3).
        payload[19] = 0x08;
        let frames = extract_voice_frames(&payload);
        // Bit 72 maps to wire-bit 0 of frame 2, which is fr[0][23].
        assert_eq!(frames[1][0][23], 1);
        // Bit 156 maps to wire-bit 36 of frame 2. wire bit 36 = first
        // bit of C1 (24..47 in wire) → wire idx 36-24 = 12 → fr[1][11].
        assert_eq!(frames[1][1][11], 1);
    }

    #[test]
    fn first_bit_of_frame3_lands_at_byte_24() {
        let mut payload = [0u8; 33];
        payload[24] = 0x80;
        let frames = extract_voice_frames(&payload);
        assert_eq!(frames[2][0][23], 1);
    }
}
