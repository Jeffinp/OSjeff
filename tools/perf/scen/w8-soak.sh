# W8: open/close soak. 100 rounds of editor + calculator opened from the dock and
# closed with Esc. Build with --features perf-trace and compare the serial
# `[trace]   heap used=` lines: they must not drift (tools/perf/w8-heap.sh).
source "$(dirname "$0")/../lib.sh"
wait_first_frame
for r in $(seq 1 100); do
  dock 559; click; sleep 0.6; key esc; sleep 0.6
  dock 666; click; sleep 0.6; key esc; sleep 0.6
done
sleep 2
finish
