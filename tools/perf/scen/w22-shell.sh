# Busca, the calculator in it, the restart sheet, the gallery, the HUD, a context menu, Alt+Tab.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
key ctrl-spc; sleep 0.6; typestr "cal"; sleep 0.6; shot sh-search-app
key backspace; key backspace; key backspace; typestr "12*(3+4)"; sleep 0.6; shot sh-search-calc
key esc; sleep 0.5
typestr ""; key ctrl-spc; sleep 0.4; typestr "lei"; sleep 0.6; shot sh-search-file
key esc; sleep 0.5
key ctrl-alt-g; sleep 1.0; shot sh-gallery0
goto 330 126; click; sleep 0.4; shot sh-gallery1
goto 430 126; click; sleep 0.4; shot sh-gallery2
goto 530 126; click; sleep 0.4; shot sh-gallery3
key ctrl-alt-h; sleep 1.0; shot sh-hud
key ctrl-alt-h; sleep 0.5
goto 20 14; click; sleep 0.5; goto 40 150; sleep 0.3; click; sleep 0.8; shot sh-dialog
key esc; sleep 0.5
goto 1150 300; rclick; sleep 0.6; shot sh-context
key esc; sleep 0.3
dock_icon calc; rclick; sleep 0.6; shot sh-dockmenu
key esc; sleep 0.3
finish
