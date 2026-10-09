# Cost of a visible Tarefas window: open it on the CPU tab and let it run 25 s, with a
# second window behind it. Compare `animdm` / `clockl` / `settle` per-frame means in summ.py.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon tasks; click; sleep 25
finish
