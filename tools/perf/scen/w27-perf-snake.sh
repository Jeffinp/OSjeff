# Cost of the Snake game running next to a visible Tarefas window (the combination that flickered):
# Tarefas on the CPU tab, then Snake from Busca, 25 s of both. Compare `animdm` / `animrb` /
# `clockl` per-frame means in tools/perf/summ.py.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon tasks; click; sleep 2
key ctrl-spc; sleep 1.0; typestr "snake"; sleep 0.8; key ret; sleep 25
finish
