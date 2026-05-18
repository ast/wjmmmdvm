#!/bin/bash

# Flash STM32 over UART using pinctrl with GPIO 20 (BOOT0) and GPIO 21 (RESET).
#
# Works around the stm32flash autoboot issue on newer Raspberry Pi models
# where GPIO toggling via /sys/class/gpio no longer works as expected.
# See: https://sourceforge.net/p/stm32flash/wiki/Hints/

set -euo pipefail

BINFILE="${1:-bin/mmdvm_f1.bin}"  # Default binary path

# Check for required tools
command -v stm32flash >/dev/null || { echo "stm32flash not found"; exit 1; }
command -v pinctrl >/dev/null || { echo "pinctrl not found"; exit 1; }

# Confirm binary exists
if [ ! -f "$BINFILE" ]; then
  echo "Error: Firmware binary '$BINFILE' not found."
  exit 1
fi

# Boot sequence: 20 (BOOT0), 21 (RESET)
echo "[INFO] Setting GPIO 20 (BOOT0) high, pulsing 21 (RESET)..."
pinctrl 20 op dh      # BOOT0 = High
pinctrl 21 op dl      # RESET = Low
sleep 0.1
pinctrl 21 op dh      # RESET = High
sleep 0.2

# Flash firmware
echo "[INFO] Flashing firmware to /dev/ttyAMA0..."
stm32flash -b 57600 -v -w "$BINFILE" -g 0x0 /dev/ttyAMA0
RESULT=$?

# Exit sequence
echo "[INFO] Clearing BOOT0 and RESET..."
sleep 0.5
pinctrl 20 op dl      # BOOT0 = Low
pinctrl 21 op dl      # RESET = Low
sleep 0.1
pinctrl 21 op dh      # RESET = High

if [ "$RESULT" -eq 0 ]; then
  echo "[SUCCESS] Flash complete."
else
  echo "[ERROR] Flashing failed with code $RESULT."
fi

exit $RESULT
