#!/usr/bin/env bash
# Linux/macOS counterpart of run.ps1: build the image and boot it in QEMU.
#
#   tools/run.sh [bios|uefi] [-- extra qemu args]
#
# Opens a QEMU window (no hardware acceleration needed; pass `-- -accel kvm`
# on Linux to use KVM). QEMU_RNG=none leaves out the virtio-rng entropy device. The persistent filesystem disk lives in osjeff-fs.img.
# For headless runs (CI, screenshots) use tools/qemu-headless.sh instead.
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:-bios}
[ $# -gt 0 ] && shift
[ "${1:-}" = "--" ] && shift

cargo build --release -p os
img=$(find target/release/build -path "*/out/osjeff-$mode.img" -printf '%T@ %p\n' | sort -rn | head -1 | cut -d' ' -f2-)
[ -f "$img" ] || { echo "no $mode image found" >&2; exit 1; }
# 64 MiB sparse file (OJFS v3 needs >= 1 MiB). An existing 64 KiB disk from an older
# checkout still boots: the kernel logs "disk too small for OJFS v3" and stays on v2.
[ -f osjeff-fs.img ] || truncate -s "${FS_SIZE:-64M}" osjeff-fs.img

# Entropy device: QEMU_RNG=virtio (default) adds `-device virtio-rng-pci` so the kernel
# has a host entropy source (RNG: strong); QEMU_RNG=none leaves it out to exercise the
# timing-jitter path (QEMU's default CPU, qemu64, has no RDRAND/RDSEED either; pass
# `-- -cpu max` to add them). Skipped silently on a QEMU build without the device.
rng_dev=()
case "${QEMU_RNG:-virtio}" in
  virtio) if qemu-system-x86_64 -device help 2>&1 | grep -q 'name "virtio-rng-pci"'; then
            rng_dev=(-device virtio-rng-pci)
          fi ;;
  none)   ;;
  *) echo "unknown QEMU_RNG '${QEMU_RNG}' (use virtio or none)" >&2; exit 2 ;;
esac

# 256M: the kernel BSS is ~91 MiB and UEFI needs it in conventional RAM.
args=(-m 256M -drive "format=raw,file=$img"
      -drive "format=raw,file=osjeff-fs.img,if=ide,index=2"
      -netdev user,id=n0 -device ne2k_isa,netdev=n0,mac=52:54:00:12:34:56 "${rng_dev[@]}"
      -object "filter-dump,id=dump,netdev=n0,file=osjeff-net.pcap")
if [ "$mode" = uefi ]; then
  code=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
  vars=${OVMF_VARS:-/usr/share/OVMF/OVMF_VARS_4M.fd}
  [ -f osjeff-ovmf-vars.fd ] || cp "$vars" osjeff-ovmf-vars.fd
  args=(-drive "if=pflash,format=raw,readonly=on,file=$code"
        -drive "if=pflash,format=raw,file=osjeff-ovmf-vars.fd" "${args[@]}")
fi
exec qemu-system-x86_64 "${args[@]}" "$@"
