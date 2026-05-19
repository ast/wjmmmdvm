# Workspace task runner. `just` with no args lists recipes.
#
# Two-terminal workflow:
#   Terminal 1:  just serve-ambed   (codec daemon on the Pi, foreground)
#   Terminal 2:  just listen-dmr    (DMR listener locally, foreground)

TARGET     := "armv7-unknown-linux-musleabihf"
HOST       := "mmdvm"
MD380TOOLS := env_var_or_default("MD380TOOLS", "$HOME/src/md380tools")

# listen-dmr defaults — override on the command line if needed.
BIND       := "0.0.0.0:62031"
CODEC_TCP  := HOST + ":2460"
OUTPUT_DIR := "./dmr-rec"

default:
    @just --list

# ─── firmware (one-time) ──────────────────────────────────────────────

# Copy MD-380 firmware + RAM core into md380_emu_ambed/firmware/ where
# include_bytes! expects them. Safe to re-run.
sync-firmware:
    @mkdir -p md380_emu_ambed/firmware
    cp -v {{MD380TOOLS}}/firmware/unwrapped/D002.032.img md380_emu_ambed/firmware/
    cp -v {{MD380TOOLS}}/cores/d02032-core.img         md380_emu_ambed/firmware/

# ─── md380-emu-ambed (codec daemon on the Pi) ─────────────────────────

# Cross-compile the codec daemon for armv7 (release).
build-ambed: sync-firmware
    cross build -p md380_emu_ambed --target {{TARGET}} --release

# Build + scp the codec binary to the Pi.
deploy-ambed: build-ambed
    scp target/{{TARGET}}/release/md380-emu-ambed {{HOST}}:~/

# Run the codec daemon on the Pi in the foreground. Ctrl-C to stop.
# Deploys first so it's always the current build.
serve-ambed: deploy-ambed
    ssh -t {{HOST}} 'RUST_LOG=md380_emu_ambed=info ./md380-emu-ambed serve --tcp 0.0.0.0:2460 --unix /tmp/md380.sock'

# ─── mmdvm_sip listen-dmr (local) ─────────────────────────────────────

# Run mmdvm_sip listen-dmr locally; voice bursts get decoded via the
# codec at {{CODEC_TCP}}. MMDVMHost on the Pi must have
# GatewayAddress=<this-workstation-IP>:62031 (see MMDVM-testing.ini).
listen-dmr:
    cargo build --release -p mmdvm_sip
    @mkdir -p {{OUTPUT_DIR}}
    RUST_LOG=mmdvm_sip=info \
      target/release/mmdvm_sip listen-dmr \
        --bind {{BIND}} \
        --codec-tcp {{CODEC_TCP}} \
        --output-dir {{OUTPUT_DIR}}

# ─── file-based smoke tests (encode/decode via ssh) ───────────────────

# Encode a PCM file via the codec on the Pi.
# Usage: just test-encode local.pcm remote.ambe
test-encode pcm ambe: deploy-ambed
    scp {{pcm}} {{HOST}}:/tmp/in.pcm
    ssh {{HOST}} './md380-emu-ambed encode /tmp/in.pcm /tmp/out.ambe'
    scp {{HOST}}:/tmp/out.ambe {{ambe}}

# Decode an AMBE file via the codec on the Pi.
# Usage: just test-decode remote.ambe local.pcm
test-decode ambe pcm: deploy-ambed
    scp {{ambe}} {{HOST}}:/tmp/in.ambe
    ssh {{HOST}} './md380-emu-ambed decode /tmp/in.ambe /tmp/out.pcm'
    scp {{HOST}}:/tmp/out.pcm {{pcm}}

# Round-trip a PCM through encode + decode on the Pi.
# Usage: just test-roundtrip local-in.pcm local-out.pcm
test-roundtrip pcm-in pcm-out: deploy-ambed
    scp {{pcm-in}} {{HOST}}:/tmp/in.pcm
    ssh {{HOST}} './md380-emu-ambed encode /tmp/in.pcm /tmp/mid.ambe && \
                   ./md380-emu-ambed decode /tmp/mid.ambe /tmp/out.pcm'
    scp {{HOST}}:/tmp/out.pcm {{pcm-out}}
