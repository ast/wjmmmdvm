use clap::Parser;

mod ambe_codec_client;
mod command;
mod config;
mod digest_auth;
mod dmr;
mod dmr_listener;
mod error;
mod fec;
mod pcm_writer;
mod registration;
mod sip_client;
mod udp_transport;

#[derive(Parser, Debug)]
#[command(name = "mmdvm_sip", version, about = "DMR <-> SIP gateway (experimental)")]
struct Cli {
    #[command(subcommand)]
    command: command::Command,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    cli.command.run().await
}
