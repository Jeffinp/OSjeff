# Scenario helpers. Needs $OUT (run dir) and $MODE.
mon() { echo "$@" | socat - "UNIX-CONNECT:${MON_SOCK:-$(cat "$OUT/mon.path")}" >/dev/null 2>&1; }
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
