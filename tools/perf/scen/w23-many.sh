# W23: a folder with 2000 files must scroll smoothly (virtualised rows, inertial wheel). Needs a
# disk with `fs3_inject <img> --files 2000 /many`; run with the perf-trace image and read the frame
# times with tools/perf/summ.py.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon files; click; sleep 2.5
goto 700 580
shot m1-root
for r in 1 2 3 4; do key down; sleep 0.15; done   # apps Documentos etc Imagens many
key ret; sleep 2.5                       # into /many
shot m2-many
echo "[scen] wheel start $(date +%s.%N)" >> "$OUT/scen.log"
goto 700 400
for r in $(seq 1 40); do mon "mouse_move 0 0 -1"; sleep 0.05; done
shot m3-wheel
for r in $(seq 1 40); do mon "mouse_move 0 0 1"; sleep 0.05; done
for r in $(seq 1 25); do key pgdn; sleep 0.12; done
for r in $(seq 1 40); do key down; sleep 0.06; done
shot m4-keys
key end; sleep 1.5
shot m5-end
key home; sleep 1.5
# Drag the scrollbar thumb.
goto 1068 232
mon "mouse_button 1"; sleep 0.2
goto 1068 340; goto 1068 470; sleep 0.5
shot m6-thumb
mon "mouse_button 0"; sleep 0.5
echo "[scen] wheel end $(date +%s.%N)" >> "$OUT/scen.log"
sleep 2
finish
