# W30 i18n: Arquivos in Portuguese, then (after the switch in Ajustes, with the window left open)
# in English: list, selection, information sheet, menus, sort, icons, preview, Apps, empty states.
# Disk: /Imagens/{foto.png,foto.bmp,foto.ppm,transparente.png,corrompida.png}, /Documentos,
# /Projetos, /vazia (empty), /big.bin (3 MB), /leiame.txt (see docs/TESTING.md, fs3_inject).
#   FS_IMG=<disk.img> QEMU_MEM=512M tools/perf/run.sh <img> uefi <out> 300 tools/perf/scen/w30-files.sh
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
  goto 279 374; click; sleep 1          # Disco (the root)
  goto 600 400; click; sleep 0.7        # big.bin
  shot $t-select
  key ctrl-i; sleep 1; shot $t-props
  key esc; sleep 0.5
  rclick; sleep 0.8; shot $t-menu-file; key esc; sleep 0.5
  key ctrl-a; sleep 0.5; shot $t-select-all
  key ctrl-i; sleep 1; shot $t-props-multi; key esc; sleep 0.5
  goto 700 540; rclick; sleep 0.8; shot $t-menu-blank; key esc; sleep 0.5
  goto 862 164; click; sleep 0.8; shot $t-sort; key esc; sleep 0.5
  key ctrl-2; sleep 1; shot $t-icons; key ctrl-1; sleep 0.8
  goto 288 252; click; sleep 1.2        # Imagens
  goto 600 232; click; sleep 0.6        # corrompida.png first
  key spc; sleep 1.5; shot $t-preview-bad
  key down; key down; sleep 1.2; shot $t-preview-image   # foto.png
  key spc; sleep 0.8
  goto 278 282; click; sleep 1.2; shot $t-apps          # Apps place
  key ctrl-i; sleep 1; shot $t-props-app; key esc; sleep 0.5
  goto 282 312; click; sleep 1.2; shot $t-trash-empty
  goto 279 374; click; sleep 1
  goto 600 372; click; key ret; sleep 1.2; shot $t-empty-folder   # /vazia
  key ctrl-f; sleep 0.4; typestr "zzz"; sleep 0.9; shot $t-search-none
  key esc; sleep 0.5; key esc; sleep 0.5
  goto 600 150; sleep 0.4
}
pass pt
to_english
shot lang-switched                       # the window as it was, retranslated
pass en
finish
