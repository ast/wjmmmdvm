//! AMBE-3000F over-the-wire packet protocol.
//!
//! Every packet on the serial / USB / TCP / Unix link looks like:
//!
//! ```text
//! +------+-----------+------+-------------------+
//! | 0x61 | length BE | type | payload           |
//! +------+-----------+------+-------------------+
//!   1 B     2 B        1 B    (length - 1) B
//! ```
//!
//! `length` counts the TYPE byte plus all subsequent payload bytes.
//!
//! Type tags:
//! - `0x00` Control — configuration / reset / product ID
//! - `0x01` Channel — compressed AMBE frames
//! - `0x02` Speech  — 8 kHz signed-16 PCM
//!
//! ## Control sub-fields (inside payload[0])
//!
//! - `0x30` PKT_PRODID — product ID query / response
//! - `0x33` PKT_RESET  — soft reset
//! - `0x09` PKT_RATEP  — rate via parameter set
//! - `0x0A` PKT_RATET  — rate via table index (DMR = 33 = 0x21)
//! - `0x39` PKT_RESET_ACK / generic Control ack returned by the chip
//! - `0x32` PKT_READY — ready notification
//!
//! ## Channel packet
//!
//! Channel payload:
//! - `0x01` CHAND tag
//! - `N` bit count
//! - `ceil(N/8)` bytes of packed AMBE bits, MSB-first
//!
//! For DMR rate (index 33) the chip sends 72 bits (= 9 bytes). Our
//! daemon uses md380-emu's native rate which produces **49 voice
//! bits with no FEC**, packed into 8 bytes the same way md380-emu's
//! .amb format packs them (byte 0 = status, bytes 1..7 = 48 bits
//! MSB-first, byte 7 LSB = 49th bit). Wire format: bit count = 49,
//! 7 bytes packed (we don't include the status byte over the wire).
//!
//! ## Speech packet
//!
//! Speech payload:
//! - `0x00` SPEECHD tag
//! - `N` sample count (typically 160 = 0xA0)
//! - `N` `i16` samples, **big-endian** per the DVSI protocol spec

use std::io;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zerocopy::big_endian::{I16, U16};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

/// 4-byte wire header: start byte, length (big-endian, counts TYPE +
/// payload), type byte. Parsed and built via zerocopy so the
/// endianness lives in the type, not in scattered byte-twiddling.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C, packed)]
pub struct PacketHeader {
    pub start: u8,
    pub length: U16,
    pub type_byte: u8,
}

/// 2-byte sub-header inside a Channel packet payload.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C, packed)]
pub struct ChannelHeader {
    pub tag: u8, // = CHAND_TAG
    pub bit_count: u8,
}

/// 2-byte sub-header inside a Speech packet payload.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout, Unaligned)]
#[repr(C, packed)]
pub struct SpeechHeader {
    pub tag: u8, // = SPEECHD_TAG
    pub sample_count: u8,
}

pub const START_BYTE: u8 = 0x61;
pub const TYPE_CONTROL: u8 = 0x00;
pub const TYPE_CHANNEL: u8 = 0x01;
pub const TYPE_SPEECH: u8 = 0x02;

/// Channel-field tag inside a Channel packet payload.
pub const CHAND_TAG: u8 = 0x01;
/// Speech-data tag inside a Speech packet payload.
pub const SPEECHD_TAG: u8 = 0x00;

pub const CTRL_PRODID: u8 = 0x30;
pub const CTRL_RESET: u8 = 0x33;
pub const CTRL_READY: u8 = 0x39;
pub const CTRL_RATEP: u8 = 0x09;
pub const CTRL_RATET: u8 = 0x0A;

/// Sample count we send in Speech packets — one 20 ms frame at 8 kHz.
pub const SPEECH_SAMPLES: usize = 160;
/// Voice-bit count for md380-emu's native AMBE rate (no FEC).
pub const AMBE_VOICE_BITS: u8 = 49;
/// Packed bytes for those 49 bits, MSB-first.
pub const AMBE_VOICE_BYTES: usize = 7;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("I/O: {0}")]
    Io(#[from] io::Error),
    #[error("malformed packet: {0}")]
    Malformed(&'static str),
    #[error("packet too large: length={0}")]
    TooLarge(u16),
}

/// One AMBE-3000F packet, parsed.
#[derive(Debug)]
pub enum Packet {
    /// Control packet — payload includes the field tag and any
    /// sub-fields. We keep it as raw bytes since there are many
    /// sub-types.
    Control(Vec<u8>),
    /// Channel packet carrying packed AMBE bits.
    Channel {
        bit_count: u8,
        data: Vec<u8>,
    },
    /// Speech packet carrying PCM samples (already endian-swapped).
    Speech(Vec<i16>),
}

impl Packet {
    /// Encode this packet to bytes ready to send on the wire.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Packet::Control(payload) => build(TYPE_CONTROL, payload),
            Packet::Channel { bit_count, data } => {
                let mut body = Vec::with_capacity(2 + data.len());
                body.extend_from_slice(
                    ChannelHeader {
                        tag: CHAND_TAG,
                        bit_count: *bit_count,
                    }
                    .as_bytes(),
                );
                body.extend_from_slice(data);
                build(TYPE_CHANNEL, &body)
            }
            Packet::Speech(samples) => {
                let mut body = Vec::with_capacity(2 + samples.len() * 2);
                body.extend_from_slice(
                    SpeechHeader {
                        tag: SPEECHD_TAG,
                        sample_count: samples.len() as u8,
                    }
                    .as_bytes(),
                );
                for s in samples {
                    body.extend_from_slice(I16::new(*s).as_bytes());
                }
                build(TYPE_SPEECH, &body)
            }
        }
    }
}

