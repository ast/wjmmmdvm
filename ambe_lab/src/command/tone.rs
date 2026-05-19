use std::path::PathBuf;

use clap::Args;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::info;

use ambe_lab::tone_gen::ToneGen;

#[derive(Args, Debug)]
pub struct ToneCmd {
    /// Output file (raw s16le 8 kHz mono PCM).
    output: PathBuf,
    /// Tone frequency in Hz.
    #[arg(short, long, default_value_t = 1000.0)]
    frequency: f32,
    /// Duration in seconds.
    #[arg(short, long, default_value_t = 2.0)]
    seconds: f32,
    /// Amplitude (0.0 – 1.0 of full scale).
    #[arg(short, long, default_value_t = 0.5)]
    amplitude: f32,
}

impl ToneCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let mut gen_tone = ToneGen::new(self.frequency, self.amplitude);
        let samples = gen_tone.generate_seconds(self.seconds);
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for s in &samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let mut file = File::create(&self.output).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        info!(
            target: "ambe_lab::tone",
            path = %self.output.display(),
            samples = samples.len(),
            seconds = self.seconds,
            frequency_hz = self.frequency,
            "wrote PCM tone"
        );
        Ok(())
    }
}
