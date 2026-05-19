use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AmbeError {
    #[error("I/O: {0}")]
    Io(#[from] io::Error),

    #[error("short read: wanted {wanted}, got {got}")]
    ShortRead { wanted: usize, got: usize },

    #[error("bad start byte: expected 0x61, got 0x{0:02X}")]
    BadStartByte(u8),

    #[error("unexpected packet type: 0x{0:02X}")]
    UnexpectedType(u8),

    #[error("malformed packet: {0}")]
    Malformed(&'static str),

    #[error("md380-emu rejected our control packet (replied: {0:?})")]
    InitRejected(Vec<u8>),
}

pub type Result<T> = std::result::Result<T, AmbeError>;
