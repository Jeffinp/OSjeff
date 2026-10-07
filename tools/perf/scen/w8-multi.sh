# W8: several instances of the same app keep their own state.
# 3 terminals + 2 editors with different text, then the Task Manager lists them
# (shell, shell 2, shell 3, editor, editor 2) and ends `shell 3` with DEL.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
# type a word into the focused window (space = spc)
typew() { local s=$1 i ch; for ((i = 0; i < ${#s}; i++)); do ch=${s:i:1}; [ "$ch" = " " ] && ch=spc; key "$ch"; sleep 0.12; done; }

typew "echo t1"; key ret; sleep 0.5
key ctrl-n; sleep 1.5
typew "echo t2"; key ret; sleep 0.5
key ctrl-n; sleep 1.5
typew "echo t3"; key ret; sleep 0.5
shot w8_terminals

dock 559; click; sleep 2
typew "hello from one"; sleep 0.5
key ctrl-n; sleep 1.5
typew "second editor text"; sleep 0.5
shot w8_editors

# focus editor 1 again (its title strip is visible above editor 2) and add to it
goto 700 122; click; sleep 0.5
typew "!!"; sleep 0.5
shot w8_editor1_kept

# Task Manager: select `shell 3` (row 4) and end it
dock 613; click; sleep 2
for i in 1 2 3 4; do key down; sleep 0.25; done
shot w8_taskmgr_before
key delete; sleep 2
shot w8_taskmgr_after
finish
