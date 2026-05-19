use std::path::PathBuf;

use clap::Args;
use tokio::fs::{read, File};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use ambe_lab::ambe_frame::iter_frames;
use ambe_lab::codec::{
    frame_to_i16_le, reconstruct, AmbeFields, FrameKind, SpectralState, VoicedSynth,
};
use ambe_lab::AMBE_BYTES_PER_FRAME;

#[derive(Args, Debug)]
pub struct DecodeRustCmd {
    /// Input .ambe file (8 bytes per frame).
    input: PathBuf,
    /// Output raw PCM file (s16le, 8 kHz mono).
    output: PathBuf,
}

impl DecodeRustCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let raw = read(&self.input).await?;
        let total = raw.len() / AMBE_BYTES_PER_FRAME;
        info!(
            target: "ambe_lab::decode_rust",
            frames = total,
            "decoding via pure-Rust path"
        );

        let mut state = SpectralState::default();
        let mut synth = VoicedSynth::new();
        let mut peak = 0.0f32;
        let mut voice_count = 0usize;
        let mut silence_count = 0usize;
        let mut other_count = 0usize;

        let mut out = File::create(&self.output).await?;

        for frame in iter_frames(&raw) {
            let fields = AmbeFields::from_frame(&frame);
            match fields.kind() {
                FrameKind::Voice => {
                    voice_count += 1;
                    if let Some(spec) = reconstruct(&fields, &mut state) {
                        let samples = synth.render(&spec);
                        let bytes = frame_to_i16_le(&samples, &mut peak, 0.9);
                        out.write_all(&bytes).await?;
                    } else {
                        out.write_all(&[0u8; 320]).await?;
                    }
                }
                FrameKind::Silence => {
                    silence_count += 1;
                    out.write_all(&[0u8; 320]).await?;
                    synth.reset();
                }
                other => {
                    other_count += 1;
                    if other_count == 1 {
                        warn!(
                            target: "ambe_lab::decode_rust",
                            kind = ?other,
                            "frame kind not yet supported by pure-Rust decoder — emitting silence"
                        );
                    }
                    out.write_all(&[0u8; 320]).await?;
                    synth.reset();
                }
            }
        }
        out.flush().await?;

        info!(
            target: "ambe_lab::decode_rust",
            path = %self.output.display(),
            voice = voice_count,
            silence = silence_count,
            other = other_count,
            peak_magnitude = peak,
            "wrote PCM"
        );
        if voice_count == 0 {
            warn!(
                target: "ambe_lab::decode_rust",
                "no voice-mode frames in this file — pure-Rust decoder only synthesises voice frames. \
                 Try a sawtooth or noise sample (see the corpus)."
            );
        }
        Ok(())
    }
}
