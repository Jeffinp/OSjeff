# i18n wave 31: Tarefas (every tab), Registro and Calculadora in Portuguese, then in English.
# Run in UEFI (1280x800), US layout. Shots: pt-*, lang-en, en-*.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
app_views() { # <tag>
  local t=$1
  key ctrl-w; sleep 0.8                              # the start-up terminal
  dock_icon tasks; click; sleep 2.5
  shot "$t-tasks-cpu"
  goto 345 110; click; sleep 1.0; shot "$t-tasks-mem"
  goto 437 110; click; sleep 1.0; shot "$t-tasks-disk"
  goto 530 110; click; sleep 1.0; shot "$t-tasks-net"
  goto 621 110; click; sleep 1.0; shot "$t-tasks-proc"
  goto 400 318; click; sleep 0.6         # a system service: ending it asks first
  goto 985 602; click; sleep 1.0; shot "$t-tasks-confirm"
  key esc; sleep 0.5
  key ctrl-w; sleep 0.8
  key ctrl-spc; sleep 0.5; typestr "log"; sleep 0.5; key ret; sleep 2.0
  shot "$t-log"
  goto 640 400; click; sleep 0.3
  key ctrl-w; sleep 0.8
  dock_icon calc; click; sleep 1.5
  typestr "1234.5*2="; sleep 0.8
  shot "$t-calc"
  key ctrl-w; sleep 0.8
}
app_views pt
dock_icon settings; click; sleep 1.4
goto 320 295; click; sleep 0.9
goto 700 258; click; sleep 1.2
key ctrl-w; sleep 0.8
shot lang-en
app_views en
finish
