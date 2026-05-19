//! AMBE+2 codec — pure-Rust research implementation.
//!
//! Bit allocation, field semantics, and special-frame markers (silence /
//! tone / erasure) are taken from **mbelib's `ambe3600x2450.c`** —
//! BSD-licensed, the canonical open-source reference for this codec:
//!
//!   <https://github.com/szechyjs/mbelib/blob/master/ambe3600x2450.c>
//!
//! Today this module contains only the bit-unpacking layer: a 49-bit
//! voice frame in to the 9 parameter indices `b0..b8`. Future work adds
//! the codebook lookups and harmonic-synthesis math.

pub mod ambe_fields;
pub mod spectral;
pub mod synth;
pub mod tables;

pub use ambe_fields::{AmbeFields, FrameKind};
pub use spectral::{reconstruct, Spectrum, SpectralState};
pub use synth::{frame_to_i16_le, VoicedSynth};
