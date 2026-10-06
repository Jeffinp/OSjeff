source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 721; click; sleep 3
goto 600 120; click; sleep 0.5
for k in h t t p shift-semicolon slash slash 1 0 dot 0 dot 2 dot 2 shift-semicolon 8 0 0 0 slash ret; do key $k; sleep 0.25; done
sleep 12
shot b1_page
for r in 1 2 3 4 5 6 7 8; do key down; sleep 0.4; done
sleep 1
shot b2_scrolled
for r in 1 2 3 4 5 6 7 8; do key up; sleep 0.4; done
sleep 2
finish
