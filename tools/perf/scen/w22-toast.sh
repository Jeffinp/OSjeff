# Banners: needs a TEMPORARY build hook (never committed) that logs a WARN 6 s and an ERROR
# 9 and 10 s after the first desktop frame. Shows the slide in, the repeat counter, the slide out.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
until grep -aq "disco lento" "$OUT/serial.log" 2>/dev/null; do sleep 0.1; done
snap t1; sleep 0.08; snap t2; sleep 0.12; snap t3; sleep 0.6; snap t4
until grep -aq "erro de E/S" "$OUT/serial.log" 2>/dev/null; do sleep 0.1; done
sleep 1.6; snap t5
sleep 2.0; snap t6
sleep 1.4; snap t7
flush_snaps
finish
