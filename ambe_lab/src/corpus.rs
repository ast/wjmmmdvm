//! Golden-vector corpus writer. Given a set of [`Stimulus`]es, runs
//! each through `md380-emu` and writes `(input.pcm, output.ambe)` pairs
//! plus a self-describing `manifest.json`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{debug, info};

use crate::error::{AmbeError, Result};
use crate::md380_emu::Md380Emu;
use crate::stimulus::Stimulus;
use crate::{AMBE_BYTES_PER_FRAME, PCM_SAMPLE_RATE_HZ, PCM_SAMPLES_PER_FRAME};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Format {
    pub sample_rate_hz: u32,
    pub pcm_format: String,
    pub pcm_samples_per_frame: usize,
    pub ambe_frame_bytes: usize,
    pub frame_ms: u32,
    /// ms of silence prepended to each PCM input before encoding.
    /// md380-emu's encoder discards its first 25 output frames as
    /// "start noise", so we pad here to make sure the real signal
    /// survives the warm-up.
    pub encoder_warmup_pad_ms: u32,
}

impl Format {
    pub fn new(warmup_pad_ms: u32) -> Self {
        Self {
            sample_rate_hz: PCM_SAMPLE_RATE_HZ,
            pcm_format: "s16le".to_string(),
            pcm_samples_per_frame: PCM_SAMPLES_PER_FRAME,
            ambe_frame_bytes: AMBE_BYTES_PER_FRAME,
            frame_ms: 20,
            encoder_warmup_pad_ms: warmup_pad_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub pcm_path: String,
    pub ambe_path: String,
    pub stimulus: Stimulus,
    pub pcm_samples: usize,
    pub ambe_frames: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: Format,
    pub captured_at_unix: u64,
    pub md380_emu_binary: PathBuf,
    pub entries: Vec<Entry>,
}

/// Writes a corpus into `out_dir`. Each `add()` call generates one
/// `(pcm, ambe)` pair from a stimulus and records its metadata. Call
/// `write_manifest()` at the end to flush `manifest.json`.
pub struct Corpus {
    out_dir: PathBuf,
    emu: Md380Emu,
    warmup_pad_ms: u32,
    entries: Vec<Entry>,
}

impl Corpus {
    pub async fn create(out_dir: impl Into<PathBuf>, emu: Md380Emu, warmup_pad_ms: u32) -> Result<Self> {
        let out_dir = out_dir.into();
        fs::create_dir_all(&out_dir).await?;
        Ok(Self {
            out_dir,
            emu,
            warmup_pad_ms,
            entries: Vec::new(),
        })
    }

    pub async fn add(&mut self, stimulus: Stimulus) -> Result<()> {
        let name = stimulus.name();
        let pcm = stimulus.generate();
        let padded = pad_warmup(&pcm, self.warmup_pad_ms);
        let ambe = self.emu.encode(&padded).await?;

        let pcm_filename = format!("{name}.pcm");
        let ambe_filename = format!("{name}.ambe");
        let pcm_path = self.out_dir.join(&pcm_filename);
        let ambe_path = self.out_dir.join(&ambe_filename);
        fs::write(&pcm_path, pcm_bytes_le(&padded)).await?;
        fs::write(&ambe_path, &ambe).await?;

        let entry = Entry {
            name: name.clone(),
            pcm_path: pcm_filename,
            ambe_path: ambe_filename,
            stimulus,
            pcm_samples: padded.len(),
            ambe_frames: ambe.len() / AMBE_BYTES_PER_FRAME,
        };
        debug!(
            target: "ambe_lab::corpus",
            name = %entry.name,
            samples = entry.pcm_samples,
            frames = entry.ambe_frames,
            "captured"
        );
        self.entries.push(entry);
        Ok(())
    }

    pub async fn write_manifest(self) -> Result<PathBuf> {
        let manifest = Manifest {
            format: Format::new(self.warmup_pad_ms),
            captured_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            md380_emu_binary: self.emu.binary().to_path_buf(),
            entries: self.entries,
        };
        let path = self.out_dir.join("manifest.json");
        let json = serde_json::to_string_pretty(&manifest)
            .map_err(|_| AmbeError::Malformed("manifest JSON serialization failed"))?;
        fs::write(&path, json.as_bytes()).await?;
        info!(
            target: "ambe_lab::corpus",
            path = %path.display(),
            entries = manifest.entries.len(),
            "wrote manifest"
        );
        Ok(path)
    }

    pub fn out_dir(&self) -> &Path {
        &self.out_dir
    }
}

fn pad_warmup(pcm: &[i16], warmup_pad_ms: u32) -> Vec<i16> {
    if warmup_pad_ms == 0 {
        return pcm.to_vec();
    }
    let pad_samples = (warmup_pad_ms as usize * PCM_SAMPLE_RATE_HZ as usize) / 1000;
    let mut out = Vec::with_capacity(pad_samples + pcm.len());
    out.resize(pad_samples, 0);
    out.extend_from_slice(pcm);
    out
}

fn pcm_bytes_le(pcm: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pcm.len() * 2);
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}
