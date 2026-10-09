# W24: the browser's page rendering on the local test site (tools/w24-site.py).
#   tools/w24-site.py &
#   QEMU_NETDEV="user,id=n0,net=203.0.113.0/24,host=203.0.113.5,dhcpstart=203.0.113.15,dns=203.0.113.3" \
#     tools/perf/run.sh <img> bios <out> 200 tools/perf/scen/w24-pages.sh
# Env: PAGES (space separated routes, default all), WAIT (seconds per page, default 6).
source "$(dirname "$0")/../lib.sh"
PAGES=${PAGES:-"type lists tables forms images long intl styled quote"}
WAIT=${WAIT:-6}
wait_first_frame
dock_icon browser; click; sleep 3
shot start
for p in $PAGES; do
  key ctrl-l; sleep 0.3
  typestr "http://203.0.113.5:8079/$p"
  key ret
  sleep "$WAIT"
  shot "p-$p"
  key pgdn; sleep 0.6
  shot "p-$p-2"
done
finish
