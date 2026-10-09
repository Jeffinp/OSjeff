# W27 compositor oracle, round set A: no window changes by itself, so the screen at rest must be
# byte-identical to a from-scratch recomposition of the same state.
#
# Many windows (Terminal, Editor, Calculadora, Arquivos, Ajustes, Tarefas, Navegador) are moved,
# resized, focused, minimised, restored, tiled, maximised, sent to another workspace; popovers,
# menus and the window menu are opened. After every step the pointer is parked and
#   rest<N>.png   is shot (what the incremental compositor left on the screen), then
#   clean<N>.png  after Ctrl+Alt+R (reference mode: every frame recomposes everything).
# tools/perf/w27-oracle-check.sh <out> counts the differing pixels of each pair.
#
#   QEMU_MEM=256M tools/perf/run.sh <img> bios <out> 600 tools/perf/scen/w27-oracle.sh
#   tools/perf/w27-oracle-check.sh <out>
#
# Env: W27_SET = a (default: windows only), b (Snake, a running terminal and Tarefas, which change
# by themselves), c (the browser and drags inside windows), all.
source "$(dirname "$0")/../lib.sh"
SET=${W27_SET:-a}
# W27_VERIFY=1 switches the kernel's verify mode on (Ctrl+Alt+V: every frame is compared with a
# full redraw; mismatches are logged on the serial port as `compositor-verify: MISMATCH`).
maybe_verify() { [ "${W27_VERIFY:-0}" = 1 ] && { key ctrl-alt-v; sleep 0.5; }; true; }
: > "$OUT/masks.txt"
N=0
# pair [mask x y w h]...: the screen at rest, then the ground truth. With W27_VOLATILE=1 a
# second and third ground-truth shots (clean2_<N>, clean3_<N>) are taken a moment later: what differs between the two
# truths is content that changes by itself (a live chart) and the checker ignores it.
pair() {
  N=$((N + 1))
  local m
  for m in "$@"; do echo "$N $m" >> "$OUT/masks.txt"; done
  goto 1272 380; sleep 0.8
  shot rest$N
  key ctrl-alt-r; sleep 1.6
  shot clean$N
  if [ "${W27_VOLATILE:-0}" = 1 ]; then sleep 1.0; shot clean2_$N; sleep 1.0; shot clean3_$N; fi
  key ctrl-alt-r; sleep 1.2
}
# burst <prefix> <n>: n instant screenshots 0.2 s apart (flicker: a window or a shadow missing
# in some frames). Checked by w27-burst-check.sh.
burst() {
  local i
  for i in $(seq 1 "$2"); do snap "$1$i"; sleep 0.2; done
  flush_snaps
}
# the title bar buttons of a window at (x, y, w, h): close x+w-20, maximise x+w-60, minimise x+w-100
btn_close() { goto $(( $1 + $3 - 20 )) $(( $2 + 16 )); click; sleep 1.2; }
btn_min() { goto $(( $1 + $3 - 100 )) $(( $2 + 16 )); click; sleep 1.2; }
btn_menu() { goto $(( $1 + $3 - 122 )) $(( $2 + 16 )); click; sleep 0.8; }

