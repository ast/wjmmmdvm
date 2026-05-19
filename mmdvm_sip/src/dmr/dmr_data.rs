use zerocopy::big_endian::U32;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// 4-byte ASCII magic at the start of a DMR data burst packet.
pub const MAGIC: &[u8; 4] = b"DMRD";

/// HomeBrew DMRD packet — 55 bytes, sent by MMDVMHost for every RF voice
/// or data burst. Layout (offsets):
///
/// ```text
/// 0–3    magic "DMRD"
/// 4      sequence number
/// 5–7    source DMR ID (24-bit big-endian)
/// 8–10   destination DMR ID (24-bit big-endian)
/// 11–14  repeater ID (32-bit big-endian, our DMR ID)
/// 15     flags (slot / group-or-private / sync / frame type)
/// 16–19  stream ID (32-bit, random per call)
/// 20–52  33-byte DMR payload (AMBE voice frames or data)
/// 53     BER
/// 54     RSSI
/// ```
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C, packed)]
pub struct DmrData {
    pub magic: [u8; 4],
    pub seq: u8,
    pub src_id: [u8; 3],
    pub dst_id: [u8; 3],
    pub repeater_id: U32,
    pub flags: u8,
    pub stream_id: U32,
    pub payload: [u8; 33],
    pub ber: u8,
    pub rssi: u8,
}

const _: () = assert!(std::mem::size_of::<DmrData>() == 55);

impl DmrData {
    pub fn src_id_u32(&self) -> u32 {
        u24_be(self.src_id)
    }

    pub fn dst_id_u32(&self) -> u32 {
        u24_be(self.dst_id)
    }

    pub fn flags(&self) -> DmrFlags {
        DmrFlags(self.flags)
    }
}

/// Decoded view of the byte-15 flags field.
#[derive(Debug, Clone, Copy)]
pub struct DmrFlags(pub u8);

impl DmrFlags {
    /// `0` = slot 1, `1` = slot 2 (bit 7).
    pub fn slot(self) -> u8 {
        ((self.0 >> 7) & 0x01) + 1
    }

    /// `true` if this is a private (1:1) call, `false` for a group call (bit 6).
    pub fn is_private(self) -> bool {
        (self.0 >> 6) & 0x01 == 1
    }

    /// True if this burst carries a data sync (bit 5).
    pub fn data_sync(self) -> bool {
        (self.0 >> 5) & 0x01 == 1
    }

    /// True if this burst carries a voice sync (bit 4).
    pub fn voice_sync(self) -> bool {
        (self.0 >> 4) & 0x01 == 1
    }

    /// Voice frame number within a superframe, or data type identifier,
    /// depending on which sync bit is set (low 4 bits).
    pub fn frame_or_data_type(self) -> u8 {
        self.0 & 0x0F
    }
}

fn u24_be(b: [u8; 3]) -> u32 {
    (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::FromBytes;

    #[test]
    fn size_is_55() {
        assert_eq!(std::mem::size_of::<DmrData>(), 55);
    }

    #[test]
    fn parses_a_sample_burst() {
        // Hand-crafted: magic, seq=0x01, src=0x010203, dst=0x040506,
        // rpt=0x07080910, flags=0b1100_0001 (slot2, private, voice frame 1),
        // stream=0xDEADBEEF, payload = 33 bytes of 0xAB, ber=5, rssi=200.
        let mut bytes = vec![];
        bytes.extend_from_slice(b"DMRD");
        bytes.push(0x01);
        bytes.extend_from_slice(&[0x01, 0x02, 0x03]);
        bytes.extend_from_slice(&[0x04, 0x05, 0x06]);
        bytes.extend_from_slice(&0x07080910u32.to_be_bytes());
        bytes.push(0b1100_0001);
        bytes.extend_from_slice(&0xDEADBEEFu32.to_be_bytes());
        bytes.extend_from_slice(&[0xAB; 33]);
        bytes.push(5);
        bytes.push(200);
        assert_eq!(bytes.len(), 55);

        let pkt = DmrData::ref_from_bytes(&bytes).expect("parse");
        assert_eq!(&pkt.magic, b"DMRD");
        assert_eq!(pkt.seq, 0x01);
        assert_eq!(pkt.src_id_u32(), 0x010203);
        assert_eq!(pkt.dst_id_u32(), 0x040506);
        assert_eq!(pkt.repeater_id.get(), 0x07080910);
        assert_eq!(pkt.stream_id.get(), 0xDEADBEEF);
        assert_eq!(pkt.ber, 5);
        assert_eq!(pkt.rssi, 200);

        let f = pkt.flags();
        assert_eq!(f.slot(), 2);
        assert!(f.is_private());
        assert_eq!(f.frame_or_data_type(), 1);
    }
}
