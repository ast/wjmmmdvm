//! Subprocess wrapper around the file-based `md380-emu` binary.
//!
//! ## File formats
//!
//! - **PCM**: signed 16-bit, little-endian, 8 kHz mono.
//! - **AMB (md380-emu native)**: 4-byte header `".amb"` followed by N
//!   frames of 8 bytes each. md380-emu's decoder has a magic-check bug
//!   (`!strcmp(header, ".amb")` triggers an `exit(1)` on a *correct*
//!   header), so this driver writes `\0\0\0\0` as the header when
//!   feeding the decoder — which sails past the buggy check.
//! - **AMB frame**: byte[0] = status (0 = good), bytes[1..7] = 6 bytes
//!   of packed voice bits MSB-first (48 bits), byte[7] = the 49th bit
//!   in its LSB.
//!
//! ## Quirks
//!
//! - The encoder discards the first 25 output frames (~500 ms) as
//!   "start noise". To get a useful encoded result, pre-pad your PCM
//!   input with at least 500 ms of silence.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;
use tracing::debug;

use crate::error::{AmbeError, Result};

/// 4-byte magic written by md380-emu's encoder at the start of its .amb
/// output. Stripped on encode, **not** re-added on decode — see the
/// magic-check bug note above.
const AMB_MAGIC: &[u8; 4] = b".amb";
/// Header bytes we prepend to the decoder's input so the buggy magic
/// check skips its `exit(1)` branch.
const DECODE_INPUT_HEADER: [u8; 4] = [0, 0, 0, 0];

/// One subprocess invocation of `md380-emu` per encode or decode call.
/// Not real-time; intended for offline corpus building.
pub struct Md380Emu {
    binary: PathBuf,
}

impl Md380Emu {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    /// Encode raw 8 kHz s16le PCM into 8-byte AMBE frames. The
    /// returned byte stream does *not* include the `".amb"` magic
    /// header — it's just N×8 bytes of frames.
    pub async fn encode(&self, pcm: &[i16]) -> Result<Vec<u8>> {
        let tmp = tempfile::tempdir()?;
        let pcm_path = tmp.path().join("in.pcm");
        let amb_path = tmp.path().join("out.amb");

        tokio::fs::write(&pcm_path, &pcm_to_bytes(pcm)).await?;
        run(&self.binary, "-e", &pcm_path, &amb_path).await?;
        let mut amb = tokio::fs::read(&amb_path).await?;
        if amb.len() < 4 || &amb[..4] != AMB_MAGIC {
            return Err(AmbeError::Malformed("md380-emu encoder didn't write .amb magic"));
        }
        amb.drain(..4);
        debug!(
            target: "ambe_lab::md380_emu",
            frames = amb.len() / 8,
            bytes = amb.len(),
            "encoded"
        );
        Ok(amb)
    }

    /// Decode N×8 AMBE bytes back to 8 kHz s16le PCM. Input must NOT
    /// include the `".amb"` magic header (we prepend our own four
    /// nulls to dodge the decoder's magic-check bug).
    pub async fn decode(&self, amb_frames: &[u8]) -> Result<Vec<i16>> {
        let tmp = tempfile::tempdir()?;
        let amb_path = tmp.path().join("in.amb");
        let pcm_path = tmp.path().join("out.pcm");

        let mut amb_input = Vec::with_capacity(4 + amb_frames.len());
        amb_input.extend_from_slice(&DECODE_INPUT_HEADER);
        amb_input.extend_from_slice(amb_frames);
        tokio::fs::write(&amb_path, &amb_input).await?;

        run(&self.binary, "-d", &amb_path, &pcm_path).await?;
        let pcm_bytes = tokio::fs::read(&pcm_path).await?;
        let pcm = bytes_to_pcm(&pcm_bytes);
        debug!(
            target: "ambe_lab::md380_emu",
            samples = pcm.len(),
            seconds = pcm.len() as f32 / 8000.0,
            "decoded"
        );
        Ok(pcm)
    }
}

async fn run(binary: &Path, mode: &str, input: &Path, output: &Path) -> Result<()> {
    let status = Command::new(binary)
        .arg(mode)
        .arg("-i")
        .arg(input)
        .arg("-o")
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|e| {
            AmbeError::Malformed(if e.kind() == std::io::ErrorKind::NotFound {
                "md380-emu binary not found — set MD380_EMU=/path or add it to PATH"
            } else {
                "failed to spawn md380-emu"
            })
        })?;
    if !status.success() {
        return Err(AmbeError::Malformed("md380-emu exited non-zero"));
    }
    Ok(())
}

fn pcm_to_bytes(pcm: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pcm.len() * 2);
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn bytes_to_pcm(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}
