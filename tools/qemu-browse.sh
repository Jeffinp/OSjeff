#!/usr/bin/env bash
# Drive the OSjeff browser in headless QEMU: open it from the dock, type an
# address, wait, and take screenshots. Uses the monitor socket of
# tools/qemu-headless.sh (no display needed).
#
#   tools/qemu-browse.sh <outdir> <url> [load_wait_s] [shot_name] [-- more actions]
#
# Environment: MODE (bios|uefi, default bios), QEMU_NIC / QEMU_NETDEV / QEMU_MEM as
# in qemu-headless.sh, BOOT_WAIT (default 12 s before the first click), CLICK_AFTER
# "x,y" (a second click inside the page after the load, e.g. a button), CLICK_WAIT,
# KEY_AFTER (a QEMU key name such as alt-left, sent after the click) and KEY_WAIT.
# Writes <outdir>/<shot_name>.png (default page.png), serial.log, and
# <outdir>/boot.png (desktop before opening the browser).
set -euo pipefail
cd "$(dirname "$0")/.."
out=${1:?usage: qemu-browse.sh <outdir> <url> [load_wait_s] [shot_name]}
url=${2:?url}
load_wait=${3:-40}
shot=${4:-page}
mode=${MODE:-bios}
boot_wait=${BOOT_WAIT:-12}

mkdir -p "$out"
total=$((boot_wait + load_wait + ${CLICK_WAIT:-0} + 40))
tools/qemu-headless.sh "$mode" "$out" "$total" -- ${EXTRA_QEMU:-} >/dev/null 2>&1 &
hpid=$!
for _ in $(seq 1 50); do [ -s "$out/mon.path" ] && break; sleep 0.2; done
sock=$(cat "$out/mon.path")
for _ in $(seq 1 50); do [ -S "$sock" ] && break; sleep 0.2; done

mon() { echo "$@" | socat - "UNIX-CONNECT:$sock" >/dev/null; }
shot_to() { mon "screendump $out/$1.ppm"; sleep 1; convert "$out/$1.ppm" "$out/$1.png"; rm -f "$out/$1.ppm"; }
keyname() { # char -> qemu key name
  case "$1" in
    .) echo dot ;; /) echo slash ;; :) echo shift-semicolon ;; -) echo minus ;;
    '?') echo shift-slash ;; =) echo equal ;; _) echo shift-minus ;; '&') echo shift-7 ;;
    %) echo shift-5 ;; '#') echo shift-3 ;; ' ') echo spc ;;
    [A-Z]) echo "shift-$(echo "$1" | tr A-Z a-z)" ;;
    *) echo "$1" ;;
  esac
}
type_text() {
  local s=$1 i
  for ((i = 0; i < ${#s}; i++)); do mon "sendkey $(keyname "${s:i:1}")"; sleep 0.08; done
}
move_to() { # absolute move from the tracked pointer position, in steps of <= 100
  local tx=$1 ty=$2 sx sy
  while [ "$px" -ne "$tx" ] || [ "$py" -ne "$ty" ]; do
    sx=$((tx - px)); sy=$((ty - py))
    [ "$sx" -gt 100 ] && sx=100; [ "$sx" -lt -100 ] && sx=-100
    [ "$sy" -gt 100 ] && sy=100; [ "$sy" -lt -100 ] && sy=-100
    mon "mouse_move $sx $sy"; px=$((px + sx)); py=$((py + sy)); sleep 0.05
  done
}
click() { mon "mouse_button 1"; sleep 0.15; mon "mouse_button 0"; sleep 0.3; }

sleep "$boot_wait"
shot_to boot
# The pointer is clamped at the screen corner by repeated moves, then walked.
for _ in $(seq 1 14); do mon "mouse_move -100 -100"; sleep 0.05; done
px=0; py=0
move_to 720 671
click
sleep 3
# The address bar is focused in a new browser window.
type_text "$url"
mon "sendkey ret"
sleep "$load_wait"
shot_to "$shot"
if [ -n "${CLICK_AFTER:-}" ]; then
  move_to "${CLICK_AFTER%,*}" "${CLICK_AFTER#*,}"
  click
  sleep "${CLICK_WAIT:-20}"
  shot_to "${shot}-after"
fi
if [ -n "${KEY_AFTER:-}" ]; then
  mon "sendkey $KEY_AFTER"
  sleep "${KEY_WAIT:-10}"
  shot_to "${shot}-key"
fi
mon "quit" || true
wait "$hpid" 2>/dev/null || true
echo "screens: $out/boot.png $out/$shot.png"
echo "serial : $out/serial.log"
