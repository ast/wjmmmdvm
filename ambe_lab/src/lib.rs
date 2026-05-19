//! AMBE+2 research harness. Drives the file-based `md380-emu` binary
//! from the [md380tools](https://github.com/travisgoodspeed/md380tools)
//! project as a subprocess to encode 8 kHz PCM ↔ raw 8-byte AMBE frames.
//!
//! See README.org for the no-distribution warning — this crate is
//! research-only and intentionally excluded from the workspace's
//! `default-members`.

pub mod ambe_frame;
pub mod codec;
pub mod corpus;
pub mod error;
pub mod md380_emu;
pub mod stimulus;
pub mod tone_gen;

/// 8 kHz mono signed-16 PCM is the codec's only supported rate.
pub const PCM_SAMPLE_RATE_HZ: u32 = 8_000;
/// One AMBE frame covers 20 ms = 160 samples of audio.
pub const PCM_SAMPLES_PER_FRAME: usize = 160;
/// md380-emu's `.amb` frame layout: 1 status byte + 6 bytes of packed
/// voice bits (MSB-first) + 1 byte holding the 49th bit in its LSB.
/// **Note**: this is the raw 49-bit voice frame, with no FEC. The DMR
/// wire format (DMRD payload) adds a 23-bit Golay code on top of these
/// 49 bits for a total of 72 bits per frame.
pub const AMBE_BYTES_PER_FRAME: usize = 8;
