use clap::Parser;
use tracing_subscriber::EnvFilter;

mod command;

#[derive(Parser, Debug)]
#[command(
    name = "ambe_lab",
    version,
    about = "AMBE+2 research harness against md380-emu (not for distribution)"
)]
struct Cli {
    #[command(subcommand)]
    command: command::Command,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("ambe_lab=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let cli = Cli::parse();
    cli.command.run().await
}
