#!/usr/bin/env bash
# Headless QEMU harness for CI / cloud sessions (no display, no KVM needed).
#
#   tools/qemu-headless.sh <bios|uefi> <outdir> [wait_seconds] [-- extra qemu args]
#
# Boots the image built by `cargo build --release -p os`, captures COM1 to
# <outdir>/serial.log, and after <wait_seconds> (default 20) dumps the screen to
# <outdir>/screen.png via the QEMU monitor. A packet capture of the NIC goes to
# <outdir>/net.pcap. Exits 0 if the screenshot was taken.
#
# Extra monitor commands (sendkey, mouse_move, ...) can be sent while it runs
# through the monitor socket, whose path is printed at start and is also
# available as <outdir>/mon.path (the socket itself lives under /tmp because a
# Unix socket path is limited to 107 bytes and <outdir> may be longer), e.g.:
#   echo "sendkey a" | socat - UNIX-CONNECT:$(cat <outdir>/mon.path)
set -euo pipefail

mode=${1:?usage: qemu-headless.sh <bios|uefi> <outdir> [wait_seconds]}
out=${2:?outdir}
wait_s=${3:-20}
shift 3 2>/dev/null || shift $#
[ "${1:-}" = "--" ] && shift

root=$(cd "$(dirname "$0")/.." && pwd)
img=$(find "$root/target/release/build" -path "*/out/osjeff-$mode.img" -printf '%T@ %p\n' 2>/dev/null \
        | sort -rn | head -1 | cut -d" " -f2-)
[ -f "$img" ] || { echo "image not found; run: cargo build --release -p os" >&2; exit 2; }

mkdir -p "$out"
rm -f "$out"/serial.log "$out"/mon.sock "$out"/mon.path "$out"/screen.ppm "$out"/screen.png
sock=$(mktemp -u "${TMPDIR:-/tmp}/osjeff-mon.XXXXXX")
echo "$sock" > "$out/mon.path"
# Fresh persistent-FS disk each run so results are reproducible (KEEP_FS=1 keeps
# an existing <outdir>/fs.img, e.g. to test persistence or corrupted disks).
fsimg="$out/fs.img"
if [ -z "${KEEP_FS:-}" ] || [ ! -f "$fsimg" ]; then
  dd if=/dev/zero of="$fsimg" bs=1k count=64 status=none
fi

args=(-m "${QEMU_MEM:-128M}" -display none -no-reboot -no-shutdown
      -serial "file:$out/serial.log"
      -monitor "unix:$sock,server,nowait"
      -drive "format=raw,file=$img"
      -drive "format=raw,file=$fsimg,if=ide,index=2"
      -netdev user,id=n0 -device ne2k_isa,netdev=n0,mac=52:54:00:12:34:56
      -object "filter-dump,id=dump,netdev=n0,file=$out/net.pcap")
if [ "$mode" = uefi ]; then
  # Ubuntu's OVMF "4M" build is split into CODE + VARS and must be loaded as
  # pflash; give each run a private copy of VARS so runs stay independent.
  code=${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
  vars=${OVMF_VARS:-/usr/share/OVMF/OVMF_VARS_4M.fd}
  cp "$vars" "$out/ovmf_vars.fd"
  args=(-drive "if=pflash,format=raw,readonly=on,file=$code"
        -drive "if=pflash,format=raw,file=$out/ovmf_vars.fd" "${args[@]}")
fi

qemu-system-x86_64 "${args[@]}" "$@" &
qpid=$!
trap 'kill $qpid 2>/dev/null || true; rm -f "$sock"' EXIT

sleep "$wait_s"
echo "screendump $out/screen.ppm" | socat - "UNIX-CONNECT:$sock" >/dev/null
sleep 1
if command -v convert >/dev/null; then convert "$out/screen.ppm" "$out/screen.png"; fi
echo "image : $img ($(stat -c %s "$img") bytes)"
echo "serial: $out/serial.log ($(wc -l < "$out/serial.log") lines)"
echo "screen: $out/screen.png"
echo "quit" | socat - "UNIX-CONNECT:$sock" >/dev/null 2>&1 || true
