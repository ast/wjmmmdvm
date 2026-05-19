use clap::Subcommand;

pub mod echo_dmr;
pub mod listen_dmr;
pub mod register;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Register to the configured Asterisk PBX and keep the registration alive.
    Register(register::RegisterCmd),
    /// Listen for HBP DMR Network packets from MMDVMHost and log them.
    ListenDmr(listen_dmr::ListenDmrCmd),
    /// Record one DMR call and replay it back to MMDVMHost — a
    /// "parrot" without transcoding.
    EchoDmr(echo_dmr::EchoDmrCmd),
}

impl Command {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Command::Register(cmd) => cmd.run().await,
            Command::ListenDmr(cmd) => cmd.run().await,
            Command::EchoDmr(cmd) => cmd.run().await,
        }
    }
}
