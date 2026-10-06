source "$(dirname "$0")/../lib.sh"
wait_first_frame
move -340 -265; sleep 0.5
mon "mouse_button 1"; sleep 0.2
for i in $(seq 1 60); do move 5 2; sleep 0.06; done
for i in $(seq 1 60); do move -5 -2; sleep 0.06; done
mon "mouse_button 0"; sleep 1
finish
