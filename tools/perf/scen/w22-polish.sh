# Shell pieces in dark (Auto at night) and in light: desktop, system menu, Quick Settings, calendar and notification centre, Apps, Busca, sheet.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
pass() { # <tag>
  local t=$1
  shot "$t-desktop"
  panel_item apps; rclick; sleep 0.7; shot "$t-menu"; key esc; sleep 0.4
  panel_item tray; click; sleep 0.8; shot "$t-control"; key esc; sleep 0.4
  panel_item clock; click; sleep 0.8; shot "$t-calendar"; key esc; sleep 0.4
  dock_icon apps; click; sleep 1.0; shot "$t-apps"; key esc; sleep 0.6
  key ctrl-spc; sleep 0.5; typestr "e"; sleep 0.6; shot "$t-busca"; key esc; sleep 0.5
  panel_item tray; click; sleep 0.5; quick_tile restart; sleep 0.3; click; sleep 0.8; shot "$t-sheet"; key esc; sleep 0.5
  dock_icon files; click; sleep 1.0; goto 1150 300; rclick; sleep 0.6; shot "$t-context"; key esc; sleep 0.3
}
pass dark
light_mode
pass light
finish
