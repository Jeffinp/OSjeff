# W26 taskbar: hover lift and tooltip, running indicators (pill = focused, dot = running), a click that
# focuses / minimises, dragging a pinned icon to reorder it, the context menu with the app's windows,
# the show-desktop sliver and Ctrl+Alt+D. Light: QEMU_EXTRA="-rtc base=2026-10-08T12:00:00".
source "$(dirname "$0")/../lib.sh"
wait_first_frame
shot task-desktop
dock_icon files; sleep 0.8; shot task-hover
click; sleep 1.5
dock_icon calc; click; sleep 1.2
dock_icon editor; click; sleep 1.2
shot task-running
# click the focused app's icon: it minimises (flies into the icon)
dock_icon editor; click; sleep 1.0; shot task-minimised
# drag the Calculadora icon to the left, past Arquivos
dock_icon calc; mon "mouse_button 1"; sleep 0.2
move -40 0; sleep 0.1; move -40 0; sleep 0.1; move -40 0; sleep 0.1; move -40 0; sleep 0.1; move -40 0; sleep 0.1
snap task-drag; flush_snaps
mon "mouse_button 0"; sleep 1.2; CX=$((CX - 200)); shot task-reordered
# the context menu of Arquivos
dock_icon files; rclick; sleep 0.7; shot task-menu
key esc; sleep 0.5
# show the desktop, then bring everything back
dock_icon desktop; sleep 0.6; shot task-sliver-hover
click; sleep 1.5; shot task-desktop-shown
click; sleep 1.5; shot task-restored
finish
