# W20: compressed pages that do not fit (or are damaged) render what was decoded, with a notice.
# Needs tools/nettest-pages.py running on the host (see its header) and the guest on the public-
# looking SLIRP range:
#   tools/nettest-pages.py &
#   QEMU_NETDEV="user,id=n0,net=203.0.113.0/24,host=203.0.113.5,dhcpstart=203.0.113.15,dns=203.0.113.3" \
#     QEMU_MEM=256M tools/perf/run.sh <img> bios <out> 400 tools/perf/scen/w20-browser.sh
# Env: CASES (default: every route, space separated), WAIT (seconds per page, default 20).
source "$(dirname "$0")/../lib.sh"
CASES=${CASES:-"gz-small gz-big gz-huge gz-chunked gz-chunked-big deflate deflate-raw gz-badcrc gz-cut gz-twice GZ-CASE br plain gz-nolen"}
WAIT=${WAIT:-20}
wait_first_frame
dock 721; click; sleep 3
for c in $CASES; do
  goto 600 120; click; sleep 0.4
  key ctrl-a; sleep 0.2
  typestr "http://203.0.113.5:8077/$c"
  key ret
  sleep "$WAIT"
  shot "p-$c"
  # the end of the page (the "END OF PAGE" line exists only when everything was decoded)
  goto 600 400; click; key end; sleep 2   # focus the page, jump to its end
  shot "p-$c-end"
done
finish
