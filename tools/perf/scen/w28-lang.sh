# i18n: the shell in Portuguese, the switch in Ajustes > Idioma e região, then the same views in
# English. Run in UEFI (1280x800). Shots: pt-*, lang-*, en-*.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
views() { # <tag>
  local t=$1
  goto 900 600; shot "$t-panel"
  dock_icon files; sleep 1.4; shot "$t-tooltip"
  rclick; sleep 0.7; shot "$t-taskbar-menu"; key esc; sleep 0.5
  dock_icon apps; click; sleep 1.2; goto 900 700; shot "$t-launcher"; key esc; sleep 0.7
  key ctrl-spc; sleep 0.5; typestr "12*7"; sleep 0.6; shot "$t-search"; key esc; sleep 0.5
  panel_item tray; click; sleep 0.9; shot "$t-quick"; key esc; sleep 0.6
  panel_item clock; click; sleep 0.9; shot "$t-calendar"; key esc; sleep 0.6
  goto 640 400; rclick; sleep 0.7; shot "$t-context"; key esc; sleep 0.5
}
key ctrl-w; sleep 0.8
views pt
dock_icon settings; click; sleep 1.4
goto 320 295; click; sleep 0.9; shot lang-pt
goto 700 258; click; sleep 1.2; goto 900 700; shot lang-en
key ctrl-w; sleep 0.8
views en
finish