set_a() {
  wait_first_frame
  sleep 8                                        # let the boot toasts leave
  maybe_verify
  pair                                           # 1: the terminal alone
  dock_icon editor; click; sleep 1.8; pair       # 2: editor over the terminal (610,110,560,350)
  dock_icon calc; click; sleep 1.8; pair         # 3: calculator (470,100,320,520)
  dock_icon files; click; sleep 2.0; pair        # 4: files (220,110,860,520) over everything
  dock_icon settings; click; sleep 2.0; pair     # 5: settings (220,56,820,596)
  # focus the editor by its visible title: it rises and its shadow falls on the others
  goto 700 126; click; sleep 1.0; pair           # 6
  # move the editor away and back over the files window
  drag 700 126 -420 150; pair                    # 7: editor now at (190,260,560,350)
  drag 400 276 330 -130; pair                    # 8
  # resize the files window from its bottom-right corner, then its left edge
  goto 1079 629; drag 1079 629 -200 -120; pair   # 9
  drag 221 300 90 0; pair                        # 10
  # the terminal comes forward, the calculator goes away
  goto 120 96; click; sleep 1.0; pair            # 11
  btn_min 470 100 320; pair                      # 12: calculator minimised
  dock_icon calc; click; sleep 1.6; pair         # 13: restored from the bar
  # tiling and maximising with the keyboard
  key alt-left; sleep 1.6; pair                  # 14
  key alt-up; sleep 1.6; pair                    # 15
  key alt-up; sleep 1.6; pair                    # 16
  key alt-down; sleep 1.6; pair                  # 17
  key alt-down; sleep 1.6; pair                  # 18
  # drag a title to the left screen edge: the snap preview, then the drop
  dock_icon editor; click; sleep 1.5
  drag 500 126 -480 60; sleep 1.0; pair          # 19
  # workspaces: go right (empty), come back, carry a window along
  key ctrl-alt-right; sleep 1.5; pair            # 20
  key ctrl-alt-left; sleep 1.5; pair             # 21
  key ctrl-alt-shift-right; sleep 1.5; pair      # 22
  key ctrl-alt-left; sleep 1.5; pair             # 23
  # popovers and menus
  panel_item tray; click; sleep 0.8; pair        # 24: quick settings
  key esc; sleep 0.8; pair                       # 25
  panel_item clock; click; sleep 0.8; pair       # 26: calendar
  key esc; sleep 0.8; pair                       # 27
  panel_item apps; click; sleep 1.0; pair        # 28: the Apps launcher
  key esc; sleep 1.0; pair                       # 29
  rclick; sleep 0.6; pair                        # 30: a context menu
  key esc; sleep 0.8; pair                       # 31
  # Alt+Tab (a quick press goes to the previous window)
  mon "sendkey alt-tab"; sleep 1.6; pair         # 32
  mon "sendkey alt-tab"; sleep 1.6; pair         # 33
  # close everything that is left, one window at a time
  key ctrl-d; sleep 1.0; pair                    # 34
}
# Set B: windows that change by themselves. Snake runs (and is over after a few seconds: static
# pixels, but the compositor treats it as animating), a terminal runs `sleep`, Tarefas glides.
set_b() {
  export W27_VOLATILE=1
  wait_first_frame
  sleep 8
  maybe_verify
  key ctrl-spc; sleep 1.0; typestr "snake"; sleep 0.8; key ret; sleep 14   # Snake (240,130,720,470)
  pair                                           # 1: snake over the terminal
  goto 150 96; click; sleep 0.8; typestr "sleep 900"; key ret; sleep 1.5
  pair                                           # 2: the terminal runs a command
  goto 700 146; click; sleep 1.0; pair           # 3: snake in front again
  dock_icon editor; click; sleep 1.8; pair       # 4: editor (610,110,560,350) over snake
  drag 700 126 -300 220; pair                    # 5: the editor moves over the terminal
  dock_icon tasks; click; sleep 2.2; pair        # 6: tarefas (190,52,860,592) over everything
  drag 600 68 330 60; pair                       # 7: tarefas moved, hanging off the right edge
  goto 300 400; click; sleep 1.0; pair           # 8: a click raises what is under the pointer
  burst b1_ 12                                   # snake + terminal + tarefas in motion
  btn_min 520 112 860; pair                      # 9: tarefas minimised
  burst b2_ 12                                   # the same without tarefas: must be perfectly still
  dock_icon tasks; click; sleep 1.8; pair        # 10: and back
  key alt-left; sleep 1.6; pair                  # 11
  key alt-up; sleep 1.6; pair                    # 12
  key alt-up; sleep 1.6; pair                    # 13
  key alt-down; sleep 1.6; pair                  # 14
  key alt-down; sleep 1.6; pair                  # 15
  key ctrl-alt-right; sleep 1.5; pair            # 16
  key ctrl-alt-left; sleep 1.5; pair             # 17
  panel_item tray; click; sleep 0.8; pair        # 18: quick settings over live windows
  key esc; sleep 0.8; pair                       # 19
  panel_item clock; click; sleep 0.8; pair       # 20
  key esc; sleep 0.8; pair                       # 21
  mon "sendkey alt-tab"; sleep 1.6; pair         # 22
  mon "sendkey alt-tab"; sleep 1.6; pair         # 23
  burst b3_ 12
}
# Set C: the browser (W24) next to other windows. Its page area repaints on its own while it
# scrolls, hovers or types (a dirty rectangle of its layer); a window that overlaps it, its shadow
# and the title bar around the page must come out exactly as in a full redraw. `kitsune://sobre`
# is a long internal page, so no network is needed. Also drags inside Arquivos (a rubber band) and
# Ajustes (a slider), which do not move their windows.
set_c() {
  wait_first_frame
  sleep 8
  maybe_verify
  dock_icon editor; click; sleep 1.8              # editor (610,110,560,350)
  dock_icon browser; click; sleep 3               # browser (150,60,916,560) over it
  goto 600 100; click; sleep 0.4                  # the address bar
  key ctrl-a; typestr "kitsune://sobre"; key ret; sleep 3
  pair                                            # 1: the page
  goto 500 300; click; sleep 0.4
  for i in 1 2 3; do key pgdn; sleep 0.5; done
  pair                                            # 2: scrolled
  for i in 1 2 3 4 5 6 7 8; do key down; sleep 0.15; done; pair   # 3
  drag 700 76 -400 200; pair                      # 4: the browser moved over the terminal's place
  dock_icon terminal; click; sleep 1.5            # a terminal in front: it overlaps the page
  goto 1000 500; click; sleep 0.3
  for i in 1 2 3; do key pgup; sleep 0.5; done
  pair                                            # 5: scrolling under a window's shadow
  key end; sleep 0.8; pair                        # 6
  mon "sendkey ctrl-t"; sleep 1.2; pair           # 7: a second tab (the strip changes)
  mon "sendkey ctrl-w"; sleep 1.2; pair           # 8
  dock_icon files; click; sleep 2.2; pair         # 9: Arquivos
  drag 400 260 250 150; pair                      # 10: a rubber band in the list
  dock_icon settings; click; sleep 2.2; pair      # 11: Ajustes
  goto 500 300; click; sleep 0.5
  drag 450 300 120 0; pair                        # 12: a drag that starts on a control
  burst c1_ 12
}
case "$SET" in
  a) set_a ;;
  b) set_b ;;
  c) set_c ;;
  all) set_a; set_b; set_c ;;
esac
finish
