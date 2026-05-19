use std::path::PathBuf;

use clap::Args;
use tracing::{info, warn};

use crate::codec::{Md380Codec, AMBE_FRAME_BYTES, FRAME_PCM_SAMPLES};
use crate::firmware::Firmware;

#[derive(Args, Debug)]
pub struct EncodeCmd {
    /// Input PCM file (s16le, 8 kHz mono).
    input: PathBuf,
    /// Output AMBE file (8 bytes per 20 ms frame, no .amb header).
    output: PathBuf,
    /// Discard the first N output frames before writing. md380-emu's
    /// upstream encoder discards the first 26 frames as "start
    /// noise"; matching that default keeps us byte-compatible with
    /// .amb files md380-emu produces.
    #[arg(long, default_value_t = 26)]
    skip_warmup: usize,
}

impl EncodeCmd {
    pub fn run(self) -> anyhow::Result<()> {
        let firmware = Firmware::load()?;
        let mut codec = Md380Codec::new(firmware);

        let raw = std::fs::read(&self.input)?;
        if raw.len() % 2 != 0 {
            warn!(
                target: "md380_emu_ambed::encode",
                "input has an odd byte count, last byte dropped"
            );
        }
        let samples: Vec<i16> = raw
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        info!(
            target: "md380_emu_ambed::encode",
            samples = samples.len(),
            seconds = samples.len() as f32 / 8000.0,
            "loaded PCM"
        );

        let mut frame_count = 0usize;
        let mut skipped = 0usize;
        let mut out = Vec::with_capacity((samples.len() / FRAME_PCM_SAMPLES) * AMBE_FRAME_BYTES);
        let mut chunks = samples.chunks_exact(FRAME_PCM_SAMPLES);
        for chunk in &mut chunks {
            let mut pcm_frame = [0i16; FRAME_PCM_SAMPLES];
            pcm_frame.copy_from_slice(chunk);
            let ambe = codec.encode(&pcm_frame);
            if skipped < self.skip_warmup {
                skipped += 1;
                continue;
            }
            out.extend_from_slice(&ambe);
            frame_count += 1;
        }
        if !chunks.remainder().is_empty() {
            warn!(
                target: "md380_emu_ambed::encode",
                trailing_samples = chunks.remainder().len(),
                "input not a whole number of 20 ms frames; remainder discarded"
            );
        }

        std::fs::write(&self.output, &out)?;
        info!(
            target: "md380_emu_ambed::encode",
            path = %self.output.display(),
            frames = frame_count,
            skipped = skipped,
            bytes = out.len(),
            "wrote AMBE"
        );
        Ok(())
    }
}
