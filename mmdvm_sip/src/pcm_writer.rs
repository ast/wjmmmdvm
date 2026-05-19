//! Per-call PCM file writer.
//!
//! Opens a new s16le PCM file when a fresh DMR stream starts (new
//! `stream_id`), appends 480-sample bursts (3 × 160 samples = 60 ms
//! per DMRD voice burst), and closes the file when either:
//!
//! - a different `stream_id` shows up, or
//! - a configurable idle timeout elapses (default 1 s).
//!
//! Files are named
//! `dmr-{YYYY-MM-DD_HH-MM-SS}-src{N}-dst{N}-slot{N}.pcm` in the
//! configured output directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum WriterError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

/// Identifies a logical DMR call. Combination of stream_id and slot
/// because a single repeater carries two interleaved time-slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallKey {
    pub stream_id: u32,
    pub slot: u8,
}

/// Metadata captured at the start of each call, kept for logging on
/// close.
#[derive(Debug)]
struct CallMeta {
    src: u32,
    dst: u32,
    path: PathBuf,
    file: File,
    last_write: Instant,
    bytes_written: u64,
}

/// One PcmWriter manages many concurrent calls across both DMR
/// timeslots. Caller pumps `.handle_burst(...)` once per decoded
/// 60 ms voice burst; the writer routes the samples to the right
/// open file based on `(stream_id, slot)`.
pub struct PcmWriter {
    out_dir: PathBuf,
    idle_timeout: Duration,
    active: HashMap<CallKey, CallMeta>,
}

impl PcmWriter {
    pub async fn new(out_dir: PathBuf) -> Result<Self, WriterError> {
        tokio::fs::create_dir_all(&out_dir).await?;
        Ok(Self {
            out_dir,
            idle_timeout: Duration::from_secs(1),
            active: HashMap::new(),
        })
    }

    #[allow(dead_code)]
    pub fn with_idle_timeout(mut self, t: Duration) -> Self {
        self.idle_timeout = t;
        self
    }

    /// Append one 60 ms voice burst (3 × 160 = 480 samples) to the
    /// call's file. Opens a new file on first sight of the
    /// `(stream_id, slot)` pair.
    pub async fn handle_burst(
        &mut self,
        key: CallKey,
        src: u32,
        dst: u32,
        samples: &[i16],
    ) -> Result<(), WriterError> {
        // Garbage-collect calls that have gone quiet.
        self.flush_idle().await?;

        if !self.active.contains_key(&key) {
            let path = self.path_for(&key, src, dst);
            let file = File::create(&path).await?;
            info!(
                target: "mmdvm_sip::pcm_writer",
                stream_id = format!("0x{:08x}", key.stream_id),
                slot = key.slot,
                src,
                dst,
                path = %path.display(),
                "call started"
            );
            self.active.insert(
                key,
                CallMeta {
                    src,
                    dst,
                    path,
                    file,
                    last_write: Instant::now(),
                    bytes_written: 0,
                },
            );
        }

        let meta = self.active.get_mut(&key).expect("just inserted");
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        meta.file.write_all(&bytes).await?;
        meta.last_write = Instant::now();
        meta.bytes_written += bytes.len() as u64;
        Ok(())
    }

    /// Close any calls that haven't received a burst within
    /// `idle_timeout`. Called automatically on each `handle_burst`,
    /// but the listener should also call it periodically when no
    /// bursts are arriving (e.g. via a `tokio::time::interval`).
    pub async fn flush_idle(&mut self) -> Result<(), WriterError> {
        let now = Instant::now();
        let stale: Vec<CallKey> = self
            .active
            .iter()
            .filter(|(_, m)| now.duration_since(m.last_write) > self.idle_timeout)
            .map(|(k, _)| *k)
            .collect();
        for key in stale {
            self.close_call(key).await?;
        }
        Ok(())
    }

    /// Close every open call. Useful on shutdown / ctrl-c.
    #[allow(dead_code)]
    pub async fn close_all(&mut self) -> Result<(), WriterError> {
        let keys: Vec<CallKey> = self.active.keys().copied().collect();
        for key in keys {
            self.close_call(key).await?;
        }
        Ok(())
    }

    async fn close_call(&mut self, key: CallKey) -> Result<(), WriterError> {
        if let Some(mut meta) = self.active.remove(&key) {
            if let Err(e) = meta.file.flush().await {
                warn!(
                    target: "mmdvm_sip::pcm_writer",
                    error = %e,
                    path = %meta.path.display(),
                    "flush on close failed"
                );
            }
            let seconds = meta.bytes_written as f32 / 16000.0; // s16 @ 8 kHz
            info!(
                target: "mmdvm_sip::pcm_writer",
                stream_id = format!("0x{:08x}", key.stream_id),
                slot = key.slot,
                src = meta.src,
                dst = meta.dst,
                path = %meta.path.display(),
                seconds,
                "call ended"
            );
        }
        Ok(())
    }

    fn path_for(&self, key: &CallKey, src: u32, dst: u32) -> PathBuf {
        let ts = utc_timestamp();
        let name = format!(
            "dmr-{ts}-src{src}-dst{dst}-slot{slot}.pcm",
            slot = key.slot,
        );
        self.out_dir.join(name)
    }
}

/// Format current time as `YYYY-MM-DD_HH-MM-SSZ` from `SystemTime`,
/// avoiding the chrono / time crate dependency.
fn utc_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil-time conversion from unix seconds (Howard Hinnant's
    // algorithm). Avoids a chrono dep.
    let days = (secs / 86_400) as i64;
    let secs_of_day = (secs % 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    let hh = secs_of_day / 3600;
    let mm = (secs_of_day / 60) % 60;
    let ss = secs_of_day % 60;
    format!("{y:04}-{m:02}-{d:02}_{hh:02}-{mm:02}-{ss:02}Z")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = y + if m <= 2 { 1 } else { 0 };
    (y, m as u32, d as u32)
}

/// Convenience for the listener: get the output directory.
#[allow(dead_code)]
pub fn out_dir(p: &Path) -> &Path {
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_unix_epoch_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_from_days_handles_a_known_date() {
        // 2026-05-19 - 1970-01-01 = 20592 days (inclusive of 1970-01-01).
        assert_eq!(civil_from_days(20592), (2026, 5, 19));
    }

    #[test]
    fn timestamp_is_iso_ish() {
        let s = utc_timestamp();
        assert_eq!(s.len(), 20); // YYYY-MM-DD_HH-MM-SSZ
        assert!(s.ends_with('Z'));
    }
}
