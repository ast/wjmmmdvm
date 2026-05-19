//! TCP client speaking the AMBE-3000F packet protocol against the
//! `md380_emu_ambed` daemon (or, in principle, any AMBE-3000F-over-TCP
//! server).
//!
//! Speaks the same packet format the daemon serves:
//! `0x61 | length(BE u16) | type | payload`, with Channel packets
//! carrying 49-bit (7-byte) raw AMBE and Speech packets carrying 160
//! big-endian s16 PCM samples.
//!
//! We only need the **decode** direction (Channel → Speech) for the
//! listen-dmr use case, but the implementation is symmetric so encode
//! works too. Calls are synchronous-feeling on top of tokio: each
//! request blocks the connection until the matching reply arrives.

use std::io;

use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, ToSocketAddrs};
use tracing::debug;

pub const FRAME_PCM_SAMPLES: usize = 160;
pub const AMBE_VOICE_BITS: u8 = 49;
pub const AMBE_VOICE_BYTES: usize = 7;

const START_BYTE: u8 = 0x61;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const CHAND_TAG: u8 = 0x01;
const SPEECHD_TAG: u8 = 0x00;
const CTRL_PRODID: u8 = 0x30;
const CTRL_RESET: u8 = 0x33;
const CTRL_RATET: u8 = 0x0A;
const DMR_RATE_INDEX: u8 = 33;

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("connect failed: {0}")]
    Connect(#[source] io::Error),
    #[error("I/O: {0}")]
    Io(#[from] io::Error),
    #[error("malformed reply from daemon: {0}")]
    Malformed(&'static str),
    #[error("unexpected packet type 0x{0:02x} (wanted Channel or Speech)")]
    UnexpectedReply(u8),
    #[error("daemon returned channel bit_count={got}, expected {want}")]
    #[allow(dead_code)]
    WrongBitCount { got: u8, want: u8 },
    #[error("daemon returned speech sample_count={got}, expected {want}")]
    WrongSampleCount { got: usize, want: usize },
}

/// One TCP connection to an AMBE-3000F daemon.
pub struct AmbeCodecClient {
    stream: TcpStream,
}

impl AmbeCodecClient {
    /// Connect to the daemon. Doesn't send any handshake yet — call
    /// [`AmbeCodecClient::handshake`] if your daemon needs it.
    pub async fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self, CodecError> {
        let stream = TcpStream::connect(addr)
            .await
            .map_err(CodecError::Connect)?;
        // Disable Nagle so a Channel/Speech request flushes immediately.
        stream.set_nodelay(true)?;
        let peer = stream
            .peer_addr()
            .map(|p| p.to_string())
            .unwrap_or_else(|_| "?".into());
        debug!(target: "mmdvm_sip::codec", peer = %peer, "connected to AMBE-3000F daemon");
        Ok(Self { stream })
    }

    /// Polite warm-up: PRODID query, then a RATET to DMR rate. The
    /// `md380_emu_ambed` daemon ignores rate selection (it always
    /// does 49-bit raw) but the round-trip confirms the link is up.
    pub async fn handshake(&mut self) -> Result<(), CodecError> {
        self.send_control(&[CTRL_RESET]).await?;
        let _ = self.recv_packet().await?; // ack
        self.send_control(&[CTRL_PRODID]).await?;
        let prodid = self.recv_packet().await?;
        if let Packet::Control(payload) = prodid {
            let name = String::from_utf8_lossy(
                payload.get(1..).unwrap_or(b"").split(|b| *b == 0).next().unwrap_or(b""),
            )
            .into_owned();
            debug!(target: "mmdvm_sip::codec", product = %name, "AMBE-3000F handshake ok");
        }
        self.send_control(&[CTRL_RATET, DMR_RATE_INDEX]).await?;
        let _ = self.recv_packet().await?;
        Ok(())
    }

    /// Send 49 voice bits packed into 7 bytes (MSB-first, 48th bit
    /// in the MSB of the last byte). Receive 160 PCM samples back.
    pub async fn decode(
        &mut self,
        ambe: &[u8; AMBE_VOICE_BYTES],
    ) -> Result<[i16; FRAME_PCM_SAMPLES], CodecError> {
        let mut body = Vec::with_capacity(2 + AMBE_VOICE_BYTES);
        body.push(CHAND_TAG);
        body.push(AMBE_VOICE_BITS);
        body.extend_from_slice(ambe);
        self.send_packet(TYPE_CHANNEL, &body).await?;

        match self.recv_packet().await? {
            Packet::Speech(samples) => {
                if samples.len() != FRAME_PCM_SAMPLES {
                    return Err(CodecError::WrongSampleCount {
                        got: samples.len(),
                        want: FRAME_PCM_SAMPLES,
                    });
                }
                let mut out = [0i16; FRAME_PCM_SAMPLES];
                out.copy_from_slice(&samples);
                Ok(out)
            }
            other => Err(CodecError::UnexpectedReply(other.type_byte())),
        }
    }

    async fn send_control(&mut self, payload: &[u8]) -> Result<(), CodecError> {
        self.send_packet(TYPE_CONTROL, payload).await
    }

    async fn send_packet(&mut self, type_byte: u8, body: &[u8]) -> Result<(), CodecError> {
        let length = (1 + body.len()) as u16;
        let mut buf = Vec::with_capacity(3 + body.len());
        buf.push(START_BYTE);
        buf.extend_from_slice(&length.to_be_bytes());
        buf.push(type_byte);
        buf.extend_from_slice(body);
        self.stream.write_all(&buf).await?;
        self.stream.flush().await?;
        Ok(())
    }

    async fn recv_packet(&mut self) -> Result<Packet, CodecError> {
        // Resync on a stray byte the same way the daemon does.
        let mut hdr = [0u8; 4];
        loop {
            self.stream.read_exact(&mut hdr[..1]).await?;
            if hdr[0] == START_BYTE {
                break;
            }
        }
        self.stream.read_exact(&mut hdr[1..]).await?;
        let length = u16::from_be_bytes([hdr[1], hdr[2]]) as usize;
        if length == 0 || length > 4096 {
            return Err(CodecError::Malformed("bad length"));
        }
        let type_byte = hdr[3];
        let mut payload = vec![0u8; length - 1];
        self.stream.read_exact(&mut payload).await?;
        match type_byte {
            TYPE_CONTROL => Ok(Packet::Control(payload)),
            TYPE_CHANNEL => Ok(parse_channel(&payload)?),
            TYPE_SPEECH => Ok(parse_speech(&payload)?),
            other => Err(CodecError::UnexpectedReply(other)),
        }
    }
}

enum Packet {
    Control(Vec<u8>),
    #[allow(dead_code)]
    Channel { bit_count: u8, data: Vec<u8> },
    Speech(Vec<i16>),
}

impl Packet {
    fn type_byte(&self) -> u8 {
        match self {
            Packet::Control(_) => TYPE_CONTROL,
            Packet::Channel { .. } => TYPE_CHANNEL,
            Packet::Speech(_) => TYPE_SPEECH,
        }
    }
}

fn parse_channel(payload: &[u8]) -> Result<Packet, CodecError> {
    if payload.len() < 2 {
        return Err(CodecError::Malformed("channel payload < 2 bytes"));
    }
    if payload[0] != CHAND_TAG {
        return Err(CodecError::Malformed("channel: missing CHAND tag"));
    }
    let bit_count = payload[1];
    let want = (bit_count as usize).div_ceil(8);
    if payload.len() < 2 + want {
        return Err(CodecError::Malformed("channel: truncated"));
    }
    Ok(Packet::Channel {
        bit_count,
        data: payload[2..2 + want].to_vec(),
    })
}

fn parse_speech(payload: &[u8]) -> Result<Packet, CodecError> {
    if payload.len() < 2 {
        return Err(CodecError::Malformed("speech payload < 2 bytes"));
    }
    if payload[0] != SPEECHD_TAG {
        return Err(CodecError::Malformed("speech: missing SPEECHD tag"));
    }
    let samples = payload[1] as usize;
    let want = samples * 2;
    if payload.len() < 2 + want {
        return Err(CodecError::Malformed("speech: truncated"));
    }
    let mut out = Vec::with_capacity(samples);
    for chunk in payload[2..2 + want].chunks_exact(2) {
        out.push(i16::from_be_bytes([chunk[0], chunk[1]]));
    }
    Ok(Packet::Speech(out))
}
