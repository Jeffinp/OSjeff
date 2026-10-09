# W24: security popover, suggestions, context menu, zoom pill, tab close, bookmark star.
# LIGHT=1 switches to the light appearance first (bios 1280x720). Needs tools/w24-site.py.
source "$(dirname "$0")/../lib.sh"
SITE=${SITE:-http://203.0.113.5:8079}
wait_first_frame
t=dark
if [ "${LIGHT:-0}" = 1 ]; then
  t=light
  goto 1100 14; click; sleep 0.6; goto 953 188; click; sleep 0.6; key esc; sleep 0.6
fi
key ctrl-w; sleep 0.8
dock_icon browser; click; sleep 2
key ctrl-l; typestr "$SITE/lists"; key ret; sleep 5
key ctrl-l; typestr "$SITE/"; sleep 0.8
shot "$t-suggest"
key esc; key esc; sleep 0.3
goto 340 116; click; sleep 0.8
shot "$t-popover"
key esc; sleep 0.4
goto 997 116; click; sleep 0.6
shot "$t-star"
goto 325 204; sleep 0.5
shot "$t-hover"
rclick; sleep 0.6
shot "$t-menu"
key esc; sleep 0.3
key ctrl-equal; key ctrl-equal; sleep 0.6
shot "$t-zoom"
key ctrl-0; sleep 0.3
key ctrl-t; sleep 0.5; key ctrl-t; sleep 0.5; key ctrl-t; sleep 1
shot "$t-tabs"
key ctrl-w; sleep 0.12
snap "$t-tabclose"
sleep 0.6
flush_snaps
finish
