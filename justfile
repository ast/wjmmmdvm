# Workspace task runner. Run `just` with no args to list recipes.

# Defaults — override on the command line, e.g. `just deploy-ambed HOST=mmdvm2`.
TARGET := "armv7-unknown-linux-musleabihf"
HOST   := "mmdvm"
MD380TOOLS := env_var_or_default("MD380TOOLS", "$HOME/src/md380tools")

# Show the recipe list.
default:
    @just --list

# Copy MD-380 firmware + RAM core into md380_emu_ambed/firmware/ where
# include_bytes! expects them. Idempotent; safe to re-run.
sync-firmware:
    @mkdir -p md380_emu_ambed/firmware
    cp -v {{MD380TOOLS}}/firmware/unwrapped/D002.032.img md380_emu_ambed/firmware/
    cp -v {{MD380TOOLS}}/cores/d02032-core.img         md380_emu_ambed/firmware/

# Debug-mode cross build for armv7. Requires `cross` installed:
#   cargo install cross --git https://github.com/cross-rs/cross
build-ambed: sync-firmware
    cross build -p md380_emu_ambed --target {{TARGET}}

# Release-mode cross build (smaller, faster binary).
build-ambed-release: sync-firmware
    cross build -p md380_emu_ambed --target {{TARGET}} --release

# Copy the built debug binary to the Pi.
deploy-ambed: build-ambed
    scp target/{{TARGET}}/debug/md380-emu-ambed {{HOST}}:~/

# Copy the release binary to the Pi.
deploy-ambed-release: build-ambed-release
    scp target/{{TARGET}}/release/md380-emu-ambed {{HOST}}:~/

# Smoke test: encode a PCM file on the Pi via the deployed binary.
# Usage: just test-encode local.pcm remote.ambe
test-encode pcm ambe: deploy-ambed
    scp {{pcm}} {{HOST}}:/tmp/in.pcm
    ssh {{HOST}} './md380-emu-ambed encode /tmp/in.pcm /tmp/out.ambe'
    scp {{HOST}}:/tmp/out.ambe {{ambe}}

# Smoke test: decode an AMBE file on the Pi.
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

# Tail the daemon's logs on the Pi (once we have a `serve` subcommand).
# Placeholder for now.
logs:
    ssh {{HOST}} 'journalctl --user -u md380-emu-ambed -f' || true

# Local mmdvm_sip listen-dmr that decodes audio via the codec daemon
# running on the Pi. MMDVMHost on the Pi should be configured with
# GatewayAddress=<this-workstation-IP>:62031 (see MMDVM-testing.ini).
BIND       := "0.0.0.0:62031"
CODEC_TCP  := HOST + ":2460"
OUTPUT_DIR := "./dmr-rec"

listen-dmr:
    cargo build --release -p mmdvm_sip
    @mkdir -p {{OUTPUT_DIR}}
    RUST_LOG=mmdvm_sip=info \
      target/release/mmdvm_sip listen-dmr \
        --bind {{BIND}} \
        --codec-tcp {{CODEC_TCP}} \
        --output-dir {{OUTPUT_DIR}}
