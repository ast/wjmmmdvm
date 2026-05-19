use std::path::PathBuf;

use clap::Args;
use tokio::fs::{File, read};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use ambe_lab::md380_emu::Md380Emu;
use ambe_lab::{AMBE_BYTES_PER_FRAME, PCM_SAMPLES_PER_FRAME};

#[derive(Args, Debug)]
pub struct DecodeCmd {
    /// Input AMBE byte file (8 bytes per 20 ms frame, no ".amb" header).
    input: PathBuf,
    /// Output PCM file (s16le, 8 kHz mono).
    output: PathBuf,
    /// Path to the md380-emu binary. Defaults to "md380-emu" on $PATH;
    /// override via the MD380_EMU env var or this flag.
    #[arg(long, env = "MD380_EMU", default_value = "md380-emu")]
    binary: PathBuf,
}

impl DecodeCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let raw = read(&self.input).await?;
        let trailing = raw.len() % AMBE_BYTES_PER_FRAME;
        if trailing != 0 {
            warn!(
                target: "ambe_lab::decode",
                trailing_bytes = trailing,
                "input not a whole number of 8-byte frames; md380-emu will ignore the remainder"
            );
        }
        info!(
            target: "ambe_lab::decode",
            bytes = raw.len(),
            frames = raw.len() / AMBE_BYTES_PER_FRAME,
            "loaded AMBE"
        );

        let emu = Md380Emu::new(self.binary);
        let pcm = emu.decode(&raw).await?;

        let mut out = File::create(&self.output).await?;
        let mut bytes = Vec::with_capacity(pcm.len() * 2);
        for s in &pcm {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        out.write_all(&bytes).await?;
        out.flush().await?;
        info!(
            target: "ambe_lab::decode",
            path = %self.output.display(),
            samples = pcm.len(),
            frames = pcm.len() / PCM_SAMPLES_PER_FRAME,
            seconds = pcm.len() as f32 / 8000.0,
            "wrote PCM"
        );
        Ok(())
    }
}
