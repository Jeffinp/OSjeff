# W18 proof (b): the Files "Apps" place. A (or the sidebar entry) opens it; Down moves,
# Del removes the selected app (the Apps launcher follows), I installs it again, a
# right click offers Abrir / Remover / Propriedades (the manifest and permissions),
# Enter on an uninstalled bundled package installs and runs it. Rows are in name
# order: Notas, Ola, Pintura, Plasma, Relogio, Snake.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 829; click; sleep 2
key a; sleep 1
shot a1-apps
key down; sleep 0.3; key delete; sleep 1                 # remove "Ola"
shot a2-removed
key delete; sleep 0.8                                    # again: error line "nao instalado"
shot a3-error
dock 451; click; sleep 1; key end; sleep 0.5; shot a4-start-without-ola
key esc; sleep 0.5
goto 640 400; click; sleep 0.5                           # refocus Files
key i; sleep 1                                           # install it again
shot a5-reinstalled
key i; sleep 0.6; shot a6-already                        # "ja instalado"
goto 560 262; rclick; sleep 0.6; shot a7-menu            # context menu on a row
goto 600 330; click; sleep 0.8                           # "Propriedades"
shot a8-props
key esc; sleep 0.5
key down; key down; key ret; sleep 2.5                   # run Pintura
shot a9-run
finish
