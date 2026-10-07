# W8: maximize / restore, double-click maximize, minimize + dock dot + restore,
# Alt+Tab, resize. Geometry: the shell opens at (70,80) 512x320, so its
# title-bar buttons sit at min x=508..526, max x=532..550, y=86..104 (they
# appear while the pointer is over the window). Long pointer jumps get a pause
# so the click lands where the screenshot says.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
far() { goto "$1" "$2"; for _ in 1 2 3 4 5 6; do move 1 0; move -1 0; done; sleep 0.7; }  # QEMU PS/2 sends >255 px jumps in 255 px packets, one per later input event: flush them
dock 559; click; sleep 2        # an editor, to have a second window

# hover the shell, then maximize it with the title-bar button
far 300 200; click; sleep 0.5
goto 541 95; sleep 0.5
shot w8_hover_buttons
click; sleep 1.5
shot w8_maximized
# restore with the same button (now at the work area's top right)
far 1221 91; click; sleep 1.5
shot w8_restored

# double-click the title bar maximizes, again restores
far 300 95; click; click; sleep 1.5
shot w8_dblclick_max
click; click; sleep 1.5

# minimize the shell: a dot appears under its dock icon
far 517 95; click; sleep 2
shot w8_minimized
# clicking the dock icon restores it
dock 507; click; sleep 2
shot w8_restored2

# Alt+Tab: hold it so the overlay is on screen when the picture is taken (QEMU
# delivers the held keys several seconds late, so take two pictures), then wait
# for the release: the previous window (the editor) gets the focus.
mon "sendkey alt-tab 8000"; sleep 6
shot w8_alttab
sleep 3
shot w8_alttab2
sleep 8
shot w8_alttab_done

# resize: drag the shell's bottom-right corner (it is at 70,80 512x320 again)
far 300 200; click; sleep 0.3
far 579 397
mon "mouse_button 1"; sleep 0.3
for i in 1 2 3 4 5 6 7 8; do move 20 10; sleep 0.2; done
mon "mouse_button 0"; sleep 1.5
shot w8_resized
finish
