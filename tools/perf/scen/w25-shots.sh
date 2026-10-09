# Captures of the system apps for the README and the docs (UEFI, 1280x800). LIGHT=1 switches to
# the light appearance first; tag = dark or light. The toast needs a build with a way to raise one.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
t=dark
if [ "${LIGHT:-0}" = 1 ]; then
  t=light
  goto 1100 14; click; sleep 0.6; goto 953 188; click; sleep 0.6; key esc; sleep 0.6
fi
key ctrl-w; sleep 0.8
dock_icon tasks; click; sleep 1.2; goto 1230 760
sleep 12; shot "$t-tarefas-cpu"
key 2; sleep 1.2; shot "$t-tarefas-mem"
key 5; sleep 1.2; shot "$t-tarefas-procs"
key ctrl-w; sleep 0.6
key ctrl-spc; sleep 0.5; typestr regis; sleep 0.6; key ret; sleep 1.6; goto 1230 760; shot "$t-registro"; key ctrl-w; sleep 0.6
dock_icon settings; click; sleep 1.5; goto 1230 760; shot "$t-ajustes"; key ctrl-w; sleep 0.6
dock_icon calc; click; sleep 1.2; typestr "1250*12="; sleep 0.3; typestr "+200"; sleep 0.6; goto 1230 760; shot "$t-calc"; key ctrl-w; sleep 0.6
key ctrl-alt-t; sleep 1.0; shot "$t-toast"
finish
