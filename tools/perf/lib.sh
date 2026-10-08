# Scenario helpers. Needs $OUT (run dir) and $MODE.
mon() { echo "$@" | socat - "UNIX-CONNECT:${MON_SOCK:-$(cat "$OUT/mon.path")}" >/dev/null 2>&1; }
wait_first_frame() {
  until grep -aq "first desktop frame" "$OUT/serial.log" 2>/dev/null; do sleep 0.2; done
  sleep 2.5   # let the first (animated) frames settle
}
# Relative moves are split into steps of at most 100 px: one big PS/2 packet overflows
# (the guest sees a clipped, unpredictable delta), which broke long `goto` jumps.
move() {
  local dx=$1 dy=$2 sx sy
  while [ "$dx" -ne 0 ] || [ "$dy" -ne 0 ]; do
    sx=$dx; [ "$sx" -gt 100 ] && sx=100; [ "$sx" -lt -100 ] && sx=-100
    sy=$dy; [ "$sy" -gt 100 ] && sy=100; [ "$sy" -lt -100 ] && sy=-100
    mon "mouse_move $sx $sy"; dx=$((dx - sx)); dy=$((dy - sy)); sleep 0.012
  done
}
click() { mon "mouse_button 1"; sleep 0.08; mon "mouse_button 0"; sleep 0.08; }
# click without the trailing settle sleep: the action starts on the press, so a snap right after
# lands inside its animation
tap() { mon "mouse_button 1"; sleep 0.03; mon "mouse_button 0"; }
rclick() { mon "mouse_button 2"; sleep 0.08; mon "mouse_button 0"; sleep 0.08; }
key() { mon "sendkey $1"; }
# absolute cursor tracking (cursor starts at screen center)
CX=640
if [ "$MODE" = uefi ]; then CY=400; H=800; else CY=360; H=720; fi
goto() { move $(( $1 - CX )) $(( $2 - CY )); CX=$1; CY=$2; sleep 0.3; }
DOCKY=$(( H - 40 )); DY=$(( DOCKY - CY ))
# The app bar (floating, centred): 10 icons of 48 px, 8 px gap, 12 px padding, a 12 px
# separator after the first. Slots: apps files browser terminal editor calc viewer tasks
# monitor settings. `dock_icon <name>` moves the pointer onto an icon's centre.
DOCK_Y_CENTER=$(( H - 40 ))
dock_x() {
  local i
  case "$1" in
    apps) i=0 ;; files) i=1 ;; browser) i=2 ;; terminal) i=3 ;; editor) i=4 ;;
    calc) i=5 ;; viewer) i=6 ;; tasks) i=7 ;; monitor) i=8 ;; settings) i=9 ;; *) i=0 ;;
  esac
  local x0=$(( 640 - (10 * 48 + 9 * 8 + 12 + 24) / 2 + 12 ))
  local extra=0; [ "$i" -ge 1 ] && extra=12
  echo $(( x0 + i * 56 + extra + 24 ))
}
dock_icon() { goto "$(dock_x "$1")" "$DOCK_Y_CENTER"; }
dock() { goto "$1" "$DOCKY"; }
dock_to() { move "$1" "$DY"; sleep 0.3; }
shot() { mon "screendump $OUT/$1.ppm"; sleep 1.2; convert "$OUT/$1.ppm" "$OUT/$1.png" 2>/dev/null; rm -f "$OUT/$1.ppm"; }
finish() { touch "$OUT/done"; }
# instant screendump (no wait): fire it a few tens of ms after an action to catch an animation
# mid-flight; convert them all with flush_snaps before finish.
snap() { mon "screendump $OUT/$1.ppm"; }
flush_snaps() { sleep 2; for f in "$OUT"/*.ppm; do [ -f "$f" ] && convert "$f" "${f%.ppm}.png" 2>/dev/null && rm -f "$f"; done; }
# type a string key by key (US layout): letters, digits and the usual punctuation
typestr() {
  local s="$1" i c k
  for ((i = 0; i < ${#s}; i++)); do
    c="${s:i:1}"
    case "$c" in
      [a-z0-9]) k=$c ;;
      [A-Z]) k="shift-${c,,}" ;;
      ' ') k=spc ;; '.') k=dot ;; '/') k=slash ;; '-') k=minus ;; '_') k=shift-minus ;;
      '=') k=equal ;; '+') k=shift-equal ;; ';') k=semicolon ;; ':') k=shift-semicolon ;;
      ',') k=comma ;; "'") k=apostrophe ;; '"') k=shift-apostrophe ;;
      '|') k=shift-backslash ;; '\') k=backslash ;; '>') k=shift-dot ;; '<') k=shift-comma ;;
      '?') k=shift-slash ;; '!') k=shift-1 ;; '@') k=shift-2 ;; '#') k=shift-3 ;; '$') k=shift-4 ;;
      '%') k=shift-5 ;; '&') k=shift-7 ;; '*') k=shift-8 ;; '(') k=shift-9 ;; ')') k=shift-0 ;;
      '[') k=bracket_left ;; ']') k=bracket_right ;;
      '{') k=shift-bracket_left ;; '}') k=shift-bracket_right ;; '~') k=shift-grave_accent ;;
      *) continue ;;
    esac
    key "$k"
    sleep 0.06
  done
}
