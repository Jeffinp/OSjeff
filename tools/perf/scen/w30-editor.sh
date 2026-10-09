# W30 i18n: the Editor in Portuguese, then (after the switch in Ajustes, with the window and its
# unsaved text left open) in English: status bar, find and replace bar with its notices, go to
# line, the Open and Save-as dialogs, the overwrite question and the "save changes?" sheet.
# Disk as for w30-files.sh (/leiame.txt exists, for the overwrite question).
#   FS_IMG=<disk.img> QEMU_MEM=512M tools/perf/run.sh <img> uefi <out> 300 tools/perf/scen/w30-editor.sh
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon editor; click; sleep 2.5
retype() { # <word>: the same three lines in each pass
  key ctrl-a; typestr "$1 beta"; key ret; typestr "gamma"; key ret; typestr "$1 delta"; sleep 0.6
}
retype alpha
to_english() {
  dock_icon settings; click; sleep 1.6
  goto 320 295; click; sleep 0.9
  goto 700 258; click; sleep 1.2
  key ctrl-w; sleep 1
}
pass() { # <tag> <word in the text> <its replacement>
  local t=$1
  goto 700 560; sleep 0.4
  retype "$2"
  key ctrl-home; sleep 0.6; shot $t-status
  key ctrl-f; sleep 0.4; key ctrl-u; typestr "zzz"; sleep 0.9; shot $t-find-none
  key ctrl-u; typestr "$2"; key ret; key ret; key ret; sleep 0.9; shot $t-find-wrapped
  key esc; sleep 0.4
  key ctrl-h; sleep 0.4; key tab; key ctrl-u; typestr "$3"; key alt-a; sleep 0.9; shot $t-replaced
  key esc; sleep 0.4
  key ctrl-g; sleep 0.4; key ctrl-u; typestr "x"; key ret; sleep 0.8; shot $t-goto-invalid
  key esc; sleep 0.4
  key ctrl-o; sleep 1.2; shot $t-open
  key esc; sleep 0.6
  key ctrl-shift-s; sleep 1.2; shot $t-saveas
  typestr "leiame.txt"; key ret; sleep 1; shot $t-overwrite
  key esc; sleep 0.6; key esc; sleep 0.6
  key ctrl-q; sleep 1.2; shot $t-close
  key esc; sleep 0.6
}
pass pt alpha omega
to_english
shot lang-switched
pass en omega alpha
finish
