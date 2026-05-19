use std::path::PathBuf;

use clap::Args;
use tokio::fs::read;

use ambe_lab::ambe_frame::iter_frames;
use ambe_lab::AMBE_BYTES_PER_FRAME;

#[derive(Args, Debug)]
pub struct DiffAmbeCmd {
    /// First .ambe file.
    a: PathBuf,
    /// Second .ambe file.
    b: PathBuf,
    /// Skip the first N frames.
    #[arg(long, default_value_t = 0)]
    start: usize,
    /// Compare at most N frames after `start`. 0 = compare all.
    #[arg(long, default_value_t = 0)]
    count: usize,
    /// Only print frames whose Hamming distance is >= this.
    #[arg(long, default_value_t = 0)]
    min_distance: u32,
}

impl DiffAmbeCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let raw_a = read(&self.a).await?;
        let raw_b = read(&self.b).await?;
        let total_a = raw_a.len() / AMBE_BYTES_PER_FRAME;
        let total_b = raw_b.len() / AMBE_BYTES_PER_FRAME;
        let common = total_a.min(total_b);
        let end = if self.count == 0 {
            common
        } else {
            (self.start + self.count).min(common)
        };

        println!(
            "{}: {} frames    {}: {} frames    comparing {}..{}",
            self.a.display(),
            total_a,
            self.b.display(),
            total_b,
            self.start,
            end
        );
        if total_a != total_b {
            println!(
                "note: file lengths differ by {} frames; comparing only the common prefix",
                total_a.abs_diff(total_b)
            );
        }

        let mut frames_a = iter_frames(&raw_a);
        let mut frames_b = iter_frames(&raw_b);
        let mut identical = 0usize;
        let mut differing = 0usize;
        let mut total_distance: u64 = 0;

        for idx in 0..end {
            let fa = frames_a.next().unwrap();
            let fb = frames_b.next().unwrap();
            if idx < self.start {
                continue;
            }
            let distance = fa.voice_bit_distance(&fb);
            if distance == 0 {
                identical += 1;
                if self.min_distance == 0 {
                    println!("Frame {:>4}: identical", idx);
                }
            } else {
                differing += 1;
                total_distance += distance as u64;
                if distance >= self.min_distance {
                    let bits_a = fa.voice_bits();
                    let bits_b = fb.voice_bits();
                    let diff_positions: Vec<String> = (0..49)
                        .filter(|i| bits_a[*i] != bits_b[*i])
                        .map(|i| i.to_string())
                        .collect();
                    println!(
                        "Frame {:>4}: dist={:>2}  diffs at bits [{}]",
                        idx,
                        distance,
                        diff_positions.join(",")
                    );
                }
            }
        }

        let compared = end.saturating_sub(self.start);
        let avg = if differing > 0 {
            total_distance as f64 / differing as f64
        } else {
            0.0
        };
        println!(
            "\nSummary over {} compared frames: {} identical, {} differing (avg distance over differing: {:.1} / 49 bits)",
            compared, identical, differing, avg
        );

        Ok(())
    }
}
