# W13 proof (a): open notes + paint + clock + snake at the same time from the Start
# panel (the installed-apps list), arrange them, type into notes, draw in paint,
# steer snake, and take screenshots. Row n of the start panel is at
# y = DOCKY - 529 + 38 n; WASM windows cascade from (240,130) by 28 px each.
source "$(dirname "$0")/../lib.sh"
srow() { echo $(( DOCKY - 529 + 38 * $1 )); }
drag() { # drag <x1> <y1> <x2> <y2>: press, move, release (title-bar drag)
  goto "$1" "$2"; mon "mouse_button 1"; sleep 0.3
  move $(( $3 - $1 )) $(( $4 - $2 )); CX=$3; CY=$4; sleep 0.5
  mon "mouse_button 0"; sleep 0.5
}
type_text() { for ch in "$@"; do key "$ch"; sleep 0.12; done; }
wait_first_frame
shot a0-boot
dock 451; click; sleep 0.6; goto 440 "$(srow 7)"; click; sleep 2     # notes
dock 451; click; sleep 0.6; goto 440 "$(srow 9)"; click; sleep 2     # paint
dock 451; click; sleep 0.6; key down; key down; sleep 0.4
goto 440 "$(srow 9)"; click; sleep 2                                   # clock
dock 451; click; sleep 0.6; key down; key down; sleep 0.4
goto 440 "$(srow 10)"; click; sleep 2                                  # snake
# arrange (topmost window first): snake right, clock top-right, paint middle, notes left
drag 524 173 760 135
drag 416 201 1060 110
drag 388 173 692 215
drag 360 145 134 110
shot a1-four-open
# notes: type, save, run the sandbox self-test
type_text h e l l o spc o s j e f f
key ret; type_text n o t a spc d o spc a p p
key ctrl-s; sleep 0.8
key ctrl-t; sleep 1
shot a2-notes
# paint: pick red, draw a zig-zag stroke across the canvas, save the BMP
goto 700 215; click; sleep 0.5
goto 678 263; click; sleep 0.3
goto 640 340
mon "mouse_button 1"; sleep 0.2
for i in 1 2 3 4 5 6 7 8; do move 28 $(( (i % 2) * 24 - 12 )); sleep 0.15; done
mon "mouse_button 0"; sleep 0.4
key ctrl-s; sleep 1
shot a3-paint
# snake gets keys; clock keeps ticking
goto 700 135; click; sleep 0.5
for k in r d s a w d; do key $k; sleep 0.3; done
shot a4-snake-clock
sleep 2
shot a5-later
finish
