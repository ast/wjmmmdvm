use clap::Parser;
use tracing_subscriber::EnvFilter;

mod codec;
mod command;
mod firmware;
mod protocol;
mod server;

#[derive(Parser, Debug)]
#[command(
    name = "md380-emu-ambed",
    version,
    about = "MD-380 firmware AMBE codec daemon — runs the Tytera MD-380 firmware in-process via mmap'd ARM execution to encode and decode AMBE+2 audio."
)]
struct Cli {
    #[command(subcommand)]
    command: command::Command,
}

fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("md380_emu_ambed=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let cli = Cli::parse();
    cli.command.run()
}
