# W14: toast notifications. Needs a TEMPORARY build hook (never committed) that logs a
# WARN about 8 s after the first desktop frame and an ERROR about 14 s after it:
# a toast must appear for each, expire by itself after ~4 s, and a click on a toast
# dismisses it. The serial log lines are the cue to take the shots.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
until grep -aq "w14 hook: disk slow" "$OUT/serial.log" 2>/dev/null; do sleep 0.2; done
sleep 0.8
shot t1_warn                   # the WARN toast, ~1 s old
sleep 2.5
shot t2_expired                # ... and gone after its 4 s
until grep -aq "w14 hook: I/O error" "$OUT/serial.log" 2>/dev/null; do sleep 0.2; done
sleep 0.8
shot t3_error                  # the ERROR toast
# The toast stack sits above the clock pill: bottom-right, lowest slot at
# (sw - 16 - 420 .. sw - 16, sh - 72 - 50 .. sh - 72).
goto $(( 1280 - 16 - 210 )) $(( H - 72 - 25 ))
sleep 0.3
click; sleep 0.8
shot t4_dismissed              # a click closed it before its 4 s
finish
