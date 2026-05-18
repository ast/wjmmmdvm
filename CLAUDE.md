# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this repo is

Personal notes, host config, and a flashing script for an MMDVM HS Hat hotspot
(callsign SM6WJM, DMR ID 2406237) running on a Raspberry Pi 4 reachable as
`ssh mmdvm`. There is no build system, no tests, no application code — just
documentation and artifacts that get deployed to the Pi.

The canonical write-up is [README.org](README.org); keep it in sync when
anything in `MMDVM.ini` or `scripts/` changes meaningfully.

## Hard constraints

- **Brandmeister password in `MMDVM.ini` must stay redacted as `Password=CHANGEME`.** The real value lives only on the Pi. Do not substitute it back in, even if the user pastes it into chat.
- **Firmware cannot be flashed from a pre-compiled PiStar binary.** The HS Hat has a 12.288 MHz TCXO (not the more common 14.7456 MHz), so the firmware must be built from source with `configs/MMDVM_HS_Hat-12mhz.h` as `Config.h`. See README.org §Firmware build.
- **`scripts/flash.sh` runs on the Pi, not on the workstation.** It depends on `stm32flash` and `pinctrl`, and drives GPIO 20 (BOOT0) / GPIO 21 (RESET) directly — the manual pin sequence is the whole point of the script (works around the broken `stm32flash` autoboot on newer Pis).

## Editing notes

- Documentation is in Org mode (`.org`), not Markdown. The user's wider notes ecosystem is Emacs/Org.
- `MMDVM.ini` is `MMDVMHost`'s config format (G4KLX); section names and keys are upstream-defined — don't rename them.
