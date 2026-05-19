use clap::Subcommand;

pub mod decode;
pub mod encode;
pub mod serve;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Encode raw PCM (s16le 8 kHz mono) to AMBE frames via the
    /// in-process MD-380 firmware codec.
    Encode(encode::EncodeCmd),
    /// Decode AMBE frames to raw PCM via the in-process MD-380
    /// firmware codec.
    Decode(decode::DecodeCmd),
    /// Run the AMBE-3000F daemon on TCP and/or a Unix domain socket.
    Serve(serve::ServeCmd),
}

impl Command {
    pub fn run(self) -> anyhow::Result<()> {
        match self {
            Command::Encode(cmd) => cmd.run(),
            Command::Decode(cmd) => cmd.run(),
            Command::Serve(cmd) => cmd.run(),
        }
    }
}
