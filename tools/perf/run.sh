#!/usr/bin/env bash
# run.sh <img> <bios|uefi> <outdir> <maxsecs> [scenario-script]
#
# Headless QEMU run of a built image (`cargo build --release -p os [--features
# perf-trace]`; images are under target/release/build/os/*/out/osjeff-*.img).
# Serial goes to <outdir>/serial.log. The optional scenario script
# (tools/perf/scen/*.sh; gets $OUT and $MODE) drives the QEMU monitor (mouse/keys)
# in the background and touches $OUT/done when finished; then we screendump to
# <outdir>/screen.png and quit.
# Env: QEMU_MEM (default 128M, 512M for uefi), QEMU_EXTRA (e.g. "-icount shift=0").
set -u
img=$1; mode=$2; out=$3; secs=$4; scen=${5:-}
rm -rf "$out"; mkdir -p "$out"
# Unix socket paths are limited to 107 bytes, so keep the monitor socket short.
export MON_SOCK=$(mktemp -u /tmp/osj-mon.XXXXXX)
echo "$MON_SOCK" > "$out/mon.path"
dd if=/dev/zero of="$out/fs.img" bs=1k count=64 status=none
mem=${QEMU_MEM:-128M}; [ "$mode" = uefi ] && mem=${QEMU_MEM:-512M}
args=(-m "$mem" -display none -no-reboot -no-shutdown
  -serial "file:$out/serial.log"
  -monitor "unix:$MON_SOCK,server,nowait"
  -drive "format=raw,file=$img,file.locking=off"
  -drive "format=raw,file=$out/fs.img,if=ide,index=2"
  -netdev user,id=n0 -device ne2k_isa,netdev=n0,mac=52:54:00:12:34:56)
if [ "$mode" = uefi ]; then
  cp /usr/share/OVMF/OVMF_VARS_4M.fd "$out/ovmf_vars.fd"
  args=(-drive "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd"
        -drive "if=pflash,format=raw,file=$out/ovmf_vars.fd" "${args[@]}")
fi
qemu-system-x86_64 "${args[@]}" ${QEMU_EXTRA:-} &
qpid=$!
trap 'kill $qpid 2>/dev/null; rm -f "$MON_SOCK"' EXIT
if [ -n "$scen" ]; then (OUT=$out MODE=$mode bash "$scen") & fi
start=$(date +%s)
while [ ! -f "$out/done" ] && [ $(( $(date +%s) - start )) -lt "$secs" ]; do sleep 0.5; done
if [ -z "$scen" ]; then sleep "$secs"; fi
echo "screendump $out/screen.ppm" | socat - "UNIX-CONNECT:$MON_SOCK" >/dev/null
sleep 1
convert "$out/screen.ppm" "$out/screen.png" 2>/dev/null
rm -f "$out/screen.ppm"
echo quit | socat - "UNIX-CONNECT:$MON_SOCK" >/dev/null 2>&1
wait $qpid 2>/dev/null
