# The app bar and the menus: sweep the pointer across the bar (magnification), open a menu
# and move down it, open the Controls popover.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
goto 300 $DOCK_Y_CENTER
for i in $(seq 1 60); do move 10 0; sleep 0.05; done
for i in $(seq 1 60); do move -10 0; sleep 0.05; done
goto 160 14; click; sleep 0.5
for i in $(seq 1 12); do move 0 5; sleep 0.06; done
key esc; sleep 0.6
goto 1100 14; click; sleep 1
key esc; sleep 1
finish
