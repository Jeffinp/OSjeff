# W18 proof (a), boot 2: same disk image as w18-persist-1.sh. The note written in
# boot 1 is in Notas' list (the app reads it back from /data/notes) and its text
# opens; the serial log says no bundled app was installed again.
source "$(dirname "$0")/../lib.sh"
srow() { echo $(( DOCKY - 529 + 38 * $1 )); }
wait_first_frame
shot q1-boot
dock 451; click; sleep 0.6; key end; sleep 0.4; goto 440 "$(srow 5)"; click; sleep 2   # Notas (End scrolls the list: Notas is row 5)
shot q2-notes-list
key tab; sleep 0.3; key ret; sleep 1                                  # Tab: focus the list, Enter: open the first note
shot q3-notes-opened
finish
