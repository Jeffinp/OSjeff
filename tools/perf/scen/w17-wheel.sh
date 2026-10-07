# Wheel over the file manager and over the (unfocused) terminal: no panic, no focus change.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 829; click; sleep 3
goto 600 300                      # over the Files window
for r in $(seq 1 15); do mon "mouse_move 0 0 -1"; sleep 0.1; done
for r in $(seq 1 15); do mon "mouse_move 0 0 1"; sleep 0.1; done
shot w17-files
goto 150 250                      # over the terminal behind it
for r in $(seq 1 10); do mon "mouse_move 0 0 -1"; sleep 0.1; done
shot w17-term
finish
