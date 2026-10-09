# i18n W32: every page of Ajustes in Portuguese, the switch in "Idioma e região", then the same
# pages in English. Run in UEFI (1280x800). Shots: set-pt-*, set-en-*.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
side() { goto 320 $(( 115 + 36 * $1 )); click; sleep 0.9; }
pages() { # <tag>
  local t=$1 i
  for i in 0 1 2 3 4 5 6 7 8 9; do side "$i"; shot "$t-s$i"; done
  # the long page, scrolled to the clock editor and to the end
  side 4; goto 700 500
  for r in $(seq 1 20); do mon "mouse_move 0 0 -1"; sleep 0.05; done; sleep 0.6; shot "$t-s4-scroll"
  # the wallpaper page: a message under the field (empty path: "Escolha uma imagem")
  side 1; goto 560 450; click; sleep 0.6; shot "$t-s1-msg"
  # the clock editor message
  side 4; goto 700 500
  for r in $(seq 1 20); do mon "mouse_move 0 0 -1"; sleep 0.05; done
  sleep 0.4
}
key ctrl-w; sleep 0.8
dock_icon settings; click; sleep 1.6
pages set-pt
side 5; goto 700 258; click; sleep 1.2     # English
pages set-en
finish
