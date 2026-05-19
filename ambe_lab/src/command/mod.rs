use clap::Subcommand;

pub mod capture_corpus;
pub mod decode;
pub mod encode;
pub mod tone;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Generate s16le 8 kHz mono PCM (sine tone) to a file.
    Tone(tone::ToneCmd),
    /// Encode raw PCM (s16le 8 kHz mono) to AMBE+2 frames via md380-emu.
    Encode(encode::EncodeCmd),
    /// Decode AMBE+2 frames back to raw PCM via md380-emu.
    Decode(decode::DecodeCmd),
    /// Generate a directory of golden (pcm, ambe) pairs + manifest.json.
    CaptureCorpus(capture_corpus::CaptureCorpusCmd),
}

impl Command {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Command::Tone(cmd) => cmd.run().await,
            Command::Encode(cmd) => cmd.run().await,
            Command::Decode(cmd) => cmd.run().await,
            Command::CaptureCorpus(cmd) => cmd.run().await,
        }
    }
}
