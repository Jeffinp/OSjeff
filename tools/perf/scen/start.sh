source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_to -189
click; sleep 1
for i in $(seq 1 40); do move 0 -4; sleep 0.08; done
for i in $(seq 1 40); do move 0 4; sleep 0.08; done
sleep 1
finish
