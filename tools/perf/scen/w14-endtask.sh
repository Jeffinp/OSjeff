# W14: "end task" in the resource monitor. Opens the calculator and the monitor,
# sorts by name (the idle row first, then calc), selects calc with the arrow keys
# and ends it with DEL: the calculator window closes and its row disappears.
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
dock 451; click; sleep 1
start_row 7; sleep 3         # monitor
key n; sleep 1.2             # sort by name
key down; sleep 1
shot e1_selected
key delete; sleep 3
shot e2_ended
finish
