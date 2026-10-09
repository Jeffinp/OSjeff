# W30 i18n: Imagens in Portuguese, then (after the switch in Ajustes, with the window left open) in
# English: toolbar and caption, the information panel, the save sheet and its errors, an invalid
# image, and the empty window. Disk as for w30-files.sh.
#   FS_IMG=<disk.img> QEMU_MEM=512M tools/perf/run.sh <img> uefi <out> 300 tools/perf/scen/w30-viewer.sh
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon files; click; sleep 2.5
goto 288 252; click; sleep 1.2          # Imagens
goto 600 288; click; sleep 0.6          # foto.png
key ret; sleep 3                         # opens in Imagens
goto 700 330
sleep 4
to_english() {
  dock_icon settings; click; sleep 1.6
  goto 320 295; click; sleep 0.9
  goto 700 258; click; sleep 1.2
  key ctrl-w; sleep 1
}
pass() { # <tag>
  local t=$1
  key end; key left; sleep 2.5            # the same picture in both passes (the one before the last)
  goto 700 330; sleep 0.5
  shot $t-fit
  key i; sleep 1; shot $t-info; key i; sleep 0.6
  key s; sleep 1; shot $t-save
  typestr "foto"; key ret; sleep 0.8; shot $t-save-exists   # the stem is selected: foto.png exists
  key esc; sleep 0.6
  key 9; sleep 1; shot $t-fill; key 0; sleep 0.8
  key s; sleep 1; key ret; sleep 1.5; shot $t-saved
  key home; sleep 2; shot $t-error
}
pass pt
to_english
shot lang-switched
pass en
key ctrl-w; sleep 0.8
dock_icon viewer; click; sleep 2; shot en-empty
key ctrl-w; sleep 0.8
finish
