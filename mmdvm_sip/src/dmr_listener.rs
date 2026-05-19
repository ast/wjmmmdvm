use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use tokio::time::Instant;
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

use crate::ambe_codec_client::AmbeCodecClient;
use crate::dmr::voice_burst::extract_voice_frames;
use crate::dmr::{DmrAlias, DmrConfig, DmrData, DmrGps, Packet};
use crate::error::Result;
use crate::fec::strip_fec;
use crate::pcm_writer::{CallKey, PcmWriter};

/// Optional audio-decoding pipeline. When `Some`, voice bursts get
/// fed through FEC strip → codec daemon → per-call PCM writer.
struct Decoder {
    codec: AmbeCodecClient,
    writer: PcmWriter,
}

/// Binds a UDP socket and logs every HBP packet MMDVMHost sends to it.
/// Optionally also decodes voice bursts to PCM files via the AMBE
/// codec daemon.
pub struct DmrListener {
    socket: UdpSocket,
    decoder: Option<Decoder>,
}

impl DmrListener {
    pub async fn bind(addr: SocketAddr) -> Result<Self> {
        let socket = UdpSocket::bind(addr).await?;
        info!(target: "mmdvm_sip::dmr", local = %socket.local_addr()?, "listening for DMR Network traffic");
        Ok(Self {
            socket,
            decoder: None,
        })
    }

    /// Enable audio decoding. Connects to the AMBE-3000F codec
    /// daemon at `codec_addr` and writes per-call PCM files to
    /// `output_dir`.
    pub async fn with_audio_decode(
        mut self,
        codec_addr: &str,
        output_dir: PathBuf,
    ) -> Result<Self> {
        let mut codec = AmbeCodecClient::connect(codec_addr).await.map_err(|e| {
            crate::error::SipError::SipParse(format!("codec connect failed: {e}"))
        })?;
        codec.handshake().await.map_err(|e| {
            crate::error::SipError::SipParse(format!("codec handshake failed: {e}"))
        })?;
        let writer = PcmWriter::new(output_dir.clone())
            .await
            .map_err(|e| crate::error::SipError::SipParse(format!("pcm writer: {e}")))?;
        info!(
            target: "mmdvm_sip::dmr",
            codec = %codec_addr,
            output = %output_dir.display(),
            "audio decode enabled"
        );
        self.decoder = Some(Decoder { codec, writer });
        Ok(self)
    }

