# Favourites persist: Ctrl+D on the start page writes /home/.bookmarks (check the disk with
# fs3_inject --ls after the run); boot 2 (FS_IMG=<out>/fs.img) shows them at kitsune://favoritos.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 721; click; sleep 3
key ctrl-l; sleep 0.3
for k in o s j e f f shift-semicolon slash slash s o b r e; do key $k; sleep 0.15; done
key ret; sleep 2
key ctrl-d; sleep 1
shot b1-starred
finish
