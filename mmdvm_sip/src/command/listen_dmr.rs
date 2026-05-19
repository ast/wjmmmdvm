use std::net::SocketAddr;

use clap::Args;

use crate::dmr_listener::DmrListener;

#[derive(Args, Debug)]
pub struct ListenDmrCmd {
    /// Local UDP address to bind. Must match the GatewayAddress/GatewayPort
    /// in the modem's MMDVM.ini `[DMR Network]` section.
    #[arg(short, long, default_value = "0.0.0.0:62031")]
    bind: SocketAddr,
}

impl ListenDmrCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        init_tracing();
        let listener = DmrListener::bind(self.bind).await?;
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
