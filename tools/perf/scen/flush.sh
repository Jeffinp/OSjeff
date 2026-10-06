source "$(dirname "$0")/../lib.sh"
wait_first_frame
# Open the file manager and press N (new folder) a few times: every press runs
# `flush_disk()`, i.e. rewrites the whole 99-sector filesystem image over ATA PIO.
dock 829; click; sleep 3
for r in 1 2 3 4 5 6; do key n; sleep 1.5; done
sleep 2
finish
