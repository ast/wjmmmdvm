use clap::Parser;
use tracing_subscriber::EnvFilter;

mod codec;
mod command;
mod firmware;
mod protocol;
mod server;

/// Crate version with git commit appended, e.g. `0.1.0 (a1b2c3d)`.
/// `GIT_HASH` is set by `build.rs`; if git is unavailable at build
/// time it's the literal `unknown`.
const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")");

#[derive(Parser, Debug)]
#[command(
    name = "md380-emu-ambed",
    version = VERSION,
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
