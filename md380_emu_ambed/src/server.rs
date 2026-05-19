//! AMBE-3000F daemon. Accepts connections on TCP and/or Unix domain
//! socket, parses the AMBE-3000F packet protocol, and routes
//! encode/decode requests through a single codec worker thread.
//!
//! The codec is not safe to call concurrently (shared mutable state
//! in the firmware mmap), so we run it on a dedicated OS thread and
//! talk to it via an mpsc channel. Connection handlers are tokio
//! tasks that send a request + oneshot-receiver and await the reply.

use std::path::PathBuf;
use std::thread;

use anyhow::{Context, Result};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, UnixListener};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::codec::{Md380Codec, AMBE_FRAME_BYTES, FRAME_PCM_SAMPLES};
use crate::firmware::Firmware;
use crate::protocol::{
    self, Packet, AMBE_VOICE_BITS, AMBE_VOICE_BYTES, CTRL_PRODID, CTRL_RESET, CTRL_RATEP,
    CTRL_RATET, CTRL_READY, SPEECH_SAMPLES,
};

/// Request sent from a connection handler to the codec worker.
enum CodecRequest {
    Encode {
        pcm: [i16; FRAME_PCM_SAMPLES],
        reply: oneshot::Sender<[u8; AMBE_FRAME_BYTES]>,
    },
    Decode {
        ambe: [u8; AMBE_FRAME_BYTES],
        reply: oneshot::Sender<[i16; FRAME_PCM_SAMPLES]>,
    },
}

/// Spawn the codec worker on a dedicated OS thread. Returns the
/// sender half of the request channel.
fn spawn_codec_worker(firmware: Firmware) -> mpsc::Sender<CodecRequest> {
    let (tx, mut rx) = mpsc::channel::<CodecRequest>(64);
    thread::Builder::new()
        .name("md380-codec".into())
        .spawn(move || {
            let mut codec = Md380Codec::new(firmware);
            // Use a blocking receive — the codec thread should sleep
            // when there's nothing to do.
            while let Some(req) = rx.blocking_recv() {
                match req {
                    CodecRequest::Encode { pcm, reply } => {
                        let ambe = codec.encode(&pcm);
                        let _ = reply.send(ambe);
                    }
                    CodecRequest::Decode { ambe, reply } => {
                        let pcm = codec.decode(&ambe);
                        let _ = reply.send(pcm);
                    }
                }
            }
            info!(target: "md380_emu_ambed::server", "codec worker exiting");
        })
        .expect("failed to spawn codec worker thread");
    tx
}

/// Configuration for [`run_server`].
pub struct ServerConfig {
    pub tcp_addr: Option<String>,
    pub unix_path: Option<PathBuf>,
}

/// Bind listeners, spawn the codec worker, and run the accept loops
/// until the process is signalled. Returns when both listeners are
/// shut down.
pub async fn run_server(firmware: Firmware, cfg: ServerConfig) -> Result<()> {
    let tx = spawn_codec_worker(firmware);

    let tcp = match cfg.tcp_addr.as_deref() {
        Some(addr) => {
            let l = TcpListener::bind(addr)
                .await
                .with_context(|| format!("bind TCP {addr}"))?;
            info!(target: "md380_emu_ambed::server", addr = %addr, "listening on TCP");
            Some(l)
        }
        None => None,
    };

    let unix = match cfg.unix_path.as_ref() {
        Some(path) => {
            // Clean up a stale socket from a previous crash.
            let _ = std::fs::remove_file(path);
            let l = UnixListener::bind(path)
                .with_context(|| format!("bind Unix socket {}", path.display()))?;
            info!(
                target: "md380_emu_ambed::server",
                path = %path.display(),
                "listening on Unix socket"
            );
            Some(l)
        }
        None => None,
    };

    if tcp.is_none() && unix.is_none() {
        anyhow::bail!("at least one of --tcp / --unix must be specified");
    }

    // Accept loops on both listeners; whichever fires first spawns a
    // connection handler task.
    loop {
        tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => {
                info!(target: "md380_emu_ambed::server", "ctrl-c, shutting down");
                return Ok(());
            }
            res = async {
                match &tcp {
                    Some(l) => l.accept().await.map(|(s, a)| {
                        info!(target: "md380_emu_ambed::server", peer = %a, "TCP connection");
                        let tx = tx.clone();
                        tokio::spawn(async move {
                            let (r, w) = s.into_split();
                            if let Err(e) = handle_connection(r, w, tx).await {
                                warn!(target: "md380_emu_ambed::server", error = %e, "TCP conn ended");
                            }
                        });
                    }),
                    None => std::future::pending().await,
                }
            } => {
                if let Err(e) = res {
                    error!(target: "md380_emu_ambed::server", error = %e, "TCP accept");
                }
            }
            res = async {
                match &unix {
                    Some(l) => l.accept().await.map(|(s, _)| {
                        info!(target: "md380_emu_ambed::server", "Unix connection");
                        let tx = tx.clone();
                        tokio::spawn(async move {
                            let (r, w) = s.into_split();
                            if let Err(e) = handle_connection(r, w, tx).await {
                                warn!(target: "md380_emu_ambed::server", error = %e, "Unix conn ended");
                            }
                        });
                    }),
                    None => std::future::pending().await,
                }
            } => {
                if let Err(e) = res {
                    error!(target: "md380_emu_ambed::server", error = %e, "Unix accept");
                }
            }
        }
    }
}

