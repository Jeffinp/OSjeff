# Tarefas visible while the Snake game runs: the Tarefas window must not blink.
# Takes a burst of screenshots 0.4 s apart; w27-flicker-check.sh compares the Tarefas region.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
key ctrl-spc; sleep 0.8; typestr "tarefas"; sleep 0.5; key ret; sleep 2
sleep 2; key ctrl-spc; sleep 1.5; typestr "snake"; sleep 1; shot busca; key ret; sleep 4
goto 1200 500; sleep 0.5
for i in $(seq 1 14); do shot "f$i"; sleep 0.2; done
finish
