# The Apps overlay: open it from the app bar, sweep the pointer over the grid, close it.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon apps
click; sleep 1
goto 200 200
for i in $(seq 1 40); do move 12 0; sleep 0.06; done
for i in $(seq 1 40); do move -12 0; sleep 0.06; done
key esc; sleep 1
finish
