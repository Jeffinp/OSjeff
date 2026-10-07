# W14: the resource monitor. Opens a few apps, then the monitor from the start
# panel (it has no dock icon), looks at the process list, sorts it, lets the
# graphs fill while a temporary build hook generates load (a burning thread, a
# heap wave, network traffic), and shows the Performance and Sistema tabs.
# Start panel (9 apps + 2 power rows): app rows are 38 px high from the panel top.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
sleep 2
START_APPS=10
start_row() { # index -> click the start-panel row of app `index`
  local sy=$(( DOCKY - 20 - 12 - (20 + (START_APPS + 2) * 38 + 12) ))
  goto 451 $(( sy + 10 + 38 * $1 + 19 ))
  click
}
dock 666; click; sleep 2     # calculator
dock 559; click; sleep 2     # editor
dock 829; click; sleep 2     # files
dock 451; click; sleep 1
start_row 7; sleep 3         # monitor (index 7 in Kind::ALL)
shot m1_proc
key m; sleep 1.5             # sort by memory
shot m2_sorted
key c; sleep 0.5
# Performance tab: click it (tabs sit under the title bar of the monitor window).
goto 421 142; click; sleep 38
shot m3_perf
goto 557 142; click; sleep 2
shot m4_system
goto 285 142; click; sleep 1
finish
