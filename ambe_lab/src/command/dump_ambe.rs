use std::path::PathBuf;

use clap::Args;
use tokio::fs::read;

use ambe_lab::ambe_frame::iter_frames;
use ambe_lab::AMBE_BYTES_PER_FRAME;

#[derive(Args, Debug)]
pub struct DumpAmbeCmd {
    /// .ambe file to dump (8 bytes per frame, no .amb header).
    input: PathBuf,
    /// Skip the first N frames.
    #[arg(long, default_value_t = 0)]
    start: usize,
    /// Print at most N frames after `start`. 0 = print all.
    #[arg(long, default_value_t = 0)]
    count: usize,
}

impl DumpAmbeCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let raw = read(&self.input).await?;
        let total = raw.len() / AMBE_BYTES_PER_FRAME;
        let end = if self.count == 0 {
            total
        } else {
            (self.start + self.count).min(total)
        };

        println!(
            "{} frames total ({} bytes), printing {}..{}",
            total,
            raw.len(),
            self.start,
            end
        );

        for (idx, frame) in iter_frames(&raw).enumerate() {
            if idx < self.start {
                continue;
            }
            if idx >= end {
                break;
            }
            let raw_hex: String = frame
                .raw()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(" ");
            let bits = frame.voice_bits();
            let bit_string: String = bits[..48]
                .chunks(8)
                .map(|c| c.iter().map(|b| if *b == 1 { '1' } else { '0' }).collect::<String>())
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                "Frame {:>4}: {}  | status={}  bits[0..48]={}  bit[48]={}",
                idx,
                raw_hex,
                frame.status(),
                bit_string,
                bits[48]
            );
        }
        Ok(())
    }
}
