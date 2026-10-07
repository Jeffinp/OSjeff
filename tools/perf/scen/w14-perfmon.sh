# W14: cost of an open resource monitor (perf-trace builds): the monitor is a live
# window like the task manager, so each second the clock-tick path recomposes
# and repaints it. Compare the "Clock" path with taskmgr.sh on the same build.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
START_APPS=10
sy=$(( DOCKY - 20 - 12 - (20 + (START_APPS + 2) * 38 + 12) ))
dock 451; click; sleep 1
goto 451 $(( sy + 10 + 38 * 7 + 19 )); click    # monitor
sleep 12
finish
