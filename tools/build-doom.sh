#!/usr/bin/env bash
# Build DOOM (doomgeneric) to wasm32-wasi for OSjeff's WASM app engine.
#
# Prereqs:
#   - wasi-sdk installed (clang for wasm32-wasi). Set WASI_SDK_PATH.
#   - doomgeneric source cloned under wasm-apps/doom/doomgeneric (this script
#     clones it if missing). doomgeneric is GPLv2 — not vendored in this repo.
#
# Output: wasm-apps/doom/doom.wasm — a DOOM build that talks to the OSjeff host
# ABI (host.blit / host.time_ms) via wasm-apps/doom/doomgeneric_osjeff.c.
#
# NOTE: this wasm imports a WASI subset (wasi_snapshot_preview1) that the OSjeff
# engine must provide to RUN it — discovered import set:
#   args_get args_sizes_get environ_get environ_sizes_get clock_time_get
#   random_get proc_exit poll_oneoff
#   fd_write fd_read fd_seek fd_tell fd_sync fd_close fd_fdstat_get
#   fd_fdstat_set_flags fd_prestat_get fd_prestat_dir_name
#   path_open path_filestat_get
# The fd_*/path_* ops back DOOM's WAD file access; the rest are tiny stubs.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
doom="$here/wasm-apps/doom"
dg="$doom/doomgeneric/doomgeneric"
: "${WASI_SDK_PATH:?set WASI_SDK_PATH to your wasi-sdk install}"
CC="$WASI_SDK_PATH/bin/clang"

if [ ! -d "$doom/doomgeneric" ]; then
  git clone --depth 1 https://github.com/ozkl/doomgeneric.git "$doom/doomgeneric"
fi

# Core DOOM sources only: drop every platform backend (each has its own main())
# and the SDL/allegro sound/music backends (we run without sound).
srcs=$(ls "$dg"/*.c | grep -vE \
  "doomgeneric_(allegro|emscripten|linuxvt|sdl|soso|sosox|win|xlib)\.c|mus2mid\.c|i_(sdlsound|sdlmusic|allegrosound|allegromusic)\.c")

"$CC" --target=wasm32-wasip1 -mexec-model=reactor -O2 \
  -DDOOMGENERIC_RESX=320 -DDOOMGENERIC_RESY=200 \
  -I"$dg" \
  -Wl,--no-entry -Wl,--export=render -Wl,--export=on_key \
  -Wl,--export-memory -Wl,--allow-undefined \
  -o "$doom/doom.wasm" "$doom/doomgeneric_osjeff.c" $srcs

echo "built $doom/doom.wasm ($(stat -c%s "$doom/doom.wasm") bytes)"
