use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// 4-byte ASCII magic for Talker Alias blocks.
pub const MAGIC: &[u8; 4] = b"DMRA";

/// Talker Alias block — 15 bytes. Up to four blocks are emitted per
/// transmission, carrying the caller's alias / display name.
///
/// Detailed layout will be decoded later.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C, packed)]
pub struct DmrAlias {
    pub magic: [u8; 4],
    pub data: [u8; 11],
}

const _: () = assert!(std::mem::size_of::<DmrAlias>() == 15);