fn build(type_byte: u8, payload_after_type: &[u8]) -> Vec<u8> {
    // payload_length includes the TYPE byte plus everything after it.
    let payload_length = (1 + payload_after_type.len()) as u16;
    let header = PacketHeader {
        start: START_BYTE,
        length: U16::new(payload_length),
        type_byte,
    };
    let mut out = Vec::with_capacity(size_of::<PacketHeader>() + payload_after_type.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(payload_after_type);
    out
}

/// Read one packet from an async byte stream.
pub async fn read_packet<R>(reader: &mut R) -> Result<Packet, ProtocolError>
where
    R: AsyncRead + Unpin,
{
    // Resync on a stray byte: skip bytes until we see START_BYTE so a
    // disconnected client / desync doesn't kill the connection. Then
    // read the rest of the header into a buffer and parse it via a
    // zerocopy view of `PacketHeader`.
    let mut hdr_buf = [0u8; size_of::<PacketHeader>()];
    loop {
        reader.read_exact(&mut hdr_buf[..1]).await?;
        if hdr_buf[0] == START_BYTE {
            break;
        }
    }
    reader.read_exact(&mut hdr_buf[1..]).await?;
    let hdr = PacketHeader::ref_from_bytes(&hdr_buf[..])
        .expect("header buf has exactly the right size");

    let length = hdr.length.get();
    if length == 0 {
        return Err(ProtocolError::Malformed("zero length"));
    }
    if length > 4096 {
        return Err(ProtocolError::TooLarge(length));
    }
    let payload_len = length as usize - 1; // length includes the type byte
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).await?;

    match hdr.type_byte {
        TYPE_CONTROL => Ok(Packet::Control(payload)),
        TYPE_CHANNEL => parse_channel(&payload),
        TYPE_SPEECH => parse_speech(&payload),
        _ => Err(ProtocolError::Malformed("unknown packet type")),
    }
}

