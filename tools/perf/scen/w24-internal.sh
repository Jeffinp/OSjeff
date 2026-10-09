# W24: the kitsune:// pages (favoritos, historico, sobre). Needs tools/w24-site.py.
source "$(dirname "$0")/../lib.sh"
SITE=${SITE:-http://203.0.113.5:8079}
wait_first_frame
key ctrl-w; sleep 0.8
dock_icon browser; click; sleep 2
for p in type lists tables; do
  key ctrl-l; typestr "$SITE/$p"; key ret; sleep 4
  key ctrl-d; sleep 0.4
done
key ctrl-l; typestr "kitsune://favoritos"; key ret; sleep 2
shot i-favoritos
key ctrl-l; typestr "kitsune://historico"; key ret; sleep 2
shot i-historico
key ctrl-l; typestr "kitsune://sobre"; key ret; sleep 2
shot i-sobre
key pgdn; sleep 0.6
shot i-sobre-2
finish