    pub async fn run(mut self) -> Result<()> {
        let mut buf = vec![0u8; 4096];
        // Periodic tick for closing idle calls when no DMRD packets
        // arrive (end of a transmission).
        let mut idle_tick = tokio::time::interval(Duration::from_millis(500));
        idle_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = idle_tick.tick() => {
                    if let Some(dec) = &mut self.decoder {
                        if let Err(e) = dec.writer.flush_idle().await {
                            warn!(target: "mmdvm_sip::dmr", error = %e, "idle flush failed");
                        }
                    }
                }
                res = self.socket.recv_from(&mut buf) => {
                    let (n, peer) = res?;
                    let datagram = &buf[..n];
                    match Packet::parse(datagram) {
                        Packet::Data(p) => {
                            log_data(peer, p);
                            if let Some(dec) = &mut self.decoder {
                                if let Err(e) = decode_voice(dec, p).await {
                                    warn!(target: "mmdvm_sip::dmr", error = %e, "voice decode failed");
                                }
                            }
                        }
                        Packet::Config(p) => log_config(peer, p),
                        Packet::Gps(p) => log_gps(peer, p),
                        Packet::Alias(p) => log_alias(peer, p),
                        Packet::Unknown { magic, len } => {
                            let magic_str = String::from_utf8_lossy(&magic);
                            warn!(
                                target: "mmdvm_sip::dmr",
                                %peer,
                                magic = %magic_str,
                                len,
                                "unknown / mis-sized packet"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Extract the three AMBE frames from a voice DMRD packet, strip FEC,
/// send each through the codec daemon, and append the resulting PCM
/// to the per-call file.
async fn decode_voice(dec: &mut Decoder, p: &DmrData) -> anyhow::Result<()> {
    let flags = p.flags();
    // Only voice frames carry audio: the `frame_or_data_type` low
    // nibble is 0..=5 for voice frames A..F, and the burst is a
    // pure voice burst when neither sync flag is asserted (frames
    // B..E carry embedded signalling but still hold AMBE bits).
    if flags.data_sync() {
        return Ok(()); // header / terminator / data burst, no AMBE
    }
    let ftype = flags.frame_or_data_type();
    if ftype > 5 {
        return Ok(());
    }

    let frames = extract_voice_frames(&p.payload);
    let mut pcm = Vec::with_capacity(480);
    let mut total_errs = 0u32;
    let started = Instant::now();
    for fr in frames {
        let (voice_bits, errs) = strip_fec(fr);
        total_errs += errs;
        let amb_bytes = pack_voice_bits(&voice_bits);
        let samples = dec.codec.decode(&amb_bytes).await?;
        pcm.extend_from_slice(&samples);
    }
    let elapsed = started.elapsed();
    debug!(
        target: "mmdvm_sip::dmr",
        stream_id = format!("0x{:08x}", p.stream_id.get()),
        slot = flags.slot(),
        fec_errs = total_errs,
        codec_ms = elapsed.as_millis() as u64,
        "decoded burst"
    );

    let key = CallKey {
        stream_id: p.stream_id.get(),
        slot: flags.slot(),
    };
    dec.writer
        .handle_burst(key, p.src_id_u32(), p.dst_id_u32(), &pcm)
        .await?;
    Ok(())
}

/// Pack 49 voice bits into the 7-byte channel-packet format the
/// codec daemon expects: bytes 0..=5 hold bits 0..=47 MSB-first,
/// byte 6 holds bit 48 as its MSB.
fn pack_voice_bits(bits: &[u8; 49]) -> [u8; 7] {
    let mut out = [0u8; 7];
    for (i, bit) in bits[..48].iter().enumerate() {
        out[i / 8] |= (bit & 1) << (7 - (i % 8));
    }
    out[6] |= (bits[48] & 1) << 7;
    out
}

fn log_data(peer: SocketAddr, p: &DmrData) {
    let flags = p.flags();
    let stream_id = p.stream_id.get();
    let repeater_id = p.repeater_id.get();
    info!(
        target: "mmdvm_sip::dmr",
        %peer,
        kind = "DMRD",
        seq = p.seq,
        src = p.src_id_u32(),
        dst = p.dst_id_u32(),
        repeater = repeater_id,
        slot = flags.slot(),
        private = flags.is_private(),
        data_sync = flags.data_sync(),
        voice_sync = flags.voice_sync(),
        frame_or_dtype = flags.frame_or_data_type(),
        stream_id = format!("0x{:08X}", stream_id),
        ber = p.ber,
        rssi = p.rssi,
        "DMR data burst"
    );
}

fn log_config(peer: SocketAddr, p: &DmrConfig) {
    debug!(
        target: "mmdvm_sip::dmr",
        %peer,
        kind = "DMRC",
        bytes = p.data.len() + 4,
        "repeater config heartbeat"
    );
}

fn log_gps(peer: SocketAddr, p: &DmrGps) {
    info!(
        target: "mmdvm_sip::dmr",
        %peer,
        kind = "DMRG",
        bytes = p.data.len() + 4,
        "GPS frame"
    );
}

fn log_alias(peer: SocketAddr, p: &DmrAlias) {
    info!(
        target: "mmdvm_sip::dmr",
        %peer,
        kind = "DMRA",
        bytes = p.data.len() + 4,
        "Talker Alias block"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_voice_bits_layout() {
        let mut bits = [0u8; 49];
        bits[0] = 1; // MSB of byte 0
        bits[7] = 1; // LSB of byte 0
        bits[48] = 1; // MSB of byte 6
        let packed = pack_voice_bits(&bits);
        assert_eq!(packed[0], 0b1000_0001);
        for b in &packed[1..6] {
            assert_eq!(*b, 0);
        }
        assert_eq!(packed[6], 0b1000_0000);
    }
}
