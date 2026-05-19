//! AMBE+2 FEC strip — descramble + Golay-decode the 72-bit DMR voice
//! frame down to the 49 voice bits the codec actually needs.
//!
//! Direct port of mbelib's pipeline from `ambe3600x2450.c` (ISC):
//!
//! ```text
//! mbe_eccAmbe3600x2450C0    -> Golay-correct C0 (bits in ambe_fr[0])
//! mbe_demodulateAmbe3600x2450Data -> descramble ambe_fr[1] using a
//!                                    PRNG seeded from C0's data bits
//! mbe_eccAmbe3600x2450Data  -> Golay-correct C1 + extract 49 voice bits
//! ```
//!
//! Bit ordering inside `AmbeFr` follows mbelib's `char[4][24]`
//! convention: each row's index 23 is the **most** significant bit.

use crate::fec::golay::golay_23_12;

/// One AMBE+2 frame in mbelib's `[4][24]` layout. Each row is 24
/// bits, with bit 23 being the MSB.
///
/// Row contents (after deinterleaving from DMR over-the-air bits):
/// - `[0]`: C0 — Golay(23,12) data+parity (positions 23..1) plus a
///   24-bit extended-Golay parity bit at position 0 (currently
///   unverified — mbelib has a TODO there too).
/// - `[1]`: C1 — Golay(23,12) data+parity, scrambled with a PRNG
///   seeded from C0's data bits.
/// - `[2]`: 11 uncoded voice bits at positions 10..0 (rest unused).
/// - `[3]`: 14 uncoded voice bits at positions 13..0 (rest unused).
pub type AmbeFr = [[u8; 24]; 4];

/// Strip the FEC layer off a 72-bit DMR voice frame and return the
/// 49 voice bits ready to feed to the codec.
///
/// `bits[0..49]` is the output, where index 0 is the most
/// significant / first voice bit as the codec expects.
///
/// The return value is the **total number of FEC bit errors that
/// were corrected** (sum across the two Golay decodes). Higher
/// counts hint that the over-the-air signal was marginal.
pub fn strip_fec(mut fr: AmbeFr) -> ([u8; 49], u32) {
    let mut total_errs: u32 = 0;

    // 1. Golay-correct C0 (the most-sensitive bits).
    total_errs += ecc_c0(&mut fr);

    // 2. Descramble C1 using a PRNG seeded from C0's data bits.
    demodulate(&mut fr);

    // 3. Golay-correct C1 and extract the 49 voice bits.
    let (voice, errs) = ecc_data(&fr);
    total_errs += errs;

    (voice, total_errs)
}

/// Port of `mbe_eccAmbe3600x2450C0`. Runs Golay(23,12) over
/// `ambe_fr[0][1..=23]`. `ambe_fr[0][0]` is the extended Golay24
/// parity bit (left untouched here, matching mbelib's TODO).
fn ecc_c0(fr: &mut AmbeFr) -> u32 {
    let mut in_bits = [0u8; 23];
    for j in 0..23 {
        in_bits[j] = fr[0][j + 1];
    }
    let (out, errs) = golay_23_12(&in_bits);
    for j in 0..23 {
        fr[0][j + 1] = out[j];
    }
    errs
}

/// Port of `mbe_demodulateAmbe3600x2450Data`. C1 is scrambled at
/// encode time using a 23-element PRNG sequence whose seed is the
/// 12 data bits of C0. Recover the seed (now that C0 has been
/// corrected) and XOR the same sequence over C1 to undo it.
fn demodulate(fr: &mut AmbeFr) {
    // Read C0 data bits (positions 23..12 → 12 bits) into `foo`.
    let mut foo: u32 = 0;
    for i in (12..=23).rev() {
        foo = (foo << 1) | fr[0][i] as u32;
    }

    // pr[0..24] follows the recurrence in mbelib. The MSB of pr[i]
    // after the divide-by-32768 step is the scramble bit for C1[i].
    let mut pr = [0u32; 24];
    pr[0] = 16 * foo;
    for i in 1..24 {
        let nxt = 173u32.wrapping_mul(pr[i - 1]).wrapping_add(13849);
        pr[i] = nxt & 0xffff; // equivalent to nxt - 65536 * (nxt/65536)
    }
    // After the recurrence, mbelib divides each pr[i] (i ≥ 1) by
    // 32768 to leave just the top bit. We do the same.
    for i in 1..24 {
        pr[i] /= 32768;
    }

    // XOR the descramble sequence over C1 (ambe_fr[1][22..0]).
    let mut k = 1;
    for j in (0..23).rev() {
        fr[1][j] ^= pr[k] as u8;
        k += 1;
    }
}

/// Port of `mbe_eccAmbe3600x2450Data`. Extracts 49 voice bits in
/// the order the codec expects.
fn ecc_data(fr: &AmbeFr) -> ([u8; 49], u32) {
    let mut ambe = [0u8; 49];
    let mut idx = 0usize;

    // bits 0..11 = C0 data bits at positions 23..12 (already corrected).
    for j in (12..=23).rev() {
        ambe[idx] = fr[0][j];
        idx += 1;
    }

    // Golay-correct C1 and append its 12 data bits.
    let mut c1_in = [0u8; 23];
    for j in 0..23 {
        c1_in[j] = fr[1][j];
    }
    let (c1_out, errs) = golay_23_12(&c1_in);
    for j in (11..=22).rev() {
        ambe[idx] = c1_out[j];
        idx += 1;
    }

    // bits 23..33 = C2 bits at positions 10..0 (uncoded).
    for j in (0..=10).rev() {
        ambe[idx] = fr[2][j];
        idx += 1;
    }

    // bits 34..47 = C3 bits at positions 13..0 (uncoded).
    for j in (0..=13).rev() {
        ambe[idx] = fr[3][j];
        idx += 1;
    }

    debug_assert_eq!(idx, 49);
    (ambe, errs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descrambler_is_self_inverse() {
        // Demod twice should return to the original state, because
        // XOR with the same PRNG sequence twice is identity.
        let mut fr: AmbeFr = [[0; 24]; 4];
        // Set some C0 data bits so the PRNG seed is non-zero.
        fr[0][12] = 1;
        fr[0][15] = 1;
        fr[0][20] = 1;
        // Stuff some bits in C1 to see them flip.
        fr[1][5] = 1;
        fr[1][14] = 1;
        let before = fr;
        demodulate(&mut fr);
        demodulate(&mut fr);
        assert_eq!(fr, before);
    }
}
