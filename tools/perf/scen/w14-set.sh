# W14: the settings app. Opens it from the start panel, then changes the wallpaper
# (a built-in gradient, then an image file on the disk), the accent colour, the
# clock format, the time zone and the date/time, and the keyboard layout (typing
# ç and accented letters with ABNT2), taking a shot after each.
#
# The image wallpaper needs a file on the filesystem disk. FS v2 files are at most
# 1024 bytes, so make a small PNG and inject it before running:
#   convert -size 128x72 gradient:'#1b2a6b-#f4a259' -fill '#fff3c4' \
#     -draw 'circle 88,42 88,54' -colors 16 PNG8:papel.png
#   cargo run -p kitsune_core --example fsinject -- fs-papel.img papel.png papel.png
#   FS_IMG=fs-papel.img tools/perf/run.sh <img> bios <outdir> 120 tools/perf/scen/w14-set.sh
# Coordinates are for the default window position on a 1280x720 screen (BIOS).
source "$(dirname "$0")/../lib.sh"
wait_first_frame
sleep 2
# QEMU's PS/2 mouse sends jumps above 255 px in several packets, one per later input
# event: wiggle a little afterwards so the whole move has been delivered.
far() { goto "$1" "$2"; for _ in 1 2 3 4 5 6; do move 1 0; move -1 0; done; sleep 0.5; }
typew() { local s=$1 i ch; for ((i = 0; i < ${#s}; i++)); do ch=${s:i:1}; [ "$ch" = "." ] && ch=dot; key "$ch"; sleep 0.15; done; }
START_APPS=10
start_row() { # index -> click the start-panel row of app `index`
  local sy=$(( DOCKY - 20 - 12 - (20 + (START_APPS + 2) * 38 + 12) ))
  goto 451 $(( sy + 10 + 38 * $1 + 19 ))
  click
}
dock 451; click; sleep 1
start_row 8; sleep 2           # settings (index 8 in Kind::ALL)
shot s1_default
far 582 179; click; sleep 2    # wallpaper chip 1: Aurora
shot s2_aurora
far 524 337; click; sleep 2    # accent swatch 1: violet
shot s3_accent
far 551 406; click; sleep 1.5  # clock: 12 h
shot s4_clock12
far 320 181; click; sleep 1    # sidebar: Hora e regiao
far 638 242; click; sleep 0.5  # time zone +30 min
far 638 242; click; sleep 0.5  # ... +30 min
far 698 316; click; sleep 0.5  # hour +1
far 498 414; click; sleep 1    # Ajustar: write the RTC
shot s5_time
far 320 219; click; sleep 1    # sidebar: Teclado
far 718 173; click; sleep 1    # ABNT2
key semicolon; key bracket_left; key a; key apostrophe; key a; key apostrophe; key o
key apostrophe; key spc; key bracket_left; key e; sleep 1.5
shot s6_keyboard
far 320 143; click; sleep 1    # sidebar: Aparencia
far 677 270; click; sleep 0.5  # the image path box
typew "papel.png"
key ret; sleep 3               # apply
shot s7_image
key esc; key esc; sleep 2      # leave the box, close the window
shot s8_image_desktop
finish
