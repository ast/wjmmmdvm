use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Args;

use crate::dmr_listener::DmrListener;

#[derive(Args, Debug)]
pub struct ListenDmrCmd {
    /// Local UDP address to bind. Must match the GatewayAddress/GatewayPort
    /// in the modem's MMDVM.ini `[DMR Network]` section.
    #[arg(short, long, default_value = "0.0.0.0:62031")]
    bind: SocketAddr,

    /// AMBE codec daemon (AMBE-3000F over TCP). Accepts a hostname or
    /// IP followed by `:port`. Used to decode the voice bursts into
    /// 8 kHz s16le PCM. Run `md380-emu-ambed serve` somewhere
    /// reachable from this host.
    #[arg(long, default_value = "127.0.0.1:2460")]
    codec_tcp: String,

    /// Directory to write per-call `.pcm` files into. One file per
    /// `(stream_id, slot)`; closed when the call ends or goes idle.
    #[arg(long, default_value = "./dmr-recordings")]
    output_dir: PathBuf,

    /// Skip audio decoding — only log packet metadata. Useful when
    /// no codec daemon is reachable.
    #[arg(long)]
    no_decode: bool,
}

impl ListenDmrCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        init_tracing();
        let mut listener = DmrListener::bind(self.bind).await?;
        if !self.no_decode {
            listener = listener
                .with_audio_decode(&self.codec_tcp, self.output_dir)
                .await?;
        }
        listener.run().await?;
        Ok(())
    }
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("mmdvm_sip=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
