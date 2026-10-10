# W14: the system-log viewer. Opens it from the Apps launcher (it has no dock icon),
# takes a shot of the boot events, then types a search and cycles the level filter.
# Apps launcher (10 apps + 2 power rows): the app rows are 38 px high from the panel
# top; the log viewer is row 9 (index in Kind::ALL).
source "$(dirname "$0")/../lib.sh"
wait_first_frame
sleep 2
START_APPS=10
start_row() { # index -> click the start-panel row of app `index`
  local sy=$(( DOCKY - 20 - 12 - (20 + (START_APPS + 2) * 38 + 12) ))
  goto 451 $(( sy + 10 + 38 * $1 + 19 ))
  click
}
dock 451; click; sleep 1
shot l0_start
start_row 9; sleep 2
shot l1_log
for k in n e t; do key $k; sleep 0.2; done
sleep 1.2
shot l2_search
key delete; sleep 0.3
key tab; key tab; key tab; sleep 1.2   # Trace -> Debug -> Info -> Warn
shot l3_warn
finish
