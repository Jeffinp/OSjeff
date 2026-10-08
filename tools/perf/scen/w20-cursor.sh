# W20: the cursor leaves nothing behind. Fast mouse bursts (many PS/2 packets between two
# frames) sweep across a window's edges, the title bar, the dock and the screen corners, and the
# cursor then parks at the same spot as in the baseline shot taken before any movement. Every
# pixel of the two screenshots must be equal (only the HUD and the clock pill differ; the
# caller masks them): a leftover cursor sprite shows up as a differing pixel.
#
#   QEMU_MEM=256M tools/perf/run.sh <img> bios <out> 120 tools/perf/scen/w20-cursor.sh
#   tools/perf/w20-cursor-check.sh <out>       # compares rest0 with rest1..restN
#
# Env: STEP (pixels per mouse_move command, default 12), GAP (seconds between commands in a
# burst, default 0.003), W20_PS2 unused (PS/2 is the only pointer the guest has).
source "$(dirname "$0")/../lib.sh"
STEP=${STEP:-12}
GAP=${GAP:-0.003}

# burst dx dy n: n relative moves in ONE monitor connection (the guest sees the packets
# back to back, usually several per compositor frame).
burst() {
  local dx=$1 dy=$2 n=$3 i
  {
    for ((i = 0; i < n; i++)); do
      echo "mouse_move $dx $dy"
      [ "$GAP" != 0 ] && sleep "$GAP"
    done
  } | socat - "UNIX-CONNECT:${MON_SOCK:-$(cat "$OUT/mon.path")}" >/dev/null 2>&1
}
# Absolute position: clamp to the top-left corner, then walk (the guest clamps to the screen).
home() { burst -400 -400 6; sleep 0.4; CX=0; CY=0; }
to() { # x y : from the current spot, in bursts of STEP px
  local tx=$1 ty=$2 dx dy
  dx=$((tx - CX)); dy=$((ty - CY))
  local n=$(( (${dx#-} > ${dy#-} ? ${dx#-} : ${dy#-}) / STEP + 1 ))
  burst $((dx / n)) $((dy / n)) "$n"
  CX=$((CX + (dx / n) * n)); CY=$((CY + (dy / n) * n))
  # integer division leaves a remainder: finish exactly
  burst $((tx - CX)) $((ty - CY)) 1; CX=$tx; CY=$ty
}
park() { home; to 900 300; sleep 1.2; }

# `repaint`: Alt+Tab to the other window and back; closing the switcher forces ONE full repaint
# (see `force_full` in kernel/src/main.rs), so the shot after it is the ground truth for what
# the screen should look like with the cursor at its current spot.
repaint() {
  mon "sendkey alt-tab 700"; sleep 4
  mon "sendkey alt-tab 700"; sleep 4
}

wait_first_frame
dock 559; click; sleep 2          # the editor opens on top of the shell
mon "sendkey alt-tab 700"; sleep 4  # the shell is focused again (state S for every shot below)
shot setup
for round in 1 2 3; do
  # across the right edge and back, near the title bar and the bottom edge
  home; to 760 200; to 600 200; to 760 200; to 640 430; to 700 470; to 660 436; to 40 436
  to 80 100; to 700 90; to 120 450; to 120 520
  # the dock, then along it
  to 451 672; to 829 672; to 451 690; to 829 700; to 1270 710; to 1270 1
  to 1 1; to 1 710; to 1279 719; to 1279 0
  # a diagonal through the window
  to 70 80; to 670 440; to 70 440; to 670 80
  # wiggle on the spot (many tiny packets)
  for i in 1 2 3 4 5 6; do burst 3 2 20; burst -3 -2 20; done
  park
  shot rest$round
  repaint
  shot clean$round
done
finish
