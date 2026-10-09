# Cost of scrolling a long page in the browser (a client-area-only repaint: the page area is a
# dirty rectangle of the window's layer): the internal page osjeff://sobre, then 120 Down presses
# 0.08 s apart. Compare `steady` / `animdm` per-frame means in tools/perf/summ.py.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon browser; click; sleep 3
goto 600 100; click; sleep 0.4
key ctrl-a; typestr "osjeff://sobre"; key ret; sleep 3
goto 500 300; click; sleep 0.5
for i in $(seq 1 60); do key down; sleep 0.08; done
for i in $(seq 1 60); do key up; sleep 0.08; done
finish
