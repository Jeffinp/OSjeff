# The app bar and the menus (the window menu button): sweep the pointer across the bar (magnification), open a menu
# and move down it, open Quick Settings.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
goto 300 $DOCK_Y_CENTER
for i in $(seq 1 60); do move 10 0; sleep 0.05; done
for i in $(seq 1 60); do move -10 0; sleep 0.05; done
goto 534 96; click; sleep 0.5   # the terminal's title-bar menu button
for i in $(seq 1 12); do move 0 5; sleep 0.06; done
key esc; sleep 0.6
panel_item tray; click; sleep 1
key esc; sleep 1
finish
