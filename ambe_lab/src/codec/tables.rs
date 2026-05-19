//! Lookup tables for AMBE+2 (3600x2450) — ported verbatim from
//! [mbelib](https://github.com/szechyjs/mbelib)'s
//! `ambe3600x2450_const.h`.
//!
//! mbelib is distributed under the ISC license (Copyright (c) 2010
//! mbelib Author). The license requires the copyright and permission
//! notice to be preserved in copies — reproduced below:
//!
//! ```text
//! Copyright (C) 2010 mbelib Author
//! GPG Key ID: 0xEA5EFE2C
//!
//! Permission to use, copy, modify, and/or distribute this software
//! for any purpose with or without fee is hereby granted, provided
//! that the above copyright notice and this permission notice appear
//! in all copies.
//!
//! THE SOFTWARE IS PROVIDED "AS IS" AND ISC DISCLAIMS ALL WARRANTIES
//! WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
//! MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL ISC BE LIABLE FOR
//! ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY
//! DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
//! WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS
//! ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
//! PERFORMANCE OF THIS SOFTWARE.
//! ```

/// Fundamental-frequency quantization table — 120 entries, each giving
/// the normalized angular fundamental frequency `f0` in cycles/sample.
/// Multiply by the sample rate (8000 Hz) to get Hz.
///
/// Indexed by `b0 ∈ [0, 119]`. Values 120–127 are reserved (erasure /
/// silence / tone markers) and have no entry here.
pub const W0_TABLE: [f32; 120] = [
    0.049971, 0.049215, 0.048471, 0.047739, 0.047010, 0.046299,
    0.045601, 0.044905, 0.044226, 0.043558, 0.042900, 0.042246,
    0.041609, 0.040979, 0.040356, 0.039747, 0.039148, 0.038559,
    0.037971, 0.037399, 0.036839, 0.036278, 0.035732, 0.035198,
    0.034672, 0.034145, 0.033636, 0.033133, 0.032635, 0.032148,
    0.031670, 0.031122, 0.030647, 0.030184, 0.029728, 0.029272,
    0.028831, 0.028395, 0.027966, 0.027538,
    0.027122, 0.026712, 0.026304, 0.025906, 0.025515, 0.025129,
    0.024746, 0.024372, 0.024002, 0.023636, 0.023279, 0.022926,
    0.022581, 0.022236, 0.021900, 0.021570, 0.021240, 0.020920,
    0.020605, 0.020294, 0.019983, 0.019684, 0.019386, 0.019094,
    0.018805, 0.018520, 0.018242, 0.017965, 0.017696, 0.017431,
    0.017170, 0.016911, 0.016657, 0.016409, 0.016163, 0.015923,
    0.015686, 0.015411, 0.015177, 0.014946,
    0.014721, 0.014496, 0.014277, 0.014061, 0.013847, 0.013636,
    0.013430, 0.013227, 0.013025, 0.012829, 0.012634, 0.012444,
    0.012253, 0.012068, 0.011887, 0.011703, 0.011528, 0.011353,
    0.011183, 0.011011, 0.010845, 0.010681, 0.010517, 0.010359,
    0.010202, 0.010050, 0.009895, 0.009747, 0.009600, 0.009453,
    0.009312, 0.009172, 0.009033, 0.008896, 0.008762, 0.008633,
    0.008501, 0.008375, 0.008249, 0.008125,
];

/// Number of harmonics `L` corresponding to each W0 index. Indexed
/// 1-based in mbelib but we store 0-based for Rust convenience.
pub const L_TABLE: [u8; 120] = [
    9, 9, 9, 9, 9, 9,
    10, 10, 10, 10, 10, 10,
    11, 11, 11, 11, 11, 11,
    12, 12, 12, 12, 12, 13,
    13, 13, 13, 13, 14, 14,
    14, 14, 15, 15, 15, 15,
    16, 16, 16, 16, 17, 17,
    17, 17, 18, 18, 18, 18,
    19, 19, 19, 20, 20, 20,
    21, 21, 21, 22, 22, 22,
    23, 23, 23, 24, 24, 24,
    25, 25, 26, 26, 26, 27,
    27, 28, 28, 29, 29, 30,
    30, 30, 31, 31, 32, 32,
    33, 33, 34, 34, 35, 36,
    36, 37, 37, 38, 38, 39,
    40, 40, 41, 42, 42, 43,
    43, 44, 45, 46, 46, 47,
    48, 48, 49, 50, 51, 52,
    52, 53, 54, 55, 56, 56,
];

/// Voiced / unvoiced decision patterns. 32 rows × 8 columns. Indexed
/// by `b1 ∈ [0, 31]`; each row gives the voicing decision (1 = voiced,
/// 0 = unvoiced) for the 8 frequency bands.
pub const VUV_TABLE: [[u8; 8]; 32] = [
    [1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 1, 0],
    [1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 0, 0],
    [1, 1, 0, 1, 1, 1, 1, 1],
    [1, 1, 1, 0, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 0, 1, 1],
    [1, 1, 1, 1, 0, 0, 0, 0],
    [1, 1, 1, 1, 1, 0, 0, 0],
    [1, 1, 1, 0, 0, 0, 0, 0],
    [1, 1, 1, 0, 0, 0, 0, 1],
    [1, 1, 0, 0, 0, 0, 0, 0],
    [1, 1, 1, 0, 0, 0, 0, 0],
    [1, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 1, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
];

/// Gain quantizer levels — 32 entries giving `Δγ` (delta log-magnitude
/// gain), the change in gain from the predicted value. The decoder
/// computes the absolute gain as `γ = Δγ + 0.5 · γ_prev`, so this
/// table by itself is not the final dB value.
pub const DG_TABLE: [f32; 32] = [
    -2.0, -0.67, 0.297941, 0.663728, 1.036829, 1.438136, 1.890077, 2.227970,
    2.478289, 2.667544, 2.793619, 2.893261, 3.020630, 3.138586, 3.237579, 3.322570,
    3.432367, 3.571863, 3.696650, 3.814917, 3.920932, 4.022503, 4.123569, 4.228291,
    4.370569, 4.543700, 4.707695, 4.848879, 5.056757, 5.326468, 5.777581, 6.874496,
];

/// PCM sample rate the codec operates at. Used to convert W0
/// (cycles/sample) to Hz.
pub const SAMPLE_RATE_HZ: f32 = 8000.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_sizes_match_mbelib() {
        assert_eq!(W0_TABLE.len(), 120);
        assert_eq!(L_TABLE.len(), 120);
        assert_eq!(VUV_TABLE.len(), 32);
        assert_eq!(VUV_TABLE[0].len(), 8);
        assert_eq!(DG_TABLE.len(), 32);
    }

    #[test]
    fn w0_table_is_strictly_decreasing() {
        // W0 = fundamental angular frequency. Lower index → higher
        // pitch. The table should be monotonically descending.
        for window in W0_TABLE.windows(2) {
            assert!(window[0] > window[1], "W0_TABLE not strictly decreasing");
        }
    }

    #[test]
    fn l_table_is_monotonic_non_decreasing() {
        // L grows as W0 shrinks (more harmonics fit in the band).
        for window in L_TABLE.windows(2) {
            assert!(window[1] >= window[0]);
        }
    }

    #[test]
    fn dg_table_is_strictly_increasing() {
        for window in DG_TABLE.windows(2) {
            assert!(window[0] < window[1], "DG_TABLE not strictly increasing");
        }
    }
}
