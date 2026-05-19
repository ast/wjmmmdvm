//! Golay(23,12) decoder used by AMBE+2's FEC layer.
//!
//! Ported from mbelib's `ecc.c` (ISC license, Copyright 2010 mbelib
//! Author):
//!
//! ```text
//! Permission to use, copy, modify, and/or distribute this software
//! for any purpose with or without fee is hereby granted, provided
//! that the above copyright notice and this permission notice appear
//! in all copies.
//! ```
//!
//! The decoder takes 23 received bits (a 12-bit data word + 11
//! parity bits) and returns the most likely transmitted codeword
//! together with the number of bit errors that were corrected.
//! Golay(23,12) can correct up to 3 errors in any of the 23 bit
//! positions.

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/fec/golay_matrix.rs"));

/// Generator polynomial coefficients — 12 entries, one per data bit.
/// Each value is the 11-bit parity contribution of setting that
/// data bit. From mbelib's `golayGenerator`.
pub const GOLAY_GENERATOR: [u32; 12] = [
    0x63a, 0x31d, 0x7b4, 0x3da, 0x1ed, 0x6cc, 0x366, 0x1b3, 0x6e3, 0x54b, 0x49f, 0x475,
];

/// Decode 23 bits via Golay(23,12). `bits[0..23]` is the received
/// codeword (each element 0 or 1, **bit 22 = MSB / data-bit 0** as
/// mbelib expects). Returns the corrected codeword (same layout)
/// and the number of single-bit errors that were repaired.
///
/// This mirrors mbelib's `mbe_golay2312` exactly.
pub fn golay_23_12(bits: &[u8; 23]) -> ([u8; 23], u32) {
    // Pack the 23 bits into a long, MSB at bit 22 of the long.
    let mut block: u32 = 0;
    for i in (0..23).rev() {
        block = (block << 1) | bits[i] as u32;
    }

    check_golay_block(&mut block);

    // Unpack the corrected codeword back to a bit array. Data bits
    // are at positions 22..11 (MSB-first), parity bits at 10..0 —
    // mbelib copies the parity bits from the input unchanged, but
    // we replicate its actual behaviour which only fills 22..11
    // from `block` and leaves the parity (positions 10..0) equal to
    // the input.
    let mut out = [0u8; 23];
    let mut b = block;
    for i in (11..23).rev() {
        out[i] = ((b & 2048) >> 11) as u8;
        b <<= 1;
    }
    for i in (0..=10).rev() {
        out[i] = bits[i];
    }

    let mut errs = 0;
    for i in (11..23).rev() {
        if out[i] != bits[i] {
            errs += 1;
        }
    }
    (out, errs)
}

/// Core Golay decode: computes the syndrome of `block` (23 bits
/// in the low end of the u32), looks up the error pattern in
/// [`GOLAY_MATRIX`], and corrects the data portion.
fn check_golay_block(block: &mut u32) {
    let block_l = *block;

    // The 12 data bits sit at positions 22..11; ecc parity at 10..0.
    let mut mask: u32 = 0x40_0000;
    let mut ecc_expected: u32 = 0;
    for i in 0..12 {
        if (block_l & mask) != 0 {
            ecc_expected ^= GOLAY_GENERATOR[i];
        }
        mask >>= 1;
    }
    let ecc_bits = block_l & 0x7ff;
    let syndrome = (ecc_expected ^ ecc_bits) & 0x7ff;

    let mut data_bits = block_l >> 11;
    data_bits ^= GOLAY_MATRIX[syndrome as usize];
    *block = data_bits;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_codeword_round_trips() {
        let bits = [0u8; 23];
        let (out, errs) = golay_23_12(&bits);
        assert_eq!(out, bits);
        assert_eq!(errs, 0);
    }

    #[test]
    fn corrects_single_bit_error() {
        // Build a valid codeword for data bits = 0xfff (all 12 ones)
        // by xor-ing the corresponding generator rows into the parity.
        let mut data = 0xfffu32;
        let mut parity = 0u32;
        let mut mask = 0x800;
        for g in &GOLAY_GENERATOR {
            if data & mask != 0 {
                parity ^= g;
            }
            mask >>= 1;
        }
        // Layout as a bit array, MSB-first into positions 22..0
        let block: u32 = ((data) << 11) | (parity & 0x7ff);
        let mut bits = [0u8; 23];
        for i in (0..23).rev() {
            bits[i] = ((block >> i) & 1) as u8;
        }

        // Verify no-error decoding returns 0 errors and same data.
        let (_, errs) = golay_23_12(&bits);
        assert_eq!(errs, 0);

        // Flip one bit in the data portion (position 22) — decoder
        // should correct it.
        let mut corrupted = bits;
        corrupted[22] ^= 1;
        let (corrected, errs) = golay_23_12(&corrupted);
        assert!(errs > 0, "expected at least one error to be reported");
        // The data-bit positions should match the original codeword.
        for i in 11..23 {
            assert_eq!(corrected[i], bits[i], "position {i}");
        }
    }
}
