#!/usr/bin/env bash
# Boot the release image on every emulated video adapter QEMU offers, in BIOS (VBE) and UEFI (GOP),
# and report which ones reach the desktop. Kitsune draws on the framebuffer the firmware hands over,
# so the same code has to cope with each adapter's pixel format, stride and resolution.
#
#   tools/gpu-matrix.sh <outdir> [bios-img uefi-img]     (default: newest images under target/release)
#
# Per case it writes <outdir>/<mode>-<name>/{serial.log,screen.png}; the verdict needs
# "TSC calibrated" in the serial log, no panic, and a screenshot that is not blank.
# Exit status is non-zero if any case fails. Needs qemu, socat, ImageMagick and OVMF.
set -u
cd "$(dirname "$0")/.."
out=${1:?usage: gpu-matrix.sh <outdir> [bios-img uefi-img]}
bios=${2:-$(ls -t target/release/build/os/*/out/kitsune-bios.img | head -1)}
uefi=${3:-$(ls -t target/release/build/os/*/out/kitsune-uefi.img | head -1)}
secs=${SECS:-40}
# name|modes|qemu args
cases=(
  "std|bios uefi|-vga std"
  "virtio-vga|bios uefi|-vga virtio"
  "virtio-gpu|uefi|-vga none -device virtio-gpu-pci"
  "vmware|bios uefi|-vga vmware"
  "qxl|bios uefi|-vga qxl"
  "cirrus|bios|-vga cirrus"
  "bochs-display|uefi|-vga none -device bochs-display"
  "ramfb|uefi|-vga none -device ramfb"
)
status=0
printf '%-28s %-6s %-8s %-10s %s\n' case boot panic size note
for c in "${cases[@]}"; do
  IFS='|' read -r name modes extra <<<"$c"
  for mode in $modes; do
    img=$bios; [ "$mode" = uefi ] && img=$uefi
    d="$out/$mode-$name"
    QEMU_EXTRA="$extra" tools/perf/run.sh "$img" "$mode" "$d" "$secs" >/dev/null 2>&1
    ok=$(grep -c 'TSC calibrated' "$d/serial.log" 2>/dev/null); bad=$(grep -ciE 'KERNEL PANIC|FATAL' "$d/serial.log" 2>/dev/null)
    size=$(identify -format '%wx%h' "$d/screen.png" 2>/dev/null || echo -)
    sd=$(convert "$d/screen.png" -format '%[fx:standard_deviation]' info: 2>/dev/null || echo 0)
    note=$(grep -m1 -iE 'framebuffer|gpu|fb:' "$d/serial.log" | cut -c1-70)
    verdict=ok
    if [ "${ok:-0}" -lt 1 ] || [ "${bad:-1}" -ne 0 ] || [ "$(echo "${sd:-0} < 0.02" | bc -l)" = 1 ]; then verdict=FAIL; status=1; fi
    printf '%-28s %-6s %-8s %-10s %s\n' "$mode-$name" "$verdict" "${bad:-?}" "$size" "$note"
  done
done
exit $status
