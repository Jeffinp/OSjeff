# W15b UI details: the wheel in the terminal, Ctrl+Shift+C / Ctrl+V, the mouse in the editor (click,
# double click, drag), the Files "open text file" path and the unsaved-changes question reached from
# it, the power guard (shutdown with unsaved changes) and maximize (the text grid follows the window).
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
# a new editor with some words: click, double click, drag
sleep 1; dock 559; click; sleep 2.5
typestr "alfa beta gamma delta"; key ret; typestr "segunda linha"; key ret; sleep 0.5
goto 725 155; click; sleep 1.2            # caret inside "beta"
click; click; sleep 0.4                   # double click selects the word
shot u6-word
mon "mouse_button 1"; move 120 0; move 60 18; mon "mouse_button 0"; sleep 0.5
CX=$((CX + 180)); CY=$((CY + 18))
shot u7-drag
# Files -> open a text file in the editor, edit it, close: the question appears
sleep 1; dock 829; click; sleep 3
shot u7b-files
key down; key down; key down; sleep 0.5   # notas.txt (after Documentos and the folder rows)
key ret; sleep 2.5
shot u8-files-open
typestr "mudei"; sleep 0.5
key ctrl-q; sleep 1
shot u9-question                          # Ctrl+Q on the edited notas.txt asks first
key esc; sleep 0.5
# shutdown with unsaved changes: an editor asks instead of powering off
goto 200 300; click; sleep 0.5            # focus the terminal
typestr "shutdown"; key ret; sleep 2.5
shot u10-power-guard
key esc; sleep 0.5
# maximize the terminal with the middle title-bar button
goto 629 95; sleep 0.4; click; sleep 1.5
shot u4-maximized
finish
