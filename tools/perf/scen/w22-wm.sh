# Window chrome states: focus, Alt+Tab, zoom, minimise, many windows.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon editor; click; sleep 0.9
dock_icon calc; click; sleep 0.9
dock_icon files; click; sleep 0.9
key ctrl-n; sleep 0.9
mon "sendkey alt-tab 1800" & sleep 0.9; shot wm-alttab; wait; sleep 0.6
shot wm-many
key ctrl-m; sleep 1.0; shot wm-minimised
finish
