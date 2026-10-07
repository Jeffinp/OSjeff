# W18 proof (a), boot 1: an installed app writes a file to its own persistent
# sandbox (/data/notes). Notas (the Start panel, scrolled to the end) saves a note with Ctrl+S;
# Files then shows it at /data/notes. Boot 2 (w18-persist-2.sh) reuses the same disk
# image: FS_IMG=<outdir of boot 1>/fs.img.
source "$(dirname "$0")/../lib.sh"
srow() { echo $(( DOCKY - 529 + 38 * $1 )); }
type_text() { for ch in "$@"; do key "$ch"; sleep 0.12; done; }
wait_first_frame
shot p1-boot
dock 451; click; sleep 0.6; key end; sleep 0.4; goto 440 "$(srow 5)"; click; sleep 2   # Notas (End scrolls the list: Notas is row 5)
type_text h e l l o spc p e r s i s t
key ret; type_text s u r v i v e s spc r e b o o t
key ctrl-s; sleep 1
shot p2-notes-saved
dock 829; click; sleep 2.5                                            # Files
key end; sleep 0.5; shot p3-files-root
finish
