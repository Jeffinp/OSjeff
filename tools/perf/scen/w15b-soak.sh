# W15b: open/close soak for the new Editor and Terminal. 50 rounds of: an editor from the dock, a
# typed character, close with the title-bar button and "Descartar"; a second terminal (Ctrl+N), a
# command that runs on the worker thread, Ctrl+D to close it. Build with --features perf-trace and
# compare the serial `[trace]   heap used=` lines (tools/perf/w8-heap.sh): they must not drift.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
for r in $(seq 1 50); do
  dock 559; click; sleep 1; typestr "x"; sleep 0.3
  goto 1153 125; click; sleep 0.6; key d; sleep 0.8
  goto 300 250; click; sleep 0.3
  key ctrl-n; sleep 0.8; typestr "seq 50"; key ret; sleep 0.8; key ctrl-d; sleep 0.8
done
sleep 2
finish
