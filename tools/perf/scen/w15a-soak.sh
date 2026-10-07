# Scenario 8 (perf-trace build): heap soak. 100 rounds of opening/closing the file manager,
# then 100 rounds of opening/closing the image viewer from a file manager.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
sleep 3
echo "[scen] files soak start" >> "$OUT/scen.log"
for r in $(seq 1 100); do
  dock 829; click; sleep 0.6; key esc; key esc; sleep 0.6
done
sleep 3
echo "[scen] viewer soak start" >> "$OUT/scen.log"
dock 829; click; sleep 2
key down; key ret; sleep 1.5              # into Imagens: corrompida.png first
key down; key down; key down; key down; sleep 0.5   # pequena.png
for r in $(seq 1 100); do
  key ret; sleep 0.9; key esc; sleep 0.6
done
sleep 3
echo "[scen] done" >> "$OUT/scen.log"
finish
