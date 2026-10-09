# Tarefas: every tab, hover on the chart, the process table with a selection and the
# confirmation sheet, in the current appearance and then in the other one.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
# Window opens at (190,52) 860x592: tabs at y=110, centres 252 344 436 528 620.
TABY=110
tab() { goto "$1" $TABY; click; sleep 1.3; }
shots() { # <tag>
  local t=$1
  tab 252; shot "$t-cpu"
  goto 760 270; sleep 0.8; shot "$t-cpu-hover"
  tab 344; shot "$t-mem"
  goto 760 270; sleep 0.8; shot "$t-mem-hover"
  tab 436; shot "$t-disk"
  goto 760 400; sleep 0.8; shot "$t-disk-hover"
  tab 528; shot "$t-net"
  goto 760 400; sleep 0.8; shot "$t-net-hover"
  tab 620; shot "$t-procs"
  goto 400 250; click; sleep 0.8; shot "$t-procs-sel"
}
dock_icon tasks; click; sleep 1.5
shots a
goto 1100 14; click; sleep 0.6; goto 953 188; click; sleep 0.6
goto 700 680; click; sleep 0.8
dock_icon tasks; click; sleep 1.5
shots b
finish
