use std::path::PathBuf;

use clap::Args;
use tracing::info;

use crate::firmware::Firmware;
use crate::server::{run_server, ServerConfig};

#[derive(Args, Debug)]
pub struct ServeCmd {
    /// TCP listen address (host:port). Defaults to 0.0.0.0:2460
    /// (the de facto AMBE-3000F-over-TCP port). Pass `none` to
    /// disable.
    #[arg(long, default_value = "0.0.0.0:2460")]
    tcp: String,
    /// Unix domain socket path. Pass `none` to disable.
    #[arg(long, default_value = "/tmp/md380-emu-ambed.sock")]
    unix: String,
}

impl ServeCmd {
    pub fn run(self) -> anyhow::Result<()> {
        let firmware = Firmware::load()?;
        info!(
            target: "md380_emu_ambed::serve",
            tcp = %self.tcp,
            unix = %self.unix,
            "starting AMBE-3000F daemon"
        );

        let cfg = ServerConfig {
            tcp_addr: parse_optional(&self.tcp),
            unix_path: parse_optional(&self.unix).map(PathBuf::from),
        };

        // tokio runtime is set up here rather than in main() because
        // encode/decode are sync and don't need it.
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(run_server(firmware, cfg))
    }
}

fn parse_optional(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(trimmed.to_string())
    }
}
