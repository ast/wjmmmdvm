use std::net::SocketAddr;

use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

use crate::dmr::{DmrAlias, DmrConfig, DmrData, DmrGps, Packet};
use crate::error::Result;

/// Binds a UDP socket and logs every HBP packet MMDVMHost sends to it.
/// This is a one-way observer for now — we don't reply with anything,
/// which is fine because MMDVMHost keeps streaming regardless.
pub struct DmrListener {
    socket: UdpSocket,
}

impl DmrListener {
    pub async fn bind(addr: SocketAddr) -> Result<Self> {
        let socket = UdpSocket::bind(addr).await?;
        info!(target: "mmdvm_sip::dmr", local = %socket.local_addr()?, "listening for DMR Network traffic");
        Ok(Self { socket })
    }

    pub async fn run(self) -> Result<()> {
        let mut buf = vec![0u8; 4096];
        loop {
            let (n, peer) = self.socket.recv_from(&mut buf).await?;
            let datagram = &buf[..n];
            match Packet::parse(datagram) {
                Packet::Data(p) => log_data(peer, p),
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
