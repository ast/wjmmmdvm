use std::path::PathBuf;

use clap::Args;
use tokio::fs::read;

use ambe_lab::ambe_frame::iter_frames;
use ambe_lab::codec::{AmbeFields, FrameKind};
use ambe_lab::AMBE_BYTES_PER_FRAME;

#[derive(Args, Debug)]
pub struct DumpFieldsCmd {
    /// .ambe file to dump (8 bytes per frame, no .amb header).
    input: PathBuf,
    /// Skip the first N frames.
    #[arg(long, default_value_t = 0)]
    start: usize,
    /// Print at most N frames after `start`. 0 = print all.
    #[arg(long, default_value_t = 0)]
    count: usize,
}

impl DumpFieldsCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let raw = read(&self.input).await?;
        let total = raw.len() / AMBE_BYTES_PER_FRAME;
        let end = if self.count == 0 {
            total
        } else {
            (self.start + self.count).min(total)
        };

        println!(
            "{} frames total, printing {}..{}",
            total, self.start, end
        );
        println!(
            "{:>5}  {:<8}  {:>3} {:>3} {:>4} {:>4} {:>3} {:>3} {:>3} {:>3} {:>3}",
            "idx", "kind", "w0", "vuv", "gain", "p24", "p58", "h5", "h6", "h7", "h8"
        );

        for (idx, frame) in iter_frames(&raw).enumerate() {
            if idx < self.start {
                continue;
            }
            if idx >= end {
                break;
            }
            let f = AmbeFields::from_frame(&frame);
            let kind = match f.kind() {
                FrameKind::Voice => "voice",
                FrameKind::Silence => "silence",
                FrameKind::Tone => "tone",
                FrameKind::Erasure => "erasure",
            };
            println!(
                "{:>5}  {:<8}  {:>3} {:>3} {:>4} {:>4} {:>3} {:>3} {:>3} {:>3} {:>3}",
                idx, kind, f.w0, f.vuv, f.gain, f.prba24, f.prba58, f.hoc5, f.hoc6, f.hoc7, f.hoc8
            );
        }
        Ok(())
    }
}