/// Write a pre-built packet to an async byte stream.
pub async fn write_packet<W>(writer: &mut W, packet: &Packet) -> Result<(), ProtocolError>
where
    W: AsyncWrite + Unpin,
{
    let bytes = packet.to_bytes();
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

fn parse_channel(payload: &[u8]) -> Result<Packet, ProtocolError> {
    let (hdr_bytes, data) = payload
        .split_at_checked(size_of::<ChannelHeader>())
        .ok_or(ProtocolError::Malformed("channel payload < 2 bytes"))?;
    let hdr = ChannelHeader::ref_from_bytes(hdr_bytes)
        .expect("split_at_checked returned exact header size");
    if hdr.tag != CHAND_TAG {
        return Err(ProtocolError::Malformed("channel: missing CHAND tag"));
    }
    let expected = (hdr.bit_count as usize).div_ceil(8);
    if data.len() < expected {
        return Err(ProtocolError::Malformed("channel: truncated AMBE data"));
    }
    Ok(Packet::Channel {
        bit_count: hdr.bit_count,
        data: data[..expected].to_vec(),
    })
}

fn parse_speech(payload: &[u8]) -> Result<Packet, ProtocolError> {
    let (hdr_bytes, data) = payload
        .split_at_checked(size_of::<SpeechHeader>())
        .ok_or(ProtocolError::Malformed("speech payload < 2 bytes"))?;
    let hdr = SpeechHeader::ref_from_bytes(hdr_bytes)
        .expect("split_at_checked returned exact header size");
    if hdr.tag != SPEECHD_TAG {
        return Err(ProtocolError::Malformed("speech: missing SPEECHD tag"));
    }
    let samples = hdr.sample_count as usize;
    let want = samples * size_of::<I16>();
    if data.len() < want {
        return Err(ProtocolError::Malformed("speech: truncated PCM data"));
    }
    // View the bytes as a slice of big-endian i16s; convert to native
    // i16 for the Vec<i16> we hand back to callers.
    let be_samples = <[I16]>::ref_from_bytes(&data[..want])
        .map_err(|_| ProtocolError::Malformed("speech: PCM alignment"))?;
    let out: Vec<i16> = be_samples.iter().map(|s| s.get()).collect();
    Ok(Packet::Speech(out))
}

/// Build a Channel packet for an md380-emu-format 8-byte AMBE frame.
/// We strip the leading status byte (byte 0, always 0 for good
/// frames) and pack only the 49 voice bits across 7 bytes.
pub fn build_channel_from_amb8(amb8: &[u8; 8]) -> Packet {
    let mut data = [0u8; AMBE_VOICE_BYTES];
    data[..6].copy_from_slice(&amb8[1..7]);
    // bit 49 lives in amb8[7] bit 0. The protocol packs it as the
    // MSB of the 7th byte for consistency with MSB-first ordering.
    data[6] = (amb8[7] & 0x01) << 7;
    Packet::Channel {
        bit_count: AMBE_VOICE_BITS,
        data: data.to_vec(),
    }
}

/// Convert a received Channel packet's 7-byte 49-bit payload back to
/// md380-emu's 8-byte .amb format (with the status byte set to 0).
pub fn amb8_from_channel(bit_count: u8, data: &[u8]) -> Result<[u8; 8], ProtocolError> {
    if bit_count != AMBE_VOICE_BITS {
        return Err(ProtocolError::Malformed(
            "channel bit_count must be 49 for md380-emu raw AMBE",
        ));
    }
    if data.len() < AMBE_VOICE_BYTES {
        return Err(ProtocolError::Malformed("channel data shorter than 7 bytes"));
    }
    let mut out = [0u8; 8];
    out[1..7].copy_from_slice(&data[..6]);
    // The 49th bit lives in data[6] bit 7 (MSB).
    out[7] = (data[6] >> 7) & 0x01;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Parse bytes through `read_packet`. Helper for round-trip tests.
    fn parse_wire(bytes: Vec<u8>) -> Packet {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        rt.block_on(async move {
            let mut cur = Cursor::new(bytes);
            read_packet(&mut cur).await.expect("parse")
        })
    }

    #[test]
    fn channel_roundtrip_through_wire() {
        let amb8 = [0x00, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x01];
        let pkt = build_channel_from_amb8(&amb8);
        let bytes = pkt.to_bytes();
        assert_eq!(bytes[0], START_BYTE);
        assert_eq!(bytes[3], TYPE_CHANNEL);

        // Round-trip: serialize → read_packet → compare.
        let parsed = parse_wire(bytes);
        if let Packet::Channel { bit_count, data } = parsed {
            assert_eq!(bit_count, 49);
            assert_eq!(data.len(), 7);
            let restored = amb8_from_channel(bit_count, &data).unwrap();
            assert_eq!(restored, amb8);
        } else {
            panic!("expected Channel packet");
        }
    }

    #[test]
    fn speech_roundtrip_through_wire() {
        let samples: Vec<i16> = (0..160).map(|i| (i as i16) * 100).collect();
        let pkt = Packet::Speech(samples.clone());
        let bytes = pkt.to_bytes();
        // length = type(1) + tag(1) + count(1) + samples*2(320) = 323.
        assert_eq!(u16::from_be_bytes([bytes[1], bytes[2]]), 323);

        let parsed = parse_wire(bytes);
        if let Packet::Speech(out) = parsed {
            assert_eq!(out, samples);
        } else {
            panic!("expected Speech packet");
        }
    }

    #[test]
    fn read_packet_skips_garbage_before_start_byte() {
        // Stray bytes before 0x61 should be consumed silently.
        let pkt = Packet::Control(vec![CTRL_READY]);
        let mut bytes = vec![0xde, 0xad, 0xbe, 0xef];
        bytes.extend_from_slice(&pkt.to_bytes());
        let parsed = parse_wire(bytes);
        match parsed {
            Packet::Control(p) => assert_eq!(p, vec![CTRL_READY]),
            _ => panic!("expected Control"),
        }
    }

    #[test]
    fn amb8_from_channel_rejects_wrong_bit_count() {
        let data = [0u8; 9];
        let err = amb8_from_channel(72, &data).expect_err("should reject 72-bit");
        assert!(matches!(err, ProtocolError::Malformed(_)));
    }

    #[test]
    fn amb8_from_channel_rejects_short_data() {
        let data = [0u8; 3];
        let err = amb8_from_channel(49, &data).expect_err("should reject short data");
        assert!(matches!(err, ProtocolError::Malformed(_)));
    }
}
