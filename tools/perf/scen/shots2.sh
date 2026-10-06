source "$(dirname "$0")/../lib.sh"
wait_first_frame
sleep 2
goto 300 95; mon "mouse_button 1"; sleep 0.3
# move terminal so its right edge sits at x~1135 (shadow reaches the clock pill, rect does not)
for i in $(seq 1 29); do move 19 10; sleep 0.12; done
mon "mouse_button 0"; sleep 3
shot t1_shadow_touch
# now cover the pill with the window body
mon "mouse_button 1"; sleep 0.3
for i in $(seq 1 10); do move 10 0; sleep 0.12; done
mon "mouse_button 0"; sleep 3
shot t2_covered
# and move it away again
mon "mouse_button 1"; sleep 0.3
for i in $(seq 1 30); do move -20 -10; sleep 0.12; done
mon "mouse_button 0"; sleep 3
shot t3_away
finish
