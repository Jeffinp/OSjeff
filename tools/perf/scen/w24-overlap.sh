# W24: a window in front of the browser must stay in front while the page scrolls and
# hovers (client-only frames); then the browser comes back to the front. Needs tools/w24-site.py.
source "$(dirname "$0")/../lib.sh"
SITE=${SITE:-http://203.0.113.5:8079}
wait_first_frame
dock_icon browser; click; sleep 2
key ctrl-l; typestr "$SITE/big?n=2000"; key ret; sleep 8
goto 100 250; click; sleep 0.8          # the terminal comes to the front over the page's left edge
shot o-front
goto 700 400
for r in $(seq 1 12); do mon "mouse_move 0 0 -1"; sleep 0.1; done
sleep 1
shot o-scrolled
goto 600 300; sleep 0.4
for r in $(seq 1 10); do mon "mouse_move 8 3"; sleep 0.05; done
shot o-hover
click; sleep 0.8                         # click the page: the browser is raised
shot o-raised
finish
