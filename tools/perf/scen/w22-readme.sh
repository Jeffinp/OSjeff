# README / docs screenshots (run in UEFI: 1280x800). LIGHT=1 switches to the light appearance
# first (Auto follows the clock); tag = dark or light. One boot per appearance.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
t=dark
if [ "${LIGHT:-0}" = 1 ]; then
  t=light
  goto 1100 14; click; sleep 0.6; goto 953 188; click; sleep 0.6; key esc; sleep 0.6
fi
key ctrl-w; sleep 0.8                              # the terminal opened at boot
dock_icon files; click; sleep 1.0; goto 900 700; shot "$t-files"; key ctrl-w; sleep 0.6
dock_icon browser; click; sleep 1.2; goto 900 700; shot "$t-browser"; key ctrl-w; sleep 0.6
dock_icon settings; click; sleep 1.0; goto 900 700; shot "$t-settings"; key ctrl-w; sleep 0.6
dock_icon tasks; click; sleep 1.5; goto 900 700; shot "$t-tarefas"; key ctrl-w; sleep 0.6
dock_icon apps; click; sleep 1.2; goto 900 700; shot "$t-apps"; key esc; sleep 0.7
key ctrl-spc; sleep 0.5; typestr "12*7"; sleep 0.6; shot "$t-busca"; key esc; sleep 0.5
goto 1100 14; click; sleep 0.8; shot "$t-controls"; key esc; sleep 0.5
key ctrl-alt-g; sleep 1.0; goto 900 700; shot "$t-gallery"; key ctrl-w; sleep 0.6
dock_icon terminal; click; sleep 0.9
typestr "ls"; key ret; sleep 0.3
dock_icon editor; click; sleep 0.9
typestr "OSjeff"; key ret; typestr "um desktop de verdade"; sleep 0.3
dock_icon calc; click; sleep 0.9
typestr "7*6"; key ret; sleep 0.5
goto 900 600; sleep 0.4
shot "$t-desktop"
finish
