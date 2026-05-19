use std::path::PathBuf;

use clap::Args;
use tracing::info;

use ambe_lab::corpus::Corpus;
use ambe_lab::md380_emu::Md380Emu;
use ambe_lab::stimulus::Stimulus;

#[derive(Args, Debug)]
pub struct CaptureCorpusCmd {
    /// Directory to write the corpus to (will be created if missing).
    output_dir: PathBuf,
    /// Path to the md380-emu binary.
    #[arg(long, env = "MD380_EMU", default_value = "md380-emu")]
    binary: PathBuf,
    /// Milliseconds of silence to prepend to each PCM input. md380-emu
    /// drops its first 25 output frames as "start noise" — padding the
    /// input by 500 ms ensures the real signal survives.
    #[arg(long, default_value_t = 500)]
    warmup_pad_ms: u32,
}

impl CaptureCorpusCmd {
    pub async fn run(self) -> anyhow::Result<()> {
        let emu = Md380Emu::new(&self.binary);
        let mut corpus = Corpus::create(&self.output_dir, emu, self.warmup_pad_ms).await?;

        for stim in default_stimuli() {
            info!(target: "ambe_lab::capture", name = %stim.name(), "capturing");
            corpus.add(stim).await?;
        }

        let manifest_path = corpus.write_manifest().await?;
        info!(
            target: "ambe_lab::capture",
            path = %manifest_path.display(),
            "corpus complete"
        );
        Ok(())
    }
}

/// Stimulus set for v1. Designed to be small but cover the main axes:
/// silence, varying tone frequencies, sweep across the speech band,
/// noise, impulse, and a dual-tone for multi-pitch behaviour.
fn default_stimuli() -> Vec<Stimulus> {
    vec![
        Stimulus::Silence {
            duration_seconds: 2.0,
        },
        Stimulus::SineTone {
            frequency_hz: 250.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
        Stimulus::SineTone {
            frequency_hz: 300.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
        Stimulus::SineTone {
            frequency_hz: 500.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
        Stimulus::SineTone {
            frequency_hz: 750.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
        Stimulus::SineTone {
            frequency_hz: 1000.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
        Stimulus::SineTone {
            frequency_hz: 1500.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
        Stimulus::SineTone {
            frequency_hz: 2000.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
        Stimulus::SineTone {
            frequency_hz: 2500.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
        Stimulus::SineTone {
            frequency_hz: 3000.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
        Stimulus::SineTone {
            frequency_hz: 1000.0,
            amplitude: 0.1,
            duration_seconds: 1.0,
        },
        Stimulus::SineTone {
            frequency_hz: 1000.0,
            amplitude: 0.3,
            duration_seconds: 1.0,
        },
        Stimulus::SineTone {
            frequency_hz: 1000.0,
            amplitude: 0.7,
            duration_seconds: 1.0,
        },
        Stimulus::SineTone {
            frequency_hz: 1000.0,
            amplitude: 0.9,
            duration_seconds: 1.0,
        },
        Stimulus::SineSweep {
            start_hz: 300.0,
            end_hz: 3400.0,
            amplitude: 0.5,
            duration_seconds: 3.0,
        },
        Stimulus::WhiteNoise {
            amplitude: 0.3,
            duration_seconds: 2.0,
            seed: 1,
        },
        Stimulus::WhiteNoise {
            amplitude: 0.3,
            duration_seconds: 2.0,
            seed: 2,
        },
        Stimulus::Impulse {
            amplitude: 0.8,
            position_seconds: 0.5,
            total_seconds: 1.0,
        },
        Stimulus::DualTone {
            f1_hz: 700.0,
            f2_hz: 1200.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
        // Harmonic-rich periodic signals — should reliably produce
        // voice-mode frames (unlike pure sines).
        Stimulus::Sawtooth {
            f0_hz: 150.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
        Stimulus::Sawtooth {
            f0_hz: 250.0,
            amplitude: 0.5,
            duration_seconds: 2.0,
        },
    ]
}
