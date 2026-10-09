# i18n W32: a file that is not an app. The Terminal makes `bad.wasm`; Files opens it (Enter); the
# installer refuses it and a toast says why, in Portuguese and then, after the switch in Ajustes,
# in English. UEFI (1280x800). Shots: install-pt-*, install-en-*.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
goto 300 250; click; sleep 0.5
typestr "echo oi > bad.wasm"; key ret; sleep 0.8
dock_icon files; click; sleep 1.8
goto 700 316; click; sleep 1.0; key ret; sleep 0.6
snap install-pt-1; sleep 1.4; snap install-pt-2; flush_snaps
sleep 4
key ctrl-w; sleep 0.8                      # close Files: a fresh window opens at the same place
dock_icon settings; click; sleep 1.6
goto 320 295; click; sleep 0.9
goto 700 258; click; sleep 1.2
key ctrl-w; sleep 0.8                      # close Ajustes
dock_icon files; click; sleep 1.8
# one more folder now: Ajustes stored its settings in /etc
goto 700 344; click; sleep 1.0; key ret; sleep 0.6
snap install-en-1; sleep 1.4; snap install-en-2; flush_snaps
finish
