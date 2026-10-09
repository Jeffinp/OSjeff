# i18n W32: the Apps overlay with the localised app names, two app windows, the component gallery
# and the Files "Apps" place, in Portuguese and then in English (switched live in Ajustes).
# UEFI (1280x800). Shots: apps-pt-*, apps-en-*.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
open_app() { key ctrl-spc; sleep 0.5; typestr "$1"; sleep 0.6; key ret; sleep 2.2; }
views() { # <tag>
  local t=$1
  dock_icon apps; click; sleep 1.2; goto 900 700; shot "$t-launcher"; key esc; sleep 0.7
  key ctrl-spc; sleep 0.5; typestr "re"; sleep 0.6; shot "$t-search"; key esc; sleep 0.5
  open_app "cl"; shot "$t-clock"
  open_app "no"; shot "$t-notes"
  key ctrl-w; sleep 0.8                       # close Notes again (its status line is written at start)
  dock_icon files; click; sleep 1.6; goto 277 282; click; sleep 1.2; shot "$t-files-apps"
  key ctrl-w; sleep 0.8
  key ctrl-alt-g; sleep 1.4; shot "$t-gallery"
  goto 324 132; click; sleep 0.7; shot "$t-gallery-type"
  goto 420 132; click; sleep 0.7; shot "$t-gallery-colors"
  goto 518 132; click; sleep 0.7; shot "$t-gallery-icons"
  goto 612 132; click; sleep 0.7; shot "$t-gallery-shell"
  key ctrl-w; sleep 0.8
}
key ctrl-w; sleep 0.8
views apps-pt
dock_icon settings; click; sleep 1.4
goto 320 295; click; sleep 0.9
goto 700 258; click; sleep 1.2             # English (the clock window is still open: its title follows)
key ctrl-w; sleep 0.8
shot apps-en-clock-retitled
views apps-en
finish
