use std::net::SocketAddr;
use std::time::Duration;

use clap::Args;
use rand::random;
use tokio::net::{UdpSocket, lookup_host};
use tokio::time::Instant;
use tracing::{info, warn};
use zerocopy::IntoBytes;
use zerocopy::big_endian::U32;

use crate::dmr::{DmrData, Packet};

#[derive(Args, Debug)]
pub struct EchoDmrCmd {
    /// Local UDP address to bind for receiving DMRD from MMDVMHost.
    /// Must match MMDVMHost's `GatewayAddress:GatewayPort` for
    /// [DMR Network].
    #[arg(short, long, default_value = "0.0.0.0:62031")]
    bind: SocketAddr,

    /// MMDVMHost's DMR Network endpoint (its `LocalAddress:LocalPort`).
    /// Replayed bursts are sent here so MMDVMHost can transmit them
    /// on the radio. Use the LAN IP (not a Tailscale hostname);
    /// MMDVMHost validates the source IP against its configured
    /// `GatewayAddress`.
    #[arg(long, default_value = "mmdvm:62032")]
    peer: String,

    /// Delay (ms) after the recorded call ends before the echo
    /// starts transmitting.
    #[arg(long, default_value_t = 1000)]
    delay_ms: u64,

    /// Idle window (ms) with no bursts that marks the call as ended.
    #[arg(long, default_value_t = 800)]
    idle_ms: u64,
}

impl EchoDmrCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        init_tracing();

        let socket = UdpSocket::bind(self.bind).await?;
        info!(
            target: "mmdvm_sip::echo",
            local = %socket.local_addr()?,
            "listening for DMR Network traffic"
        );

        let peer = lookup_host(self.peer.as_str())
            .await?
            .next()
            .ok_or_else(|| anyhow::anyhow!("could not resolve peer {}", self.peer))?;
        info!(target: "mmdvm_sip::echo", %peer, "replay destination");

        let delay = Duration::from_millis(self.delay_ms);
        let idle = Duration::from_millis(self.idle_ms);

        let mut buf = vec![0u8; 4096];
        let mut record: Vec<DmrData> = Vec::new();
        let mut current_stream: Option<u32> = None;
        let mut last_burst = Instant::now();
        let mut idle_tick = tokio::time::interval(Duration::from_millis(100));
        idle_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = idle_tick.tick() => {
                    if !record.is_empty() && last_burst.elapsed() > idle {
                        let bursts = std::mem::take(&mut record);
                        current_stream = None;
                        info!(
                            target: "mmdvm_sip::echo",
                            bursts = bursts.len(),
                            wait_ms = delay.as_millis() as u64,
                            "call ended, echoing"
                        );
                        tokio::time::sleep(delay).await;
                        if let Err(e) = echo_call(&socket, peer, &bursts).await {
                            warn!(target: "mmdvm_sip::echo", error = %e, "echo send failed");
                        }
                    }
                }
                res = socket.recv_from(&mut buf) => {
                    let (n, _src) = res?;
                    if let Packet::Data(p) = Packet::parse(&buf[..n]) {
                        let sid = p.stream_id.get();
                        match current_stream {
                            None => {
                                current_stream = Some(sid);
                                info!(
                                    target: "mmdvm_sip::echo",
                                    stream_id = format!("0x{:08x}", sid),
                                    src = p.src_id_u32(),
                                    dst = p.dst_id_u32(),
                                    slot = p.flags().slot(),
                                    "recording call"
                                );
                            }
                            Some(cur) if cur != sid => {
                                warn!(
                                    target: "mmdvm_sip::echo",
                                    old = format!("0x{:08x}", cur),
                                    new = format!("0x{:08x}", sid),
                                    "new stream while recording, dropping previous"
                                );
                                record.clear();
                                current_stream = Some(sid);
                            }
                            _ => {}
                        }
                        record.push(*p);
                        last_burst = Instant::now();
                    }
                }
            }
        }
    }
}

/// Replay the recorded bursts to `peer` with a fresh random
/// stream_id and a 60 ms cadence. Everything else (src, dst,
/// repeater_id, flags, payload) is passed through unchanged so the
/// echo appears to MMDVMHost as a duplicate of the original call.
async fn echo_call(
    socket: &UdpSocket,
    peer: SocketAddr,
    bursts: &[DmrData],
) -> anyhow::Result<()> {
    let new_stream_id: u32 = random();
    let mut tick = tokio::time::interval(Duration::from_millis(60));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    for (i, src) in bursts.iter().enumerate() {
        let mut out = *src;
        out.stream_id = U32::from(new_stream_id);
        out.seq = i as u8;
        out.ber = 0;
        out.rssi = 0;
        tick.tick().await;
        socket.send_to(out.as_bytes(), peer).await?;
    }

    info!(
        target: "mmdvm_sip::echo",
        bursts = bursts.len(),
        stream_id = format!("0x{:08x}", new_stream_id),
        "replayed"
    );
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("mmdvm_sip=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
