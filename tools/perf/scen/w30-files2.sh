# W30 i18n: Arquivos, the second half: status messages (unsupported format, copied, pasted, moved
# to the trash), the copy progress sheet, the Trash with an item and its menu, the confirmation
# sheets, the cancelled and deleted messages and the "cannot paste into the trash" error. Pt, then
# the switch in Ajustes and the same in English. Disk as for w30-files.sh (with /etc).
#   FS_IMG=<disk.img> QEMU_MEM=512M tools/perf/run.sh <img> uefi <out> 300 tools/perf/scen/w30-files2.sh
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon files; click; sleep 2.5
to_english() {
  dock_icon settings; click; sleep 1.6
  goto 320 295; click; sleep 0.9
  goto 700 258; click; sleep 1.2
  key ctrl-w; sleep 1
}
pass() { # <tag>
  local t=$1
  goto 279 374; click; sleep 1.2        # Disco (the root)
  goto 600 428; click; sleep 0.6        # big.bin
  key ret; sleep 0.8; shot $t-unsupported
  key ctrl-c; sleep 0.6; shot $t-copied-msg
  goto 600 344; click; key ret; sleep 1.2   # into Projetos
  key ctrl-v; sleep 0.7; shot $t-copying
  sleep 5; shot $t-copy-done
  key delete; sleep 1.5; shot $t-trashed
  goto 282 312; click; sleep 1.5
  goto 600 232; click; sleep 0.6; shot $t-trash-item
  rclick; sleep 0.8; shot $t-menu-trash; key esc; sleep 0.5
  key delete; sleep 1; shot $t-confirm
  key esc; sleep 0.8; shot $t-cancelled
  goto 700 520; rclick; sleep 0.8; goto 760 538; click; sleep 1; shot $t-confirm-empty
  key ret; sleep 1.5; shot $t-deleted
  key ctrl-v; sleep 0.8; shot $t-paste-trash
  goto 600 150; sleep 0.4
}
pass pt
to_english
shot lang-switched
pass en
finish
