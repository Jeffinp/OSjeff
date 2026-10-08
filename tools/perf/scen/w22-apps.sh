# Every dock app in the current appearance (Auto: by the clock) and then in the other one.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
shot_apps() { # <tag>
  local t=$1
  dock_icon files; click; sleep 0.9; shot "$t-files"
  dock_icon browser; click; sleep 0.9; shot "$t-browser"
  dock_icon editor; click; sleep 0.9; shot "$t-editor"
  dock_icon calc; click; sleep 0.9; shot "$t-calc"
  dock_icon viewer; click; sleep 0.9; shot "$t-viewer"
  dock_icon tasks; click; sleep 0.9; shot "$t-tasks"
  dock_icon monitor; click; sleep 0.9; shot "$t-monitor"
  dock_icon settings; click; sleep 0.9; shot "$t-settings"
}
shot_apps dark
goto 1100 14; click; sleep 0.6; goto 953 188; click; sleep 0.6
key esc; sleep 0.6; goto 700 400; sleep 0.3
shot light-desk
shot_apps light
finish
