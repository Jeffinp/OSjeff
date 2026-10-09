# W26 Apps launcher: the category rail (Todos, Sistema, Internet, Mídia, Utilitários), the search field,
# the Recentes row (after launching something) and the grid. Rail rows are 40 px high from y 78,
# centred at x 130; with no recents the grid starts at (264, 126) in 136x128 cells.
# Light: QEMU_EXTRA="-rtc base=2026-10-08T12:00:00".
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon apps; click; sleep 1.0; shot launcher-all
for i in 1 2 3 4; do
  goto 130 $((98 + 40 * i)); click; sleep 0.5; shot launcher-cat$i
done
goto 130 98; click; sleep 0.4
typestr "pa"; sleep 0.6; shot launcher-search
key esc; sleep 0.6
# launch two apps from the grid, then look at Recentes
dock_icon apps; click; sleep 1.0; goto 740 186; click; sleep 1.5
dock_icon apps; click; sleep 1.0; goto 400 186; click; sleep 1.5
dock_icon apps; click; sleep 1.0; shot launcher-recents
finish
