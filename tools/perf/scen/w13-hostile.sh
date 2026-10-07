# W13 proof (b): a hostile test app (built by a TEMPORARY build hook, not committed)
# attacks the platform while clock and notes keep running. Every attack must be
# contained: only that app dies / gets an error; the desktop and the others carry on.
#   1 infinite loop   2 memory.grow far past the quota   3 invalid pointer
#   4 open("../../etc/x")   5 open descriptors up to the ceiling (run after 6)
#   6 read another app's files   7 net_http_get without permission
# The start panel lists "Hostil" first among the apps (row 7), then Notas (8),
# Ola, Pintura, Plasma, Relogio, Snake.
source "$(dirname "$0")/../lib.sh"
srow() { echo $(( DOCKY - 529 + 38 * $1 )); }
wait_first_frame
# notes (row 8): type a line and save it so the other apps have something to steal
dock 451; click; sleep 0.6; goto 440 "$(srow 8)"; click; sleep 2
for ch in s e c r e t; do key $ch; sleep 0.12; done
key ctrl-s; sleep 0.8
# clock (scroll 3 rows, then row 9)
dock 451; click; sleep 0.6; key down; key down; key down; sleep 0.4
goto 440 "$(srow 9)"; click; sleep 2
shot b0-clock-notes
# hostile #1: infinite loop
dock 451; click; sleep 0.6; goto 440 "$(srow 7)"; click; sleep 2
key 1; sleep 3
shot b1-loop
goto 746 201; click; sleep 1      # close the dead window (the app is already gone)
# hostile #2: invalid pointer
dock 451; click; sleep 0.6; goto 440 "$(srow 7)"; click; sleep 2
key 3; sleep 2
shot b2-pointer
goto 746 201; click; sleep 1
# hostile #3: survivable attacks (memory.grow, sandbox escape, fds, other app, net)
dock 451; click; sleep 0.6; goto 440 "$(srow 7)"; click; sleep 2
for k in 2 4 6 5 7; do key $k; sleep 0.8; done
shot b3-survivor
# the others still run: type in notes, look at the Task Manager
dock 613; click; sleep 1.5
shot b4-taskmgr
sleep 3
shot b5-later
finish
