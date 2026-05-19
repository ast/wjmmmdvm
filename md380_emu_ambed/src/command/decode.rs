use std::path::PathBuf;

use clap::Args;
use tracing::{info, warn};

use crate::codec::Md380Codec;
use crate::firmware::Firmware;

const FRAME_PCM_SAMPLES: usize = 160;
const AMBE_FRAME_BYTES: usize = 8;

#[derive(Args, Debug)]
pub struct DecodeCmd {
    /// Input AMBE file (8 bytes per 20 ms frame, no .amb header).
    input: PathBuf,
    /// Output PCM file (s16le, 8 kHz mono).
    output: PathBuf,
}

impl DecodeCmd {
    pub fn run(self) -> anyhow::Result<()> {
        let firmware = Firmware::load()?;
        let mut codec = Md380Codec::new(firmware);

        let raw = std::fs::read(&self.input)?;
        let trailing = raw.len() % AMBE_FRAME_BYTES;
        if trailing != 0 {
            warn!(
                target: "md380_emu_ambed::decode",
                trailing_bytes = trailing,
                "input not a whole number of 8-byte AMBE frames; remainder discarded"
            );
        }
        info!(
            target: "md380_emu_ambed::decode",
            bytes = raw.len(),
            frames = raw.len() / AMBE_FRAME_BYTES,
            "loaded AMBE"
        );

        let mut out = Vec::with_capacity((raw.len() / AMBE_FRAME_BYTES) * FRAME_PCM_SAMPLES * 2);
        let mut frame_count = 0usize;
        for chunk in raw.chunks_exact(AMBE_FRAME_BYTES) {
            let mut ambe_frame = [0u8; AMBE_FRAME_BYTES];
            ambe_frame.copy_from_slice(chunk);
            let pcm = codec.decode(&ambe_frame);
            for sample in &pcm {
                out.extend_from_slice(&sample.to_le_bytes());
            }
            frame_count += 1;
        }

        std::fs::write(&self.output, &out)?;
        info!(
            target: "md380_emu_ambed::decode",
            path = %self.output.display(),
            frames = frame_count,
            samples = frame_count * FRAME_PCM_SAMPLES,
            seconds = (frame_count * FRAME_PCM_SAMPLES) as f32 / 8000.0,
            "wrote PCM"
        );
        Ok(())
    }
}
