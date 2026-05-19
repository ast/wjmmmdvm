use clap::Subcommand;

pub mod listen_dmr;
pub mod register;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Register to the configured Asterisk PBX and keep the registration alive.
    Register(register::RegisterCmd),
    /// Listen for HBP DMR Network packets from MMDVMHost and log them.
    ListenDmr(listen_dmr::ListenDmrCmd),
}

impl Command {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Command::Register(cmd) => cmd.run().await,
            Command::ListenDmr(cmd) => cmd.run().await,
        }
    }
}
