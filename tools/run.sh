#!/usr/bin/env bash
# Linux/macOS counterpart of run.ps1: build the image and boot it in QEMU.
#
#   tools/run.sh [bios|uefi] [-- extra qemu args]
#
# Opens a QEMU window (no hardware acceleration needed; pass `-- -accel kvm`
# on Linux to use KVM). The persistent filesystem disk lives in osjeff-fs.img.
# For headless runs (CI, screenshots) use tools/qemu-headless.sh instead.
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:-bios}
[ $# -gt 0 ] && shift
[ "${1:-}" = "--" ] && shift

cargo build --release -p os
img=$(find target/release/build -path "*/out/osjeff-$mode.img" -printf '%T@ %p\n' | sort -rn | head -1 | cut -d' ' -f2-)
[ -f "$img" ] || { echo "no $mode image found" >&2; exit 1; }
[ -f osjeff-fs.img ] || dd if=/dev/zero of=osjeff-fs.img bs=1k count=64 status=none

# 256M: the kernel BSS is ~91 MiB and UEFI needs it in conventional RAM.
args=(-m 256M -drive "format=raw,file=$img"
      -drive "format=raw,file=osjeff-fs.img,if=ide,index=2"
      -netdev user,id=n0 -device ne2k_isa,netdev=n0,mac=52:54:00:12:34:56
      -object "filter-dump,id=dump,netdev=n0,file=osjeff-net.pcap")
if [ "$mode" = uefi ]; then
  code=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
  vars=${OVMF_VARS:-/usr/share/OVMF/OVMF_VARS_4M.fd}
  [ -f osjeff-ovmf-vars.fd ] || cp "$vars" osjeff-ovmf-vars.fd
  args=(-drive "if=pflash,format=raw,readonly=on,file=$code"
        -drive "if=pflash,format=raw,file=osjeff-ovmf-vars.fd" "${args[@]}")
fi
exec qemu-system-x86_64 "${args[@]}" "$@"
