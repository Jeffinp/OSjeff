# W24 (perf-trace build): browser load + first paint, layout of a 2000-node page, scroll frame
# time and the idle cost of an open browser window. Needs tools/w24-site.py.
#   cargo build --release -p os --features perf-trace
#   QEMU_NETDEV="user,id=n0,net=203.0.113.0/24,host=203.0.113.5,dhcpstart=203.0.113.15,dns=203.0.113.3" \
#     tools/perf/run.sh <img> bios <out> 200 tools/perf/scen/w24-perf.sh ; tools/perf/w24-perf.py <out>
source "$(dirname "$0")/../lib.sh"
SITE=${SITE:-http://203.0.113.5:8079}
wait_first_frame
key ctrl-w; sleep 1
dock_icon browser; click; sleep 3
goto 600 400
echo "[scen] idle start" >> "$OUT/scen.log"; sleep 8; echo "[scen] idle end" >> "$OUT/scen.log"
for p in type tables; do
  key ctrl-l; typestr "$SITE/$p"; key ret; sleep 6
done
key ctrl-l; typestr "$SITE/big?n=2000"; key ret; sleep 14
echo "[scen] pgdn start" >> "$OUT/scen.log"
for r in $(seq 1 24); do key pgdn; sleep 0.2; done
echo "[scen] pgdn end" >> "$OUT/scen.log"
sleep 2
echo "[scen] wheel start" >> "$OUT/scen.log"
for r in $(seq 1 40); do mon "mouse_move 0 0 -1"; sleep 0.08; done
echo "[scen] wheel end" >> "$OUT/scen.log"
sleep 2
echo "[scen] arrows start" >> "$OUT/scen.log"
for r in $(seq 1 40); do key down; sleep 0.06; done
echo "[scen] arrows end" >> "$OUT/scen.log"
sleep 2
key ctrl-equal; sleep 3
key ctrl-0; sleep 3
shot perf-end
finish