/// Read AMBE-3000F packets from `reader` and write responses to
/// `writer` until EOF or error.
async fn handle_connection<R, W>(
    mut reader: R,
    mut writer: W,
    codec: mpsc::Sender<CodecRequest>,
) -> Result<()>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
{
    loop {
        let pkt = match protocol::read_packet(&mut reader).await {
            Ok(p) => p,
            Err(crate::protocol::ProtocolError::Io(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                debug!(target: "md380_emu_ambed::server", "client disconnected");
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };

        match pkt {
            Packet::Control(payload) => {
                // Sub-field is the first byte of the control payload.
                let tag = payload.first().copied().unwrap_or(0);
                let reply = control_response(tag, &payload);
                protocol::write_packet(&mut writer, &reply).await?;
            }
            Packet::Speech(samples) => {
                if samples.len() != SPEECH_SAMPLES {
                    warn!(
                        target: "md380_emu_ambed::server",
                        got = samples.len(),
                        want = SPEECH_SAMPLES,
                        "Speech packet has unexpected sample count; rejecting"
                    );
                    continue;
                }
                let mut pcm = [0i16; FRAME_PCM_SAMPLES];
                pcm.copy_from_slice(&samples);
                let (tx_reply, rx_reply) = oneshot::channel();
                codec
                    .send(CodecRequest::Encode { pcm, reply: tx_reply })
                    .await
                    .context("codec worker died")?;
                let ambe = rx_reply.await.context("codec reply dropped")?;
                let resp = protocol::build_channel_from_amb8(&ambe);
                protocol::write_packet(&mut writer, &resp).await?;
            }
            Packet::Channel { bit_count, data } => {
                if bit_count != AMBE_VOICE_BITS {
                    warn!(
                        target: "md380_emu_ambed::server",
                        bit_count,
                        "Channel packet with non-49 bit count; this server speaks md380-emu raw"
                    );
                    continue;
                }
                if data.len() < AMBE_VOICE_BYTES {
                    warn!(target: "md380_emu_ambed::server", "Channel data too short");
                    continue;
                }
                let amb8 = match protocol::amb8_from_channel(bit_count, &data) {
                    Ok(b) => b,
                    Err(e) => {
                        warn!(target: "md380_emu_ambed::server", error = %e, "channel parse");
                        continue;
                    }
                };
                let (tx_reply, rx_reply) = oneshot::channel();
                codec
                    .send(CodecRequest::Decode { ambe: amb8, reply: tx_reply })
                    .await
                    .context("codec worker died")?;
                let pcm = rx_reply.await.context("codec reply dropped")?;
                let resp = Packet::Speech(pcm.to_vec());
                protocol::write_packet(&mut writer, &resp).await?;
            }
        }
    }
}

/// Build a sensible response to a Control packet. md380-emu doesn't
/// actually care about most control fields (we always run AMBE+2 at
/// its native rate), but clients expect acks so we provide them.
fn control_response(tag: u8, payload: &[u8]) -> Packet {
    match tag {
        CTRL_PRODID => {
            // Reply with the same tag + a product-ID string. The
            // typical chip response is "AMBE3000R\0" but any value
            // works; clients use it for logging.
            let mut body = Vec::new();
            body.push(CTRL_PRODID);
            body.extend_from_slice(b"MD380-EMU\0");
            Packet::Control(body)
        }
        CTRL_RESET => {
            // Reply with READY to confirm reset.
            Packet::Control(vec![CTRL_READY])
        }
        CTRL_RATEP | CTRL_RATET => {
            // Echo the tag with a single zero status byte = "ok".
            let mut body = Vec::new();
            body.push(tag);
            body.push(0x00);
            Packet::Control(body)
        }
        other => {
            debug!(
                target: "md380_emu_ambed::server",
                tag = format!("0x{:02x}", other),
                payload_len = payload.len(),
                "control packet not specifically handled — replying with generic ack"
            );
            // Generic ack: echo the tag with 0x00.
            Packet::Control(vec![other, 0x00])
        }
    }
}
