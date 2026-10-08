# First look at the new shell: desktop, app bar hover, Apps, Busca, menus, popovers, windows.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
shot look-desktop
dock_icon browser; sleep 0.6; shot look-dock-hover
dock_icon files; click; sleep 1.0; shot look-files
dock_icon calc; click; sleep 1.0; shot look-calc
dock_icon apps; click; sleep 1.0; shot look-apps
key esc; sleep 0.8
goto 40 14; click; sleep 0.6; shot look-sysmenu
key esc; sleep 0.5
goto 1090 14; click; sleep 0.6; shot look-control
key esc; sleep 0.5
finish
