# Scenario 7 (perf-trace build): a folder with 2000 files: open it and scroll.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 829; click; sleep 3
key down; key down; sleep 0.5
key ret; sleep 3                      # into /many (2000 files)
shot s7-many
echo "[scen] scroll start $(date +%s.%N)" >> "$OUT/scen.log"
for r in $(seq 1 25); do key pgdn; sleep 0.15; done
for r in $(seq 1 60); do key down; sleep 0.08; done
shot s7-scrolled
key end; sleep 1
shot s7-end
for r in $(seq 1 25); do key pgup; sleep 0.15; done
key home; sleep 1
echo "[scen] scroll end $(date +%s.%N)" >> "$OUT/scen.log"
sleep 3
finish
