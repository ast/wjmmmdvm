use zerocopy::FromBytes;

use super::{DmrAlias, DmrConfig, DmrData, DmrGps, dmr_alias, dmr_config, dmr_data, dmr_gps};

/// One parsed HBP packet received from MMDVMHost. `Unknown` is reserved
/// for datagrams whose 4-byte magic we don't recognise (or whose length
/// doesn't match the expected size for the recognised magic).
#[derive(Debug)]
pub enum Packet<'a> {
    Data(&'a DmrData),
    Config(&'a DmrConfig),
    Gps(&'a DmrGps),
    Alias(&'a DmrAlias),
    Unknown { magic: [u8; 4], len: usize },
}

impl<'a> Packet<'a> {
    /// Dispatch a raw datagram to its packet type by 4-byte magic. Returns
    /// `Packet::Unknown` if the magic is unrecognised or the length
    /// doesn't match the expected size for that type.
    pub fn parse(bytes: &'a [u8]) -> Self {
        if bytes.len() < 4 {
            return Packet::Unknown {
                magic: [0; 4],
                len: bytes.len(),
            };
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&bytes[..4]);
        match &magic {
            m if m == dmr_data::MAGIC => DmrData::ref_from_bytes(bytes)
                .map(Packet::Data)
                .unwrap_or(Packet::Unknown {
                    magic,
                    len: bytes.len(),
                }),
            m if m == dmr_config::MAGIC => DmrConfig::ref_from_bytes(bytes)
                .map(Packet::Config)
                .unwrap_or(Packet::Unknown {
                    magic,
                    len: bytes.len(),
                }),
            m if m == dmr_gps::MAGIC => DmrGps::ref_from_bytes(bytes)
                .map(Packet::Gps)
                .unwrap_or(Packet::Unknown {
                    magic,
                    len: bytes.len(),
                }),
            m if m == dmr_alias::MAGIC => DmrAlias::ref_from_bytes(bytes)
                .map(Packet::Alias)
                .unwrap_or(Packet::Unknown {
                    magic,
                    len: bytes.len(),
                }),
            _ => Packet::Unknown {
                magic,
                len: bytes.len(),
            },
        }
    }
}
