//! AMBE+2 FEC layer used by DMR.
//!
//! Each 49-bit voice frame on the wire is preceded by 23 bits of
//! Golay(23,12) + extended-Golay parity protection plus a
//! data-dependent scrambler, totalling **72 bits per 20 ms frame**.
//! This module strips that protection to recover the 49 voice bits
//! that the AMBE+2 codec expects.
//!
//! The algorithm and lookup tables are direct ports of mbelib's
//! `ecc.c`, `ambe3600x2450.c::mbe_demodulateAmbe3600x2450Data`, and
//! `mbe_eccAmbe3600x2450Data` — ISC license, copyright 2010 mbelib
//! Author. See the in-source notices in `golay.rs`.

pub mod ambe;
pub mod golay;

pub use ambe::strip_fec;
