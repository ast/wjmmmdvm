use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// 4-byte ASCII magic for embedded GPS frames forwarded from RF.
pub const MAGIC: &[u8; 4] = b"DMRG";

/// GPS position packet — 14 bytes. Carries 7 bytes of GPS payload
/// extracted from the radio's embedded LC (the rest is repeater context).
///
/// Detailed layout will be decoded later; for now we keep it opaque.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C, packed)]
pub struct DmrGps {
    pub magic: [u8; 4],
    pub data: [u8; 10],
}

const _: () = assert!(std::mem::size_of::<DmrGps>() == 14);
