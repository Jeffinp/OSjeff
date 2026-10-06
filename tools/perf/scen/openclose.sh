source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_to 26
for r in 1 2 3 4; do click; sleep 2; key esc; sleep 2; done
finish
