# W13: probe -- open the start panel (apps list) and the Task Manager.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 451; click; sleep 1
shot start
key down; key down; sleep 0.5
shot start-scrolled
key esc; sleep 0.5
finish
