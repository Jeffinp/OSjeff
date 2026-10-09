# Tarefas: search, selection, the confirmation sheet, keyboard, hostile input (rapid tab
# switching, shrinking the window to its minimum).
source "$(dirname "$0")/../lib.sh"
wait_first_frame
TABY=110
dock_icon tasks; click; sleep 1.5
goto 620 $TABY; click; sleep 1.0
goto 792 $TABY; click; typestr aplic; sleep 0.6; shot t2-search
key ret; sleep 0.3
goto 400 178; click; sleep 0.6; shot t2-selected
key delete; sleep 0.8; shot t2-confirm
key esc; sleep 0.5; shot t2-cancelled
# sort by the name column, then by memory
goto 792 $TABY; click; key backspace; key backspace; key backspace; key backspace; key backspace; key ret; sleep 0.4
goto 330 150; click; sleep 0.6; shot t2-sort-name
goto 560 150; click; sleep 0.6; shot t2-sort-mem
# rapid tab switching
for i in $(seq 1 8); do
  goto 252 $TABY; click; goto 344 $TABY; click; goto 436 $TABY; click; goto 528 $TABY; click; goto 620 $TABY; click
done
sleep 1.0; shot t2-after-rapid
# shrink to the minimum: drag the bottom-right corner
goto 1049 643
mon "mouse_button 1"; sleep 0.1
move -400 -300; CX=$((CX - 400)); CY=$((CY - 300)); sleep 0.5
mon "mouse_button 0"; sleep 0.8
shot t2-min-procs
goto 252 $TABY; click; sleep 1.2; shot t2-min-cpu
goto 436 $TABY; click; sleep 1.2; shot t2-min-disk
goto 528 $TABY; click; sleep 1.2; shot t2-min-net
finish
