#!/usr/bin/env bash
# generates scen/*.sh (each sources lib.sh from the same dir)
cd "$(dirname "$0")"
mkdir -p scen
hdr='source "$(dirname "$0")/../lib.sh"
wait_first_frame'
w() { printf '%s\n%s\nfinish\n' "$hdr" "$2" > "scen/$1.sh"; }
w idle 'sleep 10'
w mouse 'for i in $(seq 1 100); do move 4 2; sleep 0.04; move -4 -2; sleep 0.04; done; sleep 1'
w drag 'move -340 -265; sleep 0.5
mon "mouse_button 1"; sleep 0.2
for i in $(seq 1 60); do move 5 2; sleep 0.06; done
for i in $(seq 1 60); do move -5 -2; sleep 0.06; done
mon "mouse_button 0"; sleep 1'
w typing 'for r in 1 2 3; do for k in h e l p ret; do key $k; sleep 0.25; done; done
for r in 1 2 3 4 5 6; do for k in a b c d e f g h i j k l m n o p q r s t u v w x y z; do key $k; sleep 0.15; done; done
sleep 1'
w openclose 'dock_to 26
for r in 1 2 3 4; do click; sleep 2; key esc; sleep 2; done'
w files 'dock_to 189
click; sleep 2.5
for r in 1 2 3 4 5 6 7 8; do key down; sleep 0.25; done
for r in 1 2 3 4 5 6 7 8; do key up; sleep 0.25; done
sleep 1'
w start 'dock_to -189
click; sleep 1
for i in $(seq 1 40); do move 0 -4; sleep 0.08; done
for i in $(seq 1 40); do move 0 4; sleep 0.08; done
sleep 1'
w wasm 'dock_to 135
click; sleep 10'
w taskmgr 'dock_to -27
click; sleep 10'
