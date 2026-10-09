# Boot 2 of w19-bookmark.sh: the favourite saved in boot 1 is listed at kitsune://favoritos.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 721; click; sleep 3
key ctrl-l; sleep 0.3
for k in o s j e f f shift-semicolon slash slash f a v o r i t o s; do key $k; sleep 0.15; done
key ret; sleep 2
shot b2-favs
finish
