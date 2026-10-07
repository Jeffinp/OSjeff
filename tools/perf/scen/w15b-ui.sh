# W15b UI details: the wheel in the terminal and in the editor, maximize (the text grid follows the
# window), the mouse in the editor (click, double click, drag), the Files "open text file" path and
# the unsaved-changes question reached from it, Ctrl+Shift+C / Ctrl+V in the terminal.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
typestr "seq 300"; key ret; sleep 2.5
goto 300 250
mon "mouse_move 0 0 1"; mon "mouse_move 0 0 1"; mon "mouse_move 0 0 1"; sleep 0.8
shot u1-wheel-up                          # scrolled up by 9 rows
mon "mouse_move 0 0 -1"; sleep 0.5
shot u2-wheel-down
typestr "echo copy-me"; key shift-ctrl-c; sleep 0.3
key ctrl-u; key ctrl-v; sleep 0.5
shot u3-paste                             # the typed line was copied and pasted back
key ctrl-u
# maximize with the title bar's middle button (it appears while the pointer is over the window)
goto 629 95; click; sleep 1.5
shot u4-maximized
typestr "seq 3"; key ret; sleep 1
goto 629 95; click; sleep 1.5             # restore
# a new editor with some words: click, double click, drag
dock 559; click; sleep 2
typestr "alfa beta gamma delta"; key ret; typestr "segunda linha"; key ret; sleep 0.5
goto 700 155; click; sleep 0.4            # caret inside "beta"
sleep 1; click; click; sleep 0.4          # double click selects the word
shot u6-word
mon "mouse_button 1"; move 120 0; move 60 18; mon "mouse_button 0"; sleep 0.5
CX=$((CX + 180)); CY=$((CY + 18))
shot u7-drag
# Files -> open a text file in the editor, edit it, close: the question appears
dock 829; click; sleep 3
shot u7b-files
key down; key down; sleep 0.5             # notas.txt
key ret; sleep 2
shot u8-files-open
typestr "mudei"; sleep 0.5
goto 1153 125; click; sleep 1             # the title-bar close of the new editor
shot u9-question
finish
