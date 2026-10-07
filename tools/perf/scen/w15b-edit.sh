# W15b editor: type, Save-as dialog, save, find/replace, undo, dirty close question (Cancelar then
# Descartar), reopen from the terminal and from Ctrl+O, a 1 MB file and a 1 MB single line.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
# the terminal prepares the hostile files first
typestr "seq 1 200000 > /grande.txt"; key ret; sleep 5
typestr "yes xxxxxxxxxxxxxxxx | head -n 60000 | tr -d '\\n' > /longa.txt"; key ret; sleep 6
typestr "ls -l"; key ret; sleep 1.5
shot e0-files
dock 559; click; sleep 2                  # a new editor
typestr "Ola mundo"; key ret
typestr "segunda linha com texto"; key ret
typestr "    indentada"; key ret
sleep 0.5
shot e1-typed
key ctrl-s; sleep 1                       # no file yet: the Save-as dialog
shot e2-saveas
typestr "teste.txt"; key ret; sleep 1.5   # typing replaces the suggested name
shot e3-saved                             # "Salvo: teste.txt", title without *
key ctrl-f; typestr "linha"; sleep 0.6
shot e4-find
key esc
key ctrl-h; key tab; typestr "LINHA"; key alt-a; sleep 0.8   # the find field is pre-filled
shot e5-replace
key esc; key ctrl-z; key ctrl-z; sleep 0.4
shot e6-undo
typestr "mais um pouco"; sleep 0.4
shot e7-dirty                             # title has " *"
# close with unsaved changes: the title-bar button (editor rect 610,110,560,350: close at 1153,125)
goto 1153 125; click; sleep 1
shot e8-close-ask
key esc; sleep 0.6                        # Cancelar: still open
shot e9-cancelled
click; sleep 1
key d; sleep 2                            # Descartar: closes
shot e10-closed
typestr "edit teste.txt"; key ret; sleep 2.5
shot e11-reopened                         # the saved text, without the discarded one
key ctrl-o; sleep 1
shot e12-open-dialog
key esc
key ctrl-end; typestr "x"; key ctrl-q; sleep 1
shot e13-ctrlq-ask
key d; sleep 1.5
goto 380 95; click; sleep 0.5             # focus the terminal again
typestr "edit /grande.txt"; key ret; sleep 6
shot e14-big
key ctrl-end; sleep 1.5
shot e14-big-end
key ctrl-home; sleep 1.5
key pgdn; key pgdn; key pgdn; sleep 1
shot e14-big-pgdn
key ctrl-q; sleep 1                       # nothing changed: closes without asking
goto 380 95; click; sleep 0.5
typestr "edit /longa.txt"; key ret; sleep 5
shot e15-long
key end; sleep 2
shot e15-long-end
key ctrl-home; sleep 1.5
shot e15-long-home
key ctrl-q; sleep 1
finish
