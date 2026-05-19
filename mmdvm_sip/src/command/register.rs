use std::path::PathBuf;

use clap::Args;

use crate::config::Config;
use crate::sip_client::SipClient;

#[derive(Args, Debug)]
pub struct RegisterCmd {
    /// Path to the TOML config file.
    #[arg(short, long, default_value = "mmdvm_sip.toml")]
    config: PathBuf,
}

impl RegisterCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        init_tracing();
        let config = Config::load(&self.config)?;
        let mut client = SipClient::new(config).await?;
        client.run_register_loop().await
    }
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("mmdvm_sip=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
