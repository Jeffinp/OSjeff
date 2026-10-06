source "$(dirname "$0")/../lib.sh"
wait_first_frame
sleep 2
shot s1_idle
dock 666; click; sleep 3
dock 559; click; sleep 3
dock 829; click; sleep 3
dock 721; click; sleep 3
shot s2_four_windows
goto 700 300; mon "mouse_button 1"; sleep 0.1; mon "mouse_button 0"; sleep 0.5
goto 300 95; mon "mouse_button 1"; sleep 0.3
for i in 1 2 3 4 5 6 7 8 9 10; do move -8 8; sleep 0.15; done
mon "mouse_button 0"; sleep 1
shot s3_dragged
goto 900 500; rclick; sleep 1.5
shot s4_menu
click; sleep 1
dock 451; click; sleep 2
shot s5_start
click; sleep 1
goto 150 300; click; sleep 0.5
for k in h e l p ret; do key $k; sleep 0.3; done
sleep 1
shot s6_typed
finish
