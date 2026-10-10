# W24: browser captures for README and docs (run in UEFI: 1280x800). LIGHT=1 for the light
# appearance. Needs tools/w24-site.py (:8079) and tools/nettest-server.py (:8078).
#   QEMU_NETDEV="user,id=n0,net=203.0.113.0/24,host=203.0.113.5,dhcpstart=203.0.113.15,dns=203.0.113.3" \
#     tools/perf/run.sh <uefi img> uefi <out> 200 tools/perf/scen/w24-readme.sh
source "$(dirname "$0")/../lib.sh"
SITE=${SITE:-http://203.0.113.5:8079}
wait_first_frame
t=dark
if [ "${LIGHT:-0}" = 1 ]; then
  t=light
  panel_item tray; click; sleep 0.7; quick_tile appearance; click; sleep 0.7; key esc; sleep 0.6
fi
key ctrl-w; sleep 0.8
dock_icon browser; click; sleep 2
goto 900 700
shot "$t-nova-aba"
key ctrl-l; typestr "$SITE/styled"; key ret; sleep 7
key ctrl-d; sleep 0.5
key ctrl-t; sleep 0.8; key ctrl-l; typestr "$SITE/type"; key ret; sleep 7
shot "$t-pagina"
key ctrl-f; sleep 0.4; typestr "texto"; sleep 0.6
shot "$t-busca"
key esc; sleep 0.3
key ctrl-l; typestr "https://203.0.113.5:8078/hello"; key ret; sleep 10
shot "$t-erro-cert"
key ctrl-l; typestr "kitsune://favoritos"; key ret; sleep 2
shot "$t-favoritos"
finish
