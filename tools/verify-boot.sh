#!/usr/bin/env bash
# Boot-verify the current tree in QEMU (BIOS and UEFI) and optionally compare the
# desktop screenshot with a baseline taken from an earlier build.
#
#   tools/verify-boot.sh <outdir>                 build, boot both, report
#   tools/verify-boot.sh <outdir> <baseline_dir>  ... and diff the screenshots
#
# A baseline is just a previous <outdir> (it holds bios/screen.png and
# uefi/screen.png). The HUD (top-right) and the clock pill (bottom-right) change
# every run, so they are masked before comparing; any other differing pixel is
# reported. Exit status is non-zero if a boot failed, panicked, or differs.
#
# Needs: qemu-system-x86_64, socat, ImageMagick (convert/compare), OVMF for UEFI.
set -u
cd "$(dirname "$0")/.."
out=${1:?usage: verify-boot.sh <outdir> [baseline_dir]}
base=${2:-}
mem=${QEMU_MEM:-256M}
status=0

cargo build --release -p os 2>&1 | tail -1

mask() { # in out
  local h; h=$(identify -format %h "$1")
  convert "$1" -fill black -draw "rectangle 1040,0 1280,90" \
    -draw "rectangle 1130,$((h - 62)) 1275,$((h - 5))" "$2"
}

for mode in bios uefi; do
  d="$out/$mode"
  QEMU_MEM=$mem tools/qemu-headless.sh "$mode" "$d" "$([ $mode = bios ] && echo 25 || echo 40)" >/dev/null 2>&1
  ok=$(grep -c 'TSC calibrated' "$d/serial.log" 2>/dev/null || true)
  bad=$(grep -ciE 'KERNEL PANIC|FATAL' "$d/serial.log" 2>/dev/null || true)
  line="$mode: boot_ok=$ok fatal=$bad"
  [ "${ok:-0}" -ge 1 ] && [ "${bad:-1}" -eq 0 ] || status=1
  if [ -n "$base" ] && [ -f "$base/$mode/screen.png" ] && [ -f "$d/screen.png" ]; then
    mask "$base/$mode/screen.png" "$d/.ma.png"; mask "$d/screen.png" "$d/.mb.png"
    ae=$(compare -metric AE "$d/.ma.png" "$d/.mb.png" null: 2>&1 || true)
    line="$line differing_pixels=$ae"
    [ "$ae" = 0 ] || status=1
    rm -f "$d/.ma.png" "$d/.mb.png"
  fi
  echo "$line"
done
exit $status
