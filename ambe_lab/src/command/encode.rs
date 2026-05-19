use std::path::PathBuf;

use clap::Args;
use tokio::fs::{File, read};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use ambe_lab::md380_emu::Md380Emu;
use ambe_lab::{AMBE_BYTES_PER_FRAME, PCM_SAMPLES_PER_FRAME};

#[derive(Args, Debug)]
pub struct EncodeCmd {
    /// Input raw PCM file (s16le, 8 kHz mono).
    input: PathBuf,
    /// Output AMBE byte file (8 bytes per 20 ms frame, no ".amb" header).
    output: PathBuf,
    /// Path to the md380-emu binary. Defaults to "md380-emu" on $PATH;
    /// override via the MD380_EMU env var or this flag.
    #[arg(long, env = "MD380_EMU", default_value = "md380-emu")]
    binary: PathBuf,
}

impl EncodeCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let raw = read(&self.input).await?;
        if raw.len() % 2 != 0 {
            warn!(target: "ambe_lab::encode", "input has an odd byte count, last byte dropped");
        }
        let samples: Vec<i16> = raw
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        let trailing = samples.len() % PCM_SAMPLES_PER_FRAME;
        if trailing != 0 {
            warn!(
                target: "ambe_lab::encode",
                trailing_samples = trailing,
                "input not a whole number of 20 ms frames; md380-emu will discard remainder"
            );
        }
        info!(
            target: "ambe_lab::encode",
            samples = samples.len(),
            seconds = samples.len() as f32 / 8000.0,
            "loaded PCM"
        );

        let emu = Md380Emu::new(self.binary);
        let amb = emu.encode(&samples).await?;
        let frames = amb.len() / AMBE_BYTES_PER_FRAME;

        let mut out = File::create(&self.output).await?;
        out.write_all(&amb).await?;
        out.flush().await?;
        info!(
            target: "ambe_lab::encode",
            path = %self.output.display(),
            frames,
            bytes = amb.len(),
            "wrote AMBE (note: md380-emu drops the first ~25 frames as warm-up)"
        );
        Ok(())
    }
}
