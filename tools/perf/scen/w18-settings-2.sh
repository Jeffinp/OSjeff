# W18 proof (c), boot 2 on the disk image of w18-settings-1.sh: the image wallpaper,
# the violet accent, the 12 h clock, the time zone and the ABNT2 layout are all back
# without touching Settings.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
sleep 2
far() { goto "$1" "$2"; for _ in 1 2 3 4 5 6; do move 1 0; move -1 0; done; sleep 0.5; }
shot u1-desktop
dock 451; click; sleep 0.8; goto 440 $(( DOCKY - 529 + 38 * 8 )); click; sleep 2   # Configuracoes
shot u2-appearance
far 320 181; click; sleep 1; shot u3-time
far 320 219; click; sleep 1; shot u4-keyboard
finish
