source "$(dirname "$0")/../lib.sh"
wait_first_frame
for i in $(seq 1 100); do move 4 2; sleep 0.04; move -4 -2; sleep 0.04; done; sleep 1
finish
