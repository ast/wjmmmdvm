use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// 4-byte ASCII magic for the periodic repeater-config heartbeat.
pub const MAGIC: &[u8; 4] = b"DMRC";

/// HomeBrew repeater config packet — 119 bytes, sent every ~10 s by
/// MMDVMHost. Contains callsign, frequencies, power, color code,
/// location, firmware version, etc.
///
/// Detailed field layout is not yet decoded — the body is kept as raw
/// bytes for hex logging and future structured parsing.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C, packed)]
pub struct DmrConfig {
    pub magic: [u8; 4],
    pub data: [u8; 115],
}

const _: () = assert!(std::mem::size_of::<DmrConfig>() == 119);
