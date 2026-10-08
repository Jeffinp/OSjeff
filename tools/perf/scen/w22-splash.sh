# The boot splash, half way and almost done.
source "$(dirname "$0")/../lib.sh"
until grep -aq "ui text engine ready" "$OUT/serial.log" 2>/dev/null; do sleep 0.2; done
sleep 1.5; shot splash-1
sleep 2.0; shot splash-2
finish
