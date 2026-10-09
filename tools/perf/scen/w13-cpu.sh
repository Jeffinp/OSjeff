# W13 proof (e): CPU per app in the Task Manager. Snake (v1) and Clock (v2, one tick per
# second) run together; the Task Manager is maximized to show the APPS section.
# Plasma (a continuous v1 app that blits a 320x180 frame every 16 ms) is no longer bundled:
# its source is wasm-apps/examples/plasma; install it from disk (see w18-net.sh) to add it.
# Launcher rows with the four bundled apps: Notas 7, Pintura 8; scrolled by 2: Relogio 9,
# Snake 10 (re-measure the rows if the launcher layout changes).
source "$(dirname "$0")/../lib.sh"
srow() { echo $(( DOCKY - 529 + 38 * $1 )); }
wait_first_frame
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
