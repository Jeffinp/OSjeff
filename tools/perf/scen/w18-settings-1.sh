# W18 proof (c), boot 1: settings that must survive a reboot. Prepare the disk once:
#   convert -size 128x72 gradient:'#1b2a6b-#f4a259' -fill '#fff3c4' \
#     -draw 'circle 88,42 88,54' -colors 16 PNG8:papel.png
#   truncate -s 64M fs.img
#   cargo run --release -p osjeff_core --example fs3_inject -- fs.img papel.png /papel.png
#   FS_IMG=fs.img tools/perf/run.sh <img> bios <out1> 120 tools/perf/scen/w18-settings-1.sh
# It sets the image wallpaper (/papel.png), the violet accent, the 12 h clock, a time
# zone change and the ABNT2 keyboard, then saves the system log ("Salvar", which writes
# /var/log/syslog.txt). Boot 2 (w18-settings-2.sh, FS_IMG=<out1>/fs.img) shows them all
# back, and /var/log/boot.log is on the disk.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
sleep 2
far() { goto "$1" "$2"; for _ in 1 2 3 4 5 6; do move 1 0; move -1 0; done; sleep 0.5; }
typew() { local s=$1 i ch; for ((i = 0; i < ${#s}; i++)); do ch=${s:i:1}; [ "$ch" = "." ] && ch=dot; [ "$ch" = "/" ] && ch=slash; key "$ch"; sleep 0.15; done; }
dock 451; click; sleep 0.8; goto 440 $(( DOCKY - 529 + 38 * 8 )); click; sleep 2   # Configuracoes
far 524 337; click; sleep 1.5          # accent swatch 1: violet
far 551 406; click; sleep 1.5          # clock: 12 h
far 677 270; click; sleep 0.5          # the image path box
typew "/papel.png"
key ret; sleep 3                       # apply: the image is the wallpaper
shot t1-image
far 320 181; click; sleep 1            # sidebar: Hora e regiao
far 638 242; click; sleep 0.5          # time zone +30 min
far 638 242; click; sleep 0.5          # ... +30 min
shot t2-tz
far 320 219; click; sleep 1            # sidebar: Teclado
far 718 173; click; sleep 1            # ABNT2
shot t3-keyboard
key esc; sleep 1; key esc; sleep 1     # leave the box, close the window
shot t4-desktop
# the system log: Salvar writes /var/log/syslog.txt
dock 451; click; sleep 0.8; goto 440 $(( DOCKY - 529 + 38 * 9 )); click; sleep 2   # Log do sistema
shot t5-log
goto 985 162; click; sleep 1.5          # Salvar
shot t6-saved
finish
