# Tarefas with a crowded table: 28 extra terminals (a process each), scrolling the list with
# the wheel and the keyboard, sorting, searching, and ending one of them.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
for i in $(seq 1 28); do key ctrl-n; sleep 0.35; done
sleep 1.0
dock_icon tasks; click; sleep 1.5
goto 620 110; click; sleep 1.0
shot t3-many
goto 600 300
for i in $(seq 1 6); do mon "mouse_move 0 0 -1"; sleep 0.1; done
sleep 0.6; shot t3-scrolled
for i in $(seq 1 12); do mon "mouse_move 0 0 -1"; sleep 0.1; done
sleep 0.6; shot t3-end
goto 330 150; click; sleep 0.6; shot t3-by-name
key down; key down; key delete; sleep 0.8; shot t3-ended
finish
