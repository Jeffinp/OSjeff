# Scenario helpers. Needs $OUT (run dir) and $MODE.
mon() { echo "$@" | socat - "UNIX-CONNECT:$OUT/mon.sock" >/dev/null 2>&1; }
wait_first_frame() {
  until grep -aq "first desktop frame" "$OUT/serial.log" 2>/dev/null; do sleep 0.2; done
  sleep 2.5   # let the first (animated) frames settle
}
move() { mon "mouse_move $1 $2"; }
click() { mon "mouse_button 1"; sleep 0.08; mon "mouse_button 0"; sleep 0.08; }
rclick() { mon "mouse_button 2"; sleep 0.08; mon "mouse_button 0"; sleep 0.08; }
key() { mon "sendkey $1"; }
# absolute cursor tracking (cursor starts at screen center)
CX=640
if [ "$MODE" = uefi ]; then CY=400; H=800; else CY=360; H=720; fi
goto() { move $(( $1 - CX )) $(( $2 - CY )); CX=$1; CY=$2; sleep 0.3; }
DOCKY=$(( H - 48 )); DY=$(( DOCKY - CY ))
# dock icon x: start 451, term 507, edit 559, task 613, calc 666, browser 721, wasm 775, files 829
dock() { goto "$1" "$DOCKY"; }
dock_to() { move "$1" "$DY"; sleep 0.3; }
shot() { mon "screendump $OUT/$1.ppm"; sleep 1.2; convert "$OUT/$1.ppm" "$OUT/$1.png" 2>/dev/null; rm -f "$OUT/$1.ppm"; }
finish() { touch "$OUT/done"; }
