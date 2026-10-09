# Ajustes: every section, in the current appearance and then in the other one, plus the
# interactions that change something (theme, accent, wallpaper, bar zoom, layout, zone search).
source "$(dirname "$0")/../lib.sh"
wait_first_frame
side() { goto 320 $(( 144 + 36 * $1 )); click; sleep 0.9; }
sections() { # <tag>
  local t=$1
  for i in 0 1 2 3 4 5 6 7 8; do side $i; shot "$t-s$i"; done
}
dock_icon settings; click; sleep 1.6
sections a
side 0
goto 943 238; click; sleep 0.8; shot ia-dark
goto 818 342; click; sleep 0.8; shot ia-accent
goto 747 238; click; sleep 0.6
goto 1100 14; click; sleep 0.6; goto 953 188; click; sleep 0.6
goto 640 20; click; sleep 0.6
side 1; goto 891 236; click; sleep 1.0; shot ib-wall
side 2; goto 700 238; click; sleep 0.8; shot ib-dock
side 3; goto 700 286; click; sleep 0.8; goto 600 386; click; typestr "'a~a"; sleep 0.6; shot ib-kbd
side 4; goto 600 478; click; typestr toq; sleep 0.8; shot ib-tz
sections b
finish
