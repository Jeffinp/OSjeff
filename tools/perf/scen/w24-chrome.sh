# W24: browser chrome: start page, tabs, omnibox, popover, find bar, context menu, errors.
#   tools/w24-site.py &
#   QEMU_NETDEV="user,id=n0,net=203.0.113.0/24,host=203.0.113.5,dhcpstart=203.0.113.15,dns=203.0.113.3" \
#     tools/perf/run.sh <img> bios <out> 200 tools/perf/scen/w24-chrome.sh
source "$(dirname "$0")/../lib.sh"
SITE=${SITE:-http://203.0.113.5:8079}
wait_first_frame
dock_icon browser; click; sleep 3
shot c-start
key ctrl-l; sleep 0.3
typestr "$SITE/type"
shot c-typing
key ret; sleep 6
shot c-page
key ctrl-t; sleep 1
shot c-newtab
key ctrl-l; typestr "$SITE/lists"; key ret; sleep 5
shot c-tab2
key ctrl-f; sleep 0.4; typestr "para"; sleep 0.5
shot c-find
key esc; sleep 0.3
key ctrl-1; sleep 1
shot c-tab1
key ctrl-l; typestr "http://203.0.113.5:9/x"; key ret; sleep 8
shot c-error
finish
