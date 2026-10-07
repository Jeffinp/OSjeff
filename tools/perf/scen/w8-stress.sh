# W8: 30+ windows without a panic. Ctrl+N 30 times (the table caps at 32, so
# the last opens are refused), the Task Manager lists them all (and scrolls),
# then DEL ends every instance.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
goto 300 200; click; sleep 0.5
for i in $(seq 1 30); do key ctrl-n; sleep 0.5; done
sleep 1
shot w8_many
dock 613; click; sleep 2
shot w8_many_taskmgr
for i in 1 2; do key down; sleep 0.25; done
for i in $(seq 1 31); do key delete; sleep 0.5; done
sleep 2
shot w8_many_closed
finish
