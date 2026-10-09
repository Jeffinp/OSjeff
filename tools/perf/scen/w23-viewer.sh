# W23: Imagens (picture viewer): fit/fill/actual, filmstrip, info inspector, pan inertia, rotate
# animation, transparency, friendly error, slideshow, save sheet. Disk as for w23-files.sh.
#   FS_IMG=<disk.img> QEMU_MEM=256M tools/perf/run.sh <img> bios <out> 200 tools/perf/scen/w23-viewer.sh
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon files; click; sleep 2.5
goto 300 252; click; sleep 1.2           # Imagens
key ret; sleep 3                          # the first picture opens in Imagens
goto 700 330
sleep 5                                    # thumbnails appear one by one
shot v01-fit
key i; sleep 1
shot v02-info
key i; sleep 0.5
key 9; sleep 1.2
shot v03-fill
key 1; sleep 1
for r in 1 2 3; do mon "mouse_move 0 0 -1"; sleep 0.1; done   # wheel zoom in
sleep 1.2
shot v04-zoom
# Drag fast and let go: a glide.
mon "mouse_button 1"; sleep 0.1
for i in 1 2 3 4 5 6; do move -18 -6; sleep 0.03; done
mon "mouse_button 0"; sleep 0.15
snap v05-glide
sleep 1.5
key 0; sleep 1.2
key r; sleep 0.09
snap v06-rotating
sleep 1
shot v07-rotated
key end; sleep 2
shot v08-transparent
key home; key right; key right; sleep 2
shot v09-error
key home; sleep 1.5
key spc; sleep 4.3
shot v10-slideshow
key esc; sleep 0.5
key ctrl-s; sleep 1.2
shot v11-save
key esc; sleep 0.5
flush_snaps
finish
