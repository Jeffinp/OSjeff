source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_to 189
click; sleep 2.5
for r in 1 2 3 4 5 6 7 8; do key down; sleep 0.25; done
for r in 1 2 3 4 5 6 7 8; do key up; sleep 0.25; done
sleep 1
finish
