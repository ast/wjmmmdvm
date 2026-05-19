//! One 8-byte AMBE frame in md380-emu's wire format.
//!
//! Byte layout (mirroring the unpacking in `md380tools/emulator/ambe.c`
//! `decode_amb_file`):
//!
//! ```text
//! byte[0]      status (0 = good)
//! bytes[1..7]  48 voice bits, MSB-first packing (bit 0 = MSB of byte 1)
//! byte[7]      bit 49 in the LSB; high 7 bits of this byte are padding
//! ```
//!
//! No FEC — this is the raw 49-bit voice frame. On the DMR wire format
//! a 23-bit Golay code is added per frame for a total of 72 bits.

use crate::AMBE_BYTES_PER_FRAME;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmbeFrame {
    raw: [u8; AMBE_BYTES_PER_FRAME],
}

impl AmbeFrame {
    pub fn from_bytes(bytes: [u8; AMBE_BYTES_PER_FRAME]) -> Self {
        Self { raw: bytes }
    }

    pub fn raw(&self) -> &[u8; AMBE_BYTES_PER_FRAME] {
        &self.raw
    }

    /// Byte 0. md380-emu uses 0 = good; non-zero indicates a bad frame.
    pub fn status(&self) -> u8 {
        self.raw[0]
    }

    /// Unpack the 49 voice bits into one bit per `u8` (each 0 or 1),
    /// MSB-first in the same order md380-emu's unpacker uses.
    pub fn voice_bits(&self) -> [u8; 49] {
        let mut out = [0u8; 49];
        let mut idx = 0;
        for i in 1..7 {
            for j in 0..8 {
                out[idx] = (self.raw[i] >> (7 - j)) & 1;
                idx += 1;
            }
        }
        out[48] = self.raw[7] & 1;
        out
    }

    /// Same as [`voice_bits`] but packed into the low 49 bits of a u64
    /// (bit 0 of the u64 is voice bit 0 / MSB of byte 1). Useful for
    /// XOR-diffing two frames in one operation.
    pub fn voice_bits_u64(&self) -> u64 {
        let mut out: u64 = 0;
        for (i, b) in self.voice_bits().iter().enumerate() {
            out |= (*b as u64) << i;
        }
        out
    }

    /// Hamming distance of voice bits between `self` and `other`.
    pub fn voice_bit_distance(&self, other: &AmbeFrame) -> u32 {
        (self.voice_bits_u64() ^ other.voice_bits_u64()).count_ones()
    }
}

/// Iterate over an .ambe byte stream as 8-byte frames. Any trailing
/// partial frame is silently dropped.
pub fn iter_frames(bytes: &[u8]) -> impl Iterator<Item = AmbeFrame> + '_ {
    bytes.chunks_exact(AMBE_BYTES_PER_FRAME).map(|chunk| {
        let mut buf = [0u8; AMBE_BYTES_PER_FRAME];
        buf.copy_from_slice(chunk);
        AmbeFrame::from_bytes(buf)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_msb_first() {
        // byte[1] = 0b1000_0000 should make voice bit 0 = 1, rest of
        // this byte's bits = 0.
        let frame = AmbeFrame::from_bytes([0, 0x80, 0, 0, 0, 0, 0, 0]);
        let bits = frame.voice_bits();
        assert_eq!(bits[0], 1);
        for b in &bits[1..48] {
            assert_eq!(*b, 0);
        }
        assert_eq!(bits[48], 0);
    }

    #[test]
    fn bit_49_comes_from_byte_7_lsb() {
        let frame = AmbeFrame::from_bytes([0, 0, 0, 0, 0, 0, 0, 0x01]);
        assert_eq!(frame.voice_bits()[48], 1);
    }

    #[test]
    fn hamming_distance_matches() {
        let a = AmbeFrame::from_bytes([0, 0x00, 0, 0, 0, 0, 0, 0]);
        let b = AmbeFrame::from_bytes([0, 0xFF, 0, 0, 0, 0, 0, 0]);
        assert_eq!(a.voice_bit_distance(&b), 8);
        assert_eq!(a.voice_bit_distance(&a), 0);
    }

    #[test]
    fn iter_yields_correct_count() {
        let bytes = vec![0u8; 8 * 5];
        assert_eq!(iter_frames(&bytes).count(), 5);
    }
}
