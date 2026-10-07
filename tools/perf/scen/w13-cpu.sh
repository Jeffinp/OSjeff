# W13 proof (e): CPU per app in the Task Manager. Plasma (a continuous v1 app that
# blits a 320x180 frame every 16 ms), Snake (v1) and Clock (v2, one tick per
# second) run together; the Task Manager is maximized to show the APPS section.
# Start-panel rows: Notas 7, Ola 8, Pintura 9, Plasma 10; scrolled by 2: Relogio 9,
# Snake 10.
source "$(dirname "$0")/../lib.sh"
srow() { echo $(( DOCKY - 529 + 38 * $1 )); }
wait_first_frame
dock 451; click; sleep 0.6; goto 440 "$(srow 10)"; click; sleep 2      # plasma
dock 451; click; sleep 0.6; key down; key down; sleep 0.4
goto 440 "$(srow 9)"; click; sleep 2                                    # clock
dock 451; click; sleep 0.6; key down; key down; sleep 0.4
goto 440 "$(srow 10)"; click; sleep 2                                   # snake
dock 613; click; sleep 1.5                                              # task manager
goto 560 215; click; click; sleep 1.5                                   # double-click title: maximize
sleep 4
shot e1-taskmgr-cpu
key down; key down; key down; key down; sleep 1
shot e2-taskmgr-cpu-scrolled
finish
