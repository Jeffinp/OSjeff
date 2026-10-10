# W23: Arquivos (file manager): list, icons, preview, search, selection, drag and
# drop, rename, menus, sheets, empty states. Needs a prepared disk (FS_IMG) with /Imagens/*.png,
# /Documentos, /Projetos, /big.bin, /leiame.txt and /etc/kitsune.conf (appearance=light|dark);
# see docs/TESTING.md, "Arquivos e Imagens (W23)".
#   FS_IMG=<disk.img> QEMU_MEM=256M tools/perf/run.sh <img> bios <out> 200 tools/perf/scen/w23-files.sh
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon files; click; sleep 2.5
goto 700 560                              # park the pointer on empty list space
shot f01-list
# Documentos: rename inline, context menu, sort menu.
goto 300 222; click; sleep 1.2
goto 520 260; click; sleep 0.6
key f2; sleep 0.8
shot f02-rename
key esc; sleep 0.4
rclick; sleep 0.9
shot f03-menu
key esc; sleep 0.4
goto 862 164; click; sleep 0.9
shot f04-sort
key esc; sleep 0.4
# Imagens: list, then icons, then the preview of a picture.
goto 300 252; click; sleep 1.2
goto 700 540
shot f05-imagens-list
key ctrl-2; sleep 1
goto 513 235; click; sleep 0.8
shot f06-icons
key spc; sleep 2
shot f07-preview-image
key spc; sleep 0.8
# Rubber band in the icon grid.
goto 430 500
mon "mouse_button 1"; sleep 0.2
goto 560 400; goto 690 270; sleep 0.5
shot f08-band
mon "mouse_button 0"; sleep 0.6
# Drag a picture onto Documentos in the sidebar.
key ctrl-1; sleep 0.8
goto 520 232; click; sleep 0.5
mon "mouse_button 1"; sleep 0.2
goto 470 240; goto 380 236; goto 300 224; sleep 0.6
shot f09-drag
mon "mouse_button 0"; sleep 1
# Confirmation sheet.
goto 520 260; click; sleep 0.5
key shift-delete; sleep 1
shot f10-sheet
key esc; sleep 0.5
# Copy a big file: progress sheet.
goto 300 374; click; sleep 1.2
goto 700 580
key end; key up; sleep 0.5               # big.bin, the last but one
key ctrl-c; sleep 0.4
goto 300 222; click; sleep 1
key ctrl-v; sleep 0.45
snap f11-copy
sleep 10
shot f12-copied
# Empty trash state, and info sheet.
goto 300 312; click; sleep 1.2
shot f13-trash
goto 300 374; click; sleep 1
goto 520 232; click; sleep 0.4
key ctrl-i; sleep 1
shot f14-info
flush_snaps
finish
