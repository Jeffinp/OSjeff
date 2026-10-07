# W8: overlays leave no pixels behind. A short Alt+Tab (the focus must move to
# the other window once Alt is released) and a context menu dismissed by a click.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 559; click; sleep 2          # editor on top, shell behind
shot o0_before
mon "sendkey alt-tab 1200"; sleep 15
shot o1_after_alttab              # the shell is focused again, no panel left
goto 900 500; rclick; sleep 1.5
shot o2_menu
goto 1000 300; click; sleep 1.5
shot o3_menu_dismissed            # same as o1: no menu remnants
finish
